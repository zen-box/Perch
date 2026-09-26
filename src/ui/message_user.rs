//! 用户消息：正文、附件、引用、编辑/重发按钮。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::chat::{attachment_badge, format_msg_time, preview};
use super::markdown_image::open_local_image_viewer;
use super::{Palette, dialogs};
use crate::app::AppState;
use crate::i18n::{AppLanguage, Key, tr};
use crate::model::{Attachment, ChatMessage};

fn render_message_attachments(
    app: &Entity<AppState>,
    attachments: &[Attachment],
    p: &Palette,
    lang: AppLanguage,
) -> impl IntoElement {
    h_flex()
        .gap_2()
        .flex_wrap()
        .justify_end()
        .max_w(relative(0.85))
        .children(attachments.iter().map(|att| {
            let abs_path = att.absolute_path();
            let name = att.name.clone();
            let view_path = abs_path.clone();
            let view_title = name.clone();
            let open_app = app.clone();
            let (icon, badge_color, type_label) = attachment_badge(att, p, lang);

            let size_kb = (att.size as f32 / 1024.0).max(0.1);
            let size_label = if size_kb > 1024.0 {
                format!("{:.1} MB", size_kb / 1024.0)
            } else {
                format!("{:.0} KB", size_kb)
            };

            let elem_id = SharedString::from(format!("msg-att-{}", att.id));
            if att.is_image() {
                div()
                    .id(elem_id)
                    .cursor_pointer()
                    .rounded_lg()
                    .overflow_hidden()
                    .border_1()
                    .border_color(p.border)
                    .bg(p.muted)
                    .shadow_xs()
                    .w(px(140.))
                    .h(px(100.))
                    .hover(|style| style.border_color(p.primary.opacity(0.8)))
                    .on_click(move |_, window, cx| {
                        open_local_image_viewer(view_path.clone(), view_title.clone(), window, cx);
                    })
                    .child(img(abs_path).size_full().object_fit(ObjectFit::Cover))
                    .into_any_element()
            } else {
                h_flex()
                    .id(elem_id)
                    .cursor_pointer()
                    .rounded_lg()
                    .border_1()
                    .border_color(p.border)
                    .bg(p.muted)
                    .shadow_xs()
                    .p_2()
                    .gap_2p5()
                    .items_center()
                    .min_w(px(160.))
                    .max_w(px(240.))
                    .hover(|style| style.border_color(p.primary.opacity(0.8)))
                    .on_click(move |_, _, cx| {
                        #[cfg(target_os = "windows")]
                        {
                            open_app.update(cx, |this, cx| this.reveal_attachment(&view_path, cx));
                        }
                        #[cfg(not(target_os = "windows"))]
                        {
                            // 其它平台还没接文件管理器，先什么也不做
                            let _ = (cx, &view_path, &open_app);
                        }
                    })
                    .child(
                        div()
                            .size(px(34.))
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(badge_color.opacity(0.15))
                            .child(Icon::new(icon).size(px(18.)).text_color(badge_color)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
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
                    .into_any_element()
            }
        }))
}

pub(super) fn render_user_message(
    app: &Entity<AppState>,
    ix: usize,
    msg: ChatMessage,
    streaming: bool,
    lang: AppLanguage,
    p: &Palette,
) -> impl IntoElement {
    let copy_app = app.clone();
    let copy_text = msg.content.clone();
    let edit_app = app.clone();
    let quote_app = app.clone();
    let delete_app = app.clone();
    let message_id = msg.id.clone();
    let edit_id = message_id.clone();
    let quote_id = message_id.clone();
    let delete_id = message_id;
    let quote = msg.quote.clone();
    let created_at = msg.created_at.clone();
    let attachments = msg.attachments;
    let content = msg.content;
    let has_content = !content.trim().is_empty();
    let has_attachments = !attachments.is_empty();

    v_flex()
        .w_full()
        .items_end()
        .gap_1()
        .group("user-message")
        .when_some(quote, |this, quote| {
            this.child(
                div()
                    .max_w(relative(0.82))
                    .px_3()
                    .py_1()
                    .border_l_2()
                    .border_color(p.border)
                    .text_xs()
                    .text_color(p.muted_foreground)
                    .child(preview(&quote, 160)),
            )
        })
        .when(has_attachments, |this| {
            this.child(render_message_attachments(app, &attachments, p, lang))
        })
        .when(has_content, |this| {
            this.child(
                div()
                    .max_w(relative(0.82))
                    .px_4()
                    .py_2p5()
                    .rounded(px(18.))
                    .bg(p.muted)
                    .text_sm()
                    .line_height(relative(1.6))
                    .child(content),
            )
        })
        .child(
            h_flex()
                .gap_1()
                .text_xs()
                .text_color(p.muted_foreground)
                .invisible()
                .group_hover("user-message", |style| style.visible())
                .child(format_msg_time(&created_at).to_string())
                .child(
                    Button::new(("copy-user", ix))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Copy)
                        .tooltip(tr(lang, Key::Copy))
                        .on_click(move |_, _, cx| {
                            copy_app.update(cx, |this, cx| this.copy_to_clipboard(&copy_text, cx));
                        }),
                )
                .child(
                    Button::new(("edit-user", ix))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Pencil)
                        .tooltip(tr(lang, Key::EditAndResend))
                        .disabled(streaming)
                        .on_click(move |_, window, cx| {
                            dialogs::open_edit_message_dialog(edit_app.clone(), window, cx);
                            edit_app.update(cx, |this, cx| this.begin_edit_message(&edit_id, window, cx));
                        }),
                )
                .child(
                    Button::new(("quote-user", ix))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Quote)
                        .tooltip(tr(lang, Key::Quote))
                        .on_click(move |_, _, cx| quote_app.update(cx, |this, cx| this.quote_message(&quote_id, cx))),
                )
                .child(
                    Button::new(("delete-user", ix))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Trash)
                        .tooltip(tr(lang, Key::Delete))
                        .disabled(streaming)
                        .on_click(move |_, _, cx| {
                            delete_app.update(cx, |this, cx| this.delete_message(&delete_id, cx))
                        }),
                ),
        )
}
