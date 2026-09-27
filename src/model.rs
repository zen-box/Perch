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

/// 会话的工作模式。
///
/// **核心区别是「能不能碰本机文件」，不是「带不带工具」**：对话也能用 MCP 与 Skills，
/// 只是碰不到你的硬盘。这个区分必须落在代码里（见 [`SessionTools::effective_sources`]），
/// 不能只靠界面不显示那个勾选框——界面漏一处，承诺就成了空话。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    /// 问答、写作、翻译、查资料。能用 MCP 与 Skills，**不给本机工具**。
    #[default]
    Chat,
    /// 让它动手改东西：对话的全部 + 读写本机文件 + 执行命令。
    Agent,
}

/// 一个工具来源。
///
/// **选择器按这个粒度勾，不按单个工具勾**：勾一台 MCP 服务器就是把它的全部工具
/// 交给模型，具体用哪个功能由模型按用户的问题自己挑——那本来就是模型该干的活。
/// 让用户先搞清 fetch 有哪几个功能再逐个决定，对普通人是门槛。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolSource {
    /// 本机文件与命令。**只有智能体模式才生效**。
    Local,
    /// 某一台 MCP 服务器（勾一台 = 整台都带）。
    Mcp { server_id: String },
    /// 用户装进数据目录的 Skills。
    Skill,
}

/// 智能体操作本机时的权限档。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// 只读本机文件免确认；写文件 / 跑命令 / 读敏感文件每次弹卡片。
    #[default]
    Default,
    /// 本机工具全部直接执行，不弹卡片。仍然保留两条硬底线（见 AGENTS.md §11）。
    Full,
}

/// 老格式（只有 `enabled` + `picked`）读进来时的暂存。
///
/// 老数据里 `picked: None` 的含义是**「当时能用的全带」**，而新模型的默认值是
/// 「智能体只带本机」——两者不是一个意思。直接按新语义读，老用户的 MCP 工具会
/// 无声消失，所以先把老字段原样接下来，等 `AppConfig` 加载完（那时才知道配置里
/// 有哪些服务器）再由应用层展开，见 `tool_ops::migrate_legacy_tool_state`。
#[derive(Clone, Debug, PartialEq)]
pub enum LegacyTools {
    /// 老数据 `picked: null`：当时能用的全带。
    All,
    /// 老数据 `picked: [...]`：用户显式勾过的这批工具名。
    Picked(Vec<String>),
}

/// 一个会话要用哪些工具、从哪些来源要、本机权限多大。
///
/// **这份状态是唯一的**：composer 上那个「对话 / 智能体」开关只是它的快捷表达。
/// 分成两份状态迟早会出现「模式说对话、清单却还在发」这种自相矛盾。
///
/// ⚠️ 新会话和旧数据都是 `ChatSession::tools == None`，按 **对话** 处理（一个工具都不带）。
/// 工具调用是有副作用的操作，默认不开比默认开安全；要用的用户在输入框上切一下就行。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "SessionToolsRaw")]
pub struct SessionTools {
    /// 模式。对话不给本机工具，智能体给。
    #[serde(default)]
    pub mode: SessionMode,
    /// 勾上的工具来源。`None` = 没动过选择器，按模式的默认值算。
    ///
    /// 存**来源**不存工具名：勾一台服务器就是一条记录，服务器换了工具清单也不用改存档。
    /// 某台服务器被删了，对应的那条记录在组装清单时自然匹配不上，不用做迁移。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<ToolSource>>,
    /// 智能体的本机权限档。对话模式下无意义。
    #[serde(default)]
    pub permission: Permission,
    /// 智能体的项目目录（绝对路径）。相对路径按它算，见 `local_tools.rs`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// 单独停用的工具（本机工具名 / MCP 暴露名）。
    ///
    /// 来源勾上了、但用户点名不要的那些。存在这里而不是只靠"不勾来源"，
    /// 是为了保住「整台服务器都带，但这个别用」这种常见诉求。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_tools: Vec<String>,
    /// 迁移暂存，**不写盘**。见 [`LegacyTools`]。
    #[serde(skip_serializing)]
    pub(crate) legacy: Option<LegacyTools>,
}

/// 反序列化用的中转结构。
///
/// 同时认两套字段：有 `mode` 的是新格式，只有 `enabled` / `picked` 的是老格式。
/// 用一个 `from` 中转而不是给每个字段写 `deserialize_with`，是因为老格式的
/// **字段组合**（而不是单个字段）决定了新值，写在 `From` 里一眼能看全。
#[derive(Deserialize)]
struct SessionToolsRaw {
    #[serde(default)]
    mode: Option<SessionMode>,
    #[serde(default)]
    sources: Option<Vec<ToolSource>>,
    #[serde(default)]
    permission: Permission,
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    disabled_tools: Vec<String>,
    /// 老格式：是不是智能体模式。
    #[serde(default)]
    enabled: Option<bool>,
    /// 老格式：勾过的工具名。`None` 表示"没动过选择器"，当时的语义是**全带**。
    #[serde(default)]
    picked: Option<Vec<String>>,
}

impl From<SessionToolsRaw> for SessionTools {
    fn from(raw: SessionToolsRaw) -> Self {
        // 有 `mode` 就是新格式，直接用
        if let Some(mode) = raw.mode {
            return Self {
                mode,
                sources: raw.sources,
                permission: raw.permission,
                workspace: raw.workspace,
                disabled_tools: raw.disabled_tools,
                legacy: None,
            };
        }
        // 老格式。来源这一项留空，由 `tool_ops::migrate_legacy_tool_state` 填——
        // 反序列化的时候拿不到配置里的服务器清单，没法在这里展开。
        let (mode, legacy) = match (raw.enabled.unwrap_or(false), raw.picked) {
            (true, None) => (SessionMode::Agent, Some(LegacyTools::All)),
            (true, Some(names)) => (SessionMode::Agent, Some(LegacyTools::Picked(names))),
            (false, None) => (SessionMode::Chat, None),
            (false, Some(names)) => (SessionMode::Chat, Some(LegacyTools::Picked(names))),
        };
        Self {
            mode,
            sources: None,
            permission: Permission::Default,
            workspace: None,
            disabled_tools: Vec::new(),
            legacy,
        }
    }
}

impl SessionTools {
    /// 这个会话是不是智能体模式（能碰本机文件）。
    ///
    /// 注意 `sources == None` 的含义**跟着模式走**：对话是「什么都不带」，
    /// 智能体是「只带本机工具」。所以切模式只要改 `mode` 就够了，
    /// 不需要另外造一份默认值——默认值本身是模式相关的。
    pub fn is_agent(&self) -> bool {
        self.mode == SessionMode::Agent
    }

    /// 存档里那份来源（`None` 时按模式默认值补上）。
    ///
    /// **改选择器要用这一份，不要用 [`Self::effective_sources`]**：后者在对话模式下
    /// 会把 `Local` 剔掉，拿它去回写就等于「在对话模式下点一次全不选，把本机那条
    /// 一起抹了」，切回智能体时凭空少一项。
    pub fn stored_sources(&self) -> Vec<ToolSource> {
        match &self.sources {
            Some(list) => list.clone(),
            None if self.is_agent() => vec![ToolSource::Local],
            None => Vec::new(),
        }
    }

    /// 真正生效的工具来源。
    ///
    /// ⚠️ **对话模式一定会把 `Local` 剔掉**，哪怕存档里塞了它（比如用户先勾了本机、
    /// 再切回对话）。「切回对话就碰不到硬盘」是给用户的承诺，得是**结构性**的，
    /// 不能指望界面记得把那个勾选框藏起来。
    pub fn effective_sources(&self) -> Vec<ToolSource> {
        let mut sources = self.stored_sources();
        if !self.is_agent() {
            sources.retain(|source| !matches!(source, ToolSource::Local));
        }
        sources
    }

    /// 这个来源这次要不要带。
    pub fn wants_source(&self, source: &ToolSource) -> bool {
        if matches!(source, ToolSource::Local) && !self.is_agent() {
            return false;
        }
        self.effective_sources().iter().any(|item| item == source)
    }

    /// 这个工具被单独停用了吗（来源勾了、但用户点名不要它）。
    pub fn is_disabled(&self, name: &str) -> bool {
        self.disabled_tools.iter().any(|item| item == name)
    }

    /// 取出迁移暂存。取走之后就是 `None`，不会再展开第二遍。
    pub(crate) fn take_legacy(&mut self) -> Option<LegacyTools> {
        self.legacy.take()
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
    /// 这条工具结果执行了多久（毫秒）。`0` 表示没测到（旧数据、没执行）。
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tool_duration_ms: u64,
    /// 这条工具结果的进程退出码。文件操作、没执行过的调用都是 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_exit_code: Option<i32>,
    /// 只在本机显示、**永远不发给模型**的消息。
    ///
    /// 目前用在手打 `/read` 读敏感文件（`.env`、私钥之类）：用户点了同意是想自己看一眼，
    /// 不代表同意把内容交给模型服务商。命令那一条和结果那一条都会标上。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local_only: bool,
}

/// `skip_serializing_if` 用：0 表示「没测到」，不写进数据。
fn is_zero(value: &u64) -> bool {
    *value == 0
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
            // 耗时和退出码只给界面看，`tool_result` 不读它们
            duration_ms: 0,
            exit_code: None,
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
                    // 这条是补出来的占位结果，没有真的执行过
                    duration_ms: 0,
                    exit_code: None,
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
            tool_duration_ms: 0,
            tool_exit_code: None,
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
            tool_duration_ms: result.duration_ms,
            tool_exit_code: result.exit_code,
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
    /// 这个会话要用哪些工具。`None`（新会话、旧数据）= **chat 模式**，一个工具都不带。
    /// 见 [`SessionTools`]。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<SessionTools>,
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
            tools: None,
        }
    }

    /// 这个会话现在是不是智能体模式（可以碰本机文件）。
    ///
    /// 这只是「第一道闸」：全局的「允许智能体读写本机文件」总开关和模型的
    /// `Capability::Tools` 各自还有一道，三处都过了才真的带工具，
    /// 见 `reply_ops::tool_list_for`。
    pub fn is_agent(&self) -> bool {
        self.tools.as_ref().is_some_and(SessionTools::is_agent)
    }

    /// 这个会话生效的工具来源。
    ///
    /// 没设过（`None`）＝ 对话模式，什么都不带。**对话模式下 `Local` 一定不在里面**，
    /// 见 [`SessionTools::effective_sources`]。
    pub fn tool_sources(&self) -> Vec<ToolSource> {
        self.tools
            .as_ref()
            .map(SessionTools::effective_sources)
            .unwrap_or_default()
    }

    /// 这个工具被这个会话单独停用了吗。
    pub fn is_tool_disabled(&self, name: &str) -> bool {
        self.tools.as_ref().is_some_and(|tools| tools.is_disabled(name))
    }

    /// 这个会话的项目目录——智能体干活的地方，也是「边界」的基准。
    ///
    /// 没设、或者存的是相对路径（基准本身就不确定，等于没有边界）时返回 `None`，
    /// 那种情况下**本机工具一个都不会交给模型**（见 `tool_ops::session_tool_specs`）。
    pub fn workspace(&self) -> Option<crate::local_tools::ProjectDir> {
        self.tools
            .as_ref()
            .and_then(|tools| tools.workspace.as_deref())
            .and_then(crate::local_tools::ProjectDir::parse)
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
    /// 打开会话库。失败时把原因**交给调用方**，不在这里 panic——
    /// 启动阶段要拿它渲染错误页，panic 掉就没有界面能显示原因了。
    pub fn try_load_or_init() -> Result<Self, String> {
        Self::open(&data_dir().join(DATABASE_FILE), &data_file(SESSIONS_FILE)).map_err(|error| error.to_string())
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

    /// 智能体的默认工具状态：没动过选择器 = 只带本机工具、不带任何 MCP。
    ///
    /// 生产代码里没有这个构造器——模式默认值是靠 `sources == None` 表达的，
    /// 写在这里只是让测试读起来短一点。
    fn agent_tools() -> SessionTools {
        SessionTools {
            mode: SessionMode::Agent,
            ..Default::default()
        }
    }

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
            ..Default::default()
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
            ..Default::default()
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

    #[test]
    fn tool_messages_keep_their_timing() {
        // 耗时和退出码要跟着消息走，界面才显示得出来
        let result = ToolResult {
            id: "call_1".into(),
            name: "run_command".into(),
            content: "ok".into(),
            is_error: false,
            duration_ms: 1234,
            exit_code: Some(0),
        };
        let message = ChatMessage::new_tool(&result);
        assert_eq!(message.tool_duration_ms, 1234);
        assert_eq!(message.tool_exit_code, Some(0));
    }

    #[test]
    fn tool_placeholders_start_without_timing() {
        // 占位块还没执行，界面不该显示耗时
        let placeholder = ChatMessage::tool_placeholder("call_1", "run_command", false);
        assert_eq!(placeholder.tool_duration_ms, 0);
        assert_eq!(placeholder.tool_exit_code, None);
    }

    #[test]
    fn timing_never_reaches_the_request_body() {
        // 它们是给界面看的：掺进发给模型的内容只会白占上下文，
        // 还会让同一段历史随执行时刻不同而序列化出不同结果（prompt 缓存失效）
        let mut message = tool_message("call_1", "run_command", "ok");
        message.tool_duration_ms = 1234;
        message.tool_exit_code = Some(0);
        let session = session_with(vec![
            ChatMessage::new_user("跑一下".into()),
            assistant_calling(&[("call_1", "run_command")]),
            message,
        ]);
        let json = serde_json::to_string(&session.api_turns(None)).unwrap();
        assert!(!json.contains("1234"), "耗时不该出现在请求体里：{json}");
        assert!(!json.contains("duration"), "请求体里不该有 duration 字段：{json}");
        assert!(!json.contains("exit_code"), "请求体里不该有 exit_code 字段：{json}");
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
            ..Default::default()
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

    #[test]
    fn sessions_without_tool_state_stay_chat() {
        // 新会话（`tools == None`）是对话：一个工具都不带。旧数据反序列化出来也是 `None`。
        let session = ChatSession::new("t".into(), "f".into(), "m".into(), "p".into());
        assert!(!session.is_agent());
        assert!(session.tool_sources().is_empty());

        // 没有工具状态的会话，落盘时不该多出 `tools` 字段——否则所有老存档都会被改写一遍。
        let json = serde_json::to_value(&session).unwrap();
        assert!(!json.as_object().unwrap().contains_key("tools"));
    }

    #[test]
    fn session_tools_round_trip() {
        let mut session = ChatSession::new("t".into(), "f".into(), "m".into(), "p".into());

        // 智能体、没动过选择器：`sources` 是 `None`，序列化时省掉。
        session.tools = Some(agent_tools());
        let json = serde_json::to_string(&session).unwrap();
        assert!(!json.contains("sources"), "没动过选择器就不写 sources");
        let back: ChatSession = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tools, Some(agent_tools()));
        assert!(back.is_agent());
        assert_eq!(back.tool_sources(), vec![ToolSource::Local], "智能体默认只带本机");

        // 显式勾了一台服务器：只有它 + 本机生效。
        session.tools = Some(SessionTools {
            sources: Some(vec![
                ToolSource::Local,
                ToolSource::Mcp {
                    server_id: "fetch".into(),
                },
            ]),
            ..agent_tools()
        });
        let json = serde_json::to_string(&session).unwrap();
        let back: ChatSession = serde_json::from_str(&json).unwrap();
        assert!(back.tools.as_ref().unwrap().wants_source(&ToolSource::Local));
        assert!(back.tools.as_ref().unwrap().wants_source(&ToolSource::Mcp {
            server_id: "fetch".into()
        }));
        assert!(!back.tools.as_ref().unwrap().wants_source(&ToolSource::Mcp {
            server_id: "other".into()
        }));
    }

    #[test]
    fn chat_mode_never_hands_out_local_tools() {
        // 这条是承诺的结构性保证：存档里塞了 `Local`，只要模式是对话就必须被剔掉。
        // 用户可能先勾了本机工具、再切回对话，界面那边漏藏一次勾选框不该变成安全漏洞。
        let tools = SessionTools {
            sources: Some(vec![
                ToolSource::Local,
                ToolSource::Mcp {
                    server_id: "fetch".into(),
                },
            ]),
            ..SessionTools::default()
        };
        assert_eq!(
            tools.effective_sources(),
            vec![ToolSource::Mcp {
                server_id: "fetch".into()
            }],
            "对话模式必须把本机来源剔掉，MCP 留着"
        );
        assert!(!tools.wants_source(&ToolSource::Local));

        // 反过来，智能体模式照样认它
        let agent = SessionTools {
            mode: SessionMode::Agent,
            ..tools
        };
        assert!(agent.wants_source(&ToolSource::Local));
    }

    #[test]
    fn legacy_tool_state_is_recognized_but_not_expanded_here() {
        // 老格式：`enabled` + `picked`。展开要等配置加载完（那时才知道有哪些服务器），
        // 所以这里只该留下一个「待展开」的标记，不能自己猜出一份来源表。
        let raw = r#"{"enabled":true,"picked":null}"#;
        let tools: SessionTools = serde_json::from_str(raw).unwrap();
        assert!(tools.is_agent());
        assert_eq!(tools.sources, None);
        assert_eq!(tools.legacy, Some(LegacyTools::All));

        let raw = r#"{"enabled":true,"picked":["read_file","mcp__fetch__fetch"]}"#;
        let tools: SessionTools = serde_json::from_str(raw).unwrap();
        assert!(tools.is_agent());
        assert_eq!(
            tools.legacy,
            Some(LegacyTools::Picked(vec![
                "read_file".into(),
                "mcp__fetch__fetch".into()
            ]))
        );

        // 老对话（`enabled: false`）没有来源要展开，直接就是空的
        let tools: SessionTools = serde_json::from_str(r#"{"enabled":false}"#).unwrap();
        assert!(!tools.is_agent());
        assert_eq!(tools.legacy, None);
        assert!(tools.effective_sources().is_empty());

        // 新格式认 `mode`，不再看老字段
        let tools: SessionTools = serde_json::from_str(r#"{"mode":"agent","enabled":false,"picked":["x"]}"#).unwrap();
        assert!(tools.is_agent());
        assert_eq!(tools.legacy, None);
    }

    #[test]
    fn the_legacy_marker_is_never_written_back() {
        // 展开标记是纯运行时的东西，写回磁盘就等于把一次性的迁移变成永久的字段。
        let tools: SessionTools = serde_json::from_str(r#"{"enabled":true,"picked":null}"#).unwrap();
        let json = serde_json::to_string(&tools).unwrap();
        assert!(!json.contains("legacy"), "{json}");
        assert!(!json.contains("enabled"), "{json}");
        assert!(!json.contains("picked"), "{json}");
        assert!(json.contains(r#""mode":"agent""#), "{json}");
    }

    #[test]
    fn tool_state_survives_a_sqlite_round_trip() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("chats.db");
        let legacy_path = dir.path().join("missing.json");
        let mut data = StorageData::open(&db_path, &legacy_path).unwrap();
        data.sessions[0].tools = Some(SessionTools {
            sources: Some(vec![
                ToolSource::Local,
                ToolSource::Mcp {
                    server_id: "fetch".into(),
                },
            ]),
            permission: Permission::Full,
            workspace: Some("C:\\work".into()),
            disabled_tools: vec!["run_command".into()],
            ..agent_tools()
        });
        data.save().unwrap();
        drop(data);

        let data = StorageData::open(&db_path, &legacy_path).unwrap();
        let session = &data.sessions[0];
        assert!(session.is_agent());
        assert!(session.is_tool_disabled("run_command"));
        assert!(!session.is_tool_disabled("read_file"));
        let tools = session.tools.as_ref().unwrap();
        assert_eq!(tools.permission, Permission::Full);
        assert_eq!(tools.workspace.as_deref(), Some("C:\\work"));
        assert!(tools.wants_source(&ToolSource::Mcp {
            server_id: "fetch".into()
        }));
    }
}
