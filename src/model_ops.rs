use gpui_kit::*;

use crate::app::{AppState, ToastLevel};
use crate::config::ModelConfig;
use crate::i18n::{Key, tr, tr_args};
use crate::model::ReasoningLevel;
use crate::model_info::{self, Capability, TokenParseError};

/// 「添加 / 编辑模型」弹窗的草稿，点保存之前不改动配置
#[derive(Clone, Debug)]
pub struct ModelEditor {
    pub provider_id: String,
    /// 编辑已有模型时是它原来的 ID；添加新模型时为空
    pub original_id: Option<String>,
    pub draft: ModelConfig,
    /// 自定义数值没法解析时的提示
    pub context_error: Option<String>,
    pub output_error: Option<String>,
}

impl ModelEditor {
    pub fn is_new(&self) -> bool {
        self.original_id.is_none()
    }
}

/// 数值输入框对应的字段
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenField {
    Context,
    Output,
}

impl AppState {
    pub fn begin_add_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let provider_id = self.selected_settings_provider_id.clone();
        self.open_model_editor(provider_id, None, ModelConfig::new("", ""), window, cx);
    }

    pub fn begin_edit_model(&mut self, provider_id: &str, model_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(model) = self
            .config
            .providers
            .iter()
            .find(|provider| provider.id == provider_id)
            .and_then(|provider| provider.models.iter().find(|model| model.id == model_id))
            .cloned()
        else {
            return;
        };
        self.open_model_editor(provider_id.to_string(), Some(model_id.to_string()), model, window, cx);
    }

    fn open_model_editor(
        &mut self,
        provider_id: String,
        original_id: Option<String>,
        draft: ModelConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_new = original_id.is_none();
        let id = draft.id.clone();
        // 显示名称和 ID 相同时输入框留空，占位文字会提示
        let name = if draft.name == draft.id {
            String::new()
        } else {
            draft.name.clone()
        };
        let context = draft
            .context_window
            .map(model_info::format_tokens_exact)
            .unwrap_or_default();
        let output = draft
            .max_output
            .map(model_info::format_tokens_exact)
            .unwrap_or_default();
        self.model_editor = Some(ModelEditor {
            provider_id,
            original_id,
            draft,
            context_error: None,
            output_error: None,
        });
        self.model_edit_id_input
            .update(cx, |input, cx| input.set_value(&id, window, cx));
        self.model_edit_name_input
            .update(cx, |input, cx| input.set_value(&name, window, cx));
        self.model_edit_context_input
            .update(cx, |input, cx| input.set_value(&context, window, cx));
        self.model_edit_output_input
            .update(cx, |input, cx| input.set_value(&output, window, cx));
        if is_new {
            self.model_edit_id_input.update(cx, |input, cx| input.focus(window, cx));
        }
        cx.notify();
    }

    /// 修改草稿（能力、思考强度、图标等按钮）
    pub fn update_model_draft(&mut self, cx: &mut Context<Self>, update: impl FnOnce(&mut ModelConfig)) {
        if let Some(editor) = self.model_editor.as_mut() {
            update(&mut editor.draft);
            // 默认强度必须是支持的档位之一
            let levels = editor.draft.effective_reasoning_levels();
            if editor
                .draft
                .default_reasoning
                .is_some_and(|level| !levels.contains(&level))
            {
                editor.draft.default_reasoning = None;
            }
            cx.notify();
        }
    }

    /// 点击预设值：同时写回输入框，`None` 表示自动识别
    pub fn set_model_draft_tokens(
        &mut self,
        field: TokenField,
        value: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.model_editor.as_mut() else {
            return;
        };
        match field {
            TokenField::Context => {
                editor.draft.context_window = value;
                editor.context_error = None;
            }
            TokenField::Output => {
                editor.draft.max_output = value;
                editor.output_error = None;
            }
        }
        let text = value.map(model_info::format_tokens_exact).unwrap_or_default();
        let input = match field {
            TokenField::Context => self.model_edit_context_input.clone(),
            TokenField::Output => self.model_edit_output_input.clone(),
        };
        input.update(cx, |input, cx| input.set_value(&text, window, cx));
        cx.notify();
    }

    /// 用户在输入框里改了数值
    pub(crate) fn sync_model_draft_tokens(&mut self, field: TokenField, cx: &mut Context<Self>) {
        let text = match field {
            TokenField::Context => self.model_edit_context_input.read(cx).value().to_string(),
            TokenField::Output => self.model_edit_output_input.read(cx).value().to_string(),
        };
        // 语言要在借用 self.model_editor 之前取，之后 self 就被 editor 借走了
        let lang = self.language();
        let Some(editor) = self.model_editor.as_mut() else {
            return;
        };
        let (value, error) = match model_info::parse_tokens(&text) {
            Ok(value) => (value, None),
            Err(error) => (
                None,
                Some(match error {
                    TokenParseError::NotANumber => tr_args(lang, Key::TokenNotANumber, &[&text]),
                    TokenParseError::OutOfRange => tr_args(lang, Key::TokenOutOfRange, &[&text]),
                }),
            ),
        };
        match field {
            TokenField::Context => {
                if error.is_none() {
                    editor.draft.context_window = value;
                }
                editor.context_error = error;
            }
            TokenField::Output => {
                if error.is_none() {
                    editor.draft.max_output = value;
                }
                editor.output_error = error;
            }
        }
        cx.notify();
    }

    /// 添加模型时，ID 一变就重新识别规格
    pub(crate) fn sync_model_draft_id(&mut self, cx: &mut Context<Self>) {
        let id = self.model_edit_id_input.read(cx).value().trim().to_string();
        if let Some(editor) = self.model_editor.as_mut().filter(|editor| editor.is_new())
            && editor.draft.id != id
        {
            editor.draft.id = id;
            cx.notify();
        }
    }

    pub fn toggle_model_draft_capability(&mut self, capability: Capability, cx: &mut Context<Self>) {
        self.update_model_draft(cx, |draft| {
            let mut capabilities = draft.effective_capabilities();
            if let Some(ix) = capabilities.iter().position(|item| *item == capability) {
                capabilities.remove(ix);
            } else {
                capabilities.push(capability);
                capabilities.sort();
            }
            draft.capabilities = Some(capabilities);
        });
    }

    pub fn toggle_model_draft_level(&mut self, level: ReasoningLevel, cx: &mut Context<Self>) {
        self.update_model_draft(cx, |draft| {
            let mut levels = draft.effective_reasoning_levels();
            if let Some(ix) = levels.iter().position(|item| *item == level) {
                levels.remove(ix);
            } else {
                levels.push(level);
                levels.sort();
            }
            draft.reasoning_levels = Some(levels);
        });
    }

    /// 清掉所有手动设置，回到按 ID 自动识别
    pub fn reset_model_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.update_model_draft(cx, |draft| {
            draft.capabilities = None;
            draft.reasoning_levels = None;
            draft.default_reasoning = None;
            draft.icon = None;
        });
        self.set_model_draft_tokens(TokenField::Context, None, window, cx);
        self.set_model_draft_tokens(TokenField::Output, None, window, cx);
    }

    /// 返回 true 表示保存成功，弹窗可以关闭
    pub fn confirm_model_editor(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(editor) = self.model_editor.clone() else {
            return false;
        };
        let id = if editor.is_new() {
            self.model_edit_id_input.read(cx).value().trim().to_string()
        } else {
            editor.draft.id.clone()
        };
        let name = self.model_edit_name_input.read(cx).value().trim().to_string();
        if id.is_empty() {
            self.toast(ToastLevel::Error, tr(self.language(), Key::ModelIdRequired));
            cx.notify();
            return false;
        }
        if let Some(error) = editor.context_error.or(editor.output_error) {
            self.toast(ToastLevel::Error, error);
            cx.notify();
            return false;
        }
        let duplicate = self
            .config
            .providers
            .iter()
            .find(|provider| provider.id == editor.provider_id)
            .is_some_and(|provider| {
                provider
                    .models
                    .iter()
                    .any(|model| model.id == id && editor.original_id.as_deref() != Some(id.as_str()))
            });
        if duplicate {
            self.toast(
                ToastLevel::Error,
                tr_args(self.language(), Key::ModelAlreadyExists, &[&id]),
            );
            cx.notify();
            return false;
        }
        let Some(provider) = self
            .config
            .providers
            .iter_mut()
            .find(|provider| provider.id == editor.provider_id)
        else {
            return false;
        };

        let mut model = editor.draft;
        model.id = id.clone();
        model.name = if name.is_empty() { id.clone() } else { name };
        let is_new = editor.original_id.is_none();
        match editor
            .original_id
            .as_deref()
            .and_then(|original| provider.models.iter().position(|item| item.id == original))
        {
            Some(ix) => provider.models[ix] = model,
            None => provider.models.push(model),
        }
        match self.config.save() {
            Ok(()) => {
                self.model_editor = None;
                let lang = self.language();
                self.toast(
                    ToastLevel::Success,
                    if is_new {
                        tr(lang, Key::ModelAdded)
                    } else {
                        tr(lang, Key::ModelSaved)
                    },
                );
                cx.notify();
                true
            }
            Err(error) => {
                self.toast(
                    ToastLevel::Error,
                    tr_args(self.language(), Key::ModelSaveFailed, &[&error.to_string()]),
                );
                cx.notify();
                false
            }
        }
    }
}
