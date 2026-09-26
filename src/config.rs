use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::path::Path;

use crate::model::ReasoningLevel;
use crate::model_info::{self, Capability, ModelSpec};
use crate::paths::{APP_NAME, LEGACY_APP_NAME, data_file, write_atomic};

const CONFIG_FILE: &str = "perch-config.json";

/// 改名前的配置文件名。旧版本还会把它写在程序目录下，启动时顺手清掉里面的明文 API Key。
const LEGACY_CONFIG_FILE: &str = "personal-control-config.json";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ChannelType {
    OpenAiChat,       // 1. OpenAI (Chat Completions 规范)
    OpenAiResponses,  // 2. OpenAI (Responses 规范)
    Gemini,           // 3. Google Gemini 规范
    Claude,           // 4. Anthropic Claude 规范
}

impl ChannelType {
    pub fn all() -> &'static [ChannelType] {
        &[
            ChannelType::OpenAiChat,
            ChannelType::OpenAiResponses,
            ChannelType::Gemini,
            ChannelType::Claude,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            ChannelType::OpenAiChat => "OpenAI (Chat Completions)",
            ChannelType::OpenAiResponses => "OpenAI (Responses 规范)",
            ChannelType::Gemini => "Google Gemini",
            ChannelType::Claude => "Anthropic Claude",
        }
    }

    pub fn default_base_url(&self) -> &'static str {
        match self {
            ChannelType::OpenAiChat => "https://api.openai.com/v1",
            ChannelType::OpenAiResponses => "https://api.openai.com/v1",
            ChannelType::Gemini => "https://generativelanguage.googleapis.com/v1beta",
            ChannelType::Claude => "https://api.anthropic.com/v1",
        }
    }

    pub fn default_api_path(&self) -> &'static str {
        match self {
            ChannelType::OpenAiChat => "/chat/completions",
            ChannelType::OpenAiResponses => "/responses",
            ChannelType::Gemini => "/models/{model}:streamGenerateContent",
            ChannelType::Claude => "/messages",
        }
    }

    #[allow(dead_code)]
    pub fn default_model(&self) -> &'static str {
        match self {
            ChannelType::OpenAiChat => "gpt-4o",
            ChannelType::OpenAiResponses => "gpt-4o",
            ChannelType::Gemini => "gemini-1.5-pro",
            ChannelType::Claude => "claude-3-5-sonnet-20241022",
        }
    }
}

 #[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
 pub struct HeaderPair {
     pub name: String,
     pub value: String,
 }
 
 fn default_timeout_secs() -> u64 {
     90
 }
 
 pub fn parse_header_lines(text: &str) -> Vec<HeaderPair> {
     text.lines()
         .filter_map(|line| {
             let line = line.trim();
             if line.is_empty() || line.starts_with('#') {
                 return None;
             }
             let (name, value) = line.split_once(':')?;
             let name = name.trim();
             if name.is_empty() {
                 return None;
             }
             Some(HeaderPair {
                 name: name.to_string(),
                 value: value.trim().to_string(),
             })
         })
         .collect()
 }
 
 pub fn format_header_lines(headers: &[HeaderPair]) -> String {
     headers
         .iter()
         .map(|header| format!("{}: {}", header.name, header.value))
         .collect::<Vec<_>>()
         .join("\n")
 }
 
fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelConfig {
    pub id: String,
    pub name: String,
    /// 旧版本的自由标签。现在由结构化字段取代，只保留用户自己写的内容
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 以下字段为空时按模型 ID 自动识别，见 `model_info::detect`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<Capability>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_levels: Option<Vec<ReasoningLevel>>,
    /// 对话没有指定思考强度时使用；为空则不发送，由接口决定
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_reasoning: Option<ReasoningLevel>,
    /// 品牌图标，为空时按模型 ID 自动匹配
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

impl ModelConfig {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            tags: String::new(),
            is_pinned: false,
            enabled: true,
            context_window: None,
            max_output: None,
            capabilities: None,
            reasoning_levels: None,
            default_reasoning: None,
            icon: None,
        }
    }

    /// 按模型 ID 识别出的默认规格
    pub fn detected(&self) -> ModelSpec {
        model_info::detect(&self.id, &self.tags)
    }

    pub fn effective_context_window(&self) -> Option<u32> {
        self.context_window.or_else(|| self.detected().context_window)
    }

    pub fn effective_max_output(&self) -> Option<u32> {
        self.max_output.or_else(|| self.detected().max_output)
    }

    pub fn effective_capabilities(&self) -> Vec<Capability> {
        match &self.capabilities {
            Some(capabilities) => capabilities.clone(),
            None => self.detected().capabilities,
        }
    }

    /// 可以选择的思考强度（按从弱到强排序）
    pub fn effective_reasoning_levels(&self) -> Vec<ReasoningLevel> {
        let mut levels = match &self.reasoning_levels {
            Some(levels) => levels.clone(),
            None => self.detected().reasoning_levels,
        };
        levels.sort();
        levels.dedup();
        levels
    }

    pub fn supports_reasoning(&self) -> bool {
        !self.effective_reasoning_levels().is_empty()
    }

    /// 会输出思考过程的模型，包括不能调节强度、总会思考的模型
    pub fn thinks(&self) -> bool {
        self.supports_reasoning() || (self.reasoning_levels.is_none() && self.detected().always_thinks)
    }

    /// 旧版本用标签记录「API」「128K」这类信息，迁移到结构化字段。返回是否有改动。
    pub fn migrate_legacy_tags(&mut self) -> bool {
        let tag = self.tags.trim();
        if tag.is_empty() {
            return false;
        }
        // 「API」是拉取模型时自动加的，「128K」是旧版添加弹窗的预填值，都不代表真实规格
        if tag.eq_ignore_ascii_case("api") || tag.eq_ignore_ascii_case("128k") {
            self.tags.clear();
            return true;
        }
        if let Ok(Some(tokens)) = model_info::parse_tokens(tag) {
            if self.context_window.is_none() {
                self.context_window = Some(tokens);
            }
            self.tags.clear();
            return true;
        }
        false
    }
}

 #[derive(Clone, Debug, Serialize, Deserialize)]
 pub struct ProviderConfig {
     pub id: String,
     pub name: String,
     pub channel_type: ChannelType,
     pub base_url: String,
     pub api_path: String,
     #[serde(default, skip_serializing)]
     pub api_key: String,
     #[serde(default)]
     pub api_key_ref: String,
     pub enabled: bool,
     pub models: Vec<ModelConfig>,
     #[serde(default = "default_timeout_secs")]
     pub timeout_secs: u64,
     #[serde(default)]
     pub retries: u8,
     #[serde(default)]
     pub proxy: String,
     #[serde(default)]
     pub extra_headers: Vec<HeaderPair>,
 }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub active_provider_id: String,
    pub model: String,
    pub temperature: f32,
    pub system_prompt: String,
    pub is_dark: bool,
    pub language: String,
    #[serde(default)]
    pub local_tools_enabled: bool,
    pub providers: Vec<ProviderConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        // 用户自行创建渠道，默认不内置写死渠道，保持纯净
        Self {
            active_provider_id: String::new(),
            model: "default".to_string(),
            temperature: 0.7,
            system_prompt: "你是强大的个人 AI 工作台 Perch，专精代码开发、架构设计与智能问答。请使用 Markdown 规范输出。".to_string(),
            is_dark: false, // 默认清爽浅色
            language: "zh-CN".to_string(),
            local_tools_enabled: false,
            providers: Vec::new(),
        }
    }
}

/// 旧版本会把配置写在程序目录下，而且里面可能有明文 API Key。
/// 只在确实还有明文 Key 时才重写那个文件（`api_key` 标了 `skip_serializing`，不会被写回去），
/// 免得每次启动都去动一个本来就已经干净的文件。
fn sanitize_legacy_cwd_config(config: &AppConfig) {
    let path = Path::new(LEGACY_CONFIG_FILE);
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    if !has_plain_api_key(&content) {
        return;
    }
    if let Ok(json) = serde_json::to_string_pretty(config) {
        let _ = write_atomic(path, &json);
    }
}

/// 在任意 JSON 结构里找非空的 `api_key` 字段。不绑定具体的配置结构，旧格式也认。
fn has_plain_api_key(content: &str) -> bool {
    fn walk(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(map) => map.iter().any(|(key, value)| {
                if key == "api_key" {
                    value.as_str().is_some_and(|text| !text.is_empty())
                } else {
                    walk(value)
                }
            }),
            serde_json::Value::Array(items) => items.iter().any(walk),
            _ => false,
        }
    }
    serde_json::from_str::<serde_json::Value>(content)
        .map(|value| walk(&value))
        .unwrap_or(false)
}

/// 读取某个引用对应的密钥。
///
/// 顺带做一次惰性迁移：新服务名里没有、旧服务名（改名前的 `PersonalControl`）里有，
/// 就读旧值并写回新服务名。这样不用一次性扫描全部凭据，用户第一次用到哪个渠道就迁移哪个。
pub fn load_provider_key(reference: &str) -> keyring::Result<String> {
    let entry = keyring::Entry::new(APP_NAME, reference)?;
    match entry.get_password() {
        Ok(secret) => Ok(secret),
        Err(keyring::Error::NoEntry) => {
            let legacy = keyring::Entry::new(LEGACY_APP_NAME, reference)?;
            match legacy.get_password() {
                Ok(secret) => {
                    // 写回失败不影响这次读取，下次还会再试一遍
                    let _ = entry.set_password(&secret);
                    Ok(secret)
                }
                Err(error) => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}

impl AppConfig {
    pub fn load() -> Self {
        Self::load_checked().unwrap_or_else(|error| panic!("Unable to open app configuration: {error}"))
    }

    fn load_checked() -> Result<Self, Box<dyn Error + Send + Sync>> {
        let path = data_file(CONFIG_FILE);
        if !path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }
        let content = fs::read_to_string(path)?;
        let mut config: AppConfig = serde_json::from_str(&content)?;
        let mut migrated = false;
        for provider in &mut config.providers {
            for model in &mut provider.models {
                migrated |= model.migrate_legacy_tags();
            }
            if provider.api_key_ref.is_empty() {
                provider.api_key_ref = format!("provider/{}", provider.id);
                migrated = true;
            }
            if !provider.api_key.is_empty() {
                // JSON 里还留着明文 Key，搬进凭据管理器，之后就不再写回 JSON
                AppConfig::store_provider_key(&provider.api_key_ref, &provider.api_key)?;
                if load_provider_key(&provider.api_key_ref)? != provider.api_key {
                    return Err("API Key verification failed during migration".into());
                }
                migrated = true;
            } else {
                provider.api_key = match load_provider_key(&provider.api_key_ref) {
                    Ok(secret) => secret,
                    Err(keyring::Error::NoEntry) => String::new(),
                    Err(error) => return Err(error.into()),
                };
            }
        }
        if migrated {
            config.save()?;
        }
        // 旧版本把配置写在程序目录下，那份文件里可能还留着明文 API Key，有就清掉。
        // 清不掉也不影响启动，只是明文还留在那里。
        sanitize_legacy_cwd_config(&config);
        Ok(config)
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(self)?;
        write_atomic(&data_file(CONFIG_FILE), &json)
    }

    pub fn store_provider_key(reference: &str, secret: &str) -> keyring::Result<()> {
        let entry = keyring::Entry::new(APP_NAME, reference)?;
        if secret.is_empty() {
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(error),
            }
        } else {
            entry.set_password(secret)
        }
    }

    pub fn get_active_provider(&self) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.id == self.active_provider_id)
            .or_else(|| self.providers.iter().find(|p| p.enabled))
            .or_else(|| self.providers.first())
    }

    pub fn get_active_base_url(&self) -> String {
        self.get_active_provider()
            .map(|p| p.base_url.clone())
            .unwrap_or_default()
    }

    pub fn get_active_api_key(&self) -> String {
        self.get_active_provider()
            .map(|p| p.api_key.clone())
            .unwrap_or_default()
    }

    pub fn get_active_provider_name(&self) -> String {
        self.get_active_provider()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "未选择渠道".to_string())
    }

    pub fn default_model_selection(&self) -> (String, String) {
        let Some(provider) = self.get_active_provider() else {
            return (String::new(), self.model.clone());
        };
        let model = provider.models.iter()
            .find(|model| model.id == self.model && model.enabled)
            .or_else(|| provider.models.iter().find(|model| model.enabled))
            .map(|model| model.id.clone())
            .unwrap_or_else(|| self.model.clone());
        (provider.id.clone(), model)
    }

    pub fn select_model(&mut self, provider_id: &str, model_id: &str) {
        self.active_provider_id = provider_id.to_string();
        self.model = model_id.to_string();
        let _ = self.save();
    }

    pub fn add_provider(&mut self, provider: ProviderConfig) -> Result<(), std::io::Error> {
        let previous = self.clone();
        let id = provider.id.clone();
        let first_model = provider.models.first().map(|m| m.id.clone()).unwrap_or_else(|| "default".to_string());
        self.providers.push(provider);
        if self.active_provider_id.is_empty() {
            self.active_provider_id = id;
            self.model = first_model;
        }
        if let Err(error) = self.save() {
            *self = previous;
            return Err(error);
        }
        Ok(())
    }

    pub fn delete_provider(&mut self, id: &str) -> Result<(), std::io::Error> {
        let previous = self.clone();
        self.providers.retain(|p| p.id != id);
        if self.active_provider_id == id {
            if let Some(first) = self.providers.first() {
                self.active_provider_id = first.id.clone();
                if let Some(first_model) = first.models.first() {
                    self.model = first_model.id.clone();
                } else {
                    self.model = "default".to_string();
                }
            } else {
                self.active_provider_id = String::new();
                self.model = "default".to_string();
            }
        }
        if let Err(error) = self.save() {
            *self = previous;
            return Err(error);
        }
        Ok(())
    }

    pub fn delete_model(&mut self, provider_id: &str, model_id: &str) {
        if let Some(p) = self.providers.iter_mut().find(|p| p.id == provider_id) {
            p.models.retain(|m| m.id != model_id);
            if self.active_provider_id == provider_id && self.model == model_id {
                if let Some(first) = p.models.first() {
                    self.model = first.id.clone();
                } else {
                    self.model = "default".to_string();
                }
            }
            let _ = self.save();
        }
    }

    pub fn toggle_model_pinned(&mut self, provider_id: &str, model_id: &str) {
        if let Some(p) = self.providers.iter_mut().find(|p| p.id == provider_id) {
            if let Some(m) = p.models.iter_mut().find(|m| m.id == model_id) {
                m.is_pinned = !m.is_pinned;
                let _ = self.save();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_api_key_is_never_serialized() {
        let mut config = AppConfig::default();
        config.providers.push(ProviderConfig {
            id: "provider-1".into(),
            name: "Test".into(),
            channel_type: ChannelType::OpenAiChat,
            base_url: "https://example.test/v1".into(),
            api_path: "/chat/completions".into(),
            api_key: "secret-value".into(),
            api_key_ref: "provider/provider-1".into(),
            enabled: true,
             models: Vec::new(),
             timeout_secs: 90,
             retries: 0,
             proxy: String::new(),
             extra_headers: Vec::new(),
        });
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("secret-value"));
        assert!(json.contains("provider/provider-1"));
    }

    #[test]
    fn old_model_entries_still_load_and_migrate() {
        let mut model: ModelConfig = serde_json::from_str(
            r#"{"id": "moonshot-v1-8k", "name": "Kimi", "tags": "API", "is_pinned": false, "enabled": true}"#,
        )
        .unwrap();
        assert!(model.migrate_legacy_tags());
        assert!(model.tags.is_empty());
        assert_eq!(model.context_window, None);
        assert_eq!(model.effective_context_window(), Some(8_192));

        let mut sized: ModelConfig =
            serde_json::from_str(r#"{"id": "relay-x", "name": "X", "tags": "64K", "is_pinned": false, "enabled": true}"#)
                .unwrap();
        assert!(sized.migrate_legacy_tags());
        assert_eq!(sized.context_window, Some(64_000));

        let mut custom = ModelConfig::new("relay-y", "Y");
        custom.tags = "公司内部".into();
        assert!(!custom.migrate_legacy_tags());
    }

    #[test]
    fn manual_settings_override_detection() {
        let mut model = ModelConfig::new("gpt-4o", "GPT-4o");
        assert!(model.effective_capabilities().contains(&Capability::Vision));
        model.capabilities = Some(vec![Capability::Tools]);
        assert!(!model.effective_capabilities().contains(&Capability::Vision));
        model.reasoning_levels = Some(vec![ReasoningLevel::High, ReasoningLevel::Low]);
        assert_eq!(model.effective_reasoning_levels(), vec![ReasoningLevel::Low, ReasoningLevel::High]);
        let json = serde_json::to_string(&ModelConfig::new("a", "b")).unwrap();
        assert!(!json.contains("context_window"), "unset fields are not written: {json}");
    }

    #[test]
    fn detects_plaintext_api_keys_in_legacy_config() {
        assert!(has_plain_api_key(r#"{"providers":[{"api_key":"sk-123"}]}"#));
        assert!(has_plain_api_key(r#"[{"nested":{"api_key":"sk-456"}}]"#));
        assert!(!has_plain_api_key(r#"{"providers":[{"api_key":""}]}"#));
        assert!(!has_plain_api_key(r#"{"providers":[{"api_key_ref":"provider/1"}]}"#));
        assert!(!has_plain_api_key("not json at all"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "uses the Windows Credential Manager"]
    fn credential_roundtrip() {
        let reference = format!("test/{}", uuid::Uuid::new_v4());
        AppConfig::store_provider_key(&reference, "roundtrip-secret").unwrap();
        let entry = keyring::Entry::new(crate::paths::APP_NAME, &reference).unwrap();
        let result = entry.get_password();
        let cleanup = AppConfig::store_provider_key(&reference, "");
        assert_eq!(result.unwrap(), "roundtrip-secret");
        cleanup.unwrap();
    }
}
