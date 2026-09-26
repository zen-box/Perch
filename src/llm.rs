use crate::config::ChannelType;
use crate::model::{Attachment, AttachmentKind, ReasoningLevel};
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, Proxy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot::Receiver;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessageReq {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

impl ChatMessageReq {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            attachments: Vec::new(),
        }
    }

    pub fn with_attachments(
        role: impl Into<String>,
        content: impl Into<String>,
        attachments: Vec<Attachment>,
    ) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            attachments,
        }
    }
}

fn read_attachment_base64(path: &str) -> Option<String> {
    crate::file_store::read_base64(path)
}

fn effective_message_text(msg: &ChatMessageReq) -> String {
    let mut parts = Vec::new();
    if !msg.content.trim().is_empty() {
        parts.push(msg.content.clone());
    }
    for att in &msg.attachments {
        if att.kind == AttachmentKind::Text {
            if let Some(text) = crate::file_store::read_text(&att.path) {
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
    }
    parts.join("\n")
}

#[derive(Clone, Debug)]
pub enum StreamEvent {
    Thinking(String),
    Content(String),
    Metrics {
        tokens_prompt: usize,
        tokens_completion: usize,
        speed_tps: f32,
        latency_ms: u64,
    },
    Done,
    Error(String),
}

#[derive(Clone, Debug)]
pub struct ChatRequest {
    pub channel_type: ChannelType,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub messages: Vec<ChatMessageReq>,
    /// 为空时不发送，由接口使用默认值（推理模型大多不接受自定义温度）
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
    pub reasoning: Option<ReasoningLevel>,
    /// 模型的最大输出上限，Claude 必须带 max_tokens，没有指定时用它
    pub max_output: Option<u32>,
    /// 模型会思考：Gemini 据此请求返回思考过程
    pub model_thinks: bool,
    pub extra_headers: Vec<(String, String)>,
    pub proxy: String,
    pub timeout_secs: u64,
    pub retries: u8,
}

 pub(crate) struct BuiltRequest {
    url: String,
    body: Value,
    headers: Vec<(String, String)>,
}

pub async fn stream_chat(request: ChatRequest, tx: UnboundedSender<StreamEvent>, mut cancel_rx: Option<Receiver<()>>) {
    if request.base_url.trim().is_empty() {
        let _ = tx.send(StreamEvent::Error("未配置接口基础地址 (Base URL)，请在设置中配置渠道。".into()));
        let _ = tx.send(StreamEvent::Done);
        return;
    }
    let is_local = request.base_url.contains("localhost")
        || request.base_url.contains("127.0.0.1")
        || request.base_url.contains("11434");
    if request.api_key.trim().is_empty() && !is_local {
        let _ = tx.send(StreamEvent::Error(
            "未配置 API 密钥 (API Key)。\n请进入设置 -> 渠道与服务商 填入该渠道的有效 API Key。".into(),
        ));
        let _ = tx.send(StreamEvent::Done);
        return;
    }

    let built = match build_request(&request) {
        Ok(built) => built,
        Err(error) => {
            let _ = tx.send(StreamEvent::Error(error));
            let _ = tx.send(StreamEvent::Done);
            return;
        }
    };

    // 超时按「多久没有收到数据」计算，而不是整个请求的总时长：
    // 总时长会把正常输出中的长回答截断。
    let idle_timeout = Duration::from_secs(if request.timeout_secs == 0 { 90 } else { request.timeout_secs });
    let mut client_builder = Client::builder()
        .connect_timeout(idle_timeout.min(Duration::from_secs(15)))
        .read_timeout(idle_timeout);
    if !request.proxy.trim().is_empty() {
        match Proxy::all(request.proxy.trim()) {
            Ok(proxy) => client_builder = client_builder.proxy(proxy),
            Err(error) => {
                let _ = tx.send(StreamEvent::Error(format!("代理地址无效: {error}")));
                let _ = tx.send(StreamEvent::Done);
                return;
            }
        }
    }
    let client = client_builder.build().unwrap_or_else(|_| Client::new());
    let start_time = Instant::now();
    let mut completion_chars = 0usize;
    let attempts = request.retries.saturating_add(1).clamp(1, 6);

    for attempt in 1..=attempts {
        if cancelled(&mut cancel_rx).await {
            let _ = tx.send(StreamEvent::Done);
            return;
        }
        let mut http = client.post(&built.url).json(&built.body);
        for (name, value) in &built.headers {
            http = http.header(name, value);
        }
        match http.send().await {
             Ok(response) if response.status().is_success() => {
                 if request.stream {
                     if let Err(error) = read_sse(response, &mut cancel_rx, &request, &tx, &mut completion_chars).await {
                         let _ = tx.send(StreamEvent::Error(error));
                     }
                 } else if let Err(error) = read_json(response, &request, &tx, &mut completion_chars).await {
                     let _ = tx.send(StreamEvent::Error(error));
                 }
                 finish(&request, &tx, start_time, completion_chars);
                 return;
             }
             Ok(response) => {
                 let status = response.status();
                 let body = response.text().await.unwrap_or_default();
                 let label = built_label(&request);
                 let message = redact(&format!("{label}返回 HTTP {status}\n响应: {body}"), &request.api_key);
                 let retryable = status.is_server_error() || status.as_u16() == 408 || status.as_u16() == 429;
                 if attempt < attempts && retryable {
                    if !sleep_or_cancel(&mut cancel_rx, attempt).await {
                        let _ = tx.send(StreamEvent::Done);
                        return;
                    }
                    continue;
                }
                let _ = tx.send(StreamEvent::Error(message));
                let _ = tx.send(StreamEvent::Done);
                return;
            }
            Err(error) => {
                if attempt < attempts {
                    if !sleep_or_cancel(&mut cancel_rx, attempt).await {
                        let _ = tx.send(StreamEvent::Done);
                        return;
                    }
                    continue;
                }
                let reason = if error.is_timeout() {
                    timeout_message(&request)
                } else {
                    format!("连接{}失败: {error}", built_label(&request))
                };
                let _ = tx.send(StreamEvent::Error(redact(
                    &format!("{reason}\n请求地址: {}", built.url),
                    &request.api_key,
                )));
                let _ = tx.send(StreamEvent::Done);
                return;
            }
        }
    }
}

fn finish(request: &ChatRequest, tx: &UnboundedSender<StreamEvent>, start_time: Instant, completion_chars: usize) {
    let elapsed = start_time.elapsed();
    let total_secs = elapsed.as_secs_f32().max(0.1);
    let _ = tx.send(StreamEvent::Metrics {
        tokens_prompt: request.messages.iter().map(|message| message.content.chars().count()).sum::<usize>() / 2,
        tokens_completion: completion_chars,
        speed_tps: (completion_chars as f32 / total_secs).max(0.0),
        latency_ms: elapsed.as_millis() as u64,
    });
    let _ = tx.send(StreamEvent::Done);
}

async fn cancelled(cancel_rx: &mut Option<Receiver<()>>) -> bool {
    if let Some(rx) = cancel_rx.as_mut() {
        matches!(futures::future::poll_immediate(rx).await, Some(Ok(())))
    } else {
        false
    }
}

async fn sleep_or_cancel(cancel_rx: &mut Option<Receiver<()>>, attempt: u8) -> bool {
    let delay = Duration::from_millis(400 * u64::from(attempt));
    if let Some(rx) = cancel_rx.as_mut() {
        tokio::select! {
            _ = rx => false,
            _ = tokio::time::sleep(delay) => true,
        }
    } else {
        tokio::time::sleep(delay).await;
        true
    }
}

async fn read_json(
    response: reqwest::Response,
    request: &ChatRequest,
    tx: &UnboundedSender<StreamEvent>,
    completion_chars: &mut usize,
) -> Result<(), String> {
    let value: Value = response.json().await.map_err(|error| format!("响应不是有效 JSON: {error}"))?;
    emit_complete(request.channel_type, &value, tx, completion_chars);
    Ok(())
}

async fn read_sse(
    response: reqwest::Response,
    cancel_rx: &mut Option<Receiver<()>>,
    request: &ChatRequest,
    tx: &UnboundedSender<StreamEvent>,
    completion_chars: &mut usize,
) -> Result<(), String> {
    let mut stream = response.bytes_stream();
    // 按字节缓存，凑齐一整行再解码：网络分块可能正好切在一个汉字的中间
    let mut buffer: Vec<u8> = Vec::new();
    loop {
        let chunk = if let Some(rx) = cancel_rx.as_mut() {
            tokio::select! {
                _ = rx => return Ok(()),
                next = stream.next() => next,
            }
        } else {
            stream.next().await
        };
        let bytes = match chunk {
            Some(Ok(bytes)) => bytes,
            Some(Err(error)) if error.is_timeout() => return Err(timeout_message(request)),
            Some(Err(error)) => return Err(format!("传输中断: {error}")),
            None => break,
        };
        buffer.extend_from_slice(&bytes);
        while let Some(pos) = buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buffer.drain(..=pos).collect();
            if handle_sse_line(&String::from_utf8_lossy(&line), request, tx, completion_chars) {
                return Ok(());
            }
        }
    }
    // 最后一行可能没有换行符
    handle_sse_line(&String::from_utf8_lossy(&buffer), request, tx, completion_chars);
    Ok(())
}

/// 处理一行 SSE 数据，返回 true 表示收到了结束标记
fn handle_sse_line(
    line: &str,
    request: &ChatRequest,
    tx: &UnboundedSender<StreamEvent>,
    completion_chars: &mut usize,
) -> bool {
    let Some(payload) = line.trim().strip_prefix("data:") else { return false };
    let payload = payload.trim();
    if payload == "[DONE]" {
        return true;
    }
    if payload.is_empty() {
        return false;
    }
    if let Ok(value) = serde_json::from_str::<Value>(payload) {
        *completion_chars += emit_delta(request.channel_type, &value, tx);
    }
    false
}

fn timeout_message(request: &ChatRequest) -> String {
    let secs = if request.timeout_secs == 0 { 90 } else { request.timeout_secs };
    format!("{}超过 {secs} 秒没有返回数据，连接已超时。可以在渠道设置里调大「超时」。", built_label(request))
}

fn emit_complete(channel: ChannelType, value: &Value, tx: &UnboundedSender<StreamEvent>, completion_chars: &mut usize) {
    match channel {
        ChannelType::OpenAiChat | ChannelType::OpenAiResponses => {
            if let Some(text) = value.pointer("/choices/0/message/reasoning_content").and_then(Value::as_str) {
                if !text.is_empty() {
                    let _ = tx.send(StreamEvent::Thinking(text.to_string()));
                }
            }
            if let Some(text) = value.pointer("/choices/0/message/content").and_then(Value::as_str) {
                *completion_chars += text.chars().count();
                let _ = tx.send(StreamEvent::Content(text.to_string()));
            }
        }
        ChannelType::Claude => {
            if let Some(blocks) = value.get("content").and_then(Value::as_array) {
                for block in blocks {
                    let kind = block.get("type").and_then(Value::as_str).unwrap_or("");
                    let text = block.get(if kind == "thinking" { "thinking" } else { "text" }).and_then(Value::as_str).unwrap_or("");
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
                    let Some(text) = part.get("text").and_then(Value::as_str) else { continue };
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

fn emit_delta(channel: ChannelType, value: &Value, tx: &UnboundedSender<StreamEvent>) -> usize {
    match channel {
        ChannelType::OpenAiChat | ChannelType::OpenAiResponses => {
            let mut count = 0;
            if let Some(text) = value.pointer("/choices/0/delta/reasoning_content").and_then(Value::as_str) {
                if !text.is_empty() {
                    let _ = tx.send(StreamEvent::Thinking(text.to_string()));
                }
            }
            if let Some(text) = value.pointer("/choices/0/delta/content").and_then(Value::as_str) {
                if !text.is_empty() {
                    count += text.chars().count();
                    let _ = tx.send(StreamEvent::Content(text.to_string()));
                }
            }
            count
        }
        ChannelType::Claude => {
            let text = value.pointer("/delta/text").and_then(Value::as_str)
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
        ChannelType::Gemini => {
            let mut count = 0;
            if let Some(parts) = value.pointer("/candidates/0/content/parts").and_then(Value::as_array) {
                for part in parts {
                    let Some(text) = part.get("text").and_then(Value::as_str) else { continue };
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

pub(crate) fn build_request(request: &ChatRequest) -> Result<BuiltRequest, String> {
    let mut headers = vec![("Content-Type".into(), "application/json".into())];
    let (url, body) = match request.channel_type {
        ChannelType::OpenAiChat | ChannelType::OpenAiResponses => {
            if !request.api_key.trim().is_empty() {
                headers.push(("Authorization".into(), format!("Bearer {}", request.api_key.trim())));
            }
            (openai_url(&request.base_url, request.channel_type), openai_body(request))
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
        HeaderName::from_bytes(name.as_bytes()).map_err(|_| format!("自定义请求头名称无效: {name}"))?;
        HeaderValue::from_str(value).map_err(|_| format!("自定义请求头的值包含非法字符: {name}"))?;
        headers.push((name.to_string(), value.clone()));
    }
    let _ = HeaderMap::new();
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

fn openai_body(request: &ChatRequest) -> Value {
    let messages: Vec<Value> = request
        .messages
        .iter()
        .map(|msg| {
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
    let method = if request.stream { "streamGenerateContent" } else { "generateContent" };
    let mut url = if base.contains(":streamGenerateContent") || base.contains(":generateContent") {
        base.replace(":streamGenerateContent", &format!(":{method}")).replace(":generateContent", &format!(":{method}"))
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
            let text_content = effective_message_text(message);
            if !text_content.is_empty() {
                parts.push(json!({"text": text_content}));
            }
            for att in message.attachments.iter().filter(|a| a.kind == AttachmentKind::Image || a.is_pdf()) {
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
    if !system.is_empty() {
        body["systemInstruction"] = json!({"parts": [{"text": system}]});
    }
    body
}

/// f32 直接转成 JSON 会带出 0.699999988 这样的尾数，保留三位小数
fn sampling_value(value: f32) -> f64 {
    (value as f64 * 1000.0).round() / 1000.0
}

fn built_label(request: &ChatRequest) -> &'static str {
    match request.channel_type {
        ChannelType::Claude => "Claude 渠道",
        ChannelType::Gemini => "Gemini 渠道",
        ChannelType::OpenAiChat | ChannelType::OpenAiResponses => "OpenAI 渠道",
    }
}

fn redact(text: &str, secret: &str) -> String {
    if secret.trim().is_empty() {
        text.to_string()
    } else {
        text.replace(secret.trim(), "[redacted]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            extra_headers: vec![("X-Test".into(), "1".into())],
            proxy: String::new(),
            timeout_secs: 90,
            retries: 1,
        }
    }

    #[test]
    fn openai_reasoning_uses_completion_token_limit() {
        let built = build_request(&request(ChannelType::OpenAiChat)).unwrap();
        assert_eq!(built.url, "https://api.openai.com/v1/chat/completions");
        assert_eq!(built.body["reasoning_effort"], "low");
        assert_eq!(built.body["max_completion_tokens"], 128);
        assert!(built.body.get("max_tokens").is_none());
        assert!(built.body.get("temperature").is_none(), "reasoning models reject custom temperature");
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
        assert_eq!(built.body["generationConfig"]["thinkingConfig"]["includeThoughts"], true);

        let mut off = request(ChannelType::Gemini);
        off.reasoning = Some(ReasoningLevel::Off);
        let body = build_request(&off).unwrap().body;
        assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 0);
        off.reasoning = None;
        let body = build_request(&off).unwrap().body;
        assert_eq!(body["generationConfig"]["thinkingConfig"]["includeThoughts"], true);
        assert!(body["generationConfig"]["thinkingConfig"].get("thinkingBudget").is_none());
    }

    #[tokio::test]
    async fn sse_chunks_split_inside_a_character_are_decoded_whole() {
        let text = "你好，这是一段中文。";
        let payload = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"choices": [{"delta": {"content": text}}]})
        );
        let bytes = payload.into_bytes();
        // 每个网络分块都切在汉字中间，模拟中转或 CDN 重新分块
        let chunks: Vec<Result<Vec<u8>, std::io::Error>> = bytes.chunks(4).map(|chunk| Ok(chunk.to_vec())).collect();
        let body = reqwest::Body::wrap_stream(futures::stream::iter(chunks));
        let response = reqwest::Response::from(gpui_kit::http_client::http::Response::new(body));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut chars = 0;
        read_sse(response, &mut None, &request(ChannelType::OpenAiChat), &tx, &mut chars).await.unwrap();
        let mut received = String::new();
        while let Ok(event) = rx.try_recv() {
            if let StreamEvent::Content(content) = event {
                received.push_str(&content);
            }
        }
        assert_eq!(received, text);
        assert_eq!(chars, text.chars().count());
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
        assert!(parts[1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,"));

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
        req_pdf.messages = vec![ChatMessageReq::with_attachments("user", "read pdf", vec![pdf_att.clone()])];
        let built_claude_pdf = build_request(&req_pdf).unwrap();
        let claude_pdf_parts = built_claude_pdf.body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(claude_pdf_parts[1]["type"], "document");
        assert_eq!(claude_pdf_parts[1]["source"]["media_type"], "application/pdf");

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
}
