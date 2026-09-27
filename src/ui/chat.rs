use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::message_scroller::MessageScroller;
use gpui_kit::component::spinner::Spinner;
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
use crate::agent_loop::PendingToolApproval;
use crate::app::AppState;
use crate::i18n::{AppLanguage, Key, tr, tr_args};
use crate::local_tools::PendingTool;
use crate::model::{Attachment, ChatMessage, DEFAULT_SESSION_TITLE};

// ================= 对话主区域 =================

pub fn render_chat_panel(state: &mut AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    state.sync_message_list(cx);

    let lang = state.language();
    let app = cx.entity();
    let has_messages = state
        .storage
        .get_active_session()
        .is_some_and(|s| !s.messages.is_empty());
    // 授权卡片只在它所属的对话里显示；挂在别的对话上时，这里只给一条提示
    let active_id = state.storage.active_session_id.clone();
    let approval_here = state.agent.pending_in(&active_id).cloned();
    let approval_elsewhere = state
        .agent
        .pending
        .as_ref()
        .filter(|pending| pending.session_id != active_id)
        .map(|pending| pending.session_id.clone());

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
                    .with_jump_button_label(tr(lang, Key::JumpToLatest))
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
                    this.child(render_import_banner(p, lang, cx))
                })
                .when(
                    state
                        .storage
                        .get_active_session()
                        .is_some_and(|session| session.has_unresolved_compare()),
                    |this| this.child(render_compare_notice(p, lang)),
                )
                .when_some(approval_here, |this, approval| {
                    this.child(render_tool_permission(&approval, lang, p, cx))
                })
                .when_some(approval_elsewhere, |this, session_id| {
                    this.child(render_pending_elsewhere(state, &session_id, p, cx))
                })
                .child(composer::render_composer(state, p, cx)),
        )
}

fn render_chat_header(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let lang = state.language();
    let (title, message_count, session_id, pinned, favorite) = state
        .storage
        .get_active_session()
        .map(|session| {
            (
                session_display_title(session, lang),
                session.messages.len(),
                session.id.clone(),
                session.pinned,
                session.favorite,
            )
        })
        .unwrap_or_else(|| (tr(lang, Key::NewChat).into(), 0, String::new(), false, false));

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
                            .child(tr_args(lang, Key::MessageCount, &[&message_count.to_string()])),
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
                            .tooltip(tr(lang, Key::NewChatShortcut))
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
                            .tooltip(if pinned {
                                tr(lang, Key::Unpin)
                            } else {
                                tr(lang, Key::Pin)
                            })
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle_session_pin(&pin_id, cx))),
                    )
                    .child(
                        Button::new("header-favorite")
                            .ghost()
                            .small()
                            .icon(if favorite { IconName::StarOff } else { IconName::Star })
                            .selected(favorite)
                            .tooltip(if favorite {
                                tr(lang, Key::Unfavorite)
                            } else {
                                tr(lang, Key::Favorite)
                            })
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle_session_favorite(&fav_id, cx))),
                    )
                })
                .child(
                    Button::new("export-markdown")
                        .ghost()
                        .small()
                        .icon(IconName::Download)
                        .tooltip(tr(lang, Key::ExportMarkdown))
                        .disabled(message_count == 0)
                        .on_click(cx.listener(|this, _, _, cx| this.export_current_session(cx))),
                )
                .child(
                    Button::new("export-json")
                        .ghost()
                        .small()
                        .icon(IconName::FileDown)
                        .tooltip(tr(lang, Key::ExportJson))
                        .on_click(cx.listener(|this, _, _, cx| this.export_json_backup(cx))),
                )
                .child(
                    Button::new("import-json")
                        .ghost()
                        .small()
                        .icon(IconName::FileUp)
                        .tooltip(tr(lang, Key::ImportJson))
                        .on_click(cx.listener(|this, _, _, cx| this.pick_import_backup(cx))),
                )
                .child(
                    Button::new("clear-session")
                        .ghost()
                        .small()
                        .icon(IconName::Eraser)
                        .tooltip(tr(lang, Key::ClearCurrentChat))
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
    let lang = app.read(cx).language();
    let content = if msg.role == "user" {
        message_user::render_user_message(app, ix, msg, streaming, lang, &p).into_any_element()
    } else if msg.role == "tool" {
        let expanded = app.read(cx).agent.expanded_results.contains(&msg.id);
        let mono_font = cx.theme().mono_font_family.clone();
        render_tool_result(app, &msg, expanded, mono_font, &p, lang)
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

// ================= 工具结果 =================

/// 折叠时最多显示的行数和字数。超出的部分点「展开全部」再看。
const TOOL_RESULT_COLLAPSED_LINES: usize = 12;
const TOOL_RESULT_COLLAPSED_CHARS: usize = 1500;

/// 一条工具执行结果。
///
/// 单独成块而不是挂在助手消息下面：一次追问里模型可能连着调好几个工具，
/// 挂在同一条消息上会挤成一堆，也看不出哪个结果对应哪次调用。
///
/// 正文按行显示：命令输出、目录列表、文件内容的换行都有意义，压成一行就没法看了。
fn render_tool_result(
    app: &Entity<AppState>,
    msg: &ChatMessage,
    expanded: bool,
    mono_font: SharedString,
    p: &Palette,
    lang: AppLanguage,
) -> AnyElement {
    let running = msg.is_streaming;
    let accent = if running {
        p.muted_foreground
    } else if msg.tool_is_error {
        p.danger
    } else {
        p.success
    };
    let title = if msg.tool_name.is_empty() {
        tr(lang, Key::ToolResultTitle).to_string()
    } else {
        msg.tool_name.clone()
    };
    let body = msg.content.trim_end();
    // 耗时和退出码是给用户看的执行细节：耗时总显示；退出码只在非 0 时显示——
    // 0 就是成功，绿框已经表达了，再写一行是噪音。
    let duration = if running { 0 } else { msg.tool_duration_ms };
    let failed_code = if running {
        None
    } else {
        msg.tool_exit_code.filter(|code| *code != 0)
    };
    let total_lines = body.lines().count();
    let long = total_lines > TOOL_RESULT_COLLAPSED_LINES || body.chars().count() > TOOL_RESULT_COLLAPSED_CHARS;
    let lines = if long && !expanded {
        collapsed_lines(body)
    } else {
        body.lines().map(str::to_string).collect()
    };
    let toggle_app = app.clone();
    let toggle_id = msg.id.clone();

    v_flex()
        .w_full()
        .max_w(CONTENT_MAX_WIDTH)
        .gap_2()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(accent.opacity(0.35))
        .bg(accent.opacity(if p.is_dark { 0.08 } else { 0.05 }))
        .child(
            h_flex()
                .gap_2()
                .child(if running {
                    Spinner::new().small().into_any_element()
                } else {
                    Icon::new(if msg.tool_is_error {
                        IconName::CircleAlert
                    } else {
                        IconName::Terminal
                    })
                    .size(px(14.))
                    .text_color(accent)
                    .into_any_element()
                })
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(accent)
                        .child(title),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(p.muted_foreground)
                        .child(format_msg_time(&msg.created_at).to_string()),
                )
                .when(duration > 0, |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(p.muted_foreground)
                            .child(format_tool_duration(duration)),
                    )
                })
                .when(failed_code.is_some(), |this| {
                    this.child(div().text_xs().text_color(p.danger).child(tr_args(
                        lang,
                        Key::ToolExitCode,
                        &[&failed_code.unwrap_or_default().to_string()],
                    )))
                }),
        )
        .when(running, |this| {
            this.child(
                div()
                    .text_xs()
                    .text_color(p.muted_foreground)
                    .child(tr(lang, Key::ToolRunning)),
            )
        })
        .when(!running && !lines.is_empty(), |this| {
            this.child(
                v_flex()
                    .w_full()
                    .font_family(mono_font)
                    .text_xs()
                    .text_color(p.foreground)
                    // 空行也要占一行的高度，不然段落之间的空行会被吃掉
                    .children(
                        lines
                            .into_iter()
                            .map(|line| div().child(if line.is_empty() { " ".to_string() } else { line })),
                    ),
            )
        })
        .when(long && !running, |this| {
            this.child(
                div().child(
                    Button::new(SharedString::from(format!("tool-toggle-{}", msg.id)))
                        .ghost()
                        .xsmall()
                        .icon(if expanded {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        })
                        .label(if expanded {
                            tr(lang, Key::CollapseResult).to_string()
                        } else {
                            tr_args(lang, Key::ShowAllLines, &[&total_lines.to_string()])
                        })
                        .on_click(move |_, _, cx| {
                            toggle_app.update(cx, |this, cx| this.toggle_tool_result(&toggle_id, cx));
                        }),
                ),
            )
        })
        .when(msg.local_only, |this| {
            this.child(
                h_flex()
                    .gap_1()
                    .text_xs()
                    .text_color(p.muted_foreground)
                    .child(Icon::new(IconName::Lock).size(px(12.)))
                    .child(tr(lang, Key::LocalOnlyNote)),
            )
        })
        .into_any_element()
}

/// 工具执行耗时的显示写法。
///
/// 用通用单位（ms / s / m）而不是本地化词：这串字符在所有语言下都看得懂，
/// 不必为它维护四份译文。
fn format_tool_duration(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let seconds = ms / 1000;
        format!("{}m{}s", seconds / 60, seconds % 60)
    }
}

/// 折叠状态下显示的开头几行：行数和总字数都有上限（一行压缩过的 JSON 就能有几万字）。
fn collapsed_lines(body: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut budget = TOOL_RESULT_COLLAPSED_CHARS;
    for line in body.lines().take(TOOL_RESULT_COLLAPSED_LINES) {
        if budget == 0 {
            break;
        }
        let piece: String = line.chars().take(budget).collect();
        budget = budget.saturating_sub(piece.chars().count().max(1));
        lines.push(piece);
    }
    lines
}

fn render_import_banner(p: &Palette, lang: AppLanguage, cx: &mut Context<AppState>) -> impl IntoElement {
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
                .child(tr(lang, Key::ImportConfirmHint)),
        )
        .child(
            Button::new("cancel-import")
                .ghost()
                .small()
                .label(tr(lang, Key::Cancel))
                .on_click(cx.listener(|this, _, _, cx| this.cancel_import(cx))),
        )
        .child(
            Button::new("confirm-import")
                .primary()
                .small()
                .label(tr(lang, Key::Restore))
                .on_click(cx.listener(|this, _, _, cx| this.confirm_import(cx))),
        )
}

fn render_compare_notice(p: &Palette, lang: AppLanguage) -> impl IntoElement {
    div()
        .w_full()
        .max_w(CONTENT_MAX_WIDTH)
        .px_3()
        .py_2()
        .rounded_lg()
        .bg(p.muted)
        .text_sm()
        .text_color(p.muted_foreground)
        .child(tr(lang, Key::AdoptCompareFirst))
}

pub(super) fn attachment_badge(att: &Attachment, p: &Palette, lang: AppLanguage) -> (IconName, Hsla, &'static str) {
    if att.is_image() {
        (IconName::Image, p.primary, tr(lang, Key::Image))
    } else if att.is_pdf() {
        (IconName::FileText, FILE_TYPE_PDF, "PDF")
    } else if att.is_text() {
        (IconName::FileCode, FILE_TYPE_CODE, tr(lang, Key::FileTypeText))
    } else {
        let ext = std::path::Path::new(&att.name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "xlsx" | "xls" | "csv" => (IconName::FileSpreadsheet, FILE_TYPE_SHEET, tr(lang, Key::FileTypeSheet)),
            "docx" | "doc" => (IconName::FileText, FILE_TYPE_DOC, "Word"),
            "pptx" | "ppt" => (IconName::FileText, FILE_TYPE_SLIDES, "PPT"),
            _ => (IconName::File, p.muted_foreground, tr(lang, Key::FileTypeFile)),
        }
    }
}

// ================= 工具授权卡片 =================

/// 授权卡片上展示的调用内容。
///
/// 命令**原样全文展示**：用户同意的就是这一串，截掉一段就可能正好藏住危险的部分。
/// 要写入的文件内容可能很长，只预览开头几十行，路径写在最前面。
fn approval_detail(tool: &PendingTool, lang: AppLanguage) -> String {
    const PREVIEW_LINES: usize = 20;
    let arg = |key: &str| {
        tool.arguments
            .get(key)
            .and_then(|value| value.as_str())
            .map(str::to_string)
    };
    match tool.name.as_str() {
        "run_command" => arg("command").unwrap_or_else(|| tool.arguments_summary()),
        "write_file" => {
            let path = arg("path").unwrap_or_default();
            let content = arg("content").unwrap_or_default();
            let lines: Vec<&str> = content.lines().collect();
            let shown = lines.len().min(PREVIEW_LINES);
            format!(
                "write_file → {path}\n{}\n{}",
                tr_args(
                    lang,
                    Key::ToolPreviewLines,
                    &[&shown.to_string(), &lines.len().to_string()]
                ),
                lines[..shown].join("\n")
            )
        }
        _ => tool.arguments_summary(),
    }
}

fn render_tool_permission(
    approval: &PendingToolApproval,
    lang: AppLanguage,
    p: &Palette,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    // 两个来源共用一个卡片：模型请求的调用（Agent 循环）和用户手打的斜杠命令
    let tool_name = approval.tool.name.clone();
    let tool_detail = approval_detail(&approval.tool, lang);
    let local_only = approval.is_local_only();
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
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(tr(lang, Key::ToolAuthRequired)),
                )
                .child(Tag::warning().small().child(tool_name)),
        )
        .child(
            div()
                .text_xs()
                .text_color(p.muted_foreground)
                .child(tr(lang, Key::ToolAuthHint)),
        )
        .child(
            v_flex()
                .w_full()
                .px_3()
                .py_2()
                .rounded_md()
                .bg(p.background)
                .border_1()
                .border_color(p.border)
                .font_family(mono_font)
                .text_xs()
                .children(tool_detail.lines().map(|line| {
                    div().child(if line.is_empty() {
                        " ".to_string()
                    } else {
                        line.to_string()
                    })
                })),
        )
        .when(local_only, |this| {
            this.child(
                h_flex()
                    .gap_1()
                    .text_xs()
                    .text_color(p.muted_foreground)
                    .child(Icon::new(IconName::Lock).size(px(12.)))
                    .child(tr(lang, Key::LocalOnlyNote)),
            )
        })
        .child(
            h_flex()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("deny-tool")
                        .ghost()
                        .small()
                        .label(tr(lang, Key::Deny))
                        .on_click(cx.listener(|this, _, _, cx| this.deny_pending_tool(cx))),
                )
                .child(
                    Button::new("allow-tool")
                        .primary()
                        .small()
                        .icon(IconName::Check)
                        .label(tr(lang, Key::AllowOnce))
                        .on_click(cx.listener(|this, _, _, cx| this.approve_pending_tool(cx))),
                ),
        )
}

/// 别的对话挂着授权卡片：这里提示一句，点「前往」切过去处理。
/// 卡片本身不搬过来——在这里点允许，结果和后续请求都属于那个对话，用户会搞混。
fn render_pending_elsewhere(
    state: &AppState,
    session_id: &str,
    p: &Palette,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let lang = state.language();
    let title = state
        .session(session_id)
        .map(|session| session_display_title(session, lang))
        .unwrap_or_default();
    let target = session_id.to_string();

    h_flex()
        .w_full()
        .max_w(CONTENT_MAX_WIDTH)
        .gap_2()
        .px_3()
        .py_2()
        .rounded_lg()
        .border_1()
        .border_color(p.warning.opacity(0.45))
        .bg(p.warning.opacity(if p.is_dark { 0.12 } else { 0.07 }))
        .child(Icon::new(IconName::ShieldAlert).size(px(14.)).text_color(p.warning))
        .child(div().flex_1().min_w_0().truncate().text_sm().child(tr_args(
            lang,
            Key::ApprovalPendingElsewhere,
            &[&title],
        )))
        .child(
            Button::new("goto-pending-approval")
                .ghost()
                .xsmall()
                .label(tr(lang, Key::GoToChat))
                .on_click(cx.listener(move |this, _, window, cx| this.switch_session(target.clone(), window, cx))),
        )
}

/// 对话在界面上显示的标题：还没被命名过的占位标题换成当前语言的「新对话」。
/// 数据里存的占位值是固定的（见 `model.rs::DEFAULT_SESSION_TITLE`），不随界面语言变。
fn session_display_title(session: &crate::model::ChatSession, lang: AppLanguage) -> String {
    if session.title_auto && session.title == DEFAULT_SESSION_TITLE {
        tr(lang, Key::NewChat).to_string()
    } else {
        session.title.clone()
    }
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

#[cfg(test)]
mod tests {
    use super::format_tool_duration;

    #[test]
    fn durations_read_naturally() {
        assert_eq!(format_tool_duration(0), "0ms");
        assert_eq!(format_tool_duration(320), "320ms");
        assert_eq!(format_tool_duration(1_200), "1.2s");
        assert_eq!(format_tool_duration(59_900), "59.9s");
        // 满一分钟换成 m+s，别让界面出现「125.4s」这种数
        assert_eq!(format_tool_duration(65_000), "1m5s");
    }
}
