//! 还没有消息时的空状态：引导语与建议问题。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::{Palette, icon_tile, model_picker};
use crate::app::AppState;

// ================= 空状态 =================

pub(super) fn render_empty_state(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let lang = state.language();
    let has_providers = !state.config.providers.is_empty();
    let subtitle = if has_providers {
        format!(
            "{} · {}",
            state.config.get_active_provider_name(lang),
            model_picker::current_model_label(state)
        )
    } else {
        "还没有配置模型渠道，先添加一个吧".to_string()
    };

    v_flex()
        .id("empty-state")
        .size_full()
        .items_center()
        .justify_center()
        .gap_8()
        .px_6()
        .overflow_y_scroll()
        .child(
            v_flex()
                .items_center()
                .gap_3()
                .child(icon_tile(IconName::Sparkles, px(48.), p.primary, p.primary_foreground))
                .child(
                    div()
                        .text_2xl()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("今天想聊点什么？"),
                )
                .child(div().text_sm().text_color(p.muted_foreground).child(subtitle)),
        )
        .when(has_providers, |this| {
            let presets: Vec<_> = state
                .prompts
                .presets
                .iter()
                .filter(|preset| !preset.system_prompt.trim().is_empty())
                .cloned()
                .collect();
            if presets.is_empty() {
                this
            } else {
                this.child(
                    h_flex()
                        .w_full()
                        .max_w(px(600.))
                        .flex_wrap()
                        .gap_2()
                        .justify_center()
                        .children(presets.into_iter().enumerate().map(|(ix, preset)| {
                            let id = preset.id;
                            Button::new(("empty-preset", ix))
                                .outline()
                                .small()
                                .label(format!("{} {}", preset.icon, preset.name))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.create_session_from_preset(&id, window, cx)
                                }))
                        })),
                )
            }
        })
        .child(if has_providers {
            div()
                .w_full()
                .max_w(px(600.))
                .grid()
                .grid_cols(2)
                .gap_3()
                .child(render_suggestion(
                    "suggest-translate",
                    IconName::Languages,
                    "翻译",
                    "中英互译，保留原文格式",
                    p,
                    cx.listener(|this, _, window, cx| {
                        this.fill_chat_input(
                            "请把下面的内容翻译成英文（如果原文是英文则翻译成中文），保留原有格式：\n",
                            window,
                            cx,
                        )
                    }),
                ))
                .child(render_suggestion(
                    "suggest-polish",
                    IconName::PencilLine,
                    "润色文字",
                    "让表达更通顺、更专业",
                    p,
                    cx.listener(|this, _, window, cx| {
                        this.fill_chat_input(
                            "请帮我润色下面这段文字，使表达更通顺专业，并说明主要改动：\n",
                            window,
                            cx,
                        )
                    }),
                ))
                .child(render_suggestion(
                    "suggest-summary",
                    IconName::FileText,
                    "总结要点",
                    "提炼长文的核心内容",
                    p,
                    cx.listener(|this, _, window, cx| {
                        this.fill_chat_input("请用要点的形式总结下面的内容：\n", window, cx)
                    }),
                ))
                .child(render_suggestion(
                    "suggest-explain",
                    IconName::Code,
                    "解释一段代码",
                    "粘贴代码，让 AI 逐段讲解",
                    p,
                    cx.listener(|this, _, window, cx| this.fill_chat_input("请逐段解释下面这段代码：\n", window, cx)),
                ))
                .into_any_element()
        } else {
            Button::new("empty-add-provider")
                .primary()
                .icon(IconName::Plus)
                .label("添加模型渠道")
                .on_click(cx.listener(|this, _, window, cx| this.open_providers_settings(window, cx)))
                .into_any_element()
        })
}

fn render_suggestion(
    id: &'static str,
    icon: IconName,
    title: &'static str,
    description: &'static str,
    p: &Palette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    h_flex()
        .id(id)
        .items_start()
        .gap_3()
        .p_3()
        .rounded_xl()
        .border_1()
        .border_color(p.border)
        .bg(p.background)
        .cursor_pointer()
        .hover(|s| s.bg(p.muted))
        .on_click(on_click)
        .child(icon_tile(icon, px(32.), p.muted, p.foreground))
        .child(
            v_flex()
                .min_w_0()
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
                .child(div().text_xs().text_color(p.muted_foreground).child(description)),
        )
}
