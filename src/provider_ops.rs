//! 渠道与模型的操作：选择、启停、增删、拉取模型列表、测试连接。

use gpui_kit::*;

use crate::app::{AppState, ToastLevel, runtime, update_state};
use crate::config::{AppConfig, ChannelType, ModelConfig, ProviderConfig};
use crate::i18n::{Key, tr, tr_args};
use crate::provider_api;

impl AppState {
    pub fn select_model(&mut self, provider_id: &str, model_id: &str, cx: &mut Context<Self>) {
        if let Some(session) = self.storage.get_active_session_mut() {
            session.provider_id = provider_id.to_string();
            session.model = model_id.to_string();
            self.persist_storage(cx);
        }
        cx.notify();
    }

    // ================= 渠道设置 =================

    pub(crate) fn ensure_settings_provider_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let exists = self
            .config
            .providers
            .iter()
            .any(|p| p.id == self.selected_settings_provider_id);
        if !exists && let Some(first) = self.config.providers.first().map(|p| p.id.clone()) {
            self.select_settings_provider(&first, window, cx);
        }
    }

    pub fn select_settings_provider(&mut self, provider_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_settings_provider_id = provider_id.to_string();
        if let Some(provider) = self.config.providers.iter().find(|p| p.id == provider_id).cloned() {
            let api_key = provider.api_key.clone();
            let base_url = provider.base_url.clone();
            self.cfg_api_key_input.update(cx, |i, cx| {
                i.set_value(&api_key, window, cx);
            });
            self.cfg_base_url_input.update(cx, |i, cx| {
                i.set_value(&base_url, window, cx);
            });
            self.load_provider_network_inputs(&provider, window, cx);
        }
        cx.notify();
    }

    pub fn toggle_provider_enabled(&mut self, provider_id: &str, cx: &mut Context<Self>) {
        if let Some(provider) = self.config.providers.iter_mut().find(|p| p.id == provider_id) {
            provider.enabled = !provider.enabled;
            self.persist_config(cx);
            cx.notify();
        }
    }

    pub fn toggle_model_enabled(&mut self, provider_id: &str, model_id: &str, cx: &mut Context<Self>) {
        if let Some(provider) = self.config.providers.iter_mut().find(|p| p.id == provider_id)
            && let Some(model) = provider.models.iter_mut().find(|m| m.id == model_id)
        {
            model.enabled = !model.enabled;
            self.persist_config(cx);
            cx.notify();
        }
    }

    pub fn save_current_provider_settings(&mut self, cx: &mut Context<Self>) {
        let lang = self.language();
        let provider_id = self.selected_settings_provider_id.clone();
        let api_key = self.cfg_api_key_input.read(cx).value().trim().to_string();
        let base_url = self.cfg_base_url_input.read(cx).value().trim().to_string();
        let (proxy, timeout, retries, headers) = self.read_provider_network(cx);

        if let Some(provider) = self.config.providers.iter_mut().find(|p| p.id == provider_id) {
            if let Err(error) = AppConfig::store_provider_key(&provider.api_key_ref, &api_key) {
                self.toast(
                    ToastLevel::Error,
                    tr_args(lang, Key::ApiKeySaveFailed, &[&error.to_string()]),
                );
                cx.notify();
                return;
            }
            provider.api_key = api_key;
            if !base_url.is_empty() {
                provider.base_url = base_url;
            }
            provider.proxy = proxy;
            provider.timeout_secs = timeout;
            provider.retries = retries;
            provider.extra_headers = headers;
            match self.config.save() {
                Ok(()) => {
                    cx.set_http_client(crate::image_http::client_for_config(&self.config));
                    self.toast(ToastLevel::Success, tr(lang, Key::ProviderSaved));
                }
                Err(error) => self.toast(
                    ToastLevel::Error,
                    tr_args(lang, Key::ProviderSaveFailed, &[&error.to_string()]),
                ),
            }
            cx.notify();
        }
    }

    pub fn select_add_channel_type(&mut self, ct: ChannelType, window: &mut Window, cx: &mut Context<Self>) {
        self.add_channel_type = ct;
        let def_base = ct.default_base_url();
        let def_name = ct.label();

        self.new_provider_base_url_input
            .update(cx, |i, cx| i.set_value(def_base, window, cx));
        self.new_provider_name_input
            .update(cx, |i, cx| i.set_value(def_name, window, cx));
        cx.notify();
    }

    /// 返回 true 表示添加成功，弹窗可以关闭
    pub fn confirm_add_provider(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let lang = self.language();
        let name = self.new_provider_name_input.read(cx).value().trim().to_string();
        let base_url = self.new_provider_base_url_input.read(cx).value().trim().to_string();
        let api_key = self.new_provider_api_key_input.read(cx).value().trim().to_string();
        let ct = self.add_channel_type;

        if name.is_empty() || base_url.is_empty() {
            self.toast(ToastLevel::Error, tr(lang, Key::ProviderNameUrlRequired));
            cx.notify();
            return false;
        }

        let provider_id = format!("channel-{}", uuid::Uuid::new_v4());

        let new_provider = ProviderConfig {
            id: provider_id.clone(),
            name,
            channel_type: ct,
            base_url,
            api_path: ct.default_api_path().to_string(),
            api_key,
            api_key_ref: format!("provider/{provider_id}"),
            enabled: true,
            models: Vec::new(),
            timeout_secs: 90,
            retries: 0,
            proxy: String::new(),
            extra_headers: Vec::new(),
        };

        if let Err(error) = AppConfig::store_provider_key(&new_provider.api_key_ref, &new_provider.api_key) {
            self.toast(
                ToastLevel::Error,
                tr_args(lang, Key::ApiKeySaveFailed, &[&error.to_string()]),
            );
            cx.notify();
            return false;
        }
        if let Err(error) = self.config.add_provider(new_provider) {
            // 渠道没加上，把刚存进去的 Key 一起删掉。删不掉也无所谓：
            // 渠道没建起来，这个引用不会再被谁读到。
            let _ = AppConfig::store_provider_key(&format!("provider/{provider_id}"), "");
            self.toast(
                ToastLevel::Error,
                tr_args(lang, Key::ProviderAddFailed, &[&error.to_string()]),
            );
            cx.notify();
            return false;
        }
        self.new_provider_api_key_input
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.select_settings_provider(&provider_id, window, cx);

        self.toast(ToastLevel::Success, tr(lang, Key::ProviderAdded));
        cx.notify();
        true
    }

    pub fn delete_selected_provider(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let lang = self.language();
        let provider_id = self.selected_settings_provider_id.clone();
        let key_ref = self
            .config
            .providers
            .iter()
            .find(|p| p.id == provider_id)
            .map(|provider| provider.api_key_ref.clone());
        if let Err(error) = self.config.delete_provider(&provider_id) {
            self.toast(
                ToastLevel::Error,
                tr_args(lang, Key::ProviderDeleteFailed, &[&error.to_string()]),
            );
            cx.notify();
            return;
        }
        let mut key_cleanup_failed = false;
        if let Some(key_ref) = key_ref
            && let Err(error) = AppConfig::store_provider_key(&key_ref, "")
        {
            self.toast(
                ToastLevel::Error,
                tr_args(lang, Key::ProviderKeyCleanupFailed, &[&error.to_string()]),
            );
            key_cleanup_failed = true;
        }
        if let Some(first) = self.config.providers.first().map(|p| p.id.clone()) {
            self.select_settings_provider(&first, window, cx);
        } else {
            self.selected_settings_provider_id = String::new();
        }
        if !key_cleanup_failed {
            self.toast(ToastLevel::Info, tr(lang, Key::ProviderDeleted));
        }
        cx.notify();
    }

    pub fn delete_model_from_provider(&mut self, provider_id: &str, model_id: &str, cx: &mut Context<Self>) {
        let lang = self.language();
        match self.config.delete_model(provider_id, model_id) {
            Ok(()) => self.toast(ToastLevel::Info, tr(lang, Key::ModelDeleted)),
            Err(error) => self.toast(
                ToastLevel::Error,
                tr_args(lang, Key::ConfigSaveFailed, &[&error.to_string()]),
            ),
        }
        cx.notify();
    }

    pub fn toggle_model_pin(&mut self, provider_id: &str, model_id: &str, cx: &mut Context<Self>) {
        let lang = self.language();
        if let Err(error) = self.config.toggle_model_pinned(provider_id, model_id) {
            self.toast(
                ToastLevel::Error,
                tr_args(lang, Key::ConfigSaveFailed, &[&error.to_string()]),
            );
        }
        cx.notify();
    }

    /// 设置默认渠道与模型（设置页那个模型下拉框用的）。
    /// 界面不直接改 `config`，一律走这里，顺带把保存失败报出来。
    pub fn set_default_model(&mut self, provider_id: &str, model_id: &str, cx: &mut Context<Self>) {
        let lang = self.language();
        if let Err(error) = self.config.select_model(provider_id, model_id) {
            self.toast(
                ToastLevel::Error,
                tr_args(lang, Key::ConfigSaveFailed, &[&error.to_string()]),
            );
        }
        cx.notify();
    }

    pub fn fetch_models_from_provider(&mut self, cx: &mut Context<Self>) {
        let lang = self.language();
        let provider_id = self.selected_settings_provider_id.clone();
        let provider = match self.config.providers.iter().find(|p| p.id == provider_id) {
            Some(p) => p.clone(),
            None => return,
        };

        self.toast(ToastLevel::Info, tr(lang, Key::FetchingModels));
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = runtime()
                .spawn(async move { provider_api::fetch_models(&provider).await })
                .await
                .unwrap_or_else(|e| Err(tr_args(lang, Key::FetchModelsFailed, &[&e.to_string()])));

            update_state(&this, cx, |state, cx| {
                match result {
                    Ok(models) if models.is_empty() => {
                        state.toast(ToastLevel::Error, tr(state.language(), Key::NoModelsReturned))
                    }
                    Ok(models) => {
                        let count = models.len();
                        state.pending_models = models;
                        state.pending_model_selection.clear();
                        state.model_fetch_query.clear();
                        state.open_model_picker = true;
                        state.toast(
                            ToastLevel::Info,
                            tr_args(state.language(), Key::FetchedModels, &[&count.to_string()]),
                        );
                    }
                    Err(err) => state.toast(ToastLevel::Error, err),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn set_pending_model(&mut self, id: &str, selected: bool, cx: &mut Context<Self>) {
        if selected {
            self.pending_model_selection.insert(id.to_string());
        } else {
            self.pending_model_selection.remove(id);
        }
        cx.notify();
    }

    pub fn select_pending_models(&mut self, ids: &[String], selected: bool, cx: &mut Context<Self>) {
        for id in ids {
            if selected {
                self.pending_model_selection.insert(id.clone());
            } else {
                self.pending_model_selection.remove(id);
            }
        }
        cx.notify();
    }

    pub fn confirm_pending_models(&mut self, cx: &mut Context<Self>) -> bool {
        let lang = self.language();
        if self.pending_model_selection.is_empty() {
            self.toast(ToastLevel::Error, tr(lang, Key::PickAtLeastOneModel));
            cx.notify();
            return false;
        }
        let provider_id = self.selected_settings_provider_id.clone();
        let selected = std::mem::take(&mut self.pending_model_selection);
        let models = std::mem::take(&mut self.pending_models);
        let Some(provider) = self
            .config
            .providers
            .iter_mut()
            .find(|provider| provider.id == provider_id)
        else {
            return false;
        };
        let mut added = 0usize;
        for (id, name) in models {
            if !selected.contains(&id) || provider.models.iter().any(|model| model.id == id) {
                continue;
            }
            provider.models.push(ModelConfig::new(id, name));
            added += 1;
        }
        match self.config.save() {
            Ok(()) if added == 0 => self.toast(ToastLevel::Info, tr(lang, Key::ModelsAlreadyAdded)),
            Ok(()) => self.toast(
                ToastLevel::Success,
                tr_args(lang, Key::ModelsAdded, &[&added.to_string()]),
            ),
            Err(error) => self.toast(
                ToastLevel::Error,
                tr_args(lang, Key::ModelListSaveFailed, &[&error.to_string()]),
            ),
        }
        cx.notify();
        true
    }

    pub fn cancel_pending_models(&mut self, cx: &mut Context<Self>) {
        self.pending_models.clear();
        self.pending_model_selection.clear();
        cx.notify();
    }

    pub fn test_provider_connection(&mut self, cx: &mut Context<Self>) {
        let lang = self.language();
        let mut provider = match self
            .config
            .providers
            .iter()
            .find(|p| p.id == self.selected_settings_provider_id)
        {
            Some(provider) => provider.clone(),
            None => return,
        };
        let base_url = self.cfg_base_url_input.read(cx).value().trim().to_string();
        if !base_url.is_empty() {
            provider.base_url = base_url;
        }
        provider.api_key = self.cfg_api_key_input.read(cx).value().trim().to_string();
        self.toast(ToastLevel::Info, tr(lang, Key::TestingConnection));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = runtime()
                .spawn(async move { provider_api::fetch_models(&provider).await })
                .await
                .unwrap_or_else(|error| Err(tr_args(lang, Key::ConnectionTestFailed, &[&error.to_string()])));
            update_state(&this, cx, |state, cx| {
                match result {
                    Ok(models) => state.toast(
                        ToastLevel::Success,
                        tr_args(state.language(), Key::ConnectionOk, &[&models.len().to_string()]),
                    ),
                    Err(error) => state.toast(ToastLevel::Error, error),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
