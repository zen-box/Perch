//! 流式响应的解析：把三个渠道各不相同的 SSE 事件翻译成统一的 `StreamEvent`。
//!
//! 从 `llm.rs` 拆出来是因为这里的渠道差异最琐碎——每家的事件名、字段路径、
//! 工具参数的分片方式都不一样（§3.3 单文件 800 行上限）。
//! 请求的**构造**仍在 `llm.rs` 的 `*_body` 里，工具协议在 `llm_tools.rs`。
//!
//! ⚠️ 改这里的任何一条路径，都要对着真实响应验一遍。这些路径**没法靠类型检查兜住**，
//! 写错了只会安静地什么都不输出（技术债原 #7 就是这么来的：
//! `OpenAiResponses` 和 `OpenAiChat` 共用一个分支，读 `/choices/0/...` 恒为空）。

use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;

use crate::config::ChannelType;
use crate::llm::{StreamEvent, ToolCall};
use crate::llm_tools::{ToolCallState, fresh_call_id, parse_arguments};

/// Gemini 的调用 id。新版接口会给 `id`，老版本不给——不给时补一个唯一的，
/// 不能拿函数名凑数（同一个函数调两次就撞 id 了，见 [`fresh_call_id`]）。
fn gemini_call_id(call: &Value) -> String {
    call.get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map_or_else(fresh_call_id, str::to_string)
}

#[derive(Default)]
pub(crate) struct UsageCounts {
    pub prompt: Option<usize>,
    pub completion: Option<usize>,
}

impl UsageCounts {
    pub(crate) fn update(&mut self, channel: ChannelType, event_name: &str, value: &Value) {
        let usage = match channel {
            ChannelType::Claude if event_name == "message_start" => value.pointer("/message/usage"),
            ChannelType::OpenAiResponses if event_name == "response.completed" => value.pointer("/response/usage"),
            ChannelType::Gemini => value.get("usageMetadata"),
            _ => value.get("usage"),
        };
        let Some(usage) = usage else { return };
        let (input, output) = match channel {
            ChannelType::Claude | ChannelType::OpenAiResponses => ("input_tokens", "output_tokens"),
            ChannelType::OpenAiChat => ("prompt_tokens", "completion_tokens"),
            ChannelType::Gemini => ("promptTokenCount", "candidatesTokenCount"),
        };
        if let Some(count) = usage.get(input).and_then(Value::as_u64) {
            let cached = if channel == ChannelType::Claude {
                ["cache_read_input_tokens", "cache_creation_input_tokens"]
                    .into_iter()
                    .filter_map(|key| usage.get(key).and_then(Value::as_u64))
                    .fold(0u64, u64::saturating_add)
            } else {
                0
            };
            self.prompt = usize::try_from(count.saturating_add(cached)).ok();
        }
        if let Some(count) = usage
            .get(output)
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
        {
            self.completion = Some(count);
        }
    }
}

pub(crate) fn emit_complete(
    channel: ChannelType,
    value: &Value,
    tx: &UnboundedSender<StreamEvent>,
    completion_chars: &mut usize,
) {
    match channel {
        ChannelType::OpenAiChat => {
            if let Some(text) = value
                .pointer("/choices/0/message/reasoning_content")
                .and_then(Value::as_str)
                && !text.is_empty()
            {
                let _ = tx.send(StreamEvent::Thinking(text.to_string()));
            }
            if let Some(text) = value.pointer("/choices/0/message/content").and_then(Value::as_str) {
                *completion_chars += text.chars().count();
                let _ = tx.send(StreamEvent::Content(text.to_string()));
            }
            // 不分片时 `arguments` 是完整 JSON 字符串
            if let Some(calls) = value.pointer("/choices/0/message/tool_calls").and_then(Value::as_array) {
                for call in calls {
                    let name = call.pointer("/function/name").and_then(Value::as_str).unwrap_or("");
                    if name.is_empty() {
                        continue;
                    }
                    let raw = call
                        .pointer("/function/arguments")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let _ = tx.send(StreamEvent::ToolCall(ToolCall {
                        id: call.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
                        name: name.to_string(),
                        arguments: parse_arguments(raw),
                    }));
                }
            }
        }
        ChannelType::OpenAiResponses => {
            // Responses 的完整响应把输出放在 `output` 数组里，每项按 `type` 区分。
            // 旧代码这里读 `/choices/0/message/content`，在 Responses 上恒为空。
            if let Some(output) = value.get("output").and_then(Value::as_array) {
                for item in output {
                    match item.get("type").and_then(Value::as_str).unwrap_or("") {
                        "message" => {
                            if let Some(content) = item.get("content").and_then(Value::as_array) {
                                for block in content {
                                    if block.get("type").and_then(Value::as_str) != Some("output_text") {
                                        continue;
                                    }
                                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                                        *completion_chars += text.chars().count();
                                        let _ = tx.send(StreamEvent::Content(text.to_string()));
                                    }
                                }
                            }
                        }
                        "reasoning" => {
                            if let Some(summary) = item.get("summary").and_then(Value::as_array) {
                                for block in summary {
                                    if let Some(text) = block.get("text").and_then(Value::as_str)
                                        && !text.is_empty()
                                    {
                                        let _ = tx.send(StreamEvent::Thinking(text.to_string()));
                                    }
                                }
                            }
                        }
                        "function_call" => {
                            let name = item.get("name").and_then(Value::as_str).unwrap_or("");
                            if name.is_empty() {
                                continue;
                            }
                            let raw = item.get("arguments").and_then(Value::as_str).unwrap_or_default();
                            let _ = tx.send(StreamEvent::ToolCall(ToolCall {
                                id: item
                                    .get("call_id")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                                name: name.to_string(),
                                arguments: parse_arguments(raw),
                            }));
                        }
                        _ => {}
                    }
                }
            }
        }
        ChannelType::Claude => {
            if let Some(blocks) = value.get("content").and_then(Value::as_array) {
                for block in blocks {
                    let kind = block.get("type").and_then(Value::as_str).unwrap_or("");
                    if kind == "tool_use" {
                        let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                        if !name.is_empty() {
                            let _ = tx.send(StreamEvent::ToolCall(ToolCall {
                                id: block.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
                                name: name.to_string(),
                                // Claude 在这里给的已经是解析好的对象
                                arguments: block.get("input").cloned().unwrap_or_else(|| json!({})),
                            }));
                        }
                        continue;
                    }
                    let text = block
                        .get(if kind == "thinking" { "thinking" } else { "text" })
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if text.is_empty() {
                        continue;
                    }
                    if kind == "thinking" {
                        let _ = tx.send(StreamEvent::Thinking(text.to_string()));
                    } else {
                        *completion_chars += text.chars().count();
                        let _ = tx.send(StreamEvent::Content(text.to_string()));
                    }
                }
            }
        }
        ChannelType::Gemini => {
            if let Some(parts) = value.pointer("/candidates/0/content/parts").and_then(Value::as_array) {
                for part in parts {
                    if let Some(call) = part.get("functionCall") {
                        let name = call.get("name").and_then(Value::as_str).unwrap_or("");
                        if !name.is_empty() {
                            let _ = tx.send(StreamEvent::ToolCall(ToolCall {
                                id: gemini_call_id(call),
                                name: name.to_string(),
                                arguments: call.get("args").cloned().unwrap_or_else(|| json!({})),
                            }));
                        }
                        continue;
                    }
                    let Some(text) = part.get("text").and_then(Value::as_str) else {
                        continue;
                    };
                    if text.is_empty() {
                        continue;
                    }
                    if part.get("thought").and_then(Value::as_bool) == Some(true) {
                        let _ = tx.send(StreamEvent::Thinking(text.to_string()));
                    } else {
                        *completion_chars += text.chars().count();
                        let _ = tx.send(StreamEvent::Content(text.to_string()));
                    }
                }
            }
        }
    }
}

pub(crate) fn emit_delta(
    channel: ChannelType,
    event_name: &str,
    value: &Value,
    tx: &UnboundedSender<StreamEvent>,
    tool_state: &mut ToolCallState,
) -> usize {
    match channel {
        ChannelType::OpenAiChat => {
            let mut count = 0;
            if let Some(text) = value
                .pointer("/choices/0/delta/reasoning_content")
                .and_then(Value::as_str)
                && !text.is_empty()
            {
                let _ = tx.send(StreamEvent::Thinking(text.to_string()));
            }
            if let Some(text) = value.pointer("/choices/0/delta/content").and_then(Value::as_str)
                && !text.is_empty()
            {
                count += text.chars().count();
                let _ = tx.send(StreamEvent::Content(text.to_string()));
            }
            // 流式工具调用：`delta.tool_calls` 是个数组，每项带 `index`
            // 指明是第几个调用，名字只在第一片里出现，参数是一片片拼的。
            if let Some(calls) = value.pointer("/choices/0/delta/tool_calls").and_then(Value::as_array) {
                for call in calls {
                    let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                    let entry = tool_state.entry(index);
                    if let Some(id) = call.get("id").and_then(Value::as_str) {
                        entry.id = id.to_string();
                    }
                    if let Some(name) = call.pointer("/function/name").and_then(Value::as_str) {
                        entry.name.push_str(name);
                    }
                    if let Some(args) = call.pointer("/function/arguments").and_then(Value::as_str) {
                        entry.raw.push_str(args);
                    }
                }
            }
            count
        }
        ChannelType::OpenAiResponses => {
            // Responses 的 SSE 每行都带 `event:` 名，且正文与参数是分开的事件。
            // 这里不能再按 `/choices/0/...` 解析——旧代码把它和 chat/completions
            // 混在一个分支里，所以永远读不出内容。
            let mut count = 0;
            match event_name {
                "response.output_text.delta" => {
                    if let Some(text) = value.get("delta").and_then(Value::as_str)
                        && !text.is_empty()
                    {
                        count += text.chars().count();
                        let _ = tx.send(StreamEvent::Content(text.to_string()));
                    }
                }
                "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                    if let Some(text) = value.get("delta").and_then(Value::as_str)
                        && !text.is_empty()
                    {
                        let _ = tx.send(StreamEvent::Thinking(text.to_string()));
                    }
                }
                // `response.output_item.added` 给出这一项是函数调用、叫什么名字；
                // 参数随后由 `response.function_call_arguments.delta` 补上。
                "response.output_item.added" => {
                    let item = value.get("item");
                    if item.and_then(|item| item.get("type")).and_then(Value::as_str) == Some("function_call") {
                        let index = value.get("output_index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let entry = tool_state.entry(index);
                        if let Some(id) = item.and_then(|item| item.get("call_id")).and_then(Value::as_str) {
                            entry.id = id.to_string();
                        }
                        if let Some(name) = item.and_then(|item| item.get("name")).and_then(Value::as_str) {
                            entry.name = name.to_string();
                        }
                    }
                }
                "response.function_call_arguments.delta" => {
                    let index = value.get("output_index").and_then(Value::as_u64).unwrap_or(0) as usize;
                    if let Some(args) = value.get("delta").and_then(Value::as_str) {
                        tool_state.entry(index).raw.push_str(args);
                    }
                }
                _ => {}
            }
            count
        }
        ChannelType::Claude => {
            // Claude 的事件名放在 `event:` 行上，载荷里的 `type` 字段和它一致，
            // 两个都看一眼更稳（中转有时会改写其中一处）。
            let kind = if event_name.is_empty() {
                value.get("type").and_then(Value::as_str).unwrap_or("")
            } else {
                event_name
            };
            match kind {
                "content_block_delta" => {
                    let delta = value.get("delta");
                    let delta_type = delta.and_then(|delta| delta.get("type")).and_then(Value::as_str);
                    match delta_type {
                        Some("thinking_delta") => {
                            if let Some(text) = delta.and_then(|delta| delta.get("thinking")).and_then(Value::as_str)
                                && !text.is_empty()
                            {
                                let _ = tx.send(StreamEvent::Thinking(text.to_string()));
                            }
                            0
                        }
                        // 工具参数在 Claude 里是 `partial_json` 碎片；`content_block_start`
                        // 才给出这次调用的 id 与函数名。
                        Some("input_json_delta") => {
                            if let Some(partial) = delta
                                .and_then(|delta| delta.get("partial_json"))
                                .and_then(Value::as_str)
                            {
                                let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                                tool_state.entry(index).raw.push_str(partial);
                            }
                            0
                        }
                        _ => {
                            let Some(text) = delta.and_then(|delta| delta.get("text")).and_then(Value::as_str) else {
                                return 0;
                            };
                            if text.is_empty() {
                                return 0;
                            }
                            let _ = tx.send(StreamEvent::Content(text.to_string()));
                            text.chars().count()
                        }
                    }
                }
                "content_block_start" => {
                    let block = value.get("content_block");
                    if block.and_then(|block| block.get("type")).and_then(Value::as_str) == Some("tool_use") {
                        let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let entry = tool_state.entry(index);
                        if let Some(id) = block.and_then(|block| block.get("id")).and_then(Value::as_str) {
                            entry.id = id.to_string();
                        }
                        if let Some(name) = block.and_then(|block| block.get("name")).and_then(Value::as_str) {
                            entry.name = name.to_string();
                        }
                    }
                    0
                }
                // 一轮结束：把所有攒着的调用解析出来发出去
                "message_delta" | "message_stop" => {
                    tool_state.flush(|call| {
                        let _ = tx.send(StreamEvent::ToolCall(call));
                    });
                    0
                }
                _ => {
                    // 兜底：有的中转不转发 `event:` 行，此时靠旧的 `/delta/text` 路径
                    let text = value
                        .pointer("/delta/text")
                        .and_then(Value::as_str)
                        .or_else(|| value.pointer("/delta/thinking").and_then(Value::as_str))
                        .unwrap_or("");
                    if text.is_empty() {
                        return 0;
                    }
                    if value.pointer("/delta/thinking").is_some() {
                        let _ = tx.send(StreamEvent::Thinking(text.to_string()));
                        0
                    } else {
                        let _ = tx.send(StreamEvent::Content(text.to_string()));
                        text.chars().count()
                    }
                }
            }
        }
        ChannelType::Gemini => {
            let mut count = 0;
            if let Some(parts) = value.pointer("/candidates/0/content/parts").and_then(Value::as_array) {
                for part in parts {
                    // Gemini 的 functionCall 参数是完整 JSON（不分片），直接收下
                    if let Some(call) = part.get("functionCall") {
                        let name = call.get("name").and_then(Value::as_str).unwrap_or("");
                        if !name.is_empty() {
                            let arguments = call.get("args").cloned().unwrap_or_else(|| json!({}));
                            let _ = tx.send(StreamEvent::ToolCall(ToolCall {
                                id: gemini_call_id(call),
                                name: name.to_string(),
                                arguments,
                            }));
                        }
                        continue;
                    }
                    let Some(text) = part.get("text").and_then(Value::as_str) else {
                        continue;
                    };
                    if text.is_empty() {
                        continue;
                    }
                    if part.get("thought").and_then(Value::as_bool) == Some(true) {
                        let _ = tx.send(StreamEvent::Thinking(text.to_string()));
                    } else {
                        count += text.chars().count();
                        let _ = tx.send(StreamEvent::Content(text.to_string()));
                    }
                }
            }
            count
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::AppLanguage;
    use crate::llm::{ChatMessageReq, ChatRequest};
    use crate::model::ReasoningLevel;
    use serde_json::json;

    #[test]
    fn usage_counts_follow_each_channels_response_shape() {
        let mut claude = UsageCounts::default();
        claude.update(
            ChannelType::Claude,
            "message_start",
            &json!({"message": {"usage": {"input_tokens": 37, "output_tokens": 1}}}),
        );
        claude.update(
            ChannelType::Claude,
            "message_delta",
            &json!({"usage": {"output_tokens": 91}}),
        );
        assert_eq!((claude.prompt, claude.completion), (Some(37), Some(91)));
        claude.update(
            ChannelType::Claude,
            "message_start",
            &json!({"message": {"usage": {"input_tokens": 37, "cache_read_input_tokens": 5, "cache_creation_input_tokens": 7}}}),
        );
        assert_eq!(claude.prompt, Some(49));

        let mut chat = UsageCounts::default();
        chat.update(
            ChannelType::OpenAiChat,
            "",
            &json!({"usage": {"prompt_tokens": 23, "completion_tokens": 42}}),
        );
        assert_eq!((chat.prompt, chat.completion), (Some(23), Some(42)));

        let mut responses = UsageCounts::default();
        responses.update(
            ChannelType::OpenAiResponses,
            "response.completed",
            &json!({"response": {"usage": {"input_tokens": 12, "output_tokens": 34}}}),
        );
        assert_eq!((responses.prompt, responses.completion), (Some(12), Some(34)));

        let mut gemini = UsageCounts::default();
        gemini.update(
            ChannelType::Gemini,
            "",
            &json!({"usageMetadata": {"promptTokenCount": 51, "candidatesTokenCount": 28}}),
        );
        gemini.update(ChannelType::Gemini, "", &json!({"candidates": []}));
        assert_eq!((gemini.prompt, gemini.completion), (Some(51), Some(28)));
    }

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
            messages: vec![ChatMessageReq::new("user", "hi")],
            tools: Vec::new(),
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: true,
            reasoning: Some(ReasoningLevel::Low),
            max_output: None,
            model_thinks: true,
            extra_headers: Vec::new(),
            proxy: String::new(),
            timeout_secs: 90,
            retries: 1,
            lang: AppLanguage::ZhCn,
        }
    }

    /// 从流事件里挑出工具调用，测试里反复要用。
    fn tool_calls_from(rx: &mut tokio::sync::mpsc::UnboundedReceiver<StreamEvent>) -> Vec<ToolCall> {
        let mut calls = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let StreamEvent::ToolCall(call) = event {
                calls.push(call);
            }
        }
        calls
    }

    /// 把一段 SSE 文本喂进解析器，返回收到的工具调用。
    /// 分块切得很碎，顺带验证跨分块的 `event:` / 参数碎片也能拼回来。
    async fn feed_sse(channel: ChannelType, payload: &str) -> Vec<ToolCall> {
        let bytes = payload.as_bytes().to_vec();
        let chunks: Vec<Result<Vec<u8>, std::io::Error>> = bytes.chunks(7).map(|c| Ok(c.to_vec())).collect();
        let body = reqwest::Body::wrap_stream(futures::stream::iter(chunks));
        let response = reqwest::Response::from(gpui_kit::http_client::http::Response::new(body));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut chars = 0;
        let mut usage = UsageCounts::default();
        crate::llm::read_sse(response, &mut None, &request(channel), &tx, &mut chars, &mut usage)
            .await
            .unwrap();
        tool_calls_from(&mut rx)
    }

    /// OpenAI 流式的工具参数是一片片字符串，第一片给 id 和函数名，
    /// 后面几片拼参数。收齐之前不能发出去（半个 JSON 上层没法用）。
    #[tokio::test]
    async fn openai_streaming_arguments_are_joined_before_emit() {
        let payload = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",",
            "\"function\":{\"name\":\"read_file\",\"arguments\":\"\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,",
            "\"function\":{\"arguments\":\"{\\\"pa\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,",
            "\"function\":{\"arguments\":\"th\\\":\\\"a.rs\\\"}\"}}]}}]}\n\n",
            "data: [DONE]\n\n",
        );
        let calls = feed_sse(ChannelType::OpenAiChat, payload).await;
        assert_eq!(calls.len(), 1, "三次分片应该合成一次调用");
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].arguments["path"], "a.rs");
    }

    /// Claude 的事件名在 `event:` 行上。旧代码把 `event:` 行整个丢掉，
    /// 所以 `content_block_start` / `input_json_delta` 全认不出来，工具参数永远拼不齐。
    #[tokio::test]
    async fn claude_streaming_reads_event_names_and_partial_json() {
        let payload = concat!(
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":",
            "{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"read_file\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":",
            "{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":",
            "{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"a.rs\\\"}\"}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\"}\n\n",
        );
        let calls = feed_sse(ChannelType::Claude, payload).await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "toolu_1");
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].arguments["path"], "a.rs");
    }

    /// Claude 的正文增量走 `text_delta`，别被工具分支抢走。
    #[tokio::test]
    async fn claude_text_delta_still_works() {
        let payload = concat!(
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":",
            "{\"type\":\"text_delta\",\"text\":\"你好\"}}\n\n",
        );
        let bytes = payload.as_bytes().to_vec();
        let chunks: Vec<Result<Vec<u8>, std::io::Error>> = bytes.chunks(6).map(|c| Ok(c.to_vec())).collect();
        let body = reqwest::Body::wrap_stream(futures::stream::iter(chunks));
        let response = reqwest::Response::from(gpui_kit::http_client::http::Response::new(body));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut chars = 0;
        let mut usage = UsageCounts::default();
        crate::llm::read_sse(
            response,
            &mut None,
            &request(ChannelType::Claude),
            &tx,
            &mut chars,
            &mut usage,
        )
        .await
        .unwrap();
        let mut text = String::new();
        while let Ok(event) = rx.try_recv() {
            if let StreamEvent::Content(content) = event {
                text.push_str(&content);
            }
        }
        assert_eq!(text, "你好");
        assert_eq!(chars, 2);
    }

    /// Responses 的 SSE 完全不同于 chat/completions：正文和函数参数各自是
    /// 独立事件，靠 `event:` 名区分。旧代码把它和 chat/completions 混在一个分支里，
    /// 所以正文一个字都读不出来。
    #[tokio::test]
    async fn responses_streaming_uses_event_names() {
        let payload = concat!(
            "event: response.output_text.delta\n",
            "data: {\"delta\":\"你好\"}\n\n",
            "event: response.output_item.added\n",
            "data: {\"output_index\":0,\"item\":{\"type\":\"function_call\",",
            "\"call_id\":\"fc_1\",\"name\":\"read_file\"}}\n\n",
            "event: response.function_call_arguments.delta\n",
            "data: {\"output_index\":0,\"delta\":\"{\\\"path\\\":\\\"a.rs\\\"}\"}\n\n",
            "event: response.completed\n",
            "data: {}\n\n",
        );
        let bytes = payload.as_bytes().to_vec();
        let chunks: Vec<Result<Vec<u8>, std::io::Error>> = bytes.chunks(5).map(|c| Ok(c.to_vec())).collect();
        let body = reqwest::Body::wrap_stream(futures::stream::iter(chunks));
        let response = reqwest::Response::from(gpui_kit::http_client::http::Response::new(body));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut chars = 0;
        let mut usage = UsageCounts::default();
        crate::llm::read_sse(
            response,
            &mut None,
            &request(ChannelType::OpenAiResponses),
            &tx,
            &mut chars,
            &mut usage,
        )
        .await
        .unwrap();

        let mut text = String::new();
        let mut calls = Vec::new();
        while let Ok(event) = rx.try_recv() {
            match event {
                StreamEvent::Content(content) => text.push_str(&content),
                StreamEvent::ToolCall(call) => calls.push(call),
                _ => {}
            }
        }
        assert_eq!(text, "你好", "Responses 的正文走 response.output_text.delta");
        assert_eq!(chars, 2);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "fc_1");
        assert_eq!(calls[0].arguments["path"], "a.rs");
    }

    /// 参数碎成半个 JSON 也得发出去，否则模型下一轮看不到自己发过什么，
    /// 只会不停地重复同一个调用。原文塞进 `_raw` 里不丢内容。
    #[tokio::test]
    async fn broken_arguments_fall_back_to_raw_text() {
        let payload = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",",
            "\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\"}}]}}]}\n\n",
            "data: [DONE]\n\n",
        );
        let calls = feed_sse(ChannelType::OpenAiChat, payload).await;
        assert_eq!(calls.len(), 1, "解析失败也要把调用发出来");
        assert_eq!(calls[0].arguments["_raw"], "{\"path\":");
    }

    /// Gemini 的 functionCall 参数是完整 JSON，而且不给调用 id，
    /// 回传时按函数名对应。
    #[tokio::test]
    async fn gemini_function_call_is_not_fragmented() {
        let payload = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":",
            "{\"name\":\"read_file\",\"args\":{\"path\":\"a.rs\"}}}]}}]}\n\n",
        );
        let calls = feed_sse(ChannelType::Gemini, payload).await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert!(!calls[0].id.is_empty(), "Gemini 不给调用 id，要补一个");
        assert_eq!(calls[0].arguments["path"], "a.rs");
    }

    /// 同一个函数一轮里被调两次（或者两轮各调一次），id 不能相同——
    /// Agent 循环按 id 判断调用有没有回过结果，撞了 id 第二次调用就被跳过了。
    #[tokio::test]
    async fn gemini_calls_to_the_same_function_get_distinct_ids() {
        let payload = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[",
            "{\"functionCall\":{\"name\":\"read_file\",\"args\":{\"path\":\"a.rs\"}}},",
            "{\"functionCall\":{\"name\":\"read_file\",\"args\":{\"path\":\"b.rs\"}}}",
            "]}}]}\n\n",
        );
        let calls = feed_sse(ChannelType::Gemini, payload).await;
        assert_eq!(calls.len(), 2);
        assert_ne!(calls[0].id, calls[1].id);
    }

    /// 没有任何工具调用时不该凭空冒出 ToolCall 事件。
    #[tokio::test]
    async fn plain_content_emits_no_tool_calls() {
        let payload = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: [DONE]\n\n";
        assert!(feed_sse(ChannelType::OpenAiChat, payload).await.is_empty());
    }
    #[test]
    fn complete_tool_calls_are_decoded_for_all_non_chat_channels() {
        let cases = [
            (
                ChannelType::OpenAiResponses,
                json!({
                    "output": [{
                        "type": "function_call",
                        "call_id": "fc_1",
                        "name": "read_file",
                        "arguments": "{\"path\":\"a.rs\"}"
                    }]
                }),
                "fc_1",
            ),
            (
                ChannelType::Claude,
                json!({
                    "content": [{
                        "type": "tool_use",
                        "id": "toolu_1",
                        "name": "read_file",
                        "input": {"path": "a.rs"}
                    }]
                }),
                "toolu_1",
            ),
            (
                ChannelType::Gemini,
                json!({
                    "candidates": [{
                        "content": {
                            "parts": [{
                                "functionCall": {"name": "read_file", "args": {"path": "a.rs"}}
                            }]
                        }
                    }]
                }),
                "",
            ),
        ];

        for (channel, value, expected_id) in cases {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let mut chars = 0;
            emit_complete(channel, &value, &tx, &mut chars);
            let calls = tool_calls_from(&mut rx);
            assert_eq!(calls.len(), 1, "{channel:?} 应解析出一个工具调用");
            assert_eq!(calls[0].name, "read_file");
            assert_eq!(calls[0].arguments["path"], "a.rs");
            if !expected_id.is_empty() {
                assert_eq!(calls[0].id, expected_id);
            } else {
                assert!(!calls[0].id.is_empty(), "Gemini 没有 id 时要补一个");
            }
        }
    }
}
