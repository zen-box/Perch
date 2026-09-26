//! 底部输入框：待发送附件、引用、斜杠命令菜单、发送与停止。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::chat::{attachment_badge, preview};
use super::{CONTENT_MAX_WIDTH, Palette, model_picker};
use crate::app::AppState;
use crate::model::Attachment;

// ================= 输入框 =================

fn render_pending_attachments(attachments: &[Attachment], p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
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
            let (icon, badge_color, type_label) = attachment_badge(att, p);

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
                        .tooltip("移除")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remove_pending_attachment(&id, cx);
                        })),
                )
        }))
}

fn is_current_model_vision_capable(state: &AppState) -> bool {
    let session = state.storage.get_active_session();
    let provider_id = session
        .map(|s| s.provider_id.as_str())
        .filter(|id| !id.is_empty())
        .unwrap_or(&state.config.active_provider_id);
    let default_model = state.config.default_model_selection().1;
    let model_id = session
        .map(|s| s.model.as_str())
        .filter(|id| !id.is_empty() && *id != "default")
        .unwrap_or(&default_model);
    let model_config = state
        .config
        .providers
        .iter()
        .find(|p| p.id == provider_id)
        .or_else(|| state.config.get_active_provider())
        .and_then(|p| p.models.iter().find(|m| m.id == model_id));

    if let Some(model) = model_config {
        model
            .effective_capabilities()
            .contains(&crate::model_info::Capability::Vision)
    } else {
        crate::model_info::detect(model_id, "")
            .capabilities
            .contains(&crate::model_info::Capability::Vision)
    }
}

pub(super) fn render_composer(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let is_streaming = state.is_streaming;
    let draft = state.chat_input.read(cx).value().to_string();
    let has_attachments = !state.pending_attachments.is_empty();
    let has_images = state.pending_attachments.iter().any(|a| a.is_image());
    let is_vision = is_current_model_vision_capable(state);
    let input_empty = draft.trim().is_empty() && !has_attachments;
    let slash = slash_matches(state, &draft);
    let quote = state.pending_quote.clone();

    v_flex()
        .w_full()
        .max_w(CONTENT_MAX_WIDTH)
        .gap_2()
        .when(!slash.is_empty(), |this| this.child(render_slash_menu(slash, p, cx)))
        .when_some(quote, |this, quote| this.child(render_quote_chip(&quote, p, cx)))
        .child(
            v_flex()
                .w_full()
                .rounded(px(16.))
                .border_1()
                .border_color(p.border)
                .bg(p.background)
                .shadow_sm()
                .when(has_attachments, |this| {
                    this.child(render_pending_attachments(&state.pending_attachments, p, cx))
                })
                .when(has_images && !is_vision, |this| {
                    this.child(
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
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.warning)
                                    .child("提示：当前选中的模型未标注视觉能力，建议切换为支持视觉的多模态模型"),
                            ),
                    )
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
                        .gap_2()
                        .px_2()
                        .pb_2()
                        .child(
                            h_flex()
                                .min_w_0()
                                .gap_0p5()
                                .child(
                                    Button::new("composer-pick-attachment")
                                        .ghost()
                                        .small()
                                        .icon(IconName::Paperclip)
                                        .tooltip("添加附件 (图片/文档/表格/代码)")
                                        .on_click(cx.listener(|this, _, _, cx| this.pick_attachments(cx))),
                                )
                                .child(model_picker::render_model_picker(state, p, cx))
                                .child(super::params::render_params_button(cx))
                                .child(super::params::render_compare_button(state, cx)),
                        )
                        .child(if is_streaming {
                            Button::new("stop")
                                .primary()
                                .small()
                                .rounded(px(999.))
                                .icon(IconName::Square)
                                .tooltip("停止生成")
                                .on_click(cx.listener(|this, _, _, cx| this.cancel_streaming(cx)))
                        } else {
                            Button::new("send")
                                .primary()
                                .small()
                                .rounded(px(999.))
                                .icon(IconName::ArrowUp)
                                .tooltip(if state.compare_selection.is_empty() {
                                    "发送 (Enter)"
                                } else {
                                    "对比发送 (Enter)"
                                })
                                .disabled(input_empty)
                                .on_click(cx.listener(|this, _, window, cx| this.send_message(window, cx)))
                        }),
                ),
        )
}

fn render_quote_chip(quote: &str, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
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
                .tooltip("取消引用")
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
                .child("提示词模板 · 回车或点击插入"),
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
