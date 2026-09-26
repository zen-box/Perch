use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::config::AppConfig;
use crate::model::ChatSession;
use crate::paths::write_atomic;
use crate::prompts::PromptLibrary;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackupFile {
    pub version: u32,
    pub exported_at: String,
    pub active_session_id: String,
    pub sessions: Vec<ChatSession>,
    pub prompts: PromptLibrary,
    pub config: AppConfig,
}

pub fn write_backup(
    path: &Path,
    active_session_id: &str,
    sessions: &[ChatSession],
    prompts: &PromptLibrary,
    config: &AppConfig,
) -> Result<(), String> {
    let mut sessions = sessions.to_vec();
    for session in &mut sessions {
        for message in &mut session.messages {
            message.is_streaming = false;
            for variant in &mut message.variants {
                variant.is_streaming = false;
            }
        }
    }
    let backup = BackupFile {
        version: 1,
        exported_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        active_session_id: active_session_id.to_string(),
        sessions,
        prompts: prompts.clone(),
        config: config.clone(),
    };
    let json = serde_json::to_string_pretty(&backup).map_err(|error| error.to_string())?;
    write_atomic(path, &json).map_err(|error| error.to_string())
}

pub fn read_backup(path: &Path) -> Result<BackupFile, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let backup: BackupFile = serde_json::from_str(&text).map_err(|error| format!("备份文件无法解析: {error}"))?;
    if backup.version != 1 {
        return Err(format!("不支持的备份版本: {}", backup.version));
    }
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn backup_roundtrip_omits_api_keys() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("backup.json");
        let mut config = AppConfig::default();
        config.providers.push(crate::config::ProviderConfig {
            id: "p".into(),
            name: "P".into(),
            channel_type: crate::config::ChannelType::OpenAiChat,
            base_url: "https://example.test/v1".into(),
            api_path: "/chat/completions".into(),
            api_key: "secret-value".into(),
            api_key_ref: "provider/p".into(),
            enabled: true,
            models: Vec::new(),
            timeout_secs: 30,
            retries: 2,
            proxy: "http://127.0.0.1:7890".into(),
            extra_headers: Vec::new(),
        });
        let session = ChatSession::new("标题".into(), "默认".into(), "model".into(), "p".into());
        write_backup(
            &path,
            &session.id,
            std::slice::from_ref(&session),
            &PromptLibrary::default(),
            &config,
        )
        .unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("secret-value"));
        let backup = read_backup(&path).unwrap();
        assert_eq!(backup.sessions[0].title, "标题");
        assert_eq!(backup.config.providers[0].timeout_secs, 30);
        assert!(backup.config.providers[0].api_key.is_empty());
    }
}
