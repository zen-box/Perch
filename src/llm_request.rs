//! 请求体的构造：把内部的 `ChatRequest` 翻译成三个渠道各自要求的 JSON。
//!
//! 从 `llm.rs` 拆出来（§3.3 单文件 800 行上限）。这里**只负责"怎么发"**：
//! 流式响应的解析在 `llm_stream.rs`，工具协议在 `llm_tools.rs`，
//! 发送与重试仍在 `llm.rs::stream_chat`。
//!
//! ⚠️ 请求体的字段名与结构**直接影响 prompt 缓存**：同样的对话历史如果因为
//! 界面语言或工具清单开关而改变，服务端缓存就会失效、成本上升。
//! 所以 `tools` 这类可选字段一律"为空时完全不出现"，不要发空数组占位。

use reqwest::header::{HeaderName, HeaderValue};
use serde_json::{Value, json};

use crate::config::ChannelType;
use crate::i18n::{Key, tr_args};
use crate::llm::{BuiltRequest, ChatMessageReq, ChatRequest};
use crate::llm_tools::{
    claude_message, claude_tools, gemini_function_call_parts, gemini_tool_response, gemini_tools,
    needs_claude_special_case, openai_responses_tools, openai_tool_calls, openai_tools,
};
use crate::model::{AttachmentKind, ReasoningLevel};

/// 读取附件内容并 base64 编码。
fn read_attachment_base64(path: &str) -> Option<String> {
    crate::file_store::read_base64(path)
}

/// 把纯文本类附件拼进消息正文。
///
/// ⚠️ 这段文本**写给模型看**，属于请求体的一部分，所以刻意不跟着界面语言走：
/// 同一段对话切换语言后请求体必须不变，否则 prompt 缓存会全部失效。
/// 它在 `i18n_skip.txt` 白名单里。
pub(crate) fn effective_message_text(msg: &ChatMessageReq) -> String {
    let mut parts = Vec::new();
    if !msg.content.trim().is_empty() {
        parts.push(msg.content.clone());
    }
    for att in &msg.attachments {
        if att.kind == AttachmentKind::Text
            && let Some(text) = crate::file_store::read_text(&att.path)
        {
            let ext = std::path::Path::new(&att.name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let fence = if text.contains("```") { "````" } else { "```" };
            parts.push(format!(
                "\n\n---\n**附件文件: {}**\n{fence}{ext}\n{text}\n{fence}",
                att.name
            ));
        }
    }
    parts.join("\n")
}

pub(crate) fn build_request(request: &ChatRequest) -> Result<BuiltRequest, String> {
    let mut headers = vec![("Content-Type".into(), "application/json".into())];
    let (url, body) = match request.channel_type {
        ChannelType::OpenAiChat => {
            if !request.api_key.trim().is_empty() {
                headers.push(("Authorization".into(), format!("Bearer {}", request.api_key.trim())));
            }
            (
                openai_url(&request.base_url, ChannelType::OpenAiChat),
                openai_chat_body(request),
            )
        }
        ChannelType::OpenAiResponses => {
            if !request.api_key.trim().is_empty() {
                headers.push(("Authorization".into(), format!("Bearer {}", request.api_key.trim())));
            }
            (
                openai_url(&request.base_url, ChannelType::OpenAiResponses),
                openai_responses_body(request),
            )
        }
        ChannelType::Claude => {
            headers.push(("anthropic-version".into(), "2023-06-01".into()));
            if !request.api_key.trim().is_empty() {
                headers.push(("x-api-key".into(), request.api_key.trim().to_string()));
            }
            (claude_url(&request.base_url), claude_body(request))
        }
        ChannelType::Gemini => (gemini_url(request)?, gemini_body(request)),
    };
    for (name, value) in &request.extra_headers {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        HeaderName::from_bytes(name.as_bytes()).map_err(|_| tr_args(request.lang, Key::ErrBadHeaderName, &[name]))?;
        HeaderValue::from_str(value).map_err(|_| tr_args(request.lang, Key::ErrBadHeaderValue, &[name]))?;
        headers.push((name.to_string(), value.clone()));
    }
    Ok(BuiltRequest { url, body, headers })
}

fn openai_url(base_url: &str, channel: ChannelType) -> String {
    if base_url.ends_with("/chat/completions") || base_url.ends_with("/responses") {
        base_url.to_string()
    } else if channel == ChannelType::OpenAiResponses {
        format!("{}/responses", base_url.trim_end_matches('/'))
    } else {
        format!("{}/chat/completions", base_url.trim_end_matches('/'))
    }
}

fn openai_chat_body(request: &ChatRequest) -> Value {
    let messages: Vec<Value> = request
        .messages
        .iter()
        .map(|msg| {
            if msg.role == "tool" {
                return json!({
                    "role": "tool",
                    "tool_call_id": msg.tool_call_id,
                    "content": msg.content,
                });
            }
            if !msg.tool_calls.is_empty() {
                return json!({
                    "role": msg.role,
                    "content": msg.content,
                    "tool_calls": openai_tool_calls(msg),
                });
            }
            let media_attachments: Vec<_> = msg
                .attachments
                .iter()
                .filter(|a| a.kind == AttachmentKind::Image || a.is_pdf())
                .collect();
            let text_content = effective_message_text(msg);
            if media_attachments.is_empty() {
                json!({
                    "role": msg.role,
                    "content": text_content,
                })
            } else {
                let mut parts = Vec::new();
                if !text_content.is_empty() {
                    parts.push(json!({
                        "type": "text",
                        "text": text_content,
                    }));
                }
                for att in media_attachments {
                    if let Some(b64) = read_attachment_base64(&att.path) {
                        let mime = if att.mime.is_empty() { "image/jpeg" } else { &att.mime };
                        parts.push(json!({
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:{mime};base64,{b64}")
                            }
                        }));
                    }
                }
                json!({
                    "role": msg.role,
                    "content": parts,
                })
            }
        })
        .collect();

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "stream": request.stream,
    });
    if !request.tools.is_empty() {
        body["tools"] = json!(openai_tools(&request.tools));
    }
    if let Some(level) = request.reasoning {
        // 推理模型只接受默认的采样参数，这里不发送温度和 top_p
        body["reasoning_effort"] = json!(level.openai_effort());
        if let Some(max_tokens) = request.max_tokens {
            body["max_completion_tokens"] = json!(max_tokens);
        }
        return body;
    }
    if let Some(temperature) = request.temperature {
        body["temperature"] = json!(sampling_value(temperature));
    }
    if let Some(top_p) = request.top_p {
        body["top_p"] = json!(sampling_value(top_p));
    }
    if let Some(max_tokens) = request.max_tokens {
        body["max_tokens"] = json!(max_tokens);
    }
    body
}

fn responses_message(msg: &ChatMessageReq) -> Value {
    let text_type = if msg.role == "assistant" {
        "output_text"
    } else {
        "input_text"
    };
    let text_content = effective_message_text(msg);
    let mut content = Vec::new();
    if !text_content.is_empty() {
        content.push(json!({"type": text_type, "text": text_content}));
    }
    for att in msg
        .attachments
        .iter()
        .filter(|a| a.kind == AttachmentKind::Image || a.is_pdf())
    {
        let Some(b64) = read_attachment_base64(&att.path) else {
            continue;
        };
        if att.is_pdf() {
            content.push(json!({
                "type": "input_file",
                "filename": att.name,
                "file_data": format!("data:application/pdf;base64,{b64}"),
            }));
        } else {
            let mime = if att.mime.is_empty() { "image/jpeg" } else { &att.mime };
            content.push(json!({
                "type": "input_image",
                "image_url": format!("data:{mime};base64,{b64}"),
            }));
        }
    }
    if content.is_empty() {
        content.push(json!({"type": text_type, "text": ""}));
    }
    json!({"type": "message", "role": msg.role, "content": content})
}

fn openai_responses_input(request: &ChatRequest) -> Vec<Value> {
    let mut input = Vec::new();
    for msg in &request.messages {
        if msg.role == "tool" {
            input.push(json!({
                "type": "function_call_output",
                "call_id": msg.tool_call_id,
                "output": msg.content,
            }));
            continue;
        }
        if !msg.content.trim().is_empty() || msg.tool_calls.is_empty() {
            input.push(responses_message(msg));
        }
        for call in &msg.tool_calls {
            input.push(json!({
                "type": "function_call",
                "call_id": call.id,
                "name": call.name,
                "arguments": crate::llm_tools::arguments_text(&call.arguments),
            }));
        }
    }
    input
}

fn openai_responses_body(request: &ChatRequest) -> Value {
    let mut body = json!({
        "model": request.model,
        "input": openai_responses_input(request),
        "stream": request.stream,
    });
    if !request.tools.is_empty() {
        body["tools"] = json!(openai_responses_tools(&request.tools));
    }
    if let Some(level) = request.reasoning {
        body["reasoning"] = json!({"effort": level.openai_effort()});
    } else {
        if let Some(temperature) = request.temperature {
            body["temperature"] = json!(sampling_value(temperature));
        }
        if let Some(top_p) = request.top_p {
            body["top_p"] = json!(sampling_value(top_p));
        }
    }
    if let Some(max_tokens) = request.max_tokens {
        body["max_output_tokens"] = json!(max_tokens);
    }
    body
}

fn claude_url(base_url: &str) -> String {
    if base_url.ends_with("/messages") {
        base_url.to_string()
    } else {
        format!("{}/messages", base_url.trim_end_matches('/'))
    }
}

fn claude_body(request: &ChatRequest) -> Value {
    let system = request
        .messages
        .iter()
        .filter(|message| message.role == "system")
        .map(|message| message.content.clone())
        .collect::<Vec<_>>()
        .join("\n\n");
    let messages: Vec<Value> = request
        .messages
        .iter()
        .filter(|message| message.role != "system")
        .map(|msg| {
            // 带工具调用的助手消息、以及工具结果消息，Claude 的格式和 OpenAI 差得远，
            // 走单独一条路径；其余情况保持原来的多模态拼装。
            if needs_claude_special_case(msg) {
                return claude_message(msg);
            }
            let media_attachments: Vec<_> = msg
                .attachments
                .iter()
                .filter(|a| a.kind == AttachmentKind::Image || a.is_pdf())
                .collect();
            let text_content = effective_message_text(msg);
            if media_attachments.is_empty() {
                json!({
                    "role": msg.role,
                    "content": text_content,
                })
            } else {
                let mut parts = Vec::new();
                if !text_content.is_empty() {
                    parts.push(json!({
                        "type": "text",
                        "text": text_content,
                    }));
                }
                for att in media_attachments {
                    if let Some(b64) = read_attachment_base64(&att.path) {
                        let mime = if att.mime.is_empty() { "image/jpeg" } else { &att.mime };
                        if att.is_pdf() {
                            parts.push(json!({
                                "type": "document",
                                "source": {
                                    "type": "base64",
                                    "media_type": "application/pdf",
                                    "data": b64,
                                }
                            }));
                        } else {
                            parts.push(json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": mime,
                                    "data": b64,
                                }
                            }));
                        }
                    }
                }
                json!({
                    "role": msg.role,
                    "content": parts,
                })
            }
        })
        .collect();
    // Claude 必须指定 max_tokens：优先用对话参数，其次是模型的输出上限
    let mut max_tokens = request.max_tokens.or(request.max_output).unwrap_or(4096);
    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "stream": request.stream,
    });
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    if !request.tools.is_empty() {
        body["tools"] = json!(claude_tools(&request.tools));
    }
    match request.reasoning.filter(|level| *level != ReasoningLevel::Off) {
        Some(level) => {
            // 思考预算至少 1024，并且要给正文留出空间，不能超过模型的输出上限
            let mut budget = level.budget_tokens().max(1024);
            if let Some(cap) = request.max_output {
                budget = budget.min(cap.saturating_sub(1024)).max(1024);
            }
            max_tokens = max_tokens.max(budget + 1024);
            if let Some(cap) = request.max_output {
                max_tokens = max_tokens.min(cap).max(budget + 1);
            }
            body["thinking"] = json!({"type": "enabled", "budget_tokens": budget});
        }
        None => {
            if let Some(temperature) = request.temperature {
                body["temperature"] = json!(sampling_value(temperature));
            }
            if let Some(top_p) = request.top_p {
                body.as_object_mut().unwrap().remove("temperature");
                body["top_p"] = json!(sampling_value(top_p));
            }
        }
    }
    body["max_tokens"] = json!(max_tokens);
    body
}

fn gemini_url(request: &ChatRequest) -> Result<String, String> {
    let base = request.base_url.trim().trim_end_matches('/');
    let method = if request.stream {
        "streamGenerateContent"
    } else {
        "generateContent"
    };
    let mut url = if base.contains(":streamGenerateContent") || base.contains(":generateContent") {
        base.replace(":streamGenerateContent", &format!(":{method}"))
            .replace(":generateContent", &format!(":{method}"))
    } else {
        format!("{base}/models/{}:{method}", request.model)
    };
    if !request.api_key.trim().is_empty() && !url.contains("key=") {
        let joiner = if url.contains('?') { '&' } else { '?' };
        url.push(joiner);
        url.push_str("key=");
        url.push_str(request.api_key.trim());
    }
    if request.stream && !url.contains("alt=") {
        let joiner = if url.contains('?') { '&' } else { '?' };
        url.push(joiner);
        url.push_str("alt=sse");
    }
    Ok(url)
}

fn gemini_body(request: &ChatRequest) -> Value {
    let system = request
        .messages
        .iter()
        .filter(|message| message.role == "system")
        .map(|message| message.content.clone())
        .collect::<Vec<_>>()
        .join("\n\n");
    let contents: Vec<_> = request
        .messages
        .iter()
        .filter(|message| message.role != "system")
        .map(|message| {
            let mut parts = Vec::new();
            // 工具调用与工具结果在 Gemini 里都是 candidate/user 的 `parts` 元素，
            // 没有独立的角色或字段，所以在这里就地拼出来。
            if message.role == "tool" {
                return gemini_tool_response(message);
            }
            if !message.tool_calls.is_empty() {
                // 模型只调工具不输出文字时，content 是空的，不要塞一个空的 text 块
                return json!({"role": "model", "parts": gemini_function_call_parts(message)});
            }
            let text_content = effective_message_text(message);
            if !text_content.is_empty() {
                parts.push(json!({"text": text_content}));
            }
            for att in message
                .attachments
                .iter()
                .filter(|a| a.kind == AttachmentKind::Image || a.is_pdf())
            {
                if let Some(b64) = read_attachment_base64(&att.path) {
                    let mime = if att.mime.is_empty() { "image/jpeg" } else { &att.mime };
                    parts.push(json!({
                        "inline_data": {
                            "mime_type": mime,
                            "data": b64,
                        }
                    }));
                }
            }
            if parts.is_empty() {
                parts.push(json!({"text": ""}));
            }
            json!({
                "role": if message.role == "assistant" { "model" } else { "user" },
                "parts": parts,
            })
        })
        .collect();
    let mut generation = json!({});
    if let Some(temperature) = request.temperature {
        generation["temperature"] = json!(sampling_value(temperature));
    }
    if let Some(top_p) = request.top_p {
        generation["topP"] = json!(sampling_value(top_p));
    }
    if let Some(max_tokens) = request.max_tokens {
        generation["maxOutputTokens"] = json!(max_tokens);
    }
    match request.reasoning {
        Some(ReasoningLevel::Off) => generation["thinkingConfig"] = json!({"thinkingBudget": 0}),
        Some(level) => {
            generation["thinkingConfig"] = json!({"thinkingBudget": level.budget_tokens(), "includeThoughts": true});
        }
        // 没有指定强度时让模型自己决定，但要求返回思考过程
        None if request.model_thinks => generation["thinkingConfig"] = json!({"includeThoughts": true}),
        None => {}
    }
    let mut body = json!({"contents": contents, "generationConfig": generation});
    if !request.tools.is_empty() {
        body["tools"] = json!([{"functionDeclarations": gemini_tools(&request.tools)}]);
    }
    if !system.is_empty() {
        body["systemInstruction"] = json!({"parts": [{"text": system}]});
    }
    body
}

/// f32 直接转成 JSON 会带出 0.699999988 这样的尾数，保留三位小数
fn sampling_value(value: f32) -> f64 {
    (value as f64 * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::AppLanguage;
    use crate::llm_tools::{ToolCall, ToolResult, ToolSpec};
    use crate::model::{Attachment, AttachmentKind};
    use serde_json::json;

    fn request(channel: ChannelType) -> ChatRequest {
        ChatRequest {
            channel_type: channel,
            base_url: match channel {
                ChannelType::Claude => "https://api.anthropic.com/v1".into(),
                ChannelType::Gemini => "https://generativelanguage.googleapis.com/v1beta".into(),
                _ => "https://api.openai.com/v1".into(),
            },
            api_key: "secret".into(),
            model: "test-model".into(),
            provider_id: "test-provider".into(),
            messages: vec![
                ChatMessageReq::new("system", "be brief"),
                ChatMessageReq::new("user", "hi"),
            ],
            temperature: Some(0.2),
            top_p: Some(0.9),
            max_tokens: Some(128),
            stream: true,
            reasoning: Some(ReasoningLevel::Low),
            max_output: None,
            model_thinks: true,
            tools: Vec::new(),
            extra_headers: vec![("X-Test".into(), "1".into())],
            proxy: String::new(),
            timeout_secs: 90,
            retries: 1,
            lang: AppLanguage::ZhCn,
        }
    }

    #[test]
    fn openai_reasoning_uses_completion_token_limit() {
        let built = build_request(&request(ChannelType::OpenAiChat)).unwrap();
        assert_eq!(built.url, "https://api.openai.com/v1/chat/completions");
        assert_eq!(built.body["reasoning_effort"], "low");
        assert_eq!(built.body["max_completion_tokens"], 128);
        assert!(built.body.get("max_tokens").is_none());
        assert!(
            built.body.get("temperature").is_none(),
            "reasoning models reject custom temperature"
        );
        assert!(built.body.get("top_p").is_none());
        assert!(built.headers.iter().any(|(name, _)| name == "X-Test"));
    }

    #[test]
    fn openai_without_reasoning_keeps_sampling_and_omits_unset_values() {
        let mut plain = request(ChannelType::OpenAiChat);
        plain.reasoning = None;
        let body = build_request(&plain).unwrap().body;
        assert_eq!(body["max_tokens"], 128);
        assert_eq!(body["temperature"], 0.2);
        assert_eq!(body["top_p"], 0.9);
        plain.temperature = None;
        plain.top_p = None;
        let body = build_request(&plain).unwrap().body;
        assert!(body.get("temperature").is_none() && body.get("top_p").is_none());
        plain.reasoning = Some(ReasoningLevel::Off);
        assert_eq!(build_request(&plain).unwrap().body["reasoning_effort"], "none");
    }

    #[test]
    fn claude_splits_system_prompt_and_enables_thinking() {
        let built = build_request(&request(ChannelType::Claude)).unwrap();
        assert_eq!(built.body["system"], "be brief");
        assert_eq!(built.body["messages"][0]["role"], "user");
        assert_eq!(built.body["thinking"]["budget_tokens"], 1024);
        assert!(built.body.get("temperature").is_none());
        assert!(built.body["max_tokens"].as_u64().unwrap() > 1024);
    }

    #[test]
    fn claude_budget_respects_model_output_limit() {
        let mut max = request(ChannelType::Claude);
        max.reasoning = Some(ReasoningLevel::Max);
        max.max_tokens = None;
        max.max_output = Some(32_000);
        let body = build_request(&max).unwrap().body;
        let budget = body["thinking"]["budget_tokens"].as_u64().unwrap();
        let max_tokens = body["max_tokens"].as_u64().unwrap();
        assert_eq!(max_tokens, 32_000);
        assert!(budget < max_tokens && budget >= 1024);

        max.reasoning = Some(ReasoningLevel::Off);
        let body = build_request(&max).unwrap().body;
        assert!(body.get("thinking").is_none());
        assert_eq!(body["max_tokens"], 32_000, "defaults to the model's output limit");
    }

    #[test]
    fn gemini_puts_system_instruction_and_thinking_budget() {
        let built = build_request(&request(ChannelType::Gemini)).unwrap();
        assert!(built.url.contains("/models/test-model:streamGenerateContent"));
        assert!(built.url.contains("key=secret"));
        assert!(built.url.contains("alt=sse"));
        assert_eq!(built.body["systemInstruction"]["parts"][0]["text"], "be brief");
        assert_eq!(built.body["contents"][0]["role"], "user");
        assert_eq!(built.body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 1024);
        assert_eq!(
            built.body["generationConfig"]["thinkingConfig"]["includeThoughts"],
            true
        );

        let mut off = request(ChannelType::Gemini);
        off.reasoning = Some(ReasoningLevel::Off);
        let body = build_request(&off).unwrap().body;
        assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 0);
        off.reasoning = None;
        let body = build_request(&off).unwrap().body;
        assert_eq!(body["generationConfig"]["thinkingConfig"]["includeThoughts"], true);
        assert!(
            body["generationConfig"]["thinkingConfig"]
                .get("thinkingBudget")
                .is_none()
        );
    }

    #[test]
    fn test_multimodal_request_bodies() {
        let temp_dir = std::env::temp_dir();
        let test_img_path = temp_dir.join("test_multimodal.png");
        let _ = std::fs::write(&test_img_path, b"\x89PNG\r\n\x1a\nfakeimagebytes");

        let attachment = Attachment {
            id: "att-1".into(),
            kind: AttachmentKind::Image,
            name: "test.png".into(),
            mime: "image/png".into(),
            path: test_img_path.to_string_lossy().to_string(),
            size: 16,
            hash: "fakehash".into(),
        };

        let mut req = request(ChannelType::OpenAiChat);
        req.messages = vec![
            ChatMessageReq::new("system", "sys prompt"),
            ChatMessageReq::with_attachments("user", "describe this", vec![attachment.clone()]),
        ];

        // OpenAI
        let built_openai = build_request(&req).unwrap();
        let openai_user_msg = &built_openai.body["messages"][1];
        assert_eq!(openai_user_msg["role"], "user");
        let parts = openai_user_msg["content"].as_array().unwrap();
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[0]["text"], "describe this");
        assert_eq!(parts[1]["type"], "image_url");
        assert!(
            parts[1]["image_url"]["url"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png;base64,")
        );

        // Claude
        // Responses：图片要用 `input_image`，不是 Chat Completions 的 `image_url`。
        let mut req_responses = request(ChannelType::OpenAiResponses);
        req_responses.messages = req.messages.clone();
        let built_responses = build_request(&req_responses).unwrap();
        let responses_content = &built_responses.body["input"][1]["content"];
        assert_eq!(responses_content[0]["type"], "input_text");
        assert_eq!(responses_content[1]["type"], "input_image");
        assert!(
            responses_content[1]["image_url"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png;base64,")
        );

        // Claude
        let mut req_claude = request(ChannelType::Claude);
        req_claude.messages = req.messages.clone();
        let built_claude = build_request(&req_claude).unwrap();
        let claude_user_msg = &built_claude.body["messages"][0];
        assert_eq!(claude_user_msg["role"], "user");
        let claude_parts = claude_user_msg["content"].as_array().unwrap();
        assert_eq!(claude_parts[0]["type"], "text");
        assert_eq!(claude_parts[1]["type"], "image");
        assert_eq!(claude_parts[1]["source"]["type"], "base64");
        assert_eq!(claude_parts[1]["source"]["media_type"], "image/png");

        // Gemini
        let mut req_gemini = request(ChannelType::Gemini);
        req_gemini.messages = req.messages.clone();
        let built_gemini = build_request(&req_gemini).unwrap();
        let gemini_parts = built_gemini.body["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(gemini_parts[0]["text"], "describe this");
        assert_eq!(gemini_parts[1]["inline_data"]["mime_type"], "image/png");

        let _ = std::fs::remove_file(test_img_path);

        // Test PDF document support in Claude & Gemini
        let pdf_path = temp_dir.join("test_doc.pdf");
        let _ = std::fs::write(&pdf_path, b"%PDF-1.4 fake pdf data");
        let pdf_att = Attachment {
            id: "att-pdf".into(),
            kind: AttachmentKind::Document,
            name: "test_doc.pdf".into(),
            mime: "application/pdf".into(),
            path: pdf_path.to_string_lossy().to_string(),
            size: 24,
            hash: "pdfhash".into(),
        };

        let mut req_pdf = request(ChannelType::Claude);
        req_pdf.messages = vec![ChatMessageReq::with_attachments(
            "user",
            "read pdf",
            vec![pdf_att.clone()],
        )];
        let built_claude_pdf = build_request(&req_pdf).unwrap();
        let claude_pdf_parts = built_claude_pdf.body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(claude_pdf_parts[1]["type"], "document");
        assert_eq!(claude_pdf_parts[1]["source"]["media_type"], "application/pdf");
        let mut req_responses_pdf = request(ChannelType::OpenAiResponses);
        req_responses_pdf.messages = req_pdf.messages.clone();
        let built_responses_pdf = build_request(&req_responses_pdf).unwrap();
        let responses_pdf_content = &built_responses_pdf.body["input"][0]["content"];
        assert_eq!(responses_pdf_content[1]["type"], "input_file");
        assert_eq!(responses_pdf_content[1]["filename"], "test_doc.pdf");
        assert!(
            responses_pdf_content[1]["file_data"]
                .as_str()
                .unwrap()
                .starts_with("data:application/pdf;base64,")
        );

        // Test Text attachment prompt injection
        let txt_path = temp_dir.join("snippet.rs");
        let _ = std::fs::write(&txt_path, b"fn add(a: i32, b: i32) -> i32 { a + b }");
        let txt_att = Attachment {
            id: "att-txt".into(),
            kind: AttachmentKind::Text,
            name: "snippet.rs".into(),
            mime: "text/x-rust".into(),
            path: txt_path.to_string_lossy().to_string(),
            size: 38,
            hash: "txthash".into(),
        };

        let mut req_txt = request(ChannelType::OpenAiChat);
        req_txt.messages = vec![ChatMessageReq::with_attachments("user", "review code", vec![txt_att])];
        let built_openai_txt = build_request(&req_txt).unwrap();
        let content_str = built_openai_txt.body["messages"][0]["content"].as_str().unwrap();
        assert!(content_str.contains("review code"));
        assert!(content_str.contains("fn add(a: i32, b: i32)"));

        let _ = std::fs::remove_file(pdf_path);
        let _ = std::fs::remove_file(txt_path);
    }

    /// P3-1 的关键约束：没挂工具时，请求体必须和加工具协议之前**逐字段一致**，
    /// 否则老会话的 prompt 缓存会全部失效。四个渠道都验一遍。
    #[test]
    fn omitting_tools_keeps_request_body_untouched() {
        for channel in [
            ChannelType::OpenAiChat,
            ChannelType::OpenAiResponses,
            ChannelType::Claude,
            ChannelType::Gemini,
        ] {
            let body = build_request(&request(channel)).unwrap().body;
            let object = body.as_object().expect("请求体必须是对象");
            assert!(
                !object.contains_key("tools"),
                "{channel:?} 在没挂工具时不该出现 tools 字段"
            );
            assert!(
                !object.contains_key("tool_choice"),
                "{channel:?} 在没挂工具时不该出现 tool_choice 字段"
            );
        }
    }

    #[test]
    fn responses_uses_native_input_and_function_items() {
        let call = ToolCall {
            id: "call_1".into(),
            name: "read_file".into(),
            arguments: json!({"path": "a.rs"}),
        };
        let result = ToolResult {
            id: "call_1".into(),
            name: "read_file".into(),
            content: "fn main() {}".into(),
            ..Default::default()
        };
        let mut request = request(ChannelType::OpenAiResponses);
        request.messages = vec![
            ChatMessageReq::new("system", "be brief"),
            ChatMessageReq::new("user", "read a.rs"),
            ChatMessageReq::assistant_tool_calls("", vec![call]),
            ChatMessageReq::tool_result(result),
        ];
        request.tools = vec![ToolSpec::no_args("list_sessions", "List sessions")];

        let body = build_request(&request).unwrap().body;
        assert_eq!(body["input"][0]["type"], "message");
        assert_eq!(body["input"][0]["role"], "system");
        assert_eq!(body["input"][1]["content"][0]["type"], "input_text");
        assert_eq!(body["input"][2]["type"], "function_call");
        assert_eq!(body["input"][2]["call_id"], "call_1");
        assert_eq!(body["input"][2]["arguments"], "{\"path\":\"a.rs\"}");
        assert_eq!(body["input"][3]["type"], "function_call_output");
        assert_eq!(body["input"][3]["output"], "fn main() {}");
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["name"], "list_sessions");
        assert!(body["tools"][0].get("function").is_none());
        assert_eq!(body["reasoning"]["effort"], "low");
        assert_eq!(body["max_output_tokens"], 128);
        assert!(body.get("messages").is_none());
        assert!(body.get("max_completion_tokens").is_none());
    }

    #[test]
    fn each_channel_serializes_tools_its_own_way() {
        let spec = ToolSpec::new(
            "read_file",
            "读取一个文件",
            json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
        );

        // 没有参数的工具也得带上合法的空 schema——三个渠道都要求这个字段存在
        let no_args = ToolSpec::no_args("list_sessions", "列出会话");
        assert_eq!(no_args.parameters["type"], "object");
        assert!(no_args.parameters["properties"].as_object().unwrap().is_empty());
        let mut openai_no_args = request(ChannelType::OpenAiChat);
        openai_no_args.tools = vec![no_args];
        let body = build_request(&openai_no_args).unwrap().body;
        assert_eq!(body["tools"][0]["function"]["parameters"]["type"], "object");

        // OpenAI：`tools[].type = "function"`，schema 放在 `function.parameters`
        let mut openai = request(ChannelType::OpenAiChat);
        openai.tools = vec![spec.clone()];
        let body = build_request(&openai).unwrap().body;
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "read_file");
        assert_eq!(
            body["tools"][0]["function"]["parameters"]["properties"]["path"]["type"],
            "string"
        );

        // Claude：字段名是 `input_schema`，没有 `type: function` 这一层
        let mut claude = request(ChannelType::Claude);
        claude.tools = vec![spec.clone()];
        let body = build_request(&claude).unwrap().body;
        assert_eq!(body["tools"][0]["name"], "read_file");
        assert_eq!(
            body["tools"][0]["input_schema"]["required"][0], "path",
            "Claude 用 input_schema 而不是 parameters"
        );
        assert!(body["tools"][0].get("function").is_none());

        // Gemini：包在 `tools[].functionDeclarations` 里
        let mut gemini = request(ChannelType::Gemini);
        gemini.tools = vec![spec];
        let body = build_request(&gemini).unwrap().body;
        assert_eq!(body["tools"][0]["functionDeclarations"][0]["name"], "read_file");
        assert_eq!(
            body["tools"][0]["functionDeclarations"][0]["parameters"]["properties"]["path"]["type"],
            "string"
        );
    }

    #[test]
    fn tool_messages_are_serialized_per_channel() {
        let call = ToolCall {
            id: "call_1".into(),
            name: "read_file".into(),
            arguments: json!({"path": "a.rs"}),
        };
        let result = ToolResult {
            id: "call_1".into(),
            name: "read_file".into(),
            content: "fn main() {}".into(),
            is_error: false,
            ..Default::default()
        };
        let messages = vec![
            ChatMessageReq::new("system", "be brief"),
            ChatMessageReq::assistant_tool_calls("", vec![call]),
            ChatMessageReq::tool_result(result),
        ];

        // OpenAI：assistant 带 `tool_calls`（arguments 是字符串），
        // 结果是一条独立的 `role: "tool"` 消息
        let mut openai = request(ChannelType::OpenAiChat);
        openai.messages = messages.clone();
        let body = build_request(&openai).unwrap().body;
        assert_eq!(body["messages"][1]["tool_calls"][0]["id"], "call_1");
        assert_eq!(body["messages"][1]["tool_calls"][0]["type"], "function");
        assert_eq!(
            body["messages"][1]["tool_calls"][0]["function"]["arguments"], "{\"path\":\"a.rs\"}",
            "OpenAI 要的是 JSON 字符串，不是对象"
        );
        assert_eq!(body["messages"][2]["role"], "tool");
        assert_eq!(body["messages"][2]["tool_call_id"], "call_1");

        // Claude：assistant 用 `tool_use` 内容块，结果必须塞进 user 消息的
        // `tool_result` 块——Claude 不接受独立的 tool 角色
        let mut claude = request(ChannelType::Claude);
        claude.messages = messages.clone();
        let body = build_request(&claude).unwrap().body;
        assert_eq!(body["messages"][0]["content"][0]["type"], "tool_use");
        assert_eq!(body["messages"][0]["content"][0]["input"]["path"], "a.rs");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"][0]["type"], "tool_result");
        assert_eq!(body["messages"][1]["content"][0]["tool_use_id"], "call_1");

        // Gemini：调用放在 model 的 parts 里，结果放在 user 的 parts 里
        let mut gemini = request(ChannelType::Gemini);
        gemini.messages = messages;
        let body = build_request(&gemini).unwrap().body;
        assert_eq!(body["contents"][0]["role"], "model");
        assert_eq!(body["contents"][0]["parts"][0]["functionCall"]["name"], "read_file");
        assert_eq!(body["contents"][0]["parts"][0]["functionCall"]["args"]["path"], "a.rs");
        assert_eq!(body["contents"][1]["role"], "user");
        assert_eq!(body["contents"][1]["parts"][0]["functionResponse"]["name"], "read_file");
    }
}
