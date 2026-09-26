//! 备份的导出与导入：导出 JSON、选文件、确认导入、取消、把备份写进状态。

use std::collections::HashMap;

use gpui_kit::*;

use crate::app::{AppState, ToastLevel, update_state};
use crate::backup::{self, BackupFile};
use crate::i18n::{Key, tr, tr_args};
use crate::paths::data_dir;

impl AppState {
    pub fn export_json_backup(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_new_path(data_dir(), Some("perch-backup.json"));
        cx.spawn(async move |this, cx| {
            let path = receiver.await.ok().and_then(|result| result.ok()).flatten();
            let Some(path) = path else { return };
            update_state(&this, cx, |state, cx| {
                match backup::write_backup(
                    &path,
                    &state.storage.active_session_id,
                    &state.storage.sessions,
                    &state.prompts,
                    &state.config,
                ) {
                    Ok(()) => state.toast(
                        ToastLevel::Success,
                        tr_args(state.language(), Key::BackupExported, &[&path.display().to_string()]),
                    ),
                    Err(error) => state.toast(
                        ToastLevel::Error,
                        tr_args(state.language(), Key::ExportFailed, &[&error.to_string()]),
                    ),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn pick_import_backup(&mut self, cx: &mut Context<Self>) {
        let lang = self.language();
        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let file = rfd::FileDialog::new()
                .set_title(tr(lang, Key::PickBackupTitle))
                .add_filter(tr(lang, Key::FilterJsonBackup), &["json"])
                .pick_file();
            let _ = tx.send(file);
        });

        cx.spawn(async move |this, cx| {
            let Ok(Some(path)) = rx.await else { return };
            let loaded = backup::read_backup(&path);
            update_state(&this, cx, |state, cx| {
                match loaded {
                    Ok(file) => {
                        state.pending_import = Some(file);
                        state.toast(ToastLevel::Info, tr(state.language(), Key::BackupLoaded));
                    }
                    Err(error) => state.toast(ToastLevel::Error, error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn confirm_import(&mut self, cx: &mut Context<Self>) {
        let lang = self.language();
        let Some(backup) = self.pending_import.take() else {
            return;
        };
        self.apply_backup(backup);
        self.persist_storage(cx);
        if let Err(error) = self.prompts.save() {
            self.toast(
                ToastLevel::Error,
                tr_args(lang, Key::PromptsSaveFailed, &[&error.to_string()]),
            );
        }
        if let Err(error) = self.config.save() {
            self.toast(
                ToastLevel::Error,
                tr_args(lang, Key::ConfigSaveFailed, &[&error.to_string()]),
            );
        } else {
            self.toast(ToastLevel::Success, tr(lang, Key::BackupRestored));
        }
        cx.notify();
    }

    pub fn cancel_import(&mut self, cx: &mut Context<Self>) {
        self.pending_import = None;
        cx.notify();
    }

    fn apply_backup(&mut self, backup: BackupFile) {
        let old_keys: HashMap<String, String> = self
            .config
            .providers
            .iter()
            .map(|provider| (provider.id.clone(), provider.api_key.clone()))
            .collect();
        self.storage.sessions = backup.sessions;
        self.storage.active_session_id = if self
            .storage
            .sessions
            .iter()
            .any(|session| session.id == backup.active_session_id)
        {
            backup.active_session_id
        } else {
            self.storage
                .sessions
                .first()
                .map(|session| session.id.clone())
                .unwrap_or_default()
        };
        self.prompts = backup.prompts;
        self.config.providers = backup.config.providers;
        self.config.temperature = backup.config.temperature;
        self.config.system_prompt = backup.config.system_prompt;
        self.config.model = backup.config.model;
        self.config.active_provider_id = backup.config.active_provider_id;
        for provider in &mut self.config.providers {
            if provider.api_key.is_empty() {
                if let Some(existing) = old_keys.get(&provider.id) {
                    provider.api_key = existing.clone();
                } else if !provider.api_key_ref.is_empty()
                    && let Ok(secret) = crate::config::load_provider_key(&provider.api_key_ref)
                {
                    provider.api_key = secret;
                }
            }
        }
    }
}
