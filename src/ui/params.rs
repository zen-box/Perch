use super::Palette;
use super::brand_icon::model_avatar;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::popover::{Popover, PopoverState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use crate::app::AppState;
use crate::i18n::{Key, tr, tr_args};

pub fn render_params_button(cx: &mut Context<AppState>) -> impl IntoElement {
    let app = cx.entity();
    let lang = app.read(cx).language();
    Popover::new("chat-params")
        .anchor(Anchor::BottomLeft)
        .w(px(360.))
        .trigger(
            Button::new("params-trigger")
                .ghost()
                .xsmall()
                .icon(IconName::SlidersHorizontal)
                .label(tr(lang, Key::Params)),
        )
        .content(move |_, _, cx| render_params(&app, cx))
}

fn render_params(app: &Entity<AppState>, cx: &mut Context<PopoverState>) -> impl IntoElement + use<> {
    let p = Palette::new(cx);
    let lang = app.read(cx).language();
    let (params, levels, model_default) = {
        let state = app.read(cx);
        let session = state.storage.get_active_session();
        let params = session.and_then(|session| session.params.clone()).unwrap_or_default();
        let model = session.and_then(|session| {
            state
                .config
                .providers
                .iter()
                .find(|provider| provider.id == session.provider_id)
                .and_then(|provider| provider.models.iter().find(|model| model.id == session.model).cloned())
        });
        let levels = model
            .as_ref()
            .map(|model| model.effective_reasoning_levels())
            .unwrap_or_default();
        (params, levels, model.and_then(|model| model.default_reasoning))
    };
    let prompt = app.read(cx).params_prompt_input.clone();
    let temperature = params.temperature;
    let top_p = params.top_p;
    let max_tokens = params.max_tokens;
    let context_limit = params.context_limit;
    let reasoning = params.reasoning;
    let stream = params.stream;

    v_flex()
        .gap_3()
        .max_h(px(480.))
        .overflow_y_scrollbar()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(tr(lang, Key::ChatParams)),
        )
        .child(
            div()
                .text_xs()
                .text_color(p.muted_foreground)
                .child(tr(lang, Key::ParamsDefaultHint)),
        )
        .child(Textarea::new(&prompt))
        .child(choice_row(
            app,
            ChoiceRow {
                id_prefix: "temp",
                title: tr(lang, Key::TemperatureLabel),
                current: temperature
                    .map(|value| format!("{value:.1}"))
                    .unwrap_or_else(|| tr(lang, Key::DefaultValue).into()),
                selected: temperature,
                options: vec![
                    (None, tr(lang, Key::DefaultValue)),
                    (Some(0.2), "0.2"),
                    (Some(0.7), "0.7"),
                    (Some(1.0), "1.0"),
                ],
            },
            &p,
            |value, this, cx| this.patch_params(cx, |params| params.temperature = value),
        ))
        .child(choice_row(
            app,
            ChoiceRow {
                id_prefix: "top-p",
                title: "top_p",
                current: top_p
                    .map(|value| format!("{value:.2}"))
                    .unwrap_or_else(|| tr(lang, Key::DefaultValue).into()),
                selected: top_p,
                options: vec![
                    (None, tr(lang, Key::DefaultValue)),
                    (Some(0.8), "0.8"),
                    (Some(0.95), "0.95"),
                    (Some(1.0), "1.0"),
                ],
            },
            &p,
            |value, this, cx| this.patch_params(cx, |params| params.top_p = value),
        ))
        .child(choice_row(
            app,
            ChoiceRow {
                id_prefix: "max-tokens",
                title: tr(lang, Key::MaxTokens),
                current: max_tokens
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| tr(lang, Key::DefaultValue).into()),
                selected: max_tokens,
                options: vec![
                    (None, tr(lang, Key::DefaultValue)),
                    (Some(1024u32), "1024"),
                    (Some(4096), "4096"),
                    (Some(8192), "8192"),
                ],
            },
            &p,
            |value, this, cx| this.patch_params(cx, |params| params.max_tokens = value),
        ))
        .child(choice_row(
            app,
            ChoiceRow {
                id_prefix: "context",
                title: tr(lang, Key::ContextMessages),
                current: context_limit
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| tr(lang, Key::All).into()),
                selected: context_limit,
                options: vec![
                    (None, tr(lang, Key::All)),
                    (Some(10usize), "10"),
                    (Some(20), "20"),
                    (Some(40), "40"),
                ],
            },
            &p,
            |value, this, cx| this.patch_params(cx, |params| params.context_limit = value),
        ))
        // 档位来自模型设置；「默认」表示用模型设置里的默认强度
        .when(!levels.is_empty(), |this| {
            let app = app.clone();
            let current = reasoning.filter(|level| levels.contains(level));
            let default_label = match model_default {
                Some(level) => tr_args(lang, Key::DefaultWithArg, &[level.label(lang)]),
                None => tr(lang, Key::DefaultValue).to_string(),
            };
            let value_label = current
                .map(|level| level.label(lang).to_string())
                .unwrap_or_else(|| default_label.clone());
            this.child(labeled(
                tr(lang, Key::ReasoningEffort).to_string(),
                value_label,
                &p,
                h_flex().flex_wrap().gap_1().children(
                    std::iter::once(None)
                        .chain(levels.iter().copied().map(Some))
                        .enumerate()
                        .map(|(ix, level)| {
                            let app = app.clone();
                            Button::new(("reason", ix))
                                .xsmall()
                                .map(|button| {
                                    if current == level {
                                        button.primary()
                                    } else {
                                        button.ghost()
                                    }
                                })
                                .label(
                                    level
                                        .map(|level| level.label(lang))
                                        .unwrap_or(tr(lang, Key::DefaultValue)),
                                )
                                .on_click(move |_, _, cx| {
                                    app.update(cx, |this, cx| this.patch_params(cx, |params| params.reasoning = level));
                                })
                        }),
                ),
            ))
        })
        .child(
            h_flex()
                .justify_between()
                .child(div().text_sm().child(tr(lang, Key::StreamingOutput)))
                .child(Switch::new("stream-toggle").checked(stream).on_click({
                    let app = app.clone();
                    move |checked, _, cx| {
                        let checked = *checked;
                        app.update(cx, |this, cx| this.patch_params(cx, |params| params.stream = checked));
                    }
                })),
        )
        .child(
            Button::new("reset-params")
                .outline()
                .small()
                .label(tr(lang, Key::RestoreDefaults))
                .on_click({
                    let app = app.clone();
                    move |_, window, cx| app.update(cx, |this, cx| this.reset_params(window, cx))
                }),
        )
}

fn labeled(title: String, value: String, p: &Palette, control: impl IntoElement) -> AnyElement {
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .justify_between()
                .child(div().text_sm().child(title))
                .child(div().text_xs().text_color(p.muted_foreground).child(value)),
        )
        .child(control)
        .into_any_element()
}

/// 一行互斥选项的展示信息：`id_prefix` 用来生成按钮 id，`current` 是右侧显示的当前值文本。
struct ChoiceRow<'a, T> {
    id_prefix: &'static str,
    title: &'a str,
    current: String,
    selected: Option<T>,
    options: Vec<(Option<T>, &'static str)>,
}

fn choice_row<T: Copy + PartialEq + 'static>(
    app: &Entity<AppState>,
    row: ChoiceRow<'_, T>,
    p: &Palette,
    on_select: impl Fn(Option<T>, &mut AppState, &mut Context<AppState>) + 'static,
) -> AnyElement {
    let ChoiceRow {
        id_prefix,
        title,
        current,
        selected,
        options,
    } = row;
    let on_select = std::rc::Rc::new(on_select);
    labeled(
        title.to_string(),
        current,
        p,
        h_flex()
            .gap_1()
            .children(options.into_iter().enumerate().map(|(ix, (value, label))| {
                let app = app.clone();
                let on_select = on_select.clone();
                let active = value == selected;
                Button::new((id_prefix, ix))
                    .xsmall()
                    .map(|button| if active { button.primary() } else { button.ghost() })
                    .label(label)
                    .on_click(move |_, _, cx| {
                        app.update(cx, |this, cx| on_select(value, this, cx));
                    })
            })),
    )
}

pub fn render_compare_button(state: &AppState, cx: &mut Context<AppState>) -> impl IntoElement {
    let app = cx.entity();
    let lang = state.language();
    let compare_count = if state.compare_selection.is_empty() {
        0
    } else {
        state.compare_targets().len()
    };
    Popover::new("compare-picker")
        .anchor(Anchor::BottomLeft)
        .w(px(320.))
        .trigger(
            Button::new("compare-trigger")
                .ghost()
                .xsmall()
                .icon(IconName::Columns2)
                .when(compare_count >= 2, |this| this.primary())
                .label(if compare_count == 0 {
                    tr(lang, Key::Compare).into()
                } else {
                    tr_args(lang, Key::CompareCount, &[&compare_count.to_string()])
                }),
        )
        .content(move |_, _, cx| render_compare(&app, cx))
}

fn render_compare(app: &Entity<AppState>, cx: &mut Context<PopoverState>) -> impl IntoElement + use<> {
    let p = Palette::new(cx);
    let lang = app.read(cx).language();
    let (providers, selected, current_target) = {
        let state = app.read(cx);
        (
            state.config.providers.clone(),
            state.compare_selection.clone(),
            state.active_target(),
        )
    };

    let clear_app = app.clone();
    let has_selection = !selected.is_empty();

    let current_model_cfg = providers
        .iter()
        .find(|p| p.id == current_target.0)
        .and_then(|p| p.models.iter().find(|m| m.id == current_target.1))
        .cloned();
    let current_name = current_model_cfg
        .as_ref()
        .map(|m| m.name.clone())
        .unwrap_or_else(|| current_target.1.clone());

    let mut rows = Vec::new();
    for provider in providers.iter().filter(|provider| provider.enabled) {
        for model in provider.models.iter().filter(|model| model.enabled) {
            let key = (provider.id.clone(), model.id.clone());
            if key == current_target {
                continue;
            }
            let checked = selected.contains(&key);
            let row_app = app.clone();
            let pid = provider.id.clone();
            let mid = model.id.clone();
            rows.push(
                h_flex()
                    .id(SharedString::from(format!("cmp-{}-{}", provider.id, model.id)))
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .cursor_pointer()
                    .when(checked, |style| style.bg(p.accent))
                    .hover(|style| style.bg(p.accent))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        row_app.update(cx, |this, cx| this.toggle_compare_model(&pid, &mid, cx));
                    }))
                    .child(
                        div()
                            .flex_none()
                            .w(px(16.))
                            .h(px(16.))
                            .rounded_sm()
                            .border_1()
                            .border_color(if checked { p.primary } else { p.border })
                            .when(checked, |this| this.bg(p.primary))
                            .items_center()
                            .justify_center()
                            .when(checked, |this| {
                                this.child(
                                    Icon::new(IconName::Check)
                                        .size(px(12.))
                                        .text_color(p.primary_foreground),
                                )
                            }),
                    )
                    .child(model_avatar(model, px(18.), &p))
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(model.name.clone()))
                    .into_any_element(),
            );
        }
    }

    let total_count = 1 + selected.len();
    let can_run = !selected.is_empty();
    let run_app = app.clone();

    v_flex()
        .gap_2()
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(tr(lang, Key::ModelCompare)),
                )
                .when(has_selection, |this| {
                    this.child(
                        Button::new("clear-cmp")
                            .ghost()
                            .xsmall()
                            .label(tr(lang, Key::Clear))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                clear_app.update(cx, |this, cx| this.clear_compare_selection(cx));
                            })),
                    )
                }),
        )
        .child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(p.muted_foreground)
                        .child(tr(lang, Key::CurrentModelBaseline)),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1p5()
                        .rounded_md()
                        .bg(p.accent)
                        .child(
                            div()
                                .flex_none()
                                .w(px(16.))
                                .h(px(16.))
                                .rounded_sm()
                                .bg(p.primary)
                                .border_1()
                                .border_color(p.primary)
                                .items_center()
                                .justify_center()
                                .child(
                                    Icon::new(IconName::Check)
                                        .size(px(12.))
                                        .text_color(p.primary_foreground),
                                ),
                        )
                        .child(match &current_model_cfg {
                            Some(m) => model_avatar(m, px(18.), &p),
                            None => super::brand_icon::model_id_avatar(&current_target.1, px(18.), &p),
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(current_name),
                        )
                        .child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded_sm()
                                .bg(p.primary.opacity(0.12))
                                .text_xs()
                                .text_color(p.primary)
                                .child(tr(lang, Key::Current)),
                        ),
                ),
        )
        .child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(p.muted_foreground)
                        .child(tr(lang, Key::ComparePickModels)),
                )
                .child(if rows.is_empty() {
                    div()
                        .text_sm()
                        .text_color(p.muted_foreground)
                        .child(tr(lang, Key::NoOtherEnabledModel))
                        .into_any_element()
                } else {
                    div()
                        .max_h(px(200.))
                        .child(v_flex().children(rows).overflow_y_scrollbar())
                        .into_any_element()
                }),
        )
        .child(
            Button::new("run-compare")
                .primary()
                .small()
                .label(if can_run {
                    tr_args(lang, Key::StartCompareWithCount, &[&total_count.to_string()])
                } else {
                    tr(lang, Key::PleasePickCompareModel).into()
                })
                .disabled(!can_run)
                .on_click(cx.listener(move |popover, _, window, cx| {
                    run_app.update(cx, |this, cx| this.send_compare(window, cx));
                    popover.dismiss(window, cx);
                })),
        )
}
