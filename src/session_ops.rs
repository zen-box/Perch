use std::collections::HashMap;

use gpui_kit::*;
use tokio::sync::oneshot;

use crate::app::{AppState, ToastLevel, runtime, update_state};
use crate::backup::{self, BackupFile};
use crate::config::{ChannelType, ModelConfig, ProviderConfig, format_header_lines, parse_header_lines};
use crate::llm::{ChatMessageReq, ChatRequest, StreamEvent, stream_chat};
use crate::model::{AttachmentKind, ChatMessage, ChatParams, MessageVariant, ReasoningLevel, ResolvedParams};
use crate::paths::data_dir;
use crate::prompts::{PromptPreset, PromptTemplate, expand_variables};

struct Job {
    key: String,
    message_id: String,
    variant_id: Option<String>,
    request: ChatRequest,
}

impl AppState {
    pub fn cancel_streaming(&mut self, cx: &mut Context<Self>) {
        let senders: Vec<_> = self.active_streams.drain().map(|(_, sender)| sender).collect();
        for sender in senders {
            let _ = sender.send(());
        }
        self.is_streaming = false;
        for session in &mut self.storage.sessions {
            for message in &mut session.messages {
                message.is_streaming = false;
                for variant in &mut message.variants {
                    variant.is_streaming = false;
                }
            }
        }
        self.persist_storage(cx);
        cx.notify();
    }

    pub fn active_target(&self) -> (String, String) {
        let (default_provider, default_model) = self.config.default_model_selection();
        self.storage
            .get_active_session()
            .map(|session| {
                (
                    if session.provider_id.is_empty() {
                        default_provider.clone()
                    } else {
                        session.provider_id.clone()
                    },
                    if session.model.is_empty() || session.model == "default" {
                        default_model.clone()
                    } else {
                        session.model.clone()
                    },
                )
            })
            .unwrap_or((default_provider, default_model))
    }

    pub fn compare_targets(&self) -> Vec<(String, String)> {
        let current = self.active_target();
        let mut targets = vec![current.clone()];
        for item in &self.compare_selection {
            if item != &current && !targets.contains(item) {
                targets.push(item.clone());
            }
        }
        targets
    }

    pub fn send_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let user_prompt = self.chat_input.read(cx).value().trim().to_string();
        if (user_prompt.is_empty() && self.pending_attachments.is_empty()) || self.is_streaming {
            return;
        }
        if self
            .storage
            .get_active_session()
            .is_some_and(|session| session.has_unresolved_compare())
        {
            self.toast(ToastLevel::Error, "请先采用一条对比回答，再继续对话");
            cx.notify();
            return;
        }
        let targets = self.compare_targets();
        if !self.compare_selection.is_empty() && targets.len() >= 2 {
            self.send_compare(window, cx);
            return;
        }
        self.chat_input.update(cx, |input, cx| input.set_value("", window, cx));
        if self.handle_local_tool(&user_prompt, cx) {
            return;
        }
        let quote = self.pending_quote.take();
        self.prepare_user_turn(&user_prompt, quote);
        self.start_reply(None, cx);
    }

    pub fn send_compare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let user_prompt = self.chat_input.read(cx).value().trim().to_string();
        if self.is_streaming {
            return;
        }
        if user_prompt.is_empty() && self.pending_attachments.is_empty() {
            self.toast(ToastLevel::Info, "请在输入框输入问题或添加图片后再开始对比");
            cx.notify();
            return;
        }
        let targets = self.compare_targets();
        if targets.len() < 2 {
            self.toast(ToastLevel::Error, "请至少勾选 1 个要对比的模型");
            cx.notify();
            return;
        }
        self.compare_selection.clear();
        self.chat_input.update(cx, |input, cx| input.set_value("", window, cx));
        let quote = self.pending_quote.take();
        self.prepare_user_turn(&user_prompt, quote);
        self.start_compare(&targets, cx);
    }

    pub fn regenerate_message(
        &mut self,
        message_id: &str,
        provider_id: Option<String>,
        model_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.is_streaming {
            return;
        }
        let active_id = self.storage.active_session_id.clone();
        let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) else {
            return;
        };
        let Some(index) = session
            .messages
            .iter()
            .position(|message| message.id == message_id && message.role == "assistant")
        else {
            return;
        };
        if !session.messages[..index].iter().any(|message| message.role == "user") {
            self.toast(ToastLevel::Error, "没有可重答的用户消息");
            cx.notify();
            return;
        }
        session.messages.truncate(index);
        if let (Some(provider_id), Some(model_id)) = (provider_id, model_id) {
            session.provider_id = provider_id;
            session.model = model_id;
        }
        self.start_reply(None, cx);
    }

    pub fn continue_message(&mut self, cx: &mut Context<Self>) {
        if self.is_streaming {
            return;
        }
        let active_id = self.storage.active_session_id.clone();
        let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) else {
            return;
        };
        let Some(message) = session.messages.last_mut() else {
            return;
        };
        if message.role != "assistant" || message.content.is_empty() || !message.variants.is_empty() {
            self.toast(ToastLevel::Error, "只能在已完成的回答后继续生成");
            cx.notify();
            return;
        }
        message.is_streaming = true;
        let message_id = message.id.clone();
        let model = message.model.clone();
        let provider_id = session.provider_id.clone();
        let mut history = self.history_messages();
        history.push(ChatMessageReq::new(
            "user",
            "请从上次中断的地方继续，不要重复已有内容。",
        ));
        let Some(job) = self.make_job(&message_id, None, &provider_id, &model, history) else {
            self.toast(ToastLevel::Error, "当前模型不可用");
            cx.notify();
            return;
        };
        self.spawn_jobs(vec![job], cx);
    }

    pub fn delete_message(&mut self, message_id: &str, cx: &mut Context<Self>) {
        if self.is_streaming {
            self.toast(ToastLevel::Error, "生成过程中不能删除消息");
            cx.notify();
            return;
        }
        let active_id = self.storage.active_session_id.clone();
        if let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) {
            session.messages.retain(|message| message.id != message_id);
        }
        self.persist_storage(cx);
        cx.notify();
    }

    pub fn quote_message(&mut self, message_id: &str, cx: &mut Context<Self>) {
        let quote = self.storage.get_active_session().and_then(|session| {
            session
                .messages
                .iter()
                .find(|message| message.id == message_id)
                .map(|message| {
                    let text = if message.content.is_empty() {
                        message
                            .variants
                            .iter()
                            .map(|variant| variant.content.as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                    } else {
                        message.content.clone()
                    };
                    text.chars().take(800).collect::<String>()
                })
        });
        self.pending_quote = quote.filter(|text| !text.trim().is_empty());
        cx.notify();
    }

    pub fn clear_quote(&mut self, cx: &mut Context<Self>) {
        self.pending_quote = None;
        cx.notify();
    }

    pub fn begin_edit_message(&mut self, message_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.storage.get_active_session().and_then(|session| {
            session
                .messages
                .iter()
                .find(|message| message.id == message_id && message.role == "user")
                .map(|message| message.content.clone())
        });
        let Some(content) = content else { return };
        self.edit_message_id = Some(message_id.to_string());
        self.edit_message_input.update(cx, |input, cx| {
            input.set_value(&content, window, cx);
            input.focus(window, cx);
        });
    }

    pub fn confirm_edit_message(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(message_id) = self.edit_message_id.clone() else {
            return false;
        };
        let text = self.edit_message_input.read(cx).value().trim().to_string();
        if text.is_empty() || self.is_streaming {
            return false;
        }
        let active_id = self.storage.active_session_id.clone();
        let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) else {
            return false;
        };
        let Some(index) = session.messages.iter().position(|message| message.id == message_id) else {
            return false;
        };
        session.messages[index].content = text;
        session.messages.truncate(index + 1);
        self.edit_message_id = None;
        self.edit_message_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.start_reply(None, cx);
        true
    }

    pub fn adopt_variant(&mut self, message_id: &str, variant_id: &str, cx: &mut Context<Self>) {
        let active_id = self.storage.active_session_id.clone();
        let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) else {
            return;
        };
        let Some(message) = session.messages.iter_mut().find(|message| message.id == message_id) else {
            return;
        };
        let Some(variant) = message
            .variants
            .iter()
            .find(|variant| variant.id == variant_id)
            .cloned()
        else {
            return;
        };
        message.content = variant.content;
        message.reasoning_content = variant.reasoning_content;
        message.error = variant.error;
        message.model = variant.model.clone();
        message.prompt_tokens = variant.prompt_tokens;
        message.completion_tokens = variant.completion_tokens;
        message.speed_tps = variant.speed_tps;
        message.latency_ms = variant.latency_ms;
        message.is_streaming = false;
        // 保留 variants，以便用户随时左右翻页查看其他模型的回答
        session.model = variant.model;
        session.provider_id = variant.provider_id;
        self.persist_storage(cx);
        cx.notify();
    }

    pub fn switch_variant(&mut self, message_id: &str, target_variant_ix: usize, cx: &mut Context<Self>) {
        let active_id = self.storage.active_session_id.clone();
        let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) else {
            return;
        };
        let Some(message) = session.messages.iter_mut().find(|message| message.id == message_id) else {
            return;
        };
        if target_variant_ix >= message.variants.len() {
            return;
        }
        let variant = message.variants[target_variant_ix].clone();
        message.content = variant.content;
        message.reasoning_content = variant.reasoning_content;
        message.error = variant.error;
        message.model = variant.model.clone();
        message.prompt_tokens = variant.prompt_tokens;
        message.completion_tokens = variant.completion_tokens;
        message.speed_tps = variant.speed_tps;
        message.latency_ms = variant.latency_ms;
        session.model = variant.model;
        session.provider_id = variant.provider_id;
        self.persist_storage(cx);
        cx.notify();
    }

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
                "默认".into()
            } else {
                folder.trim().to_string()
            };
        }
        self.persist_storage(cx);
        cx.notify();
    }

    pub fn begin_move_folder(&mut self, id: &str, current: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.folder_target_id = Some(id.to_string());
        let value = if current == "默认" { "" } else { current };
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

    pub fn insert_template(&mut self, template_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(body) = self
            .prompts
            .templates
            .iter()
            .find(|template| template.id == template_id)
            .map(|template| template.body.clone())
        else {
            return;
        };
        let expanded = expand_variables(&body, &clipboard_text(cx), &self.selection_text(cx));
        self.chat_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.insert(&expanded, window, cx);
            input.focus(window, cx);
        });
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

    pub fn apply_slash_template(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let value = self.chat_input.read(cx).value();
        let token = value.trim();
        let Some(name) = token.strip_prefix('/') else {
            return false;
        };
        if name.is_empty() || name.contains(char::is_whitespace) {
            return false;
        }
        let Some(body) = self
            .prompts
            .templates
            .iter()
            .find(|template| template.name.eq_ignore_ascii_case(name))
            .map(|template| template.body.clone())
        else {
            return false;
        };
        let expanded = expand_variables(&body, &clipboard_text(cx), &self.selection_text(cx));
        self.chat_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.insert(&expanded, window, cx);
            input.focus(window, cx);
        });
        true
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
        let id = self.storage.create_session(&preset.name, "默认", &model, &provider_id);
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
            self.toast(ToastLevel::Error, "最多选择 2 个对比模型（共 3 个模型 PK）");
        }
        cx.notify();
    }

    pub fn clear_compare_selection(&mut self, cx: &mut Context<Self>) {
        self.compare_selection.clear();
        cx.notify();
    }

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
                    Ok(()) => state.toast(ToastLevel::Success, format!("已导出备份 {}", path.display())),
                    Err(error) => state.toast(ToastLevel::Error, format!("导出失败: {error}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn pick_import_backup(&mut self, cx: &mut Context<Self>) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let file = rfd::FileDialog::new()
                .set_title("选择 JSON 备份")
                .add_filter("JSON 备份文件", &["json"])
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
                        state.toast(ToastLevel::Info, "已读取备份，请确认是否恢复");
                    }
                    Err(error) => state.toast(ToastLevel::Error, error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn confirm_import(&mut self, cx: &mut Context<Self>) {
        let Some(backup) = self.pending_import.take() else {
            return;
        };
        self.apply_backup(backup);
        self.persist_storage(cx);
        if let Err(error) = self.prompts.save() {
            self.toast(ToastLevel::Error, format!("提示词保存失败: {error}"));
        }
        if let Err(error) = self.config.save() {
            self.toast(ToastLevel::Error, format!("配置保存失败: {error}"));
        } else {
            self.toast(
                ToastLevel::Success,
                "备份已恢复。API Key 需在本机凭据中存在，否则请重新填写",
            );
        }
        cx.notify();
    }

    pub fn cancel_import(&mut self, cx: &mut Context<Self>) {
        self.pending_import = None;
        cx.notify();
    }

    pub fn save_prompt_from_inputs(&mut self, is_template: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let name = self.prompt_name_input.read(cx).value().trim().to_string();
        let body = self.prompt_body_input.read(cx).value().to_string();
        let icon = self.prompt_icon_input.read(cx).value().trim().to_string();
        if name.is_empty() || body.trim().is_empty() {
            self.toast(ToastLevel::Error, "名称和内容不能为空");
            cx.notify();
            return false;
        }
        let edit_id = self.prompt_edit_id.clone();
        if is_template {
            if let Some(id) = edit_id {
                if let Some(template) = self.prompts.templates.iter_mut().find(|template| template.id == id) {
                    template.name = name;
                    template.body = body;
                }
            } else {
                self.prompts.templates.push(PromptTemplate {
                    id: uuid::Uuid::new_v4().to_string(),
                    name,
                    body,
                });
            }
        } else if let Some(id) = edit_id {
            if let Some(preset) = self.prompts.presets.iter_mut().find(|preset| preset.id == id) {
                preset.name = name;
                preset.icon = if icon.is_empty() { "✨".into() } else { icon };
                preset.system_prompt = body;
            }
        } else {
            self.prompts.presets.push(PromptPreset {
                id: uuid::Uuid::new_v4().to_string(),
                name,
                icon: if icon.is_empty() { "✨".into() } else { icon },
                system_prompt: body,
                provider_id: String::new(),
                model: String::new(),
                params: ChatParams::default(),
            });
        }
        self.prompt_edit_id = None;
        self.prompt_name_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.prompt_icon_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.prompt_body_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        if let Err(error) = self.prompts.save() {
            self.toast(ToastLevel::Error, format!("保存失败: {error}"));
        } else {
            self.toast(ToastLevel::Success, "已保存");
        }
        cx.notify();
        true
    }

    pub fn delete_prompt(&mut self, id: &str, is_template: bool, cx: &mut Context<Self>) {
        if is_template {
            self.prompts.templates.retain(|template| template.id != id);
        } else {
            self.prompts.presets.retain(|preset| preset.id != id);
        }
        if let Err(error) = self.prompts.save() {
            self.toast(ToastLevel::Error, format!("删除失败: {error}"));
        }
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

    fn handle_local_tool(&mut self, user_prompt: &str, cx: &mut Context<Self>) -> bool {
        if !self.config.local_tools_enabled {
            return false;
        }
        let (tool, arg, needs_confirm) = if user_prompt.starts_with("/ls") || user_prompt.starts_with("/dir") {
            (
                "list_dir",
                user_prompt
                    .trim_start_matches("/ls")
                    .trim_start_matches("/dir")
                    .trim()
                    .to_string(),
                false,
            )
        } else if let Some(path) = user_prompt.strip_prefix("/read ") {
            ("read_file", path.trim().to_string(), false)
        } else if user_prompt == "/git" || user_prompt.starts_with("/git ") {
            ("git_status", String::new(), false)
        } else if let Some(cmd) = user_prompt
            .strip_prefix("/bash ")
            .or_else(|| user_prompt.strip_prefix("/sh "))
        {
            ("bash", cmd.trim().to_string(), true)
        } else {
            return false;
        };
        let session = match self.storage.get_active_session_mut() {
            Some(session) => session,
            None => return true,
        };
        session.messages.push(ChatMessage::new_user(user_prompt.to_string()));
        if needs_confirm {
            self.pending_tool_name = Some("Bash".into());
            self.pending_tool_cmd = Some(arg);
            self.persist_storage(cx);
            cx.notify();
        } else {
            self.execute_agent_tool(tool, &arg, cx);
        }
        true
    }

    fn prepare_user_turn(&mut self, user_prompt: &str, quote: Option<String>) {
        let active_id = self.storage.active_session_id.clone();
        let attachments = std::mem::take(&mut self.pending_attachments);
        let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) else {
            return;
        };
        if session.messages.is_empty() && session.title_auto {
            let title_source = if user_prompt.trim().is_empty() {
                if let Some(att) = attachments.first() {
                    let kind_label = match att.kind {
                        AttachmentKind::Image => "图片",
                        AttachmentKind::Document => "文档",
                        AttachmentKind::Text => "代码/文本",
                        AttachmentKind::Other => "附件",
                    };
                    format!("[{kind_label}] {}", att.name)
                } else {
                    "新对话".to_string()
                }
            } else {
                user_prompt.to_string()
            };
            session.title = truncate_title(&title_source);
        }
        let mut message = ChatMessage::new_user(user_prompt.to_string());
        message.quote = quote.filter(|text| !text.trim().is_empty());
        message.attachments = attachments;
        session.messages.push(message);
        session.updated_at = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    }

    fn start_reply(&mut self, override_model: Option<(String, String)>, cx: &mut Context<Self>) {
        let active_id = self.storage.active_session_id.clone();
        let (default_provider, default_model) = self.config.default_model_selection();
        let (provider_id, model) = override_model.unwrap_or_else(|| {
            self.storage
                .get_active_session()
                .map(|session| {
                    (
                        if session.provider_id.is_empty() {
                            default_provider.clone()
                        } else {
                            session.provider_id.clone()
                        },
                        if session.model.is_empty() || session.model == "default" {
                            default_model.clone()
                        } else {
                            session.model.clone()
                        },
                    )
                })
                .unwrap_or((default_provider, default_model))
        });
        if self.config.providers.is_empty() {
            self.push_local_assistant(
                "尚未创建任何 AI 渠道。\n\n点击右上角的设置图标，进入「模型渠道」添加你的第一个渠道。",
            );
            self.persist_storage(cx);
            cx.notify();
            return;
        }
        let mut assistant = ChatMessage::new_assistant();
        assistant.model = model.clone();
        let message_id = assistant.id.clone();
        if let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) {
            session.provider_id = provider_id.clone();
            session.model = model.clone();
            session.messages.push(assistant);
        }
        let history = self.history_messages();
        let Some(job) = self.make_job(&message_id, None, &provider_id, &model, history) else {
            if let Some(message) = self.find_message_mut(&message_id) {
                message.is_streaming = false;
                message.error = Some("当前渠道没有可用模型，请先添加或启用模型。".into());
            }
            self.persist_storage(cx);
            cx.notify();
            return;
        };
        self.scroll_to_end_pending = true;
        self.spawn_jobs(vec![job], cx);
    }

    fn start_compare(&mut self, targets: &[(String, String)], cx: &mut Context<Self>) {
        let active_id = self.storage.active_session_id.clone();
        let history = self.history_messages();
        let mut variants = Vec::new();
        let mut jobs = Vec::new();
        for (provider_id, model) in targets {
            let variant = MessageVariant {
                id: uuid::Uuid::new_v4().to_string(),
                provider_id: provider_id.clone(),
                model: model.clone(),
                content: String::new(),
                reasoning_content: None,
                error: None,
                is_streaming: true,
                prompt_tokens: 0,
                completion_tokens: 0,
                speed_tps: 0.0,
                latency_ms: 0,
            };
            if let Some(job) = self.make_job("pending", Some(&variant.id), provider_id, model, history.clone()) {
                variants.push(variant);
                jobs.push(job);
            }
        }
        if jobs.len() < 2 {
            self.toast(ToastLevel::Error, "选中的模型里没有足够的可用模型");
            cx.notify();
            return;
        }
        let mut assistant = ChatMessage::new_assistant();
        assistant.model = jobs[0].request.model.clone();
        let message_id = assistant.id.clone();
        for job in &mut jobs {
            job.message_id = message_id.clone();
            job.key = stream_key(&message_id, job.variant_id.as_deref());
        }
        assistant.variants = variants;
        if let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) {
            session.messages.push(assistant);
        }
        self.scroll_to_end_pending = true;
        self.spawn_jobs(jobs, cx);
    }

    fn history_messages(&self) -> Vec<ChatMessageReq> {
        let Some(session) = self.storage.get_active_session() else {
            return Vec::new();
        };
        let resolved = session.resolved_params(&self.config.system_prompt, self.config.temperature);
        let mut messages = vec![ChatMessageReq::new("system", resolved.system_prompt)];
        messages.extend(
            session
                .api_turns(resolved.context_limit)
                .into_iter()
                .map(|(role, content, attachments)| ChatMessageReq::with_attachments(role, content, attachments)),
        );
        messages
    }

    /// 找到要调用的渠道和模型。模型被停用或删除时退回同渠道第一个可用模型。
    fn resolve_model(&self, provider_id: &str, model_id: &str) -> Option<(&ProviderConfig, &ModelConfig)> {
        let provider = self
            .config
            .providers
            .iter()
            .find(|provider| provider.id == provider_id)
            .or_else(|| self.config.get_active_provider())?;
        let model = provider
            .models
            .iter()
            .find(|model| model.id == model_id && model.enabled)
            .or_else(|| provider.models.iter().find(|model| model.enabled))?;
        Some((provider, model))
    }

    fn make_job(
        &self,
        message_id: &str,
        variant_id: Option<&str>,
        provider_id: &str,
        model_id: &str,
        messages: Vec<ChatMessageReq>,
    ) -> Option<Job> {
        let (provider, model) = self.resolve_model(provider_id, model_id)?;
        let session = self.storage.get_active_session()?;
        let resolved = session.resolved_params(&self.config.system_prompt, self.config.temperature);
        let explicit_temperature = session
            .params
            .as_ref()
            .is_some_and(|params| params.temperature.is_some());
        Some(Job {
            key: stream_key(message_id, variant_id),
            message_id: message_id.to_string(),
            variant_id: variant_id.map(str::to_string),
            request: chat_request(provider, model, messages, &resolved, explicit_temperature),
        })
    }

    fn spawn_jobs(&mut self, jobs: Vec<Job>, cx: &mut Context<Self>) {
        if jobs.is_empty() {
            return;
        }
        self.is_streaming = true;
        for job in jobs {
            let (cancel_tx, cancel_rx) = oneshot::channel();
            self.active_streams.insert(job.key.clone(), cancel_tx);
            let key = job.key.clone();
            let message_id = job.message_id.clone();
            let variant_id = job.variant_id.clone();
            let request = job.request;
            cx.spawn(async move |this, cx| {
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
                runtime().spawn(async move {
                    stream_chat(request, tx, Some(cancel_rx)).await;
                });
                while let Some(event) = rx.recv().await {
                    update_state(&this, cx, |state, cx| {
                        state.apply_stream_event(&key, &message_id, variant_id.as_deref(), event, cx)
                    });
                }
                update_state(&this, cx, |state, cx| {
                    state.apply_stream_event(&key, &message_id, variant_id.as_deref(), StreamEvent::Done, cx)
                });
            })
            .detach();
        }
        self.persist_storage(cx);
        cx.notify();
    }

    fn apply_stream_event(
        &mut self,
        key: &str,
        message_id: &str,
        variant_id: Option<&str>,
        event: StreamEvent,
        cx: &mut Context<Self>,
    ) {
        if !self.active_streams.contains_key(key) {
            return;
        }
        let finished = matches!(event, StreamEvent::Done | StreamEvent::Error(_));
        if let Some(message) = self.find_message_mut(message_id) {
            if let Some(variant_id) = variant_id {
                if let Some(variant) = message.variants.iter_mut().find(|variant| variant.id == variant_id) {
                    apply_to_variant(variant, &event);
                }
                message.is_streaming = message.variants.iter().any(|variant| variant.is_streaming);
            } else {
                apply_to_message(message, &event);
            }
        }
        if finished {
            self.active_streams.remove(key);
            self.is_streaming = !self.active_streams.is_empty();
            if !self.is_streaming {
                self.persist_storage(cx);
                self.maybe_autotitle(cx);
            }
        }
        cx.notify();
    }

    fn maybe_autotitle(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.storage.get_active_session() else {
            return;
        };
        if !session.title_auto || session.messages.len() < 2 {
            return;
        }
        let session_id = session.id.clone();
        let excerpt = session
            .messages
            .iter()
            .find(|message| message.role == "user")
            .map(|message| message.content.chars().take(400).collect::<String>())
            .unwrap_or_default();
        if excerpt.is_empty() {
            return;
        }
        if let Some(session) = self
            .storage
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.title_auto = false;
        }
        let provider_id = self
            .storage
            .get_active_session()
            .map(|s| s.provider_id.clone())
            .unwrap_or_default();
        let model_id = self
            .storage
            .get_active_session()
            .map(|s| s.model.clone())
            .unwrap_or_default();
        let Some(job) = self.make_job(
            "title",
            None,
            &provider_id,
            &model_id,
            vec![
                ChatMessageReq::new("system", "用不超过16个字给对话起标题，只输出标题本身，不要标点包裹。"),
                ChatMessageReq::new("user", excerpt),
            ],
        ) else {
            return;
        };
        let mut request = job.request;
        request.stream = false;
        // 起标题用最弱的思考强度；还要思考的模型不限制输出长度，否则思考就把额度用完了
        let (levels, thinks) = self
            .resolve_model(&provider_id, &model_id)
            .map(|(_, model)| (model.effective_reasoning_levels(), model.thinks()))
            .unwrap_or_default();
        request.reasoning = levels.first().copied();
        let quiet = !thinks || request.reasoning == Some(ReasoningLevel::Off);
        request.max_tokens = if quiet { Some(32) } else { None };
        if request.temperature.is_some() {
            request.temperature = Some(0.2);
        }
        cx.spawn(async move |this, cx| {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            runtime().spawn(async move {
                stream_chat(request, tx, None).await;
            });
            let mut title = String::new();
            while let Some(event) = rx.recv().await {
                if let StreamEvent::Content(text) = event {
                    title.push_str(&text);
                }
            }
            let title = title
                .trim()
                .trim_matches('"')
                .trim()
                .chars()
                .take(24)
                .collect::<String>();
            if title.is_empty() {
                return;
            }
            update_state(&this, cx, |state, cx| {
                if let Some(session) = state
                    .storage
                    .sessions
                    .iter_mut()
                    .find(|session| session.id == session_id)
                {
                    session.title = title;
                    session.title_auto = false;
                    state.persist_storage(cx);
                    cx.notify();
                }
            });
        })
        .detach();
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

    fn push_local_assistant(&mut self, text: &str) {
        let active_id = self.storage.active_session_id.clone();
        if let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) {
            let mut message = ChatMessage::new_assistant();
            message.is_streaming = false;
            message.content = text.to_string();
            session.messages.push(message);
        }
    }

    fn selection_text(&self, cx: &Context<Self>) -> String {
        self.chat_input.read(cx).selected_text().to_string()
    }
}

fn stream_key(message_id: &str, variant_id: Option<&str>) -> String {
    match variant_id {
        Some(variant_id) => format!("{message_id}:{variant_id}"),
        None => message_id.to_string(),
    }
}

fn truncate_title(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() > 18 {
        format!("{}...", trimmed.chars().take(18).collect::<String>())
    } else {
        trimmed.to_string()
    }
}

fn clipboard_text(cx: &App) -> String {
    cx.read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap_or_default()
}

fn chat_request(
    provider: &ProviderConfig,
    model: &ModelConfig,
    messages: Vec<ChatMessageReq>,
    params: &ResolvedParams,
    explicit_temperature: bool,
) -> ChatRequest {
    let levels = model.effective_reasoning_levels();
    // 对话没指定时用模型的默认强度；模型不支持的档位不发送
    let reasoning = params
        .reasoning
        .or(model.default_reasoning)
        .filter(|level| levels.contains(level));
    let max_output = model.effective_max_output();
    // OpenAI 的推理模型只接受默认温度：没有在对话参数里专门设置过就不发送
    let openai = matches!(
        provider.channel_type,
        ChannelType::OpenAiChat | ChannelType::OpenAiResponses
    );
    let omit_sampling = openai && model.thinks() && !explicit_temperature;
    ChatRequest {
        channel_type: provider.channel_type,
        base_url: provider.base_url.clone(),
        api_key: provider.api_key.clone(),
        model: model.id.clone(),
        messages,
        temperature: (!omit_sampling).then_some(params.temperature),
        top_p: if omit_sampling { None } else { params.top_p },
        max_tokens: params
            .max_tokens
            .map(|value| max_output.map_or(value, |cap| value.min(cap))),
        stream: params.stream,
        reasoning,
        max_output,
        model_thinks: model.thinks(),
        extra_headers: provider
            .extra_headers
            .iter()
            .map(|header| (header.name.clone(), header.value.clone()))
            .collect(),
        proxy: provider.proxy.clone(),
        timeout_secs: if provider.timeout_secs == 0 {
            90
        } else {
            provider.timeout_secs
        },
        retries: provider.retries,
    }
}

fn apply_to_message(message: &mut ChatMessage, event: &StreamEvent) {
    match event {
        StreamEvent::Thinking(text) => message.reasoning_content.get_or_insert_with(String::new).push_str(text),
        StreamEvent::Content(text) => message.content.push_str(text),
        StreamEvent::Metrics {
            tokens_prompt,
            tokens_completion,
            speed_tps,
            latency_ms,
        } => {
            message.prompt_tokens = *tokens_prompt;
            message.completion_tokens = *tokens_completion;
            message.speed_tps = *speed_tps;
            message.latency_ms = *latency_ms;
        }
        StreamEvent::Done => message.is_streaming = false,
        StreamEvent::Error(error) => {
            message.error = Some(error.clone());
            message.is_streaming = false;
        }
    }
}

fn apply_to_variant(variant: &mut MessageVariant, event: &StreamEvent) {
    match event {
        StreamEvent::Thinking(text) => variant.reasoning_content.get_or_insert_with(String::new).push_str(text),
        StreamEvent::Content(text) => variant.content.push_str(text),
        StreamEvent::Metrics {
            tokens_prompt,
            tokens_completion,
            speed_tps,
            latency_ms,
        } => {
            variant.prompt_tokens = *tokens_prompt;
            variant.completion_tokens = *tokens_completion;
            variant.speed_tps = *speed_tps;
            variant.latency_ms = *latency_ms;
        }
        StreamEvent::Done => variant.is_streaming = false,
        StreamEvent::Error(error) => {
            variant.error = Some(error.clone());
            variant.is_streaming = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HeaderPair;

    fn provider(channel_type: ChannelType) -> ProviderConfig {
        ProviderConfig {
            id: "p".into(),
            name: "P".into(),
            channel_type,
            base_url: "https://example.test/v1".into(),
            api_path: String::new(),
            api_key: String::new(),
            api_key_ref: String::new(),
            enabled: true,
            models: Vec::new(),
            timeout_secs: 0,
            retries: 0,
            proxy: String::new(),
            extra_headers: vec![HeaderPair {
                name: "X-A".into(),
                value: "1".into(),
            }],
        }
    }

    fn params(reasoning: Option<ReasoningLevel>, max_tokens: Option<u32>) -> ResolvedParams {
        ResolvedParams {
            system_prompt: String::new(),
            temperature: 0.7,
            top_p: Some(0.9),
            max_tokens,
            context_limit: None,
            reasoning,
            stream: true,
        }
    }

    #[std::prelude::v1::test]
    fn reasoning_models_skip_default_temperature_on_openai() {
        let o3 = ModelConfig::new("o3-mini", "o3-mini");
        let request = chat_request(
            &provider(ChannelType::OpenAiChat),
            &o3,
            Vec::new(),
            &params(None, None),
            false,
        );
        assert_eq!(request.temperature, None);
        assert_eq!(request.top_p, None);
        let request = chat_request(
            &provider(ChannelType::OpenAiChat),
            &o3,
            Vec::new(),
            &params(None, None),
            true,
        );
        assert_eq!(request.temperature, Some(0.7), "explicit temperature is kept");

        let gpt = ModelConfig::new("gpt-4o", "GPT-4o");
        let request = chat_request(
            &provider(ChannelType::OpenAiChat),
            &gpt,
            Vec::new(),
            &params(None, None),
            false,
        );
        assert_eq!(request.temperature, Some(0.7));

        let claude = ModelConfig::new("claude-sonnet-4-5", "Claude");
        let request = chat_request(
            &provider(ChannelType::Claude),
            &claude,
            Vec::new(),
            &params(None, None),
            false,
        );
        assert_eq!(
            request.temperature,
            Some(0.7),
            "Claude still honours the global temperature"
        );
        assert_eq!(request.max_output, Some(64_000));
    }

    #[std::prelude::v1::test]
    fn model_default_reasoning_and_output_limit_apply() {
        let mut model = ModelConfig::new("claude-opus-4-1", "Opus");
        model.default_reasoning = Some(ReasoningLevel::High);
        let request = chat_request(
            &provider(ChannelType::Claude),
            &model,
            Vec::new(),
            &params(None, Some(100_000)),
            false,
        );
        assert_eq!(request.reasoning, Some(ReasoningLevel::High));
        assert_eq!(request.max_tokens, Some(32_000), "clamped to the model's output limit");

        let request = chat_request(
            &provider(ChannelType::Claude),
            &model,
            Vec::new(),
            &params(Some(ReasoningLevel::Off), None),
            false,
        );
        assert_eq!(
            request.reasoning,
            Some(ReasoningLevel::Off),
            "the conversation's choice wins"
        );

        // 模型不支持的档位不发送
        let plain = ModelConfig::new("gpt-4o", "GPT-4o");
        let request = chat_request(
            &provider(ChannelType::OpenAiChat),
            &plain,
            Vec::new(),
            &params(Some(ReasoningLevel::High), None),
            false,
        );
        assert_eq!(request.reasoning, None);
    }
}
