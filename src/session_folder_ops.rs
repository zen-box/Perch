//! 会话的组织方式：置顶、收藏、文件夹与筛选。

use gpui_kit::*;

use crate::app::AppState;
use crate::model::DEFAULT_SESSION_FOLDER;

impl AppState {
    pub fn toggle_session_pin(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == id) {
            session.pinned = !session.pinned;
        }
        self.persist_storage(cx);
        cx.notify();
    }

    pub fn toggle_session_favorite(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == id) {
            session.favorite = !session.favorite;
        }
        self.persist_storage(cx);
        cx.notify();
    }

    pub fn set_folder_filter(&mut self, folder: &str, cx: &mut Context<Self>) {
        self.folder_filter = folder.to_string();
        cx.notify();
    }

    pub fn set_session_folder(&mut self, id: &str, folder: &str, cx: &mut Context<Self>) {
        if let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == id) {
            session.folder = if folder.trim().is_empty() {
                DEFAULT_SESSION_FOLDER.into()
            } else {
                folder.trim().to_string()
            };
        }
        self.persist_storage(cx);
        cx.notify();
    }

    pub fn begin_move_folder(&mut self, id: &str, current: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.folder_target_id = Some(id.to_string());
        let value = if current == DEFAULT_SESSION_FOLDER { "" } else { current };
        self.folder_name_input.update(cx, |input, cx| {
            input.set_value(value, window, cx);
            input.focus(window, cx);
        });
    }

    pub fn confirm_move_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.folder_target_id.clone() else {
            return false;
        };
        let name = self.folder_name_input.read(cx).value().trim().to_string();
        self.set_session_folder(&id, &name, cx);
        self.folder_target_id = None;
        self.folder_name_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        true
    }

    pub fn toggle_favorites_filter(&mut self, cx: &mut Context<Self>) {
        self.favorites_only = !self.favorites_only;
        if self.favorites_only {
            self.folder_filter.clear();
        }
        cx.notify();
    }

    pub fn select_folder_filter(&mut self, folder: &str, cx: &mut Context<Self>) {
        self.favorites_only = false;
        self.set_folder_filter(folder, cx);
    }

    pub fn clear_session_filters(&mut self, cx: &mut Context<Self>) {
        self.folder_filter.clear();
        self.favorites_only = false;
        cx.notify();
    }
}
