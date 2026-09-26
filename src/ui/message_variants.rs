//! 助手消息的多版本切换，以及多模型对比的分栏展示。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::Palette;
use super::brand_icon::{model_avatar, model_id_avatar};
use super::chat::markdown_view;
use crate::app::AppState;
use crate::i18n::{Key, tr};
use crate::model::ChatMessage;

pub(super) fn render_variants(
    app: &Entity<AppState>,
    ix: usize,
    msg: &ChatMessage,
    p: &Palette,
    cx: &mut App,
) -> impl IntoElement {
    let lang = app.read(cx).language();
    let (owners, expanded_ids) = {
        let state = app.read(cx);
        let owners: Vec<Option<crate::config::ModelConfig>> = msg
            .variants
            .iter()
            .map(|variant| {
                state
                    .config
                    .providers
                    .iter()
                    .find(|provider| provider.id == variant.provider_id)
                    .and_then(|provider| provider.models.iter().find(|model| model.id == variant.model).cloned())
            })
            .collect();
        (owners, state.expanded_reasoning.clone())
    };

    let columns: Vec<_> =
        msg.variants
            .iter()
            .enumerate()
            .map(|(variant_ix, variant)| {
                let owner = owners.get(variant_ix).cloned().flatten();
                let label = owner
                    .as_ref()
                    .map(|model| model.name.clone())
                    .unwrap_or_else(|| variant.model.clone());
                let avatar = match &owner {
                    Some(model) => model_avatar(model, px(20.), p),
                    None => model_id_avatar(&variant.model, px(20.), p),
                };
                let adopt_app = app.clone();
                let toggle_app = app.clone();
                let message_id = msg.id.clone();
                let variant_id = variant.id.clone();

                let variant_has_content = !variant.content.trim().is_empty();
                let variant_reasoning = variant.reasoning_content.clone().filter(|text| !text.trim().is_empty());
                let variant_thinking = variant.is_streaming && !variant_has_content;
                let variant_open = expanded_ids.contains(&variant.id) || variant_thinking;
                let show_thinking = variant_reasoning.is_some() || variant_thinking;

                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(p.border)
                    .bg(p.muted.opacity(if p.is_dark { 0.25 } else { 0.12 }))
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_2()
                            .child(
                                h_flex().items_center().gap_2().min_w_0().child(avatar).child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .truncate()
                                        .child(label),
                                ),
                            )
                            .child({
                                let adopt_var_id = variant_id.clone();
                                Button::new(SharedString::from(format!("adopt-{ix}-{variant_ix}")))
                                    .xsmall()
                                    .primary()
                                    .label(tr(lang, Key::Adopt))
                                    .disabled(variant.is_streaming)
                                    .on_click(move |_, _, cx| {
                                        adopt_app
                                            .update(cx, |this, cx| this.adopt_variant(&message_id, &adopt_var_id, cx));
                                    })
                            }),
                    )
                    .when(show_thinking, |this| {
                        let var_id = variant_id.clone();
                        this.child(
                            v_flex()
                                .w_full()
                                .rounded_md()
                                .border_1()
                                .border_color(p.border)
                                .overflow_hidden()
                                .bg(p.background)
                                .child(
                                    h_flex()
                                        .id(SharedString::from(format!("var-think-toggle-{}", variant.id)))
                                        .gap_2()
                                        .px_2p5()
                                        .py_1p5()
                                        .cursor_pointer()
                                        .text_xs()
                                        .text_color(p.muted_foreground)
                                        .hover(|s| s.bg(p.muted))
                                        .on_click(move |_, _, cx| {
                                            toggle_app.update(cx, |this, cx| this.toggle_reasoning(&var_id, cx));
                                        })
                                        .child(Icon::new(IconName::Brain).size(px(14.)))
                                        .child(div().flex_1().font_weight(FontWeight::MEDIUM).child(
                                            if variant_thinking {
                                                tr(lang, Key::Thinking)
                                            } else {
                                                tr(lang, Key::ThinkingProcess)
                                            },
                                        ))
                                        .child(
                                            Icon::new(if variant_open {
                                                IconName::ChevronDown
                                            } else {
                                                IconName::ChevronRight
                                            })
                                            .size(px(14.)),
                                        ),
                                )
                                .when(variant_open, |this| {
                                    this.when_some(variant_reasoning, |this, reasoning| {
                                        this.child(
                                            div()
                                                .px_2p5()
                                                .pb_2p5()
                                                .pt_1()
                                                .text_xs()
                                                .line_height(relative(1.7))
                                                .text_color(p.muted_foreground)
                                                .child(reasoning),
                                        )
                                    })
                                }),
                        )
                    })
                    .when(variant.is_streaming && !variant_has_content && !show_thinking, |this| {
                        this.child(
                            h_flex()
                                .gap_2()
                                .text_sm()
                                .text_color(p.muted_foreground)
                                .child(Spinner::new().small())
                                .child(tr(lang, Key::Generating)),
                        )
                    })
                    .when(variant_has_content, |this| {
                        this.child(
                            div().text_sm().child(
                                markdown_view(format!("var-{}", variant.id), variant.content.clone())
                                    .stream_fade(variant.is_streaming),
                            ),
                        )
                    })
                    .when_some(variant.error.clone(), |this, err| {
                        this.child(div().text_sm().text_color(p.danger).child(err))
                    })
                    .when(variant.speed_tps > 0.0 || variant.completion_tokens > 0, |this| {
                        this.child(div().pt_1().text_xs().text_color(p.muted_foreground).child(format!(
                            "{} tokens · {:.1} tok/s · {:.1}s",
                            variant.completion_tokens,
                            variant.speed_tps,
                            variant.latency_ms as f32 / 1000.0
                        )))
                    })
            })
            .collect();

    v_flex()
        .w_full()
        .gap_2()
        .child(
            div()
                .text_xs()
                .text_color(p.muted_foreground)
                .child(tr(lang, Key::ComparePickHint)),
        )
        .child(h_flex().w_full().items_start().gap_3().children(columns))
}
