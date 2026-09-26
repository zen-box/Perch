use chrono::Local;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use uuid::Uuid;

use crate::i18n::{AppLanguage, Key, tr};
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
    pub role: String, // "user", "assistant", "system"
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
    pub fn new_user(content: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: "user".to_string(),
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
        }
    }

    pub fn new_assistant() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: "assistant".to_string(),
            content: String::new(),
            reasoning_content: None,
            created_at: Local::now().format("%Y-%m-%d %H:%M").to_string(),
            prompt_tokens: 0,
            completion_tokens: 0,
            speed_tps: 0.0,
            latency_ms: 0,
            is_streaming: true,
            error: None,
            tool_calls: Vec::new(),
            model: String::new(),
            quote: None,
            variants: Vec::new(),
            attachments: Vec::new(),
        }
    }
}

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

    pub fn api_turns(&self, limit: Option<usize>) -> Vec<(String, String, Vec<Attachment>)> {
        let mut items: Vec<(String, String, Vec<Attachment>)> = self
            .messages
            .iter()
            .filter_map(|message| {
                if !message.variants.is_empty() && message.content.is_empty() {
                    return None;
                }
                if message.is_streaming && message.content.is_empty() {
                    return None;
                }
                let content = quoted_content(&message.content, message.quote.as_deref());
                if message.role != "user" && content.trim().is_empty() {
                    return None;
                }
                if message.role == "user" && content.trim().is_empty() && message.attachments.is_empty() {
                    return None;
                }
                Some((message.role.clone(), content, message.attachments.clone()))
            })
            .collect();
        if let Some(limit) = limit.filter(|value| *value > 0)
            && items.len() > limit
        {
            items = items.split_off(items.len() - limit);
        }
        items
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
                "新对话".to_string(),
                "默认".to_string(),
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
                "新对话".to_string(),
                "默认".to_string(),
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
