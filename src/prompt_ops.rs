//! 提示词预设与消息模板：保存、删除、插入模板、斜杠命令展开。

use gpui_kit::*;

use crate::app::{AppState, ToastLevel};
use crate::i18n::{Key, tr, tr_args};
use crate::model::ChatParams;
use crate::prompts::{PromptPreset, PromptTemplate, expand_variables};

impl AppState {
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

    pub fn save_prompt_from_inputs(&mut self, is_template: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let name = self.prompt_name_input.read(cx).value().trim().to_string();
        let body = self.prompt_body_input.read(cx).value().to_string();
        let icon = self.prompt_icon_input.read(cx).value().trim().to_string();
        if name.is_empty() || body.trim().is_empty() {
            self.toast(ToastLevel::Error, tr(self.language(), Key::NameAndBodyRequired));
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
            self.toast(
                ToastLevel::Error,
                tr_args(self.language(), Key::SaveFailed, &[&error.to_string()]),
            );
        } else {
            self.toast(ToastLevel::Success, tr(self.language(), Key::Saved));
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
            self.toast(
                ToastLevel::Error,
                tr_args(self.language(), Key::DeleteFailed, &[&error.to_string()]),
            );
        }
        cx.notify();
    }

    fn selection_text(&self, cx: &Context<Self>) -> String {
        self.chat_input.read(cx).selected_text().to_string()
    }
}

fn clipboard_text(cx: &App) -> String {
    cx.read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap_or_default()
}
