//! 设置页「MCP 服务器」：服务器列表、详情（工具 + 日志）。
//!
//! 添加 / 编辑的表单是弹窗，见 `mcp_editor_dialog.rs`——它原来是页内展开的，
//! 展开时会盖住下面选中的那台服务器的工具清单和日志，而那两样正是改命令时最想看的。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::Palette;
use super::dialogs::confirm_delete_mcp_server;
use super::icon_tile;
use super::mcp_editor_dialog::open_mcp_editor_dialog;
use super::settings::page;
use crate::app::AppState;
use crate::config::{McpServerConfig, McpTransport};
use crate::i18n::{AppLanguage, Key, tr, tr_args};
use crate::mcp_ops::ServerStatus;

pub(super) fn render_mcp(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let lang = state.language();
    let mut content = v_flex().gap_8();

    // 这里**不该**再提示「本地工具总开关关着」——2026-09-27 起那个开关只管本机工具，
    // MCP 不受它管。原来的提示会把用户引去开一个和 MCP 无关的开关，开了也没用。

    content = content.child(render_server_list(state, p, lang, cx));

    // 编辑现在是弹窗，不会再和详情抢地方，所以选中哪台就一直显示它的详情
    if let Some(server) = state
        .mcp
        .selected_server_id
        .as_ref()
        .and_then(|id| state.config.mcp_servers.iter().find(|server| &server.id == id))
    {
        content = content.child(render_detail(state, server, p, lang, cx));
    }

    page(
        "settings-mcp",
        tr(lang, Key::McpSettings),
        tr(lang, Key::McpSettingsDesc),
        p,
        content,
    )
}

/// 服务器列表。
fn render_server_list(
    state: &AppState,
    p: &Palette,
    lang: AppLanguage,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let mut rows: Vec<AnyElement> = Vec::new();
    if state.config.mcp_servers.is_empty() {
        rows.push(
            v_flex()
                .items_center()
                .gap_2()
                .px_4()
                .py_8()
                .child(Icon::new(IconName::Plug).size(px(24.)).text_color(p.muted_foreground))
                .child(
                    div()
                        .max_w(px(380.))
                        .text_center()
                        .text_sm()
                        .text_color(p.muted_foreground)
                        .child(tr(lang, Key::McpEmpty)),
                )
                .into_any_element(),
        );
    }
    for server in &state.config.mcp_servers {
        rows.push(render_server_row(state, server, p, lang, cx));
    }
    // 「添加服务器」放在卡片外面的右上角：它改的是整个列表，不属于某一行
    v_flex()
        .gap_2()
        .child(
            h_flex()
                .justify_between()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(tr(lang, Key::McpSettings)),
                )
                .child(
                    Button::new("mcp-add-server")
                        .outline()
                        .small()
                        .icon(IconName::Plus)
                        .label(tr(lang, Key::McpAddServer))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_mcp_editor(None, window, cx);
                            open_mcp_editor_dialog(cx.entity(), window, cx);
                        })),
                ),
        )
        .child(
            v_flex()
                .rounded_lg()
                .border_1()
                .border_color(p.border)
                .children(rows.into_iter().enumerate().map(|(ix, row)| {
                    div()
                        .when(ix > 0, |this| this.border_t_1().border_color(p.border))
                        .child(row)
                })),
        )
}

fn render_server_row(
    state: &AppState,
    server: &McpServerConfig,
    p: &Palette,
    lang: AppLanguage,
    cx: &mut Context<AppState>,
) -> AnyElement {
    let status = state.mcp.status(&server.id);
    let tool_count = state.mcp.tools_of(&server.id).len();
    let is_selected = state.mcp.selected_server_id.as_deref() == Some(server.id.as_str());
    let is_ready = status.is_ready();
    let connect_label = match status {
        ServerStatus::Ready => tr(lang, Key::McpDisconnect),
        // 从没连过是「连接」，连过又断了是「重连」——用户按这个按钮时想的是不同的事
        ServerStatus::Idle => tr(lang, Key::McpConnect),
        ServerStatus::Connecting | ServerStatus::Failed(_) => tr(lang, Key::McpReconnect),
    };

    let select_id = server.id.clone();
    let connect_id = server.id.clone();
    let edit_id = server.id.clone();
    let delete_id = server.id.clone();
    let delete_name = server.name.clone();
    let enable_id = server.id.clone();
    let enabled = server.enabled;

    h_flex()
        .id(SharedString::from(format!("mcp-server-{}", server.id)))
        .gap_3()
        .px_4()
        .py_3()
        .cursor_pointer()
        .when(is_selected, |this| this.bg(p.muted))
        .hover(|style| style.bg(p.muted.opacity(0.6)))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.select_mcp_server(Some(&select_id), cx);
        }))
        .child(icon_tile(IconName::Plug, px(32.), p.muted, p.muted_foreground))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(server.name.clone()),
                        )
                        .child(status_badge(&status, tool_count, p, lang)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(p.muted_foreground)
                        .child(transport_summary(server)),
                )
                // 连不上的原因要看得见：用户改完命令第一件事就是想知道它报了什么错。
                // **重连期间也留着**（转灰、加「上次失败」前缀）：这一行一消失整行就矮一截，
                // 下面几台服务器跟着往上跳，连上或者再失败时又跳回来——那就是「点重连页面闪动」。
                .children(match &status {
                    ServerStatus::Failed(error) => {
                        Some(div().text_xs().text_color(p.danger).child(first_line(error, 160)))
                    }
                    ServerStatus::Connecting => state.mcp.last_error_of(&server.id).map(|error| {
                        div().text_xs().text_color(p.muted_foreground).child(tr_args(
                            lang,
                            Key::McpLastError,
                            &[&first_line(error, 140)],
                        ))
                    }),
                    _ => None,
                }),
        )
        .child(
            h_flex()
                .flex_none()
                .gap_1()
                .child(
                    Button::new(SharedString::from(format!("mcp-connect-{}", server.id)))
                        .ghost()
                        .small()
                        .icon(if is_ready { IconName::Power } else { IconName::RefreshCw })
                        .label(connect_label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if is_ready {
                                this.disconnect_mcp_server(&connect_id, cx);
                            } else {
                                this.connect_mcp_server(&connect_id, cx);
                            }
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("mcp-edit-{}", server.id)))
                        .ghost()
                        .small()
                        .icon(IconName::Pencil)
                        .tooltip(tr(lang, Key::McpEditServer))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_mcp_editor(Some(&edit_id), window, cx);
                            open_mcp_editor_dialog(cx.entity(), window, cx);
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("mcp-delete-{}", server.id)))
                        .ghost()
                        .small()
                        .icon(IconName::Trash)
                        .tooltip(tr(lang, Key::Delete))
                        .on_click(cx.listener(move |_, _, window, cx| {
                            confirm_delete_mcp_server(cx.entity(), delete_id.clone(), delete_name.clone(), window, cx);
                        })),
                )
                // 开关单独包一层：点开关不该顺带把这一行选中
                .child(
                    h_flex().pl_2().child(
                        Switch::new(SharedString::from(format!("mcp-enabled-{}", server.id)))
                            .checked(enabled)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_mcp_server_enabled(&enable_id, !enabled, cx);
                            })),
                    ),
                ),
        )
        .into_any_element()
}

/// 状态小圆点 + 一句话。失败的具体原因单独占一行，见 [`render_server_row`]——
/// 原文可能很长，挤在徽标里会把整行撑开。
fn status_badge(status: &ServerStatus, tool_count: usize, p: &Palette, lang: AppLanguage) -> impl IntoElement {
    let (color, text) = match status {
        ServerStatus::Idle => (p.muted_foreground, tr(lang, Key::McpStatusIdle).to_string()),
        ServerStatus::Connecting => (p.warning, tr(lang, Key::McpStatusConnecting).to_string()),
        ServerStatus::Ready => (
            p.success,
            format!(
                "{} · {}",
                tr(lang, Key::McpStatusReady),
                tr_args(lang, Key::McpToolCount, &[&tool_count.to_string()])
            ),
        ),
        ServerStatus::Failed(_) => (p.danger, tr(lang, Key::McpStatusFailed).to_string()),
    };
    h_flex()
        .gap_1p5()
        .flex_none()
        .child(div().flex_none().size(px(6.)).rounded_full().bg(color))
        .child(div().text_xs().text_color(color).child(text))
}

/// 命令行摘要（`npx -y @modelcontextprotocol/server-filesystem`）。
fn transport_summary(server: &McpServerConfig) -> String {
    let summary = match &server.transport {
        McpTransport::Stdio { command, args, .. } => {
            let mut summary = command.clone();
            for arg in args {
                summary.push(' ');
                summary.push_str(arg);
            }
            summary
        }
        // HTTP 服务器没有命令行，摘要就是那个地址
        McpTransport::Http { url } => url.clone(),
    };
    first_line(&summary, 90)
}

/// 取第一行并截断：命令和环境变量里可能有换行，铺到界面上会把行高撑开。
fn first_line(text: &str, max_chars: usize) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    if line.chars().count() <= max_chars {
        return line.to_string();
    }
    let head: String = line.chars().take(max_chars).collect();
    format!("{head}…")
}

/// 选中服务器的详情：工具清单 + 运行日志。
fn render_detail(
    state: &AppState,
    server: &McpServerConfig,
    p: &Palette,
    lang: AppLanguage,
    cx: &mut Context<AppState>,
) -> impl IntoElement {
    let tools = state.mcp.tools_of(&server.id);
    let mut tool_rows: Vec<AnyElement> = Vec::new();
    if tools.is_empty() {
        tool_rows.push(
            div()
                .px_4()
                .py_4()
                .text_sm()
                .text_color(p.muted_foreground)
                .child(tr(lang, Key::McpNoTools))
                .into_any_element(),
        );
    }
    for tool in tools {
        // 停用状态按**原始工具名**查：暴露名里带着服务器 id，服务器改个名字就对不上了
        let enabled = !server.disabled_tools.contains(&tool.raw);
        let server_id = server.id.clone();
        let raw = tool.raw.clone();
        tool_rows.push(
            h_flex()
                .justify_between()
                .gap_4()
                .px_4()
                .py_3()
                .child(
                    v_flex()
                        .min_w_0()
                        .gap_0p5()
                        .child(
                            div()
                                .text_sm()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(tool.raw.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(p.muted_foreground)
                                .child(first_line(&tool.description, 120)),
                        ),
                )
                .child(
                    Switch::new(SharedString::from(format!("mcp-tool-{}-{}", server.id, tool.raw)))
                        .checked(enabled)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_mcp_tool_enabled(&server_id, &raw, !enabled, cx);
                        })),
                )
                .into_any_element(),
        );
    }

    let logs = state.mcp.logs_of(&server.id);
    let mono = cx.theme().mono_font_family.clone();
    let log_lines: Vec<AnyElement> = if logs.is_empty() {
        vec![
            div()
                .text_xs()
                .text_color(p.muted_foreground)
                .child(tr(lang, Key::McpLogsEmpty))
                .into_any_element(),
        ]
    } else {
        logs.into_iter()
            .map(|line| div().text_xs().font_family(mono.clone()).child(line).into_any_element())
            .collect()
    };

    v_flex()
        .gap_6()
        .child(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(tr(lang, Key::McpToolsSection)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(p.muted_foreground)
                        .child(tr(lang, Key::McpToolsHint)),
                )
                .child(v_flex().rounded_lg().border_1().border_color(p.border).children(
                    tool_rows.into_iter().enumerate().map(|(ix, row)| {
                        div()
                            .when(ix > 0, |this| this.border_t_1().border_color(p.border))
                            .child(row)
                    }),
                )),
        )
        .child(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(tr(lang, Key::McpLogsSection)),
                )
                .child(
                    v_flex()
                        .gap_px()
                        .p_3()
                        .max_h(px(220.))
                        .rounded_lg()
                        .border_1()
                        .border_color(p.border)
                        .bg(p.muted)
                        .overflow_y_scrollbar()
                        .id("mcp-logs")
                        .children(log_lines),
                ),
        )
}
