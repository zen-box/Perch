//! 通用弹窗：这里放所有弹窗共用的零件（`field` / `footer`），以及确认类和轻量表单类弹窗。
//! 模型编辑器那种重表单在 `model_editor_dialog.rs`，拉取模型列表在 `fetch_models_dialog.rs`。

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputState, Textarea};
use gpui_kit::component::{Icon, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Palette, channel_icon};
use crate::app::AppState;
use crate::config::ChannelType;
use crate::i18n::{AppLanguage, Key, tr, tr_args};

/// 弹窗里的表单项：标签 + 输入框 + 可选说明
pub(super) fn field(
    label: &'static str,
    hint: Option<&'static str>,
    input: impl IntoElement,
    p: &Palette,
) -> impl IntoElement {
    v_flex()
        .gap_1p5()
        .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label))
        .child(input)
        .children(hint.map(|hint| div().text_xs().text_color(p.muted_foreground).child(hint)))
}

/// 弹窗底部的「取消 / 确认」按钮
pub(super) fn footer(
    ok_label: &'static str,
    on_ok: impl Fn(&mut Window, &mut App) + 'static,
    lang: AppLanguage,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .justify_end()
        .gap_2()
        .child(
            Button::new("dialog-cancel")
                .outline()
                .label(tr(lang, Key::Cancel))
                .on_click(|_, window, cx| window.close_dialog(cx)),
        )
        .child(
            Button::new("dialog-ok")
                .primary()
                .label(ok_label)
                .on_click(move |_, window, cx| on_ok(window, cx)),
        )
}

pub fn open_rename_dialog(app: Entity<AppState>, input: Entity<InputState>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = Palette::new(cx);
        let lang = app.read(cx).language();
        let ok_app = app.clone();
        let enter_app = app.clone();
        dialog
            .title(tr(lang, Key::RenameSession))
            .w(px(420.))
            .child(field(tr(lang, Key::SessionName), None, Input::new(&input), &p))
            .footer(footer(
                tr(lang, Key::Save),
                move |window, cx| {
                    ok_app.update(cx, |this, cx| this.confirm_rename_session(window, cx));
                    window.close_dialog(cx);
                },
                lang,
            ))
            .on_ok(move |_, window, cx| {
                enter_app.update(cx, |this, cx| this.confirm_rename_session(window, cx));
                true
            })
    });
}

pub fn open_add_provider_dialog(app: Entity<AppState>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = Palette::new(cx);
        let lang = app.read(cx).language();
        let (current_ct, name_input, base_url_input, api_key_input) = {
            let state = app.read(cx);
            (
                state.add_channel_type,
                state.new_provider_name_input.clone(),
                state.new_provider_base_url_input.clone(),
                state.new_provider_api_key_input.clone(),
            )
        };
        let ok_app = app.clone();
        let enter_app = app.clone();

        dialog
            .title(tr(lang, Key::AddModelChannel))
            .w(px(520.))
            .child(
                v_flex()
                    .gap_4()
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(tr(lang, Key::ApiStandard)),
                            )
                            .child(
                                div()
                                    .grid()
                                    .grid_cols(2)
                                    .gap_2()
                                    .children(ChannelType::all().iter().map(|&ct| {
                                        let is_active = ct == current_ct;
                                        let select_app = app.clone();
                                        h_flex()
                                            .id(SharedString::from(format!("channel-type-{:?}", ct)))
                                            .gap_2()
                                            .px_3()
                                            .py_2p5()
                                            .rounded_lg()
                                            .border_1()
                                            .cursor_pointer()
                                            .text_sm()
                                            .map(|this| {
                                                if is_active {
                                                    this.border_color(p.primary)
                                                        .bg(p.primary.opacity(0.08))
                                                        .font_weight(FontWeight::MEDIUM)
                                                } else {
                                                    this.border_color(p.border).hover(|s| s.bg(p.muted))
                                                }
                                            })
                                            .on_click(move |_, window, cx| {
                                                select_app.update(cx, |this, cx| {
                                                    this.select_add_channel_type(ct, window, cx)
                                                });
                                            })
                                            .child(Icon::new(channel_icon(ct)).size(px(16.)).text_color(if is_active {
                                                p.primary
                                            } else {
                                                p.muted_foreground
                                            }))
                                            .child(ct.label())
                                    })),
                            ),
                    )
                    .child(field(tr(lang, Key::ChannelName), None, Input::new(&name_input), &p))
                    .child(field(
                        tr(lang, Key::BaseUrl),
                        Some(tr(lang, Key::BaseUrlAutoHint)),
                        Input::new(&base_url_input),
                        &p,
                    ))
                    .child(field(
                        tr(lang, Key::ApiKeyLabel),
                        None,
                        Input::new(&api_key_input).mask_toggle(),
                        &p,
                    )),
            )
            .footer(footer(
                tr(lang, Key::AddChannel),
                move |window, cx| {
                    if ok_app.update(cx, |this, cx| this.confirm_add_provider(window, cx)) {
                        window.close_dialog(cx);
                    }
                },
                lang,
            ))
            .on_ok(move |_, window, cx| enter_app.update(cx, |this, cx| this.confirm_add_provider(window, cx)))
    });
}

pub fn confirm_regenerate(
    app: Entity<AppState>,
    message_id: String,
    target: Option<(String, String)>,
    later_count: usize,
    window: &mut Window,
    cx: &mut App,
) {
    let lang = app.read(cx).language();
    let description = tr_args(lang, Key::RegenerateDesc, &[&later_count.to_string()]);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let app = app.clone();
        let message_id = message_id.clone();
        let target = target.clone();
        alert
            .title(tr(lang, Key::RegenerateConfirmTitle))
            .description(description.clone())
            .button_props(danger_props(tr(lang, Key::Regenerate), lang))
            .on_ok(move |_, _, cx| {
                let (provider_id, model_id) = target.clone().unzip();
                app.update(cx, |this, cx| {
                    this.regenerate_message(&message_id, provider_id, model_id, cx)
                });
                true
            })
    });
}

fn danger_props(ok_text: &'static str, lang: AppLanguage) -> DialogButtonProps {
    DialogButtonProps::default()
        .ok_text(ok_text)
        .ok_variant(ButtonVariant::Danger)
        .cancel_text(tr(lang, Key::Cancel))
        .show_cancel(true)
}

pub fn confirm_delete_session(
    app: Entity<AppState>,
    session_id: String,
    title: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let lang = app.read(cx).language();
    let description = tr_args(lang, Key::DeleteSessionDesc, &[title]);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let app = app.clone();
        let session_id = session_id.clone();
        alert
            .title(tr(lang, Key::DeleteSessionTitle))
            .description(description.clone())
            .button_props(danger_props(tr(lang, Key::Delete), lang))
            .on_ok(move |_, _, cx| {
                app.update(cx, |this, cx| this.delete_session(session_id.clone(), cx));
                true
            })
    });
}

pub fn confirm_clear_session(app: Entity<AppState>, window: &mut Window, cx: &mut App) {
    let lang = app.read(cx).language();
    window.open_alert_dialog(cx, move |alert, _, _| {
        let app = app.clone();
        alert
            .title(tr(lang, Key::ClearChatTitle))
            .description(tr(lang, Key::ClearChatDesc))
            .button_props(danger_props(tr(lang, Key::Clear), lang))
            .on_ok(move |_, _, cx| {
                app.update(cx, |this, cx| this.clear_current_session(cx));
                true
            })
    });
}

pub fn confirm_delete_provider(app: Entity<AppState>, provider_name: String, window: &mut Window, cx: &mut App) {
    let lang = app.read(cx).language();
    let description = tr_args(lang, Key::DeleteChannelDesc, &[&provider_name]);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let app = app.clone();
        alert
            .title(tr(lang, Key::DeleteChannelTitle))
            .description(description.clone())
            .button_props(danger_props(tr(lang, Key::Delete), lang))
            .on_ok(move |_, window, cx| {
                app.update(cx, |this, cx| this.delete_selected_provider(window, cx));
                true
            })
    });
}

pub fn open_edit_message_dialog(app: Entity<AppState>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = Palette::new(cx);
        let lang = app.read(cx).language();
        let input = app.read(cx).edit_message_input.clone();
        let ok_app = app.clone();
        let enter_app = app.clone();
        dialog
            .title(tr(lang, Key::EditResend))
            .w(px(520.))
            .child(field(
                tr(lang, Key::MessageContent),
                Some(tr(lang, Key::EditResendHint)),
                Textarea::new(&input),
                &p,
            ))
            .footer(footer(
                tr(lang, Key::Resend),
                move |window, cx| {
                    if ok_app.update(cx, |this, cx| this.confirm_edit_message(window, cx)) {
                        window.close_dialog(cx);
                    }
                },
                lang,
            ))
            .on_ok(move |_, window, cx| enter_app.update(cx, |this, cx| this.confirm_edit_message(window, cx)))
    });
}

pub fn open_folder_dialog(app: Entity<AppState>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = Palette::new(cx);
        let lang = app.read(cx).language();
        let input = app.read(cx).folder_name_input.clone();
        let ok_app = app.clone();
        let enter_app = app.clone();
        dialog
            .title(tr(lang, Key::MoveToFolderDialog))
            .w(px(420.))
            .child(field(
                tr(lang, Key::FolderName),
                Some(tr(lang, Key::FolderNameHint)),
                Input::new(&input),
                &p,
            ))
            .footer(footer(
                tr(lang, Key::Move),
                move |window, cx| {
                    if ok_app.update(cx, |this, cx| this.confirm_move_folder(window, cx)) {
                        window.close_dialog(cx);
                    }
                },
                lang,
            ))
            .on_ok(move |_, window, cx| enter_app.update(cx, |this, cx| this.confirm_move_folder(window, cx)))
    });
}
