use chrono::Local;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use uuid::Uuid;

use crate::i18n::{AppLanguage, Key, tr};
use crate::llm::ChatMessageReq;
use crate::llm_tools::{ToolCall, ToolResult};
use crate::paths::{DATABASE_FILE, SESSIONS_FILE, data_dir, data_file};
use crate::storage::{Database, StorageResult};

/// 思考强度。不同接口的叫法不同：OpenAI 叫 reasoning_effort，Claude 和 Gemini 用思考预算（token 数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ReasoningLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl ReasoningLevel {
    pub const ALL: [ReasoningLevel; 7] = [
        ReasoningLevel::Off,
        ReasoningLevel::Minimal,
        ReasoningLevel::Low,
        ReasoningLevel::Medium,
        ReasoningLevel::High,
        ReasoningLevel::XHigh,
        ReasoningLevel::Max,
    ];

    /// 界面上的档位名。
    pub fn label(self, lang: AppLanguage) -> &'static str {
        tr(
            lang,
            match self {
                ReasoningLevel::Off => Key::ReasoningOff,
                ReasoningLevel::Minimal => Key::ReasoningMinimal,
                ReasoningLevel::Low => Key::ReasoningLow,
                ReasoningLevel::Medium => Key::ReasoningMedium,
                ReasoningLevel::High => Key::ReasoningHigh,
                ReasoningLevel::XHigh => Key::ReasoningXHigh,
                ReasoningLevel::Max => Key::ReasoningMax,
            },
        )
    }

    /// OpenAI 兼容接口的 `reasoning_effort` 取值，按字面发送，由模型配置决定哪些档位可选
    pub fn openai_effort(self) -> &'static str {
        match self {
            ReasoningLevel::Off => "none",
            ReasoningLevel::Minimal => "minimal",
            ReasoningLevel::Low => "low",
            ReasoningLevel::Medium => "medium",
            ReasoningLevel::High => "high",
            ReasoningLevel::XHigh => "xhigh",
            ReasoningLevel::Max => "max",
        }
    }

    /// Claude / Gemini 的思考预算（token）。关闭时为 0。
    pub fn budget_tokens(self) -> u32 {
        match self {
            ReasoningLevel::Off => 0,
            ReasoningLevel::Minimal => 512,
            ReasoningLevel::Low => 1024,
            ReasoningLevel::Medium => 4096,
            ReasoningLevel::High => 16000,
            ReasoningLevel::XHigh => 32000,
            ReasoningLevel::Max => 64000,
        }
    }
}

fn default_stream() -> bool {
    true
}

/// 对话级参数。字段为空时使用全局设置。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatParams {
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// 只发送最近 N 条消息。空表示全部发送。
    #[serde(default)]
    pub context_limit: Option<usize>,
    /// 空表示使用模型设置里的默认思考强度
    #[serde(default)]
    pub reasoning: Option<ReasoningLevel>,
    #[serde(default = "default_stream")]
    pub stream: bool,
}

impl Default for ChatParams {
    fn default() -> Self {
        Self {
            system_prompt: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            context_limit: None,
            reasoning: None,
            stream: true,
        }
    }
}

impl ChatParams {
    pub fn is_unset(&self) -> bool {
        self.system_prompt.as_ref().is_none_or(|text| text.trim().is_empty())
            && self.temperature.is_none()
            && self.top_p.is_none()
            && self.max_tokens.is_none()
            && self.context_limit.is_none()
            && self.reasoning.is_none()
            && self.stream
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedParams {
    pub system_prompt: String,
    pub temperature: f32,
    pub top_p: Option<f32>,
    pub max_tokens: Option<u32>,
    pub context_limit: Option<usize>,
    pub reasoning: Option<ReasoningLevel>,
    pub stream: bool,
}

/// 同一次提问的多模型对比结果。采用之前不写入主 content。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MessageVariant {
    pub id: String,
    pub provider_id: String,
    pub model: String,
    pub content: String,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub is_streaming: bool,
    #[serde(default)]
    pub prompt_tokens: usize,
    #[serde(default)]
    pub completion_tokens: usize,
    #[serde(default)]
    pub speed_tps: f32,
    #[serde(default)]
    pub latency_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachmentKind {
    Image,
    Text,
    Document,
    Other,
}

impl AttachmentKind {
    /// 只有附件、没有文字时用来派生会话标题的种类前缀（如 `[图片] a.png`）。
    ///
    /// **这是要写进 `session.title` 的值，不是界面文案**，所以不参与 i18n，理由同
    /// [`DEFAULT_SESSION_TITLE`]。放在这里而不是调用点，是为了让「附件种类 -> 标题前缀」
    /// 只有一处定义，将来加种类时编译器会在这里报缺失分支。
    pub fn title_label(&self) -> &'static str {
        match self {
            AttachmentKind::Image => "图片",
            AttachmentKind::Document => "文档",
            AttachmentKind::Text => "代码/文本",
            AttachmentKind::Other => "附件",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub kind: AttachmentKind,
    pub name: String,
    pub mime: String,
    pub path: String,
    pub size: u64,
    #[serde(default)]
    pub hash: String,
}

impl Attachment {
    pub fn absolute_path(&self) -> std::path::PathBuf {
        crate::file_store::resolve_path(&self.path)
    }

    pub fn is_image(&self) -> bool {
        self.kind == AttachmentKind::Image || self.mime.starts_with("image/")
    }

    pub fn is_pdf(&self) -> bool {
        self.mime == "application/pdf" || self.name.to_lowercase().ends_with(".pdf")
    }

    pub fn is_text(&self) -> bool {
        self.kind == AttachmentKind::Text || self.mime.starts_with("text/")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String, // "user", "assistant", "system", "tool"
    pub content: String,
    pub reasoning_content: Option<String>,
    pub created_at: String,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub speed_tps: f32,
    pub latency_ms: u64,
    pub is_streaming: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<String>,
    /// 生成这条回复的模型，旧数据没有该字段时为空
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub quote: Option<String>,
    #[serde(default)]
    pub variants: Vec<MessageVariant>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    /// 模型请求调用了哪些工具。**这是完整记录**，上面那个 `tool_calls` 只是给人看的
    /// 短标签——回传给模型时用这里的结构化数据，读到一半的短标签没法还原成参数。
    ///
    /// `skip_serializing_if` 是为了不改变没有工具调用的会话的落盘内容：
    /// 老数据读进来是空的，序列化回去也不出现这个字段。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub called_tools: Vec<ToolCall>,
    /// 工具结果消息要回的是哪一次调用（Claude / OpenAI 用）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tool_call_id: String,
    /// 工具结果消息对应的函数名。Gemini 不认调用 id，只认名字。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tool_name: String,
    /// 这条工具结果是不是失败的结果，界面据此把块标成错误色。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tool_is_error: bool,
    /// 只在本机显示、**永远不发给模型**的消息。
    ///
    /// 目前用在手打 `/read` 读敏感文件（`.env`、私钥之类）：用户点了同意是想自己看一眼，
    /// 不代表同意把内容交给模型服务商。命令那一条和结果那一条都会标上。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local_only: bool,
}

/// 没有执行的工具调用在请求里补上的结果。
///
/// 写给模型看的，所以是固定英文（和 `local_tools::denial_result` 一样的取向）。
const CALL_NOT_EXECUTED: &str = "Not executed: this tool call was interrupted before it ran.";

/// 一条会话消息在请求里长什么样；`None` 表示不发。
fn api_message(message: &ChatMessage) -> Option<ChatMessageReq> {
    if message.local_only {
        return None;
    }
    if !message.variants.is_empty() && message.content.is_empty() {
        return None;
    }
    // 还在生成的回答、还在执行的工具：都还没有内容，发出去没有意义
    if message.is_streaming && message.content.is_empty() {
        return None;
    }
    let content = quoted_content(&message.content, message.quote.as_deref());

    if message.role == "tool" {
        // 没有调用 id 的是用户手打斜杠命令的输出：模型从没发起过这次调用，
        // 以 `tool` 角色发出去会被严格的接口整个拒绝（这个对话从此就发不出消息了）。
        // 当作一段普通文字交给模型，「先 /read 再提问」照样能用。
        if message.tool_call_id.is_empty() {
            return Some(ChatMessageReq::new("assistant", manual_command_text(message)));
        }
        // 工具结果即使正文为空也要发（比如写入成功但没输出），
        // 少了它模型会以为工具没被调用过，然后反复重发同一个调用。
        return Some(ChatMessageReq::tool_result(ToolResult {
            id: message.tool_call_id.clone(),
            name: message.tool_name.clone(),
            content,
            is_error: message.tool_is_error,
        }));
    }

    // 助手请求调用工具的记录：正文可能为空（只有调用），
    // 但必须发出去——渠道要求带 `tool_calls` 的助手消息存在，
    // 否则后面的工具结果找不到归属。
    if !message.called_tools.is_empty() {
        return Some(ChatMessageReq::assistant_tool_calls(
            content,
            message.called_tools.clone(),
        ));
    }

    if message.role != "user" && content.trim().is_empty() {
        return None;
    }
    if message.role == "user" && content.trim().is_empty() && message.attachments.is_empty() {
        return None;
    }
    Some(ChatMessageReq::with_attachments(
        message.role.clone(),
        content,
        message.attachments.clone(),
    ))
}

/// 手打命令的输出写成给模型看的一段文字。
fn manual_command_text(message: &ChatMessage) -> String {
    let status = if message.tool_is_error { "failed" } else { "output" };
    let body = message.content.trim_end();
    // 输出里本身有 ``` 时换成更长的围栏，免得提前闭合
    let fence = if body.contains("```") { "````" } else { "```" };
    format!(
        "[Local command {status}: `{}`, run by the user on their own computer]\n{fence}text\n{body}\n{fence}",
        message.tool_name
    )
}

/// 把工具调用和结果重新对齐，保证请求能被渠道接受。
///
/// 各渠道的规矩是一样的：带工具调用的助手消息后面，必须紧跟着**每一个**调用的结果，
/// 结果也必须能找到自己的调用。会话历史里这两条都可能被打破：
///
/// - 调用了但没有结果（点了停止、到了轮数上限、程序中途关闭）→ 补一条「未执行」；
/// - 有结果但找不到调用（调用那条被截断或删掉了）→ 丢掉这条结果；
/// - 结果的顺序和调用不一致 → 按调用的顺序排（Gemini 按顺序对应同名调用）。
fn pair_tool_results(items: Vec<ChatMessageReq>) -> Vec<ChatMessageReq> {
    let mut out = Vec::with_capacity(items.len());
    let mut items = items.into_iter().peekable();
    while let Some(item) = items.next() {
        if item.role == "tool" {
            // 能走到这里的结果，前面都没有调用它的助手消息
            continue;
        }
        let calls = item.tool_calls.clone();
        out.push(item);
        if calls.is_empty() {
            continue;
        }
        let mut results = Vec::new();
        while let Some(result) = items.next_if(|next| next.role == "tool") {
            results.push(result);
        }
        for call in &calls {
            match results.iter().position(|result| result.tool_call_id == call.id) {
                Some(ix) => out.push(results.remove(ix)),
                None => out.push(ChatMessageReq::tool_result(ToolResult {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    content: CALL_NOT_EXECUTED.to_string(),
                    is_error: true,
                })),
            }
        }
        // `results` 里剩下的不属于这些调用（重复的、串了的），不发
    }
    out
}

fn quoted_content(content: &str, quote: Option<&str>) -> String {
    match quote.map(str::trim).filter(|quote| !quote.is_empty()) {
        Some(quote) => {
            let mut out = String::new();
            for line in quote.lines() {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
            out.push('\n');
            out.push_str(content);
            out
        }
        None => content.to_string(),
    }
}

impl ChatMessage {
    fn blank(role: &str, content: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: role.to_string(),
            content,
            reasoning_content: None,
            created_at: Local::now().format("%Y-%m-%d %H:%M").to_string(),
            prompt_tokens: 0,
            completion_tokens: 0,
            speed_tps: 0.0,
            latency_ms: 0,
            is_streaming: false,
            error: None,
            tool_calls: Vec::new(),
            model: String::new(),
            quote: None,
            variants: Vec::new(),
            attachments: Vec::new(),
            called_tools: Vec::new(),
            tool_call_id: String::new(),
            tool_name: String::new(),
            tool_is_error: false,
            local_only: false,
        }
    }

    pub fn new_user(content: String) -> Self {
        Self::blank("user", content)
    }

    pub fn new_assistant() -> Self {
        Self {
            is_streaming: true,
            ..Self::blank("assistant", String::new())
        }
    }

    /// 一条工具执行结果。界面把它渲染成独立的消息块，回传时序列化成
    /// OpenAI 的 `tool` 角色 / Claude 的 `tool_result` 块 / Gemini 的 `functionResponse`。
    pub fn new_tool(result: &ToolResult) -> Self {
        Self {
            tool_name: result.name.clone(),
            tool_call_id: result.id.clone(),
            tool_is_error: result.is_error,
            ..Self::blank("tool", result.content.clone())
        }
    }

    /// 工具开始执行时先插进会话的占位块（`is_streaming` 表示「执行中」），
    /// 执行完在原地回填结果。先插再填，界面马上就能看到「正在执行」，
    /// 结果的位置也不会因为执行期间别的消息插进来而错乱。
    ///
    /// `call_id` 为空表示用户手打的命令（见 [`api_message`] 对它的处理）。
    pub fn tool_placeholder(call_id: &str, name: &str, local_only: bool) -> Self {
        Self {
            tool_name: name.to_string(),
            tool_call_id: call_id.to_string(),
            is_streaming: true,
            local_only,
            ..Self::blank("tool", String::new())
        }
    }
}

/// 新建会话的默认标题。
///
/// **这是要写进数据的值，不是界面文案**，所以不参与 i18n：换界面语言不该改动
/// 已有数据，也不该让同一个会话在两种语言下有两个名字。
pub const DEFAULT_SESSION_TITLE: &str = "新对话";

/// 新建会话的默认文件夹名。理由同 [`DEFAULT_SESSION_TITLE`]。
pub const DEFAULT_SESSION_FOLDER: &str = "默认";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    pub title: String,
    pub folder: String,
    pub model: String,
    #[serde(default)]
    pub provider_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub title_auto: bool,
    #[serde(default)]
    pub params: Option<ChatParams>,
}

impl ChatSession {
    pub fn new(title: String, folder: String, model: String, provider_id: String) -> Self {
        let now = Local::now().format("%Y-%m-%d %H:%M").to_string();
        Self {
            id: Uuid::new_v4().to_string(),
            title,
            folder,
            model,
            provider_id,
            created_at: now.clone(),
            updated_at: now,
            messages: Vec::new(),
            pinned: false,
            favorite: false,
            title_auto: true,
            params: None,
        }
    }

    pub fn resolved_params(&self, global_prompt: &str, global_temperature: f32) -> ResolvedParams {
        let params = self.params.clone().unwrap_or_default();
        ResolvedParams {
            system_prompt: params
                .system_prompt
                .filter(|text| !text.trim().is_empty())
                .unwrap_or_else(|| global_prompt.to_string()),
            temperature: params.temperature.unwrap_or(global_temperature),
            top_p: params.top_p,
            max_tokens: params.max_tokens.filter(|value| *value > 0),
            context_limit: params.context_limit.filter(|value| *value > 0),
            reasoning: params.reasoning,
            stream: params.stream,
        }
    }

    /// 挑出要发给模型的消息，从旧到新。
    ///
    /// 返回的是 [`ChatMessageReq`] 而不是 `(role, content, attachments)` 三元组：
    /// Agent 循环引入工具之后，"助手要求调用工具"和"工具执行结果"这两类消息
    /// 也需要发出去，它们各自还带调用 id 和参数，三元组装不下。
    ///
    /// **返回值一定是渠道能接受的**：工具调用和结果在最后一步重新配对（见
    /// [`pair_tool_results`]）。会话历史可能被各种方式打断——点了停止、撞到轮数上限、
    /// 程序中途关闭、上下文条数截断——但不能因此让这个对话从此再也发不出请求。
    pub fn api_turns(&self, limit: Option<usize>) -> Vec<ChatMessageReq> {
        let mut items: Vec<ChatMessageReq> = self.messages.iter().filter_map(api_message).collect();
        if let Some(limit) = limit.filter(|value| *value > 0)
            && items.len() > limit
        {
            items = items.split_off(items.len() - limit);
            // 截断的位置可能落在一组工具调用中间。从截断后的第一条用户消息开始，
            // 既不会留下半截的调用，也满足「第一条必须是用户消息」的渠道（Claude）。
            // 一条用户消息都没有时保持原样，由下面的配对兜底。
            if let Some(first_user) = items.iter().position(|item| item.role == "user") {
                items.drain(..first_user);
            }
        }
        pair_tool_results(items)
    }

    pub fn has_unresolved_compare(&self) -> bool {
        self.messages
            .iter()
            .any(|message| !message.variants.is_empty() && message.content.is_empty())
    }
}

pub struct StorageData {
    pub active_session_id: String,
    pub sessions: Vec<ChatSession>,
    database: RefCell<Database>,
    revision: Cell<u64>,
}

#[derive(Deserialize)]
struct LegacyStorageData {
    active_session_id: String,
    sessions: Vec<ChatSession>,
}

impl StorageData {
    pub fn load_or_init() -> Self {
        Self::open(&data_dir().join(DATABASE_FILE), &data_file(SESSIONS_FILE))
            .unwrap_or_else(|error| panic!("Unable to open chat storage: {error}"))
    }

    fn open(database_path: &Path, legacy_path: &Path) -> StorageResult<Self> {
        let mut database = Database::open(database_path)?;
        let (mut active_session_id, mut sessions) = database.load()?;

        if sessions.is_empty() && legacy_path.exists() {
            let content = fs::read_to_string(legacy_path)?;
            let legacy: LegacyStorageData = serde_json::from_str(&content)?;
            if !legacy.sessions.is_empty() {
                active_session_id = legacy.active_session_id;
                sessions = legacy.sessions;
            }
        }

        if sessions.is_empty() {
            let default_session = ChatSession::new(
                DEFAULT_SESSION_TITLE.to_string(),
                DEFAULT_SESSION_FOLDER.to_string(),
                "deepseek-chat".to_string(),
                String::new(),
            );
            active_session_id = default_session.id.clone();
            sessions.push(default_session);
        }
        if !sessions.iter().any(|session| session.id == active_session_id) {
            active_session_id = sessions[0].id.clone();
        }
        for session in &mut sessions {
            for message in &mut session.messages {
                // 上次关程序时还在执行的工具：没有结果，写明原因，
                // 不然界面上是一个空块，模型下一轮看到的也是一个空结果
                if message.role == "tool" && message.is_streaming && message.content.is_empty() {
                    message.content = "Interrupted: the app was closed before this finished.".to_string();
                    message.tool_is_error = true;
                }
                message.is_streaming = false;
                for variant in &mut message.variants {
                    variant.is_streaming = false;
                }
            }
        }
        let data = StorageData {
            active_session_id,
            sessions,
            database: RefCell::new(database),
            revision: Cell::new(0),
        };
        data.save()?;
        Ok(data)
    }

    pub fn save(&self) -> StorageResult<()> {
        self.database
            .borrow_mut()
            .save(&self.active_session_id, &self.sessions)?;
        self.revision.set(self.revision.get().wrapping_add(1));
        Ok(())
    }

    pub fn revision(&self) -> u64 {
        self.revision.get()
    }

    pub fn search_session_ids(&self, query: &str) -> StorageResult<HashSet<String>> {
        self.database.borrow().search_session_ids(query)
    }

    pub fn get_active_session(&self) -> Option<&ChatSession> {
        self.sessions
            .iter()
            .find(|s| s.id == self.active_session_id)
            .or_else(|| self.sessions.first())
    }

    pub fn get_active_session_mut(&mut self) -> Option<&mut ChatSession> {
        let target_id = self.active_session_id.clone();
        if let Some(pos) = self.sessions.iter().position(|s| s.id == target_id) {
            return Some(&mut self.sessions[pos]);
        }
        self.sessions.first_mut()
    }

    pub fn create_session(&mut self, title: &str, folder: &str, model: &str, provider_id: &str) -> String {
        let session = ChatSession::new(
            title.to_string(),
            folder.to_string(),
            model.to_string(),
            provider_id.to_string(),
        );
        let id = session.id.clone();
        self.sessions.insert(0, session);
        self.active_session_id = id.clone();
        id
    }

    pub fn delete_session(&mut self, id: &str) {
        self.sessions.retain(|s| s.id != id);
        if self.sessions.is_empty() {
            let session = ChatSession::new(
                DEFAULT_SESSION_TITLE.to_string(),
                DEFAULT_SESSION_FOLDER.to_string(),
                "deepseek-chat".to_string(),
                String::new(),
            );
            self.active_session_id = session.id.clone();
            self.sessions.push(session);
        } else if self.active_session_id == id {
            self.active_session_id = self.sessions[0].id.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn plain_messages_serialize_without_tool_fields() {
        // 没有工具调用的消息，落盘内容不该因为 P3-2 多出任何字段——
        // 否则所有老会话的存档都会被改写一遍。`skip_serializing_if` 就是为这个加的。
        let plain = ChatMessage::new_user("你好".into());
        let json = serde_json::to_value(&plain).unwrap();
        let object = json.as_object().unwrap();
        for field in ["called_tools", "tool_call_id", "tool_name", "tool_is_error"] {
            assert!(!object.contains_key(field), "{field} 不该出现在普通消息里");
        }
    }

    #[test]
    fn tool_messages_round_trip_through_a_session() {
        let call = ToolCall {
            id: "call_1".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({"path": "a.rs"}),
        };
        let result = ToolResult {
            id: "call_1".into(),
            name: "read_file".into(),
            content: "fn main() {}".into(),
            is_error: false,
        };

        let mut assistant = ChatMessage::new_assistant();
        assistant.is_streaming = false;
        assistant.called_tools.push(call.clone());
        let tool = ChatMessage::new_tool(&result);
        assert_eq!(tool.role, "tool");
        assert_eq!(tool.tool_call_id, "call_1");
        assert_eq!(tool.tool_name, "read_file");

        // 序列化再读回来，结构化调用要完好——这是下一轮请求的输入
        let json = serde_json::to_string(&vec![assistant, tool]).unwrap();
        let back: Vec<ChatMessage> = serde_json::from_str(&json).unwrap();
        assert_eq!(back[0].called_tools, vec![call]);
        assert_eq!(back[1].content, "fn main() {}");
        assert!(!back[1].tool_is_error);
    }

    #[test]
    fn api_turns_carry_tool_calls_and_results() {
        let mut session = ChatSession::new("t".into(), DEFAULT_SESSION_FOLDER.into(), "m".into(), String::new());
        session.messages.push(ChatMessage::new_user("看看 a.rs".into()));

        let call = ToolCall {
            id: "call_1".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({"path": "a.rs"}),
        };
        let result = ToolResult {
            id: "call_1".into(),
            name: "read_file".into(),
            content: "fn main() {}".into(),
            is_error: false,
        };
        let mut assistant = ChatMessage::new_assistant();
        assistant.is_streaming = false;
        assistant.called_tools.push(call.clone());
        session.messages.push(assistant);
        session.messages.push(ChatMessage::new_tool(&result));

        let turns = session.api_turns(None);
        let roles: Vec<&str> = turns.iter().map(|turn| turn.role.as_str()).collect();
        assert_eq!(roles, vec!["user", "assistant", "tool"]);

        // 助手的调用要带上完整参数，不能只有标签——否则模型看到自己发过调用却不知道参数
        assert_eq!(turns[1].tool_calls.len(), 1);
        assert_eq!(turns[1].tool_calls[0].arguments["path"], "a.rs");
        // 工具结果的归属信息必须在，各渠道靠它配对
        assert_eq!(turns[2].tool_call_id, "call_1");
        assert_eq!(turns[2].tool_name, "read_file");
    }

    #[test]
    fn api_turns_keeps_an_empty_tool_result() {
        // 写入成功这类结果正文可能是空的，但绝不能丢——丢了模型会以为工具没跑，
        // 然后反复重发同一个调用。
        let mut session = session_with(vec![
            ChatMessage::new_user("写个文件".into()),
            assistant_calling(&[("call_9", "write_file")]),
            tool_message("call_9", "write_file", ""),
        ]);
        session.messages.push(ChatMessage::new_user("好了吗".into()));
        let turns = session.api_turns(None);
        assert_pairs_are_valid(&turns);
        assert_eq!(turns[2].role, "tool");
        assert_eq!(turns[2].tool_call_id, "call_9");
        assert_eq!(turns[2].content, "");
    }

    fn call(id: &str, name: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: name.into(),
            arguments: serde_json::json!({}),
        }
    }

    fn assistant_calling(calls: &[(&str, &str)]) -> ChatMessage {
        let mut message = ChatMessage::new_assistant();
        message.is_streaming = false;
        message.called_tools = calls.iter().map(|(id, name)| call(id, name)).collect();
        message
    }

    fn tool_message(id: &str, name: &str, content: &str) -> ChatMessage {
        ChatMessage::new_tool(&ToolResult {
            id: id.into(),
            name: name.into(),
            content: content.into(),
            is_error: false,
        })
    }

    fn assistant_text(text: &str) -> ChatMessage {
        let mut message = ChatMessage::new_assistant();
        message.is_streaming = false;
        message.content = text.into();
        message
    }

    fn session_with(messages: Vec<ChatMessage>) -> ChatSession {
        let mut session = ChatSession::new("t".into(), DEFAULT_SESSION_FOLDER.into(), "m".into(), String::new());
        session.messages = messages;
        session
    }

    /// 和 OpenAI 的校验规则一致：带调用的助手消息后面必须紧跟每个调用的结果，
    /// 每个结果都必须能在紧挨着的前一条助手消息里找到调用。
    fn assert_pairs_are_valid(turns: &[ChatMessageReq]) {
        let mut expected: Vec<String> = Vec::new();
        for (ix, turn) in turns.iter().enumerate() {
            if turn.role == "tool" {
                let position = expected.iter().position(|id| *id == turn.tool_call_id);
                assert!(position.is_some(), "第 {ix} 条是孤立的工具结果：{turns:#?}");
                if let Some(position) = position {
                    expected.remove(position);
                }
                continue;
            }
            assert!(
                expected.is_empty(),
                "第 {ix} 条之前还有调用没有结果 {expected:?}：{turns:#?}"
            );
            expected = turn.tool_calls.iter().map(|call| call.id.clone()).collect();
        }
        assert!(expected.is_empty(), "最后还有调用没有结果 {expected:?}");
    }

    #[test]
    fn manual_command_output_is_sent_as_plain_text() {
        // 手打的 /ls 没有对应的模型调用。以 tool 角色发出去会被严格的接口拒绝，
        // 而且之后这个对话的每一次请求都会带着它，全部失败。
        let session = session_with(vec![
            ChatMessage::new_user("/ls".into()),
            tool_message("", "list_directory", "a.rs\nb.rs"),
            ChatMessage::new_user("哪个文件最大".into()),
        ]);
        let turns = session.api_turns(None);
        assert_pairs_are_valid(&turns);
        assert!(turns.iter().all(|turn| turn.role != "tool"));
        assert_eq!(turns[1].role, "assistant");
        assert!(turns[1].content.contains("list_directory"));
        assert!(
            turns[1].content.contains("a.rs\nb.rs"),
            "输出要原样带上：{}",
            turns[1].content
        );
    }

    #[test]
    fn local_only_messages_are_never_sent() {
        let mut command = ChatMessage::new_user("/read .env".into());
        command.local_only = true;
        let mut output = tool_message("", "read_file", "SECRET=abc");
        output.local_only = true;
        let session = session_with(vec![command, output, ChatMessage::new_user("你好".into())]);
        let turns = session.api_turns(None);
        assert_eq!(turns.len(), 1);
        assert!(turns.iter().all(|turn| !turn.content.contains("SECRET")));
    }

    #[test]
    fn unanswered_calls_get_a_not_executed_result() {
        // 模型一次要了两个调用，第一个还没执行就被打断（停止、轮数上限、关程序），
        // 用户接着发了新消息：请求里必须给那个调用补一条结果，而且按调用的顺序排。
        let session = session_with(vec![
            ChatMessage::new_user("看看".into()),
            assistant_calling(&[("call_1", "read_file"), ("call_2", "list_directory")]),
            tool_message("call_2", "list_directory", "a.rs"),
            ChatMessage::new_user("算了，换个问题".into()),
        ]);
        let turns = session.api_turns(None);
        assert_pairs_are_valid(&turns);
        assert_eq!(turns[2].tool_call_id, "call_1");
        assert!(turns[2].content.starts_with("Not executed"));
        assert_eq!(turns[3].tool_call_id, "call_2");
        assert_eq!(turns[3].content, "a.rs");
    }

    #[test]
    fn unanswered_calls_at_the_end_are_filled_too() {
        let session = session_with(vec![
            ChatMessage::new_user("看看".into()),
            assistant_calling(&[("call_1", "read_file")]),
        ]);
        let turns = session.api_turns(None);
        assert_pairs_are_valid(&turns);
        assert_eq!(turns.len(), 3);
    }

    #[test]
    fn running_tools_are_not_sent_but_their_calls_stay_paired() {
        let session = session_with(vec![
            ChatMessage::new_user("跑一下".into()),
            assistant_calling(&[("call_1", "run_command")]),
            ChatMessage::tool_placeholder("call_1", "run_command", false),
        ]);
        let turns = session.api_turns(None);
        assert_pairs_are_valid(&turns);
        assert_eq!(turns.len(), 3);
    }

    #[test]
    fn orphan_tool_results_are_dropped() {
        // 调用那条被删掉了，只剩下结果
        let session = session_with(vec![
            ChatMessage::new_user("看看".into()),
            tool_message("call_1", "read_file", "fn main() {}"),
            assistant_text("看完了"),
        ]);
        let turns = session.api_turns(None);
        assert_pairs_are_valid(&turns);
        assert_eq!(turns.len(), 2);
    }

    #[test]
    fn context_limit_never_cuts_through_a_tool_exchange() {
        let session = session_with(vec![
            ChatMessage::new_user("第一问".into()),
            assistant_calling(&[("call_1", "read_file")]),
            tool_message("call_1", "read_file", "内容"),
            assistant_text("第一答"),
            ChatMessage::new_user("第二问".into()),
            assistant_calling(&[("call_2", "read_file")]),
            tool_message("call_2", "read_file", "内容"),
            assistant_text("第二答"),
        ]);
        // 截最后 6 条会从 call_1 的结果开始——必须退到下一条用户消息
        let turns = session.api_turns(Some(6));
        assert_pairs_are_valid(&turns);
        assert_eq!(turns[0].role, "user");
        assert_eq!(turns[0].content, "第二问");
    }

    #[test]
    fn tools_still_running_when_the_app_closed_are_marked_on_load() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("chats.db");
        let legacy_path = dir.path().join("sessions.json");
        let mut session = ChatSession::new("t".into(), "默认".into(), "m".into(), String::new());
        session.messages.push(ChatMessage::new_user("跑一下".into()));
        session.messages.push(assistant_calling(&[("call_1", "run_command")]));
        session
            .messages
            .push(ChatMessage::tool_placeholder("call_1", "run_command", false));
        let json = serde_json::json!({ "active_session_id": session.id, "sessions": [session] }).to_string();
        fs::write(&legacy_path, json).unwrap();

        let data = StorageData::open(&db_path, &legacy_path).unwrap();
        let tool = &data.sessions[0].messages[2];
        assert!(!tool.is_streaming);
        assert!(tool.tool_is_error);
        assert!(tool.content.starts_with("Interrupted"));
    }

    #[test]
    fn imports_legacy_json_without_modifying_it() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("chats.db");
        let legacy_path = dir.path().join("sessions.json");
        let mut session = ChatSession::new("旧对话".into(), "默认".into(), "old-model".into(), String::new());
        session.messages.push(ChatMessage::new_user("旧消息".into()));
        let original = serde_json::json!({
            "active_session_id": session.id,
            "sessions": [session]
        })
        .to_string();
        fs::write(&legacy_path, &original).unwrap();

        let data = StorageData::open(&db_path, &legacy_path).unwrap();
        assert_eq!(data.sessions[0].messages[0].content, "旧消息");
        assert_eq!(fs::read_to_string(&legacy_path).unwrap(), original);
        drop(data);

        let data = StorageData::open(&db_path, &legacy_path).unwrap();
        assert_eq!(data.sessions.len(), 1);
        assert_eq!(data.sessions[0].title, "旧对话");
    }

    #[test]
    fn persists_message_edits_and_session_deletion() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("chats.db");
        let legacy_path = dir.path().join("missing.json");
        let mut data = StorageData::open(&db_path, &legacy_path).unwrap();
        let first_id = data.active_session_id.clone();
        data.sessions[0].messages.push(ChatMessage::new_user("before".into()));
        data.save().unwrap();
        data.sessions[0].messages[0].content = "after".into();
        data.save().unwrap();
        data.create_session("Second", "默认", "model", "provider");
        data.delete_session(&first_id);
        data.save().unwrap();
        drop(data);

        let data = StorageData::open(&db_path, &legacy_path).unwrap();
        assert_eq!(data.sessions.len(), 1);
        assert_eq!(data.sessions[0].title, "Second");
        assert_eq!(data.sessions[0].provider_id, "provider");
        let count: i64 = rusqlite::Connection::open(&db_path)
            .unwrap()
            .query_row("SELECT count(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn clears_interrupted_stream_state_on_restart() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("chats.db");
        let legacy_path = dir.path().join("missing.json");
        let mut data = StorageData::open(&db_path, &legacy_path).unwrap();
        data.sessions[0].messages.push(ChatMessage::new_assistant());
        data.save().unwrap();
        drop(data);

        let data = StorageData::open(&db_path, &legacy_path).unwrap();
        assert!(!data.sessions[0].messages[0].is_streaming);
    }

    #[test]
    fn searches_titles_and_message_content_as_literal_text() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("chats.db");
        let legacy_path = dir.path().join("missing.json");
        let mut data = StorageData::open(&db_path, &legacy_path).unwrap();
        let first_id = data.active_session_id.clone();
        data.sessions[0]
            .messages
            .push(ChatMessage::new_user("find %_ literally".into()));
        let second_id = data.create_session("Other title", "默认", "model", "provider");
        data.save().unwrap();

        assert_eq!(data.search_session_ids("%_").unwrap(), HashSet::from([first_id]));
        assert_eq!(data.search_session_ids("Other").unwrap(), HashSet::from([second_id]));
        assert!(data.search_session_ids("missing").unwrap().is_empty());
    }

    #[test]
    fn rejects_invalid_legacy_data_without_creating_a_default_session() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("chats.db");
        let legacy_path = dir.path().join("sessions.json");
        fs::write(&legacy_path, "not JSON").unwrap();

        assert!(StorageData::open(&db_path, &legacy_path).is_err());
        let count: i64 = rusqlite::Connection::open(&db_path)
            .unwrap()
            .query_row("SELECT count(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
