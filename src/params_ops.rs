//! 对话参数与渠道网络的编辑：温度、思考强度、系统提示词、对比模型、渠道网络设置。

use gpui_kit::*;

use crate::app::{AppState, ToastLevel};
use crate::config::{ProviderConfig, format_header_lines, parse_header_lines};
use crate::i18n::{Key, tr};
use crate::model::{ChatParams, DEFAULT_SESSION_FOLDER};

impl AppState {
    pub fn patch_params(&mut self, cx: &mut Context<Self>, update: impl FnOnce(&mut ChatParams)) {
        {
            let Some(session) = self.storage.get_active_session_mut() else {
                return;
            };
            let params = session.params.get_or_insert_with(ChatParams::default);
            update(params);
            if params.is_unset() {
                session.params = None;
            }
        }
        self.persist_storage(cx);
        cx.notify();
    }

    pub fn reset_params(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = self.storage.get_active_session_mut() {
            session.params = None;
        }
        self.params_prompt_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.persist_storage(cx);
        cx.notify();
    }

    pub fn set_session_system_prompt(&mut self, value: String, cx: &mut Context<Self>) {
        let normalized = if value.trim().is_empty() { None } else { Some(value) };
        {
            let Some(session) = self.storage.get_active_session_mut() else {
                return;
            };
            let current = session.params.as_ref().and_then(|params| params.system_prompt.clone());
            if current == normalized {
                return;
            }
            let params = session.params.get_or_insert_with(ChatParams::default);
            params.system_prompt = normalized;
            if params.is_unset() {
                session.params = None;
            }
        }
        self.persist_storage(cx);
    }

    pub fn sync_params_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = self
            .storage
            .get_active_session()
            .and_then(|session| session.params.as_ref())
            .and_then(|params| params.system_prompt.clone())
            .unwrap_or_default();
        if self.params_prompt_input.read(cx).value().as_ref() != prompt {
            self.params_prompt_input
                .update(cx, |input, cx| input.set_value(&prompt, window, cx));
        }
    }

    pub fn create_session_from_preset(&mut self, preset_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preset) = self
            .prompts
            .presets
            .iter()
            .find(|preset| preset.id == preset_id)
            .cloned()
        else {
            return;
        };
        let (default_provider, default_model) = self.config.default_model_selection();
        let provider_id = if preset.provider_id.is_empty() {
            default_provider
        } else {
            preset.provider_id.clone()
        };
        let model = if preset.model.is_empty() {
            default_model
        } else {
            preset.model.clone()
        };
        let id = self
            .storage
            .create_session(&preset.name, DEFAULT_SESSION_FOLDER, &model, &provider_id);
        if let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == id) {
            let mut params = preset.params.clone();
            if params.system_prompt.as_ref().is_none_or(|text| text.trim().is_empty())
                && !preset.system_prompt.trim().is_empty()
            {
                params.system_prompt = Some(preset.system_prompt.clone());
            }
            session.params = if params.is_unset() { None } else { Some(params) };
            session.title_auto = true;
        }
        self.view_mode = crate::app::ViewMode::Chat;
        self.sync_params_editor(window, cx);
        self.persist_storage(cx);
        cx.notify();
    }

    pub fn toggle_compare_model(&mut self, provider_id: &str, model_id: &str, cx: &mut Context<Self>) {
        let lang = self.language();
        let current = self.active_target();
        let key = (provider_id.to_string(), model_id.to_string());
        if key == current {
            return;
        }
        if let Some(index) = self.compare_selection.iter().position(|item| item == &key) {
            self.compare_selection.remove(index);
        } else if self.compare_selection.len() < 2 {
            self.compare_selection.push(key);
        } else {
            self.toast(ToastLevel::Error, tr(lang, Key::CompareMaxTwo));
        }
        cx.notify();
    }

    pub fn clear_compare_selection(&mut self, cx: &mut Context<Self>) {
        self.compare_selection.clear();
        cx.notify();
    }

    pub fn load_provider_network_inputs(
        &mut self,
        provider: &ProviderConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let proxy = provider.proxy.clone();
        let timeout = provider.timeout_secs.to_string();
        let retries = provider.retries.to_string();
        let headers = format_header_lines(&provider.extra_headers);
        self.cfg_proxy_input
            .update(cx, |input, cx| input.set_value(&proxy, window, cx));
        self.cfg_timeout_input
            .update(cx, |input, cx| input.set_value(&timeout, window, cx));
        self.cfg_retries_input
            .update(cx, |input, cx| input.set_value(&retries, window, cx));
        self.cfg_headers_input
            .update(cx, |input, cx| input.set_value(&headers, window, cx));
    }

    pub fn read_provider_network(&self, cx: &Context<Self>) -> (String, u64, u8, Vec<crate::config::HeaderPair>) {
        let proxy = self.cfg_proxy_input.read(cx).value().trim().to_string();
        let timeout = self
            .cfg_timeout_input
            .read(cx)
            .value()
            .trim()
            .parse::<u64>()
            .unwrap_or(90)
            .clamp(5, 600);
        let retries = self
            .cfg_retries_input
            .read(cx)
            .value()
            .trim()
            .parse::<u8>()
            .unwrap_or(0)
            .min(5);
        let headers = parse_header_lines(&self.cfg_headers_input.read(cx).value());
        (proxy, timeout, retries, headers)
    }
}
