use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::popover::{Popover, PopoverState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::Palette;
use super::brand_icon::{model_avatar, model_badges, model_id_avatar, provider_avatar};
use crate::app::AppState;
use crate::config::ModelConfig;
use crate::i18n::{Key, tr};

/// 当前对话使用的模型 ID，以及它在渠道里的配置（找不到时为空）
fn current_model(state: &AppState) -> (String, Option<ModelConfig>) {
    let session = state.storage.get_active_session();
    let provider_id = session
        .map(|s| s.provider_id.as_str())
        .filter(|id| !id.is_empty())
        .unwrap_or(&state.config.active_provider_id);
    let default_model = state.config.default_model_selection().1;
    let model_id = session
        .map(|s| s.model.as_str())
        .filter(|id| !id.is_empty() && *id != "default")
        .unwrap_or(&default_model)
        .to_string();
    let config = state
        .config
        .providers
        .iter()
        .find(|p| p.id == provider_id)
        .or_else(|| state.config.get_active_provider())
        .and_then(|p| p.models.iter().find(|m| m.id == model_id))
        .cloned();
    (model_id, config)
}

/// 当前模型的展示名（优先使用渠道里配置的显示名称）
pub fn current_model_label(state: &AppState) -> String {
    let lang = state.language();
    if state.config.providers.is_empty() {
        return tr(lang, Key::NoModelConfigured).to_string();
    }
    let (model_id, config) = current_model(state);
    config.map(|model| model.name).unwrap_or(model_id)
}

/// 输入框工具栏上的模型选择器
pub fn render_model_picker(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let app = cx.entity();
    let avatar = match current_model(state) {
        (_, Some(model)) => model_avatar(&model, px(16.), p),
        (model_id, None) if !state.config.providers.is_empty() => model_id_avatar(&model_id, px(16.), p),
        _ => Icon::new(IconName::Sparkles).size(px(14.)).into_any_element(),
    };

    Popover::new("model-picker")
        .anchor(Anchor::BottomLeft)
        .w(px(400.))
        .trigger(
            Button::new("model-picker-trigger")
                .ghost()
                .xsmall()
                .child(avatar)
                .child(div().whitespace_nowrap().child(current_model_label(state)))
                .dropdown_caret(true),
        )
        .content(move |popover, window, cx| render_model_list(&app, popover, window, cx))
}

fn render_model_list(
    app: &Entity<AppState>,
    _: &mut PopoverState,
    _: &mut Window,
    cx: &mut Context<PopoverState>,
) -> impl IntoElement + use<> {
    let p = Palette::new(cx);
    let lang = app.read(cx).language();
    let (providers, active_provider_id, active_model, search_input) = {
        let state = app.read(cx);
        (
            state.config.providers.clone(),
            state
                .storage
                .get_active_session()
                .map(|s| s.provider_id.clone())
                .filter(|id| !id.is_empty())
                .unwrap_or_else(|| state.config.active_provider_id.clone()),
            state
                .storage
                .get_active_session()
                .map(|s| s.model.clone())
                .filter(|id| !id.is_empty() && id != "default")
                .unwrap_or_else(|| state.config.default_model_selection().1),
            state.model_picker_search_input.clone(),
        )
    };
    let query = search_input.read(cx).value().to_lowercase();

    let mut groups: Vec<AnyElement> = Vec::new();
    for provider in providers.iter().filter(|p| p.enabled) {
        let mut models: Vec<_> = provider
            .models
            .iter()
            .filter(|m| m.enabled)
            .filter(|m| {
                query.is_empty()
                    || m.name.to_lowercase().contains(&query)
                    || m.id.to_lowercase().contains(&query)
                    || provider.name.to_lowercase().contains(&query)
            })
            .collect();
        if models.is_empty() {
            continue;
        }
        // 置顶的模型排在前面
        models.sort_by_key(|m| !m.is_pinned);

        groups.push(
            h_flex()
                .gap_1p5()
                .px_2()
                .pt_2()
                .pb_1()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.muted_foreground)
                .child(provider_avatar(provider, px(14.), &p))
                .child(provider.name.clone())
                .into_any_element(),
        );

        for model in models {
            let is_selected = provider.id == active_provider_id && model.id == active_model;
            let pid = provider.id.clone();
            let mid = model.id.clone();
            let select_app = app.clone();

            groups.push(
                h_flex()
                    .id(SharedString::from(format!("pick-{}-{}", provider.id, model.id)))
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .cursor_pointer()
                    .text_sm()
                    .when(is_selected, |this| this.bg(p.accent))
                    .hover(|s| s.bg(p.accent))
                    .on_click(cx.listener(move |popover, _, window, cx| {
                        select_app.update(cx, |this, cx| this.select_model(&pid, &mid, cx));
                        popover.dismiss(window, cx);
                    }))
                    .child(model_avatar(model, px(20.), &p))
                    .child(div().flex_1().min_w_0().truncate().child(model.name.clone()))
                    .when(model.is_pinned, |this| {
                        this.child(Icon::new(IconName::Pin).size(px(12.)).text_color(p.muted_foreground))
                    })
                    .child(model_badges(model, &p))
                    .child(div().flex_none().w(px(16.)).when(is_selected, |this| {
                        this.child(Icon::new(IconName::Check).size(px(14.)).text_color(p.primary))
                    }))
                    .into_any_element(),
            );
        }
    }

    let goto_app = app.clone();
    let has_models = !groups.is_empty();

    v_flex()
        .gap_1()
        .child(
            Input::new(&search_input).small().cleanable(true).prefix(
                Icon::new(IconName::Search)
                    .small()
                    .text_color(cx.theme().muted_foreground),
            ),
        )
        .map(|this| {
            if has_models {
                this.child(
                    div()
                        .max_h(px(340.))
                        .child(v_flex().gap_px().children(groups).overflow_y_scrollbar()),
                )
            } else {
                this.child(
                    v_flex()
                        .items_center()
                        .gap_3()
                        .py_6()
                        .text_sm()
                        .text_color(p.muted_foreground)
                        .child(if query.is_empty() {
                            tr(lang, Key::NoAvailableModel)
                        } else {
                            tr(lang, Key::NoMatchingModel)
                        })
                        .child(
                            Button::new("goto-providers")
                                .outline()
                                .small()
                                .icon(IconName::Settings)
                                .label(tr(lang, Key::ManageModelChannel))
                                .on_click(cx.listener(move |popover, _, window, cx| {
                                    goto_app.update(cx, |this, cx| this.open_providers_settings(window, cx));
                                    popover.dismiss(window, cx);
                                })),
                        ),
                )
            }
        })
}
