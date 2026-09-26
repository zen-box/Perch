//! 助手消息：正文、思考过程、重新生成浮层、用量指标、代码块操作。

use gpui_kit::base::text::CodeBlock;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::popover::{Popover, PopoverState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::brand_icon::{model_avatar, model_badges, provider_avatar};
use super::chat::{format_msg_time, markdown_view};
use super::message_variants;
use super::{Palette, dialogs};
use crate::app::AppState;
use crate::model::ChatMessage;

#[allow(clippy::too_many_arguments)]
pub(super) fn render_assistant_message(
    app: &Entity<AppState>,
    ix: usize,
    msg: ChatMessage,
    avatar: AnyElement,
    model_label: String,
    expanded: bool,
    later: usize,
    p: &Palette,
    cx: &mut App,
) -> impl IntoElement {
    let is_last = later == 0;
    let mono_font = cx.theme().mono_font_family.clone();
    let has_content = !msg.content.trim().is_empty();
    let has_unresolved_variants = !msg.variants.is_empty() && !has_content;
    let total_variants = msg.variants.len();
    let current_var_ix = msg
        .variants
        .iter()
        .position(|v| v.model == msg.model && v.content == msg.content)
        .unwrap_or(0);
    let reasoning = msg.reasoning_content.clone().filter(|text| !text.trim().is_empty());
    let thinking = msg.is_streaming && !has_content;
    let waiting = thinking && reasoning.is_none() && !has_unresolved_variants;
    let has_metrics = msg.completion_tokens > 0 || msg.speed_tps > 0.0;
    let show_actions = !msg.is_streaming && !has_unresolved_variants && (has_content || msg.error.is_some());
    let copy_app = app.clone();
    let copy_text = msg.content.clone();
    let action_id = msg.id.clone();
    let toggle_app = app.clone();
    let toggle_id = msg.id.clone();

    h_flex().w_full().items_start().gap_3().child(avatar).child(
        v_flex()
            .flex_1()
            .min_w_0()
            .gap_2()
            .child(
                h_flex()
                    .h(px(28.))
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(model_label.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.muted_foreground)
                            .child(format_msg_time(&msg.created_at).to_string()),
                    ),
            )
            .when(has_unresolved_variants, |this| {
                this.child(message_variants::render_variants(app, ix, &msg, p, cx))
            })
            // 思考过程（可折叠）
            .when_some(reasoning, |this, reasoning| {
                let open = expanded || thinking;
                this.child(
                    v_flex()
                        .w_full()
                        .rounded_lg()
                        .border_1()
                        .border_color(p.border)
                        .overflow_hidden()
                        .child(
                            h_flex()
                                .id(("reasoning-toggle", ix))
                                .gap_2()
                                .px_3()
                                .py_2()
                                .cursor_pointer()
                                .text_xs()
                                .text_color(p.muted_foreground)
                                .hover(|s| s.bg(p.muted))
                                .on_click(move |_, _, cx| {
                                    toggle_app.update(cx, |this, cx| this.toggle_reasoning(&toggle_id, cx));
                                })
                                .child(Icon::new(IconName::Brain).size(px(14.)))
                                .child(div().flex_1().font_weight(FontWeight::MEDIUM).child(if thinking {
                                    "正在思考…"
                                } else {
                                    "思考过程"
                                }))
                                .child(
                                    Icon::new(if open {
                                        IconName::ChevronDown
                                    } else {
                                        IconName::ChevronRight
                                    })
                                    .size(px(14.)),
                                ),
                        )
                        .when(open, |this| {
                            this.child(
                                div()
                                    .px_3()
                                    .pb_3()
                                    .pt_1()
                                    .text_xs()
                                    .line_height(relative(1.7))
                                    .text_color(p.muted_foreground)
                                    .child(reasoning),
                            )
                        }),
                )
            })
            // 本地工具调用记录
            .when(!msg.tool_calls.is_empty(), |this| {
                this.child(h_flex().flex_wrap().gap_2().children(msg.tool_calls.iter().map(|tc| {
                    h_flex()
                        .gap_1p5()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(p.border)
                        .bg(p.muted)
                        .text_xs()
                        .font_family(mono_font.clone())
                        .child(Icon::new(IconName::Wrench).size(px(12.)).text_color(p.muted_foreground))
                        .child(tc.clone())
                })))
            })
            .when(has_content, |this| {
                this.child(
                    div().w_full().text_sm().child(
                        markdown_view(format!("md-{}", msg.id), msg.content.clone())
                            .stream_fade(msg.is_streaming)
                            .code_block_actions(|block, _, cx| render_code_block_actions(block, cx)),
                    ),
                )
            })
            .when(waiting, |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .text_color(p.muted_foreground)
                        .child(Spinner::new().small())
                        .child("正在生成…"),
                )
            })
            .when_some(msg.error.clone(), |this, err| {
                this.child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap_2()
                        .px_3()
                        .py_2p5()
                        .rounded_lg()
                        .border_1()
                        .border_color(p.danger.opacity(0.35))
                        .bg(p.danger.opacity(if p.is_dark { 0.12 } else { 0.06 }))
                        .text_sm()
                        .text_color(p.danger)
                        .child(Icon::new(IconName::CircleAlert).size(px(16.)).mt_0p5())
                        .child(div().flex_1().min_w_0().child(err)),
                )
            })
            .when(show_actions, |this| {
                let regen_app = app.clone();
                let other_app = app.clone();
                let quote_app = app.clone();
                let delete_app = app.clone();
                let continue_app = app.clone();
                let regen_id = action_id.clone();
                let other_id = action_id.clone();
                let quote_id = action_id.clone();
                let delete_id = action_id.clone();
                this.child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .text_color(p.muted_foreground)
                        .when(total_variants > 1 && has_content, |this| {
                            let switch_app = app.clone();
                            let message_id = action_id.clone();
                            this.child(
                                h_flex()
                                    .items_center()
                                    .gap_0p5()
                                    .mr_1p5()
                                    .child(
                                        Button::new(SharedString::from(format!("prev-var-{}", action_id)))
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::ChevronLeft)
                                            .disabled(current_var_ix == 0)
                                            .tooltip("上一个模型回答")
                                            .on_click({
                                                let switch_app = switch_app.clone();
                                                let message_id = message_id.clone();
                                                move |_, _, cx| {
                                                    if current_var_ix > 0 {
                                                        switch_app.update(cx, |this, cx| {
                                                            this.switch_variant(&message_id, current_var_ix - 1, cx);
                                                        });
                                                    }
                                                }
                                            }),
                                    )
                                    .child(
                                        div()
                                            .px_1()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(p.muted_foreground)
                                            .child(format!("{}/{}", current_var_ix + 1, total_variants)),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!("next-var-{}", action_id)))
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::ChevronRight)
                                            .disabled(current_var_ix + 1 >= total_variants)
                                            .tooltip("下一个模型回答")
                                            .on_click({
                                                let switch_app = switch_app.clone();
                                                let message_id = message_id.clone();
                                                move |_, _, cx| {
                                                    if current_var_ix + 1 < total_variants {
                                                        switch_app.update(cx, |this, cx| {
                                                            this.switch_variant(&message_id, current_var_ix + 1, cx);
                                                        });
                                                    }
                                                }
                                            }),
                                    ),
                            )
                        })
                        .when(has_content, |this| {
                            this.child(
                                Button::new(("copy-assistant", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Copy)
                                    .tooltip("复制回答")
                                    .on_click(move |_, _, cx| {
                                        copy_app.update(cx, |this, cx| this.copy_to_clipboard(&copy_text, cx));
                                    }),
                            )
                        })
                        .child(
                            Button::new(("regen", ix))
                                .ghost()
                                .xsmall()
                                .icon(IconName::RefreshCw)
                                .tooltip("重新生成")
                                .on_click(move |_, window, cx| {
                                    // 较早的回答重新生成会删掉后面的对话，先确认
                                    if later > 0 {
                                        dialogs::confirm_regenerate(
                                            regen_app.clone(),
                                            regen_id.clone(),
                                            None,
                                            later,
                                            window,
                                            cx,
                                        );
                                    } else {
                                        regen_app
                                            .update(cx, |this, cx| this.regenerate_message(&regen_id, None, None, cx));
                                    }
                                }),
                        )
                        .child(
                            Popover::new(SharedString::from(format!("regen-popover-{}", other_id)))
                                .anchor(Anchor::TopLeft)
                                .w(px(320.))
                                .trigger(
                                    Button::new(("regen-other", ix))
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Repeat)
                                        .tooltip("换模型重答"),
                                )
                                .content(move |popover, window, cx| {
                                    render_regen_popover(&other_app, &other_id, later, popover, window, cx)
                                }),
                        )
                        .when(is_last && has_content, |this| {
                            this.child(
                                Button::new(("continue", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Forward)
                                    .tooltip("继续生成")
                                    .on_click(move |_, _, cx| {
                                        continue_app.update(cx, |this, cx| this.continue_message(cx))
                                    }),
                            )
                        })
                        .child(
                            Button::new(("quote-assistant", ix))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Quote)
                                .tooltip("引用")
                                .on_click(move |_, _, cx| {
                                    quote_app.update(cx, |this, cx| this.quote_message(&quote_id, cx))
                                }),
                        )
                        .child(
                            Button::new(("delete-assistant", ix))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Trash)
                                .tooltip("删除")
                                .on_click(move |_, _, cx| {
                                    delete_app.update(cx, |this, cx| this.delete_message(&delete_id, cx))
                                }),
                        )
                        .when(has_metrics, |this| {
                            this.child(render_message_metrics(&msg, &model_label, p))
                        }),
                )
            }),
    )
}

fn render_regen_popover(
    app: &Entity<AppState>,
    message_id: &str,
    later: usize,
    _: &mut PopoverState,
    _: &mut Window,
    cx: &mut Context<PopoverState>,
) -> impl IntoElement + use<> {
    let p = Palette::new(cx);
    let (providers, current_model_id) = {
        let state = app.read(cx);
        let curr = state
            .storage
            .get_active_session()
            .and_then(|s| s.messages.iter().find(|m| m.id == message_id))
            .map(|m| m.model.clone())
            .unwrap_or_default();
        (state.config.providers.clone(), curr)
    };

    let mut rows = Vec::new();
    for provider in providers.iter().filter(|p| p.enabled) {
        let models: Vec<_> = provider.models.iter().filter(|m| m.enabled).collect();
        if models.is_empty() {
            continue;
        }

        rows.push(
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
            let is_current = !current_model_id.is_empty() && model.id == current_model_id;
            let select_app = app.clone();
            let msg_id = message_id.to_string();
            let pid = provider.id.clone();
            let mid = model.id.clone();

            rows.push(
                h_flex()
                    .id(SharedString::from(format!(
                        "regen-pick-{}-{}-{}",
                        msg_id, provider.id, model.id
                    )))
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .cursor_pointer()
                    .text_sm()
                    .when(is_current, |this| this.bg(p.accent.opacity(0.5)))
                    .hover(|s| s.bg(p.accent))
                    .on_click(cx.listener(move |popover, _, window, cx| {
                        if later > 0 {
                            let target = Some((pid.clone(), mid.clone()));
                            dialogs::confirm_regenerate(select_app.clone(), msg_id.clone(), target, later, window, cx);
                        } else {
                            select_app.update(cx, |this, cx| {
                                this.regenerate_message(&msg_id, Some(pid.clone()), Some(mid.clone()), cx);
                            });
                        }
                        popover.dismiss(window, cx);
                    }))
                    .child(model_avatar(model, px(18.), &p))
                    .child(div().flex_1().min_w_0().truncate().child(model.name.clone()))
                    .child(model_badges(model, &p))
                    .when(is_current, |this| {
                        this.child(div().text_xs().text_color(p.muted_foreground).child("当前"))
                    })
                    .into_any_element(),
            );
        }
    }

    let has_models = !rows.is_empty();

    v_flex()
        .gap_1()
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .px_2()
                .pt_1()
                .pb_1p5()
                .border_b_1()
                .border_color(p.border)
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.foreground)
                        .child("换模型重答"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(p.muted_foreground)
                        .child("选择重新生成的模型"),
                ),
        )
        .map(|this| {
            if has_models {
                this.child(
                    div()
                        .max_h(px(300.))
                        .child(v_flex().gap_px().children(rows).overflow_y_scrollbar()),
                )
            } else {
                this.child(
                    v_flex()
                        .items_center()
                        .justify_center()
                        .py_6()
                        .text_xs()
                        .text_color(p.muted_foreground)
                        .child("没有可用的模型渠道"),
                )
            }
        })
}

fn render_message_metrics(msg: &ChatMessage, model_label: &str, p: &Palette) -> impl IntoElement {
    let (cost_usd, cost_cny) = crate::models_dev::calculate_cost(
        if !msg.model.is_empty() { &msg.model } else { model_label },
        msg.prompt_tokens,
        msg.completion_tokens,
        0,
    );

    let total_tokens = msg.prompt_tokens + msg.completion_tokens;
    let label = if cost_usd > 0.00001 {
        format!(
            "{} tokens · ≈${:.4} · {:.1} tok/s · {:.1}s",
            msg.completion_tokens,
            cost_usd,
            msg.speed_tps,
            msg.latency_ms as f64 / 1000.0
        )
    } else {
        format!(
            "{} tokens · {:.1} tok/s · {:.1}s",
            msg.completion_tokens,
            msg.speed_tps,
            msg.latency_ms as f64 / 1000.0
        )
    };

    let input_t = msg.prompt_tokens;
    let output_t = msg.completion_tokens;
    let speed = msg.speed_tps;
    let sec = msg.latency_ms as f64 / 1000.0;
    let reasoning_len = msg.reasoning_content.as_ref().map(|r| r.chars().count()).unwrap_or(0);
    let reasoning_t_est = if reasoning_len > 0 { reasoning_len / 2 } else { 0 };

    h_flex()
        .id(SharedString::from(format!("metrics-{}", msg.id)))
        .px_1p5()
        .py_0p5()
        .rounded_sm()
        .cursor_pointer()
        .hover(|s| s.bg(p.muted))
        .tooltip(move |window, cx| {
            let mut text = String::from("Token 与费用明细\n");
            text.push_str(&format!("• 输入 (Input):    {} tokens\n", input_t));
            text.push_str(&format!("• 输出 (Output):   {} tokens\n", output_t));
            if reasoning_t_est > 0 {
                text.push_str(&format!("  └ 思考生成:      ≈{} tokens\n", reasoning_t_est));
            }
            let tot = if total_tokens > 0 { total_tokens } else { output_t };
            text.push_str(&format!("• 总计 (Total):    {} tokens\n", tot));
            if cost_usd > 0.00001 {
                text.push_str(&format!("• 预估费用:        ${:.4} (≈ ¥{:.3})\n", cost_usd, cost_cny));
            }
            text.push_str(&format!("• 速率与耗时:       {:.1} tok/s · {:.1}s", speed, sec));
            Tooltip::new(text).build(window, cx)
        })
        .child(label)
}

fn render_code_block_actions(block: &CodeBlock, cx: &mut App) -> impl IntoElement + use<> {
    let code = block.code();
    let muted = cx.theme().muted_foreground;

    h_flex()
        .gap_1()
        .pl_2()
        .when_some(block.lang().filter(|l| !l.is_empty()), |this, lang| {
            this.child(div().text_xs().text_color(muted).child(lang))
        })
        .child(
            Button::new("copy")
                .ghost()
                .xsmall()
                .icon(IconName::Copy)
                .tooltip("复制代码")
                .on_click(move |_, window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()));
                    window.push_notification(Notification::success("代码已复制"), cx);
                }),
        )
}
