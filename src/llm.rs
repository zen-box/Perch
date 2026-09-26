use crate::config::ChannelType;
use crate::i18n::{AppLanguage, Key, tr, tr_args};
use crate::llm_request::build_request;
use crate::llm_stream::{emit_complete, emit_delta};
use crate::llm_tools::ToolCallState;
use crate::model::{Attachment, ReasoningLevel};
use futures::StreamExt;
use reqwest::{Client, Proxy};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot::Receiver;

/// 工具协议的类型对外仍从 `llm` 出口，调用方不必知道它被拆去了 `llm_tools`。
///
/// `ToolResult` 现在还没有产品代码引用（P3-2 才有），故显式豁免。
#[allow(unused_imports)]
pub use crate::llm_tools::{ToolCall, ToolResult, ToolSpec};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessageReq {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    /// 助手消息要回传的「我请求了哪些工具」。只有 assistant 角色用得上。
    /// 类型定义在 `llm_tools.rs`——工具协议整体都在那边。
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    /// 工具结果消息要指明回的是哪一次调用（Claude / OpenAI 需要）。
    #[serde(default)]
    pub tool_call_id: String,
    /// 工具结果消息对应的**函数名**。Gemini 不认调用 id，只认这个名字，
    /// 所以单独存一份，不能靠 `tool_call_id` 兼职。
    #[serde(default)]
    pub tool_name: String,
}

impl ChatMessageReq {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            attachments: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_id: String::new(),
            tool_name: String::new(),
        }
    }

    pub fn with_attachments(role: impl Into<String>, content: impl Into<String>, attachments: Vec<Attachment>) -> Self {
        Self {
            attachments,
            ..Self::new(role, content)
        }
    }

    /// 助手在本轮要求调用的工具。`content` 通常为空，但渠道要求这个字段存在。
    ///
    /// 同 `ToolResult`：P3-1 只有测试在调用，实际调用点在 P3-2 的 Agent 循环。
    #[allow(dead_code)]
    pub fn assistant_tool_calls(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            tool_calls,
            ..Self::new("assistant", content)
        }
    }

    /// 一条工具执行结果。`role` 统一写 "tool"，各渠道序列化时再决定怎么表达：
    /// OpenAI / Claude 用 `tool_call_id` 里的调用 id，Gemini 用 `tool_name`。
    #[allow(dead_code)]
    pub fn tool_result(result: crate::llm_tools::ToolResult) -> Self {
        Self {
            tool_call_id: result.id,
            tool_name: result.name,
            ..Self::new("tool", result.content)
        }
    }
}

#[derive(Clone, Debug)]
pub enum StreamEvent {
    Thinking(String),
    Content(String),
    /// 模型要求调用一个工具。已经收齐并解析好参数才发出来——流式参数是碎片，
    /// 半个 JSON 交给上层没法用。
    ToolCall(ToolCall),
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
    /// 本轮可用的工具。**为空时请求体里完全不出现 tools 字段**——老会话、没开
    /// Agent 的对话发出去的请求和以前逐字节一致，prompt 缓存不会失效。
    pub tools: Vec<ToolSpec>,
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
    /// 界面语言。错误信息会写进消息的 error 字段给用户看，所以要跟着界面走；
    /// 请求体本身（含 `effective_message_text` 那段附件文本）不受它影响。
    pub lang: AppLanguage,
}

/// 一次请求的成品：目标 URL、请求体、请求头。
///
/// 字段对 `llm_request` 开放——构造在那里（它才需要按渠道拼 JSON），
/// 发送在 `llm.rs::stream_chat`。
pub(crate) struct BuiltRequest {
    pub(crate) url: String,
    pub(crate) body: Value,
    pub(crate) headers: Vec<(String, String)>,
}

pub async fn stream_chat(request: ChatRequest, tx: UnboundedSender<StreamEvent>, mut cancel_rx: Option<Receiver<()>>) {
    if request.base_url.trim().is_empty() {
        let _ = tx.send(StreamEvent::Error(tr(request.lang, Key::ErrNoBaseUrl).into()));
        let _ = tx.send(StreamEvent::Done);
        return;
    }
    let is_local = request.base_url.contains("localhost")
        || request.base_url.contains("127.0.0.1")
        || request.base_url.contains("11434");
    if request.api_key.trim().is_empty() && !is_local {
        let _ = tx.send(StreamEvent::Error(tr(request.lang, Key::ErrNoApiKey).into()));
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
    let idle_timeout = Duration::from_secs(if request.timeout_secs == 0 {
        90
    } else {
        request.timeout_secs
    });
    let mut client_builder = Client::builder()
        .connect_timeout(idle_timeout.min(Duration::from_secs(15)))
        .read_timeout(idle_timeout);
    if !request.proxy.trim().is_empty() {
        match Proxy::all(request.proxy.trim()) {
            Ok(proxy) => client_builder = client_builder.proxy(proxy),
            Err(error) => {
                let _ = tx.send(StreamEvent::Error(tr_args(
                    request.lang,
                    Key::ErrBadProxy,
                    &[&error.to_string()],
                )));
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
                // 5xx / 408 / 429 值得重试，其余直接报错
                let retryable = {
                    let status = response.status();
                    status.is_server_error() || status.as_u16() == 408 || status.as_u16() == 429
                };
                if attempt < attempts && retryable {
                    if !sleep_or_cancel(&mut cancel_rx, attempt).await {
                        let _ = tx.send(StreamEvent::Done);
                        return;
                    }
                    continue;
                }
                let _ = tx.send(StreamEvent::Error(http_status_error(&request, response).await));
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
                let _ = tx.send(StreamEvent::Error(transport_error(&request, &built, &error)));
                let _ = tx.send(StreamEvent::Done);
                return;
            }
        }
    }
}

/// HTTP 状态码不为 2xx 时的错误文案（读响应体、脱敏、翻译）。
///
/// 单独拆出来是因为 `stream_chat` 的重试循环已经很深了，塞在里面看不清主干
/// （只有重试判定留在循环里，因为那要知道 `attempt`）。
async fn http_status_error(request: &ChatRequest, response: reqwest::Response) -> String {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let label = built_label(request);
    redact(
        &tr_args(request.lang, Key::ErrHttpStatus, &[label, &status.to_string(), &body]),
        &request.api_key,
    )
}

/// 连接层面失败（超时、DNS、代理）时的错误文案。
fn transport_error(request: &ChatRequest, built: &BuiltRequest, error: &reqwest::Error) -> String {
    let reason = if error.is_timeout() {
        timeout_message(request)
    } else {
        tr_args(
            request.lang,
            Key::ErrConnectFailed,
            &[built_label(request), &error.to_string()],
        )
    };
    redact(
        &tr_args(request.lang, Key::ErrRequestUrl, &[&reason, &built.url]),
        &request.api_key,
    )
}

fn finish(request: &ChatRequest, tx: &UnboundedSender<StreamEvent>, start_time: Instant, completion_chars: usize) {
    let elapsed = start_time.elapsed();
    let total_secs = elapsed.as_secs_f32().max(0.1);
    let _ = tx.send(StreamEvent::Metrics {
        tokens_prompt: request
            .messages
            .iter()
            .map(|message| message.content.chars().count())
            .sum::<usize>()
            / 2,
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
    let value: Value = response
        .json()
        .await
        .map_err(|error| tr_args(request.lang, Key::ErrBadJson, &[&error.to_string()]))?;
    emit_complete(request.channel_type, &value, tx, completion_chars);
    Ok(())
}

pub(crate) async fn read_sse(
    response: reqwest::Response,
    cancel_rx: &mut Option<Receiver<()>>,
    request: &ChatRequest,
    tx: &UnboundedSender<StreamEvent>,
    completion_chars: &mut usize,
) -> Result<(), String> {
    let mut stream = response.bytes_stream();
    // 按字节缓存，凑齐一整行再解码：网络分块可能正好切在一个汉字的中间
    let mut buffer: Vec<u8> = Vec::new();
    // SSE 的 `event:` 行只对它下面那条 `data:` 生效。Anthropic 靠它区分
    // `content_block_delta` / `message_delta`，OpenAI 的 Responses 也靠它区分
    // `response.output_text.delta` / `response.function_call_arguments.delta`，
    // 所以这个值必须跨行保存，不能像以前那样把 `event:` 行直接丢掉。
    let mut event_name = String::new();
    let mut tool_state = ToolCallState::default();
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
            Some(Err(error)) => {
                return Err(tr_args(
                    request.lang,
                    Key::ErrTransferInterrupted,
                    &[&error.to_string()],
                ));
            }
            None => break,
        };
        buffer.extend_from_slice(&bytes);
        while let Some(pos) = buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buffer.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line).into_owned();
            if handle_sse_line(&line, &mut event_name, request, tx, completion_chars, &mut tool_state) {
                return Ok(());
            }
        }
    }
    // 最后一行可能没有换行符
    handle_sse_line(
        &String::from_utf8_lossy(&buffer),
        &mut event_name,
        request,
        tx,
        completion_chars,
        &mut tool_state,
    );
    tool_state.flush(|call| {
        let _ = tx.send(StreamEvent::ToolCall(call));
    });
    Ok(())
}

/// 处理一行 SSE 数据，返回 true 表示收到了结束标记。
///
/// 两种行都要看：`event:` 记下事件名（下一行 `data:` 用），`data:` 才是载荷。
/// 非流式的完整响应（`emit_complete`）不走这里，它按普通 JSON 解析。
fn handle_sse_line(
    line: &str,
    event_name: &mut String,
    request: &ChatRequest,
    tx: &UnboundedSender<StreamEvent>,
    completion_chars: &mut usize,
    tool_state: &mut ToolCallState,
) -> bool {
    let trimmed = line.trim_end_matches(['\r', '\n']).trim_start();
    if trimmed.is_empty() {
        // 空行是事件之间的分隔符，事件名到此失效
        event_name.clear();
        return false;
    }
    if let Some(name) = trimmed.strip_prefix("event:") {
        *event_name = name.trim().to_string();
        return false;
    }
    let Some(payload) = trimmed.strip_prefix("data:") else {
        return false;
    };
    let payload = payload.trim();
    if payload == "[DONE]" {
        tool_state.flush(|call| {
            let _ = tx.send(StreamEvent::ToolCall(call));
        });
        return true;
    }
    if payload.is_empty() {
        return false;
    }
    if let Ok(value) = serde_json::from_str::<Value>(payload) {
        *completion_chars += emit_delta(request.channel_type, event_name, &value, tx, tool_state);
    }
    false
}

fn timeout_message(request: &ChatRequest) -> String {
    let secs = if request.timeout_secs == 0 {
        90
    } else {
        request.timeout_secs
    };
    tr_args(
        request.lang,
        Key::ErrTimeout,
        &[built_label(request), &secs.to_string()],
    )
}

/// 出错信息里用来指代渠道的短名（「Claude 渠道返回 HTTP 500」）。
fn built_label(request: &ChatRequest) -> &'static str {
    match request.channel_type {
        ChannelType::Claude => tr(request.lang, Key::LabelClaudeChannel),
        ChannelType::Gemini => tr(request.lang, Key::LabelGeminiChannel),
        ChannelType::OpenAiChat | ChannelType::OpenAiResponses => tr(request.lang, Key::LabelOpenAiChannel),
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
    use serde_json::json;

    fn request(channel: ChannelType) -> ChatRequest {
        ChatRequest {
            channel_type: channel,
            base_url: "https://api.openai.com/v1".into(),
            api_key: "secret".into(),
            model: "test-model".into(),
            messages: vec![ChatMessageReq::new("user", "hi")],
            tools: Vec::new(),
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: true,
            reasoning: None,
            max_output: None,
            model_thinks: false,
            extra_headers: Vec::new(),
            proxy: String::new(),
            timeout_secs: 90,
            retries: 1,
            lang: crate::i18n::AppLanguage::ZhCn,
        }
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
        read_sse(response, &mut None, &request(ChannelType::OpenAiChat), &tx, &mut chars)
            .await
            .unwrap();
        let mut received = String::new();
        while let Ok(event) = rx.try_recv() {
            if let StreamEvent::Content(content) = event {
                received.push_str(&content);
            }
        }
        assert_eq!(received, text);
        assert_eq!(chars, text.chars().count());
    }
}
