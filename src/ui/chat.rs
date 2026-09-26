use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::message_scroller::MessageScroller;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::brand_icon::{
    FILE_TYPE_CODE, FILE_TYPE_DOC, FILE_TYPE_PDF, FILE_TYPE_SHEET, FILE_TYPE_SLIDES, model_avatar, model_id_avatar,
};
use super::{CONTENT_MAX_WIDTH, Palette, dialogs, icon_tile};
use super::{composer, empty_state, message_assistant, message_user};
use crate::app::AppState;
use crate::model::Attachment;

// ================= 对话主区域 =================

pub fn render_chat_panel(state: &mut AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    state.sync_message_list(cx);

    let app = cx.entity();
    let has_messages = state
        .storage
        .get_active_session()
        .is_some_and(|s| !s.messages.is_empty());

    v_flex()
        .flex_1()
        .min_w_0()
        .h_full()
        .child(render_chat_header(state, p, cx))
        .child(div().flex_1().min_h_0().w_full().map(|this| {
            if has_messages {
                this.child(
                    MessageScroller::new("messages", state.message_list.clone(), {
                        let app = app.clone();
                        move |ix, window, cx| render_message_row(&app, ix, window, cx)
                    })
                    .with_list_style(StyleRefinement::default().pt_6().pb_4())
                    .with_jump_button_label("回到最新")
                    .with_bottom_fade(p.background),
                )
            } else {
                this.child(empty_state::render_empty_state(state, p, cx))
            }
        }))
        .child(
            v_flex()
                .w_full()
                .items_center()
                .gap_3()
                .px_6()
                .pb_4()
                .pt_1()
                .when(state.pending_import.is_some(), |this| {
                    this.child(render_import_banner(p, cx))
                })
                .when(
                    state
                        .storage
                        .get_active_session()
                        .is_some_and(|session| session.has_unresolved_compare()),
                    |this| this.child(render_compare_notice(p)),
                )
                .when(state.pending_tool_name.is_some(), |this| {
                    this.child(render_tool_permission(state, p, cx))
                })
                .child(composer::render_composer(state, p, cx)),
        )
}

fn render_chat_header(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let (title, message_count, session_id, pinned, favorite) = state
        .storage
        .get_active_session()
        .map(|session| {
            (
                session.title.clone(),
                session.messages.len(),
                session.id.clone(),
                session.pinned,
                session.favorite,
            )
        })
        .unwrap_or_else(|| ("新对话".into(), 0, String::new(), false, false));

    h_flex()
        .h(px(48.))
        .flex_none()
        .px_4()
        .gap_3()
        .justify_between()
        .border_b_1()
        .border_color(p.border)
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .gap_2()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .when(message_count > 0, |this| {
                    this.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(p.muted_foreground)
                            .child(format!("{} 条消息", message_count)),
                    )
                }),
        )
        .child(
            h_flex()
                .flex_none()
                .gap_1()
                .when(state.sidebar_collapsed, |this| {
                    this.child(
                        Button::new("header-new-chat")
                            .ghost()
                            .small()
                            .icon(IconName::SquarePen)
                            .tooltip("新建对话 (Ctrl+N)")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.create_new_session(window, cx);
                            })),
                    )
                })
                .when(!session_id.is_empty(), |this| {
                    let pin_id = session_id.clone();
                    let fav_id = session_id.clone();
                    this.child(
                        Button::new("header-pin")
                            .ghost()
                            .small()
                            .icon(if pinned { IconName::PinOff } else { IconName::Pin })
                            .selected(pinned)
                            .tooltip(if pinned { "取消置顶" } else { "置顶" })
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle_session_pin(&pin_id, cx))),
                    )
                    .child(
                        Button::new("header-favorite")
                            .ghost()
                            .small()
                            .icon(if favorite { IconName::StarOff } else { IconName::Star })
                            .selected(favorite)
                            .tooltip(if favorite { "取消收藏" } else { "收藏" })
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle_session_favorite(&fav_id, cx))),
                    )
                })
                .child(
                    Button::new("export-markdown")
                        .ghost()
                        .small()
                        .icon(IconName::Download)
                        .tooltip("导出为 Markdown")
                        .disabled(message_count == 0)
                        .on_click(cx.listener(|this, _, _, cx| this.export_current_session(cx))),
                )
                .child(
                    Button::new("export-json")
                        .ghost()
                        .small()
                        .icon(IconName::FileDown)
                        .tooltip("导出 JSON 备份")
                        .on_click(cx.listener(|this, _, _, cx| this.export_json_backup(cx))),
                )
                .child(
                    Button::new("import-json")
                        .ghost()
                        .small()
                        .icon(IconName::FileUp)
                        .tooltip("从 JSON 备份恢复")
                        .on_click(cx.listener(|this, _, _, cx| this.pick_import_backup(cx))),
                )
                .child(
                    Button::new("clear-session")
                        .ghost()
                        .small()
                        .icon(IconName::Eraser)
                        .tooltip("清空当前对话")
                        .disabled(message_count == 0)
                        .on_click(cx.listener(|_, _, window, cx| {
                            dialogs::confirm_clear_session(cx.entity(), window, cx);
                        })),
                ),
        )
}

// ================= 消息 =================

fn render_message_row(app: &Entity<AppState>, ix: usize, _: &mut Window, cx: &mut App) -> AnyElement {
    let (msg, owner, expanded, later, streaming) = {
        let state = app.read(cx);
        let Some(session) = state.storage.get_active_session() else {
            return div().into_any_element();
        };
        let Some(msg) = session.messages.get(ix).cloned() else {
            return div().into_any_element();
        };
        let owner = state
            .config
            .providers
            .iter()
            .find_map(|provider| provider.models.iter().find(|model| model.id == msg.model).cloned());
        let expanded = state.expanded_reasoning.contains(&msg.id);
        // 这条消息之后还有几条，重新生成时会被删掉
        let later = session.messages.len().saturating_sub(ix + 1);
        (msg, owner, expanded, later, state.is_streaming)
    };

    let p = Palette::new(cx);
    let content = if msg.role == "user" {
        message_user::render_user_message(app, ix, msg, streaming, &p).into_any_element()
    } else {
        let (avatar, model_label) = match owner {
            Some(model) => (model_avatar(&model, px(28.), &p), model.name.clone()),
            None if msg.model.is_empty() => (
                icon_tile(IconName::Sparkles, px(28.), p.primary.opacity(0.12), p.primary).into_any_element(),
                "Assistant".to_string(),
            ),
            None => (model_id_avatar(&msg.model, px(28.), &p), msg.model.clone()),
        };
        message_assistant::render_assistant_message(app, ix, msg, avatar, model_label, expanded, later, &p, cx)
            .into_any_element()
    };

    div()
        .w_full()
        .flex()
        .justify_center()
        .px_3()
        .child(div().w_full().max_w(CONTENT_MAX_WIDTH).child(content))
        .into_any_element()
}

fn render_import_banner(p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    h_flex()
        .w_full()
        .max_w(CONTENT_MAX_WIDTH)
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .rounded_lg()
        .border_1()
        .border_color(p.warning.opacity(0.45))
        .bg(p.warning.opacity(if p.is_dark { 0.12 } else { 0.07 }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .child("已读取 JSON 备份。恢复会覆盖本机会话、提示词和渠道配置，不会写入 API Key。"),
        )
        .child(
            Button::new("cancel-import")
                .ghost()
                .small()
                .label("取消")
                .on_click(cx.listener(|this, _, _, cx| this.cancel_import(cx))),
        )
        .child(
            Button::new("confirm-import")
                .primary()
                .small()
                .label("恢复")
                .on_click(cx.listener(|this, _, _, cx| this.confirm_import(cx))),
        )
}

fn render_compare_notice(p: &Palette) -> impl IntoElement {
    div()
        .w_full()
        .max_w(CONTENT_MAX_WIDTH)
        .px_3()
        .py_2()
        .rounded_lg()
        .bg(p.muted)
        .text_sm()
        .text_color(p.muted_foreground)
        .child("请先采用一条对比回答，再继续对话")
}

pub(super) fn attachment_badge(att: &Attachment, p: &Palette) -> (IconName, Hsla, &'static str) {
    if att.is_image() {
        (IconName::Image, p.primary, "图片")
    } else if att.is_pdf() {
        (IconName::FileText, FILE_TYPE_PDF, "PDF")
    } else if att.is_text() {
        (IconName::FileCode, FILE_TYPE_CODE, "文本/代码")
    } else {
        let ext = std::path::Path::new(&att.name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "xlsx" | "xls" | "csv" => (IconName::FileSpreadsheet, FILE_TYPE_SHEET, "表格"),
            "docx" | "doc" => (IconName::FileText, FILE_TYPE_DOC, "Word"),
            "pptx" | "ppt" => (IconName::FileText, FILE_TYPE_SLIDES, "PPT"),
            _ => (IconName::File, p.muted_foreground, "文件"),
        }
    }
}

// ================= 工具授权卡片 =================

fn render_tool_permission(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let tool_name = state.pending_tool_name.clone().unwrap_or_default();
    let tool_cmd = state.pending_tool_cmd.clone().unwrap_or_default();
    let exec_tool = tool_name.clone();
    let exec_arg = tool_cmd.clone();
    let mono_font = cx.theme().mono_font_family.clone();

    v_flex()
        .w_full()
        .max_w(CONTENT_MAX_WIDTH)
        .gap_3()
        .p_4()
        .rounded_xl()
        .border_1()
        .border_color(p.warning.opacity(0.45))
        .bg(p.warning.opacity(if p.is_dark { 0.12 } else { 0.07 }))
        .child(
            h_flex()
                .gap_2()
                .child(Icon::new(IconName::ShieldAlert).size(px(16.)).text_color(p.warning))
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("需要你的授权"))
                .child(Tag::warning().small().child(tool_name)),
        )
        .child(
            div()
                .text_xs()
                .text_color(p.muted_foreground)
                .child("即将在本机执行下面的命令，请确认内容安全："),
        )
        .child(
            div()
                .w_full()
                .px_3()
                .py_2()
                .rounded_md()
                .bg(p.background)
                .border_1()
                .border_color(p.border)
                .font_family(mono_font)
                .text_xs()
                .child(tool_cmd),
        )
        .child(
            h_flex()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("deny-tool")
                        .ghost()
                        .small()
                        .label("拒绝")
                        .on_click(cx.listener(|this, _, _, cx| this.deny_pending_tool(cx))),
                )
                .child(
                    Button::new("allow-tool")
                        .primary()
                        .small()
                        .icon(IconName::Check)
                        .label("允许执行一次")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.execute_agent_tool(&exec_tool, &exec_arg, cx);
                        })),
                ),
        )
}

pub(super) fn format_msg_time(created_at: &str) -> &str {
    if let Some(pos) = created_at.rfind(' ') {
        &created_at[pos + 1..]
    } else {
        created_at
    }
}

pub(super) fn markdown_view(id: impl Into<SharedString>, source: impl Into<SharedString>) -> TextView {
    let source_str: SharedString = source.into();
    let cleaned = super::markdown_image::normalize_markdown_image_urls(&source_str);
    TextView::markdown(id.into(), cleaned)
        .selectable(true)
        .plugin(super::markdown_image::ChatImagePlugin)
}

pub(super) fn preview(text: &str, limit: usize) -> String {
    let flat = text.trim().replace('\n', " ");
    let mut out: String = flat.chars().take(limit).collect();
    if flat.chars().count() > limit {
        out.push('…');
    }
    out
}
