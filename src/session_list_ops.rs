//! 会话列表的操作：搜索匹配、新建、切换、删除、重命名、导出与清空。

use std::collections::HashSet;

use gpui_kit::*;

use crate::agent::export_session_to_markdown;
use crate::app::{AppState, ToastLevel, ViewMode};
use crate::i18n::{Key, tr, tr_args};
use crate::model::{DEFAULT_SESSION_FOLDER, DEFAULT_SESSION_TITLE};

impl AppState {
    pub fn matching_session_ids(&mut self, query: &str) -> HashSet<String> {
        if query.is_empty() {
            return HashSet::new();
        }
        if self.sidebar_search_query != query || self.sidebar_search_revision != self.storage.revision() {
            match self.storage.search_session_ids(query) {
                Ok(ids) => {
                    self.sidebar_search_ids = ids;
                    self.sidebar_search_query = query.to_string();
                    self.sidebar_search_revision = self.storage.revision();
                }
                Err(_) => {
                    return self
                        .storage
                        .sessions
                        .iter()
                        .filter(|session| {
                            session.title.to_lowercase().contains(query)
                                || session
                                    .messages
                                    .iter()
                                    .any(|message| message.content.to_lowercase().contains(query))
                        })
                        .map(|session| session.id.clone())
                        .collect();
                }
            }
        }
        self.sidebar_search_ids.clone()
    }

    pub fn create_new_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (provider_id, model) = self.config.default_model_selection();
        let id = self
            .storage
            .create_session(DEFAULT_SESSION_TITLE, DEFAULT_SESSION_FOLDER, &model, &provider_id);
        self.storage.active_session_id = id;
        self.view_mode = ViewMode::Chat;
        self.pending_quote = None;
        self.chat_input.update(cx, |i, cx| {
            i.set_value("", window, cx);
            i.focus(window, cx);
        });
        self.sync_params_editor(window, cx);
        self.persist_storage(cx);
        cx.notify();
    }

    pub fn switch_session(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.storage.active_session_id = id;
        self.pending_quote = None;
        self.persist_storage(cx);
        self.sync_params_editor(window, cx);
        self.chat_input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub fn delete_session(&mut self, id: String, cx: &mut Context<Self>) {
        let lang = self.language();
        self.storage.delete_session(&id);
        self.persist_storage(cx);
        self.toast(ToastLevel::Info, tr(lang, Key::SessionDeleted));
        cx.notify();
    }

    pub fn start_rename_session(
        &mut self,
        id: String,
        current_title: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.rename_target_session_id = Some(id);
        self.rename_input.update(cx, |i, cx| {
            i.set_value(current_title, window, cx);
            i.focus(window, cx);
        });
        cx.notify();
    }

    pub fn confirm_rename_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let new_title = self.rename_input.read(cx).value().trim().to_string();
        if !new_title.is_empty()
            && let Some(target_id) = &self.rename_target_session_id
            && let Some(session) = self.storage.sessions.iter_mut().find(|s| &s.id == target_id)
        {
            session.title = new_title;
            session.title_auto = false;
            self.persist_storage(cx);
        }
        self.rename_target_session_id = None;
        self.rename_input.update(cx, |i, cx| {
            i.set_value("", window, cx);
        });
        cx.notify();
    }

    pub fn export_current_session(&mut self, cx: &mut Context<Self>) {
        let lang = self.language();
        if let Some(session) = self.storage.get_active_session() {
            match export_session_to_markdown(&session.title, &session.messages) {
                Ok(filename) => self.toast(
                    ToastLevel::Success,
                    tr_args(lang, Key::ExportedTo, &[filename.as_str()]),
                ),
                Err(e) => self.toast(ToastLevel::Error, tr_args(lang, Key::ExportFailed, &[&e.to_string()])),
            }
            cx.notify();
        }
    }

    pub fn clear_current_session(&mut self, cx: &mut Context<Self>) {
        let lang = self.language();
        if let Some(session) = self.storage.get_active_session_mut() {
            session.messages.clear();
            self.persist_storage(cx);
            self.toast(ToastLevel::Info, tr(lang, Key::ChatCleared));
            cx.notify();
        }
    }
}
