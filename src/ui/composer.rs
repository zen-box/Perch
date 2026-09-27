//! 底部输入框：待发送附件、引用、斜杠命令菜单、发送与停止。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::chat::{attachment_badge, preview};
use super::{CONTENT_MAX_WIDTH, Palette, model_picker};
use crate::app::AppState;
use crate::attachment_ops::{ATTACHMENT_FILTERS, AttachmentObstacle};
use crate::i18n::{AppLanguage, Key, tr, tr_args};
use crate::model::Attachment;
use crate::model_info::Capability;

// ================= 输入框 =================

fn render_pending_attachments(
    attachments: &[Attachment],
    p: &Palette,
    lang: AppLanguage,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    h_flex()
        .gap_2()
        .px_3()
        .pt_2p5()
        .pb_1()
        .flex_wrap()
        .children(attachments.iter().map(|att| {
            let id = att.id.clone();
            let name = att.name.clone();
            let abs_path = att.absolute_path();
            let size_kb = (att.size as f32 / 1024.0).max(0.1);
            let size_label = if size_kb > 1024.0 {
                format!("{:.1} MB", size_kb / 1024.0)
            } else {
                format!("{:.0} KB", size_kb)
            };
            let (icon, badge_color, type_label) = attachment_badge(att, p, lang);

            h_flex()
                .gap_2()
                .items_center()
                .p_1p5()
                .rounded_md()
                .bg(p.muted.opacity(0.7))
                .border_1()
                .border_color(p.border)
                .shadow_xs()
                .child(
                    div()
                        .size(px(38.))
                        .rounded_sm()
                        .overflow_hidden()
                        .bg(p.background)
                        .map(|this| {
                            if att.is_image() {
                                this.child(img(abs_path).size_full().object_fit(ObjectFit::Cover))
                            } else {
                                this.flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(badge_color.opacity(0.12))
                                    .child(Icon::new(icon).size(px(20.)).text_color(badge_color))
                            }
                        }),
                )
                .child(
                    v_flex()
                        .min_w(px(70.))
                        .max_w(px(160.))
                        .child(
                            div()
                                .truncate()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.foreground)
                                .child(name),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(p.muted_foreground)
                                .child(format!("{size_label} · {type_label}")),
                        ),
                )
                .child(
                    Button::new(SharedString::from(format!("del-pending-{}", id)))
                        .ghost()
                        .xsmall()
                        .icon(IconName::X)
                        .tooltip(tr(lang, Key::Remove))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remove_pending_attachment(&id, cx);
                        })),
                )
        }))
}

/// 「这批附件有模型接不住」的提示条。
///
/// 发送时还会再拦一道（`session_ops::send_message`）。这里是**提前**告诉用户，
/// 别等点了发送才被弹回来；反过来，用户换掉模型或者删掉附件后这条也会立刻消失。
fn render_attachment_warning(obstacle: &AttachmentObstacle, p: &Palette, lang: AppLanguage) -> impl IntoElement {
    h_flex()
        .gap_1p5()
        .px_3()
        .py_1p5()
        .mx_2()
        .mb_1()
        .rounded_md()
        .bg(p.warning.opacity(0.12))
        .items_center()
        .child(Icon::new(IconName::Info).size(px(13.)).text_color(p.warning))
        .child(div().text_xs().text_color(p.warning).child(tr_args(
            lang,
            Key::AttachmentUnsupported,
            &[&obstacle.model, obstacle.capability.label(lang)],
        )))
}

pub(super) fn render_composer(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let lang = state.language();
    let is_streaming = state.is_streaming;
    let draft = state.chat_input.read(cx).value().to_string();
    let has_attachments = !state.pending_attachments.is_empty();
    // 这批附件发给这次要用的模型，有没有接不住的。对比模式下每个模型各查一遍——
    // 三个模型里可能只有两个能看图
    let obstacle = state.attachment_obstacle();
    // 入口本身也要说清楚能加什么。按钮不能因为模型看不懂图片就整个藏掉：
    // 文本和代码会拼进正文，任何模型都读得了
    let gate = state.attachment_gate();
    let attachment_tooltip = tr(
        lang,
        match (
            gate.missing(Capability::Vision).is_none(),
            gate.missing(Capability::Files).is_none(),
        ) {
            (true, true) => Key::AddAttachment,
            (true, false) => Key::AddAttachmentNoFiles,
            (false, true) => Key::AddAttachmentNoVision,
            (false, false) => Key::AddAttachmentTextOnly,
        },
    );
    let input_empty = draft.trim().is_empty() && !has_attachments;
    let slash = slash_matches(state, &draft);
    let quote = state.pending_quote.clone();

    v_flex()
        .w_full()
        .max_w(CONTENT_MAX_WIDTH)
        .gap_2()
        .when(!slash.is_empty(), |this| {
            this.child(render_slash_menu(slash, p, lang, cx))
        })
        .when_some(quote, |this, quote| this.child(render_quote_chip(&quote, p, lang, cx)))
        .child(
            v_flex()
                .w_full()
                .rounded(px(16.))
                .border_1()
                .border_color(p.border)
                .bg(p.background)
                .shadow_sm()
                .when(has_attachments, |this| {
                    this.child(render_pending_attachments(&state.pending_attachments, p, lang, cx))
                })
                .when_some(obstacle, |this, obstacle| {
                    this.child(render_attachment_warning(&obstacle, p, lang))
                })
                .child({
                    let app = cx.entity();
                    div().px_2().pt_2().child(
                        Textarea::new(&state.chat_input)
                            .appearance(false)
                            .bordered(false)
                            // Ctrl+V 和右键「粘贴」都走输入框的 Paste 动作，在这里统一接住：
                            // 复制的文件和截图变成附件，文字照常插入。
                            // 不能用 cx.listener：它要求闭包返回 ()，而 on_paste 要返回 bool。
                            .on_paste(move |item: &ClipboardItem, _window: &mut Window, cx: &mut App| {
                                app.update(cx, |this, cx| this.handle_clipboard_paste(item, cx))
                            }),
                    )
                })
                .child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .pb_2()
                        // 左边管「这一问怎么问」：附件、模型、参数、对比
                        .child(
                            h_flex()
                                .min_w_0()
                                // 窗口太窄时左边自己裁掉，不能压到右边的模式开关和发送按钮上
                                .overflow_hidden()
                                .items_center()
                                .gap_0p5()
                                .child({
                                    let app = cx.entity();
                                    // 附件入口按类型拆成菜单，而不是一个按钮直接开文件对话框：
                                    // 模型接不住哪一类，菜单里那一项就灰掉并写明原因。只摆一个
                                    // 「所有文件」的话，纯文本模型下照样能选到图片，**选完才在
                                    // 导入时被拒**——白跑一趟，用户还不知道自己错在哪。
                                    Button::new("composer-pick-attachment")
                                        .ghost()
                                        .small()
                                        .icon(IconName::Paperclip)
                                        .tooltip(attachment_tooltip)
                                        // 这排按钮贴着窗口底边，菜单往上开（跟旁边的模型、参数一致）
                                        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                                            ATTACHMENT_FILTERS.iter().fold(menu, |menu, &filter| {
                                                let app = app.clone();
                                                // 缺能力时把模型名和缺的那一项一起写在菜单项上，
                                                // 用户才知道该换成谁，而不是猜为什么点不动
                                                let reason = filter
                                                    .requires()
                                                    .and_then(|capability| gate.missing(capability))
                                                    .and_then(|model| filter.unavailable_label(model, lang));
                                                let label = match &reason {
                                                    Some(reason) => format!("{} · {}", filter.label(lang), reason),
                                                    None => filter.label(lang).to_string(),
                                                };
                                                menu.item(
                                                    PopupMenuItem::new(label)
                                                        .icon(filter.icon())
                                                        .disabled(reason.is_some())
                                                        .on_click(move |_, _, cx| {
                                                            app.update(cx, |this, cx| {
                                                                this.pick_attachments(filter, cx)
                                                            });
                                                        }),
                                                )
                                            })
                                        })
                                })
                                .child(model_picker::render_model_picker(state, p, cx))
                                .child(super::params::render_params_button(lang, cx))
                                .child(super::params::render_compare_button(state, cx)),
                        )
                        // 右边管「怎么答」：对话还是智能体、带哪些工具，最后是发送。
                        // 模式和工具挨着发送按钮，发之前扫一眼就知道这一轮会不会动用工具
                        .child(
                            h_flex()
                                .flex_none()
                                .items_center()
                                .gap_1()
                                .child(super::tool_picker::render_mode_switch(state, p, cx))
                                .child(super::tool_picker::render_tool_picker(state, p, cx))
                                .child(render_send_button(state, is_streaming, input_empty, lang, cx)),
                        ),
                ),
        )
}

/// 发送 / 停止按钮。
fn render_send_button(
    state: &AppState,
    is_streaming: bool,
    input_empty: bool,
    lang: AppLanguage,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    if is_streaming {
        Button::new("stop")
            .primary()
            .small()
            .rounded(px(999.))
            .icon(IconName::Square)
            .tooltip(tr(lang, Key::StopGenerating))
            .on_click(cx.listener(|this, _, _, cx| this.cancel_streaming(cx)))
    } else {
        Button::new("send")
            .primary()
            .small()
            .rounded(px(999.))
            .icon(IconName::ArrowUp)
            .tooltip(if state.compare_selection.is_empty() {
                tr(lang, Key::SendEnter)
            } else {
                tr(lang, Key::SendCompareEnter)
            })
            .disabled(input_empty)
            .on_click(cx.listener(|this, _, window, cx| this.send_message(window, cx)))
    }
}

fn render_quote_chip(quote: &str, p: &Palette, lang: AppLanguage, cx: &mut Context<AppState>) -> impl IntoElement {
    h_flex()
        .gap_2()
        .px_3()
        .py_2()
        .rounded_lg()
        .bg(p.muted)
        .child(Icon::new(IconName::Quote).size(px(14.)).text_color(p.muted_foreground))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(p.muted_foreground)
                .child(preview(quote, 120)),
        )
        .child(
            Button::new("clear-quote")
                .ghost()
                .xsmall()
                .icon(IconName::X)
                .tooltip(tr(lang, Key::ClearQuote))
                .on_click(cx.listener(|this, _, _, cx| this.clear_quote(cx))),
        )
}

fn slash_matches(state: &AppState, draft: &str) -> Vec<(String, String, String)> {
    let token = draft.trim();
    if !token.starts_with('/') || token.contains(char::is_whitespace) {
        return Vec::new();
    }
    let query = token.trim_start_matches('/').to_lowercase();
    state
        .prompts
        .templates
        .iter()
        .filter(|template| query.is_empty() || template.name.to_lowercase().contains(&query))
        .take(6)
        .map(|template| (template.id.clone(), template.name.clone(), preview(&template.body, 48)))
        .collect()
}

fn render_slash_menu(
    items: Vec<(String, String, String)>,
    p: &Palette,
    lang: AppLanguage,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    v_flex()
        .w_full()
        .rounded_lg()
        .border_1()
        .border_color(p.border)
        .bg(p.background)
        .shadow_sm()
        .overflow_hidden()
        .child(
            div()
                .px_3()
                .py_1p5()
                .text_xs()
                .text_color(p.muted_foreground)
                .child(tr(lang, Key::PromptTemplateHint)),
        )
        .children(items.into_iter().enumerate().map(|(ix, (id, name, body))| {
            h_flex()
                .id(SharedString::from(format!("slash-{ix}")))
                .gap_2()
                .px_3()
                .py_2()
                .cursor_pointer()
                .hover(|style| style.bg(p.muted))
                .on_click(cx.listener(move |this, _, window, cx| this.insert_template(&id, window, cx)))
                .child(
                    Icon::new(IconName::BookOpen)
                        .size(px(14.))
                        .text_color(p.muted_foreground),
                )
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(name))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(p.muted_foreground)
                        .child(body),
                )
        }))
}
