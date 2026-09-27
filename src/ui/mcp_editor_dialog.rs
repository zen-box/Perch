//! 添加 / 编辑 MCP 服务器的弹窗。
//!
//! 原来是页内展开的表单，放在服务器列表上方。改成弹窗有两个理由：
//!
//! 1. **它和详情区抢同一块地方**。编辑器一展开，下面选中的那台服务器的工具清单和日志
//!    就得让位；用户对着日志改命令是常态，让位反而挡住了最有用的信息。
//! 2. **它是模态的**。填到一半去点别的服务器、或者顺手改个开关，草稿就会和界面对不上。
//!    弹窗把「正在填表」这件事和背景隔开，语义更直白。
//!
//! 草稿仍存在 `McpState::editor` 里（`mcp_ops.rs`），这里只负责渲染和转发，
//! 不另建一份状态。

use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::*;

use super::Palette;
use super::dialogs::{field, footer};
use crate::app::AppState;
use crate::i18n::{Key, tr};

/// 打开添加 / 编辑 MCP 服务器的弹窗。
///
/// 打开前要先调 `AppState::open_mcp_editor` 把草稿准备好（它要窗口才能建输入框实体）；
/// 草稿不在就只画个标题——理论上不会发生，但不 panic。
pub fn open_mcp_editor_dialog(app: Entity<AppState>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = Palette::new(cx);
        let lang = app.read(cx).language();
        let Some(editor) = app.read(cx).mcp.editor.as_ref() else {
            return dialog.title(tr(lang, Key::McpAddServer));
        };
        let editing = editor.editing_id.is_some();
        let (name, command, args, cwd, env) = (
            editor.name.clone(),
            editor.command.clone(),
            editor.args.clone(),
            editor.cwd.clone(),
            editor.env.clone(),
        );

        let ok_app = app.clone();
        let enter_app = app.clone();
        let close_app = app.clone();
        dialog
            .title(if editing {
                tr(lang, Key::McpEditServer)
            } else {
                tr(lang, Key::McpAddServer)
            })
            .w(px(560.))
            .margin_top(px(48.))
            .child(
                v_flex()
                    .gap_4()
                    .pb_1()
                    .child(field(tr(lang, Key::McpName), None, Input::new(&name), &p))
                    .child(field(
                        tr(lang, Key::McpCommand),
                        Some(tr(lang, Key::McpCommandHint)),
                        Input::new(&command),
                        &p,
                    ))
                    .child(field(
                        tr(lang, Key::McpArgs),
                        Some(tr(lang, Key::McpArgsHint)),
                        Textarea::new(&args),
                        &p,
                    ))
                    .child(field(
                        tr(lang, Key::McpCwd),
                        Some(tr(lang, Key::McpCwdHint)),
                        Input::new(&cwd),
                        &p,
                    ))
                    .child(field(
                        tr(lang, Key::McpEnv),
                        Some(tr(lang, Key::McpEnvHint)),
                        Textarea::new(&env),
                        &p,
                    )),
            )
            .footer(footer(
                if editing {
                    tr(lang, Key::Save)
                } else {
                    tr(lang, Key::McpAddServer)
                },
                move |window, cx| {
                    // 名字或命令为空时 `save_mcp_editor` 会提示并返回 false，
                    // 这时**不能关弹窗**——一关用户填的东西就没了，得留在原地改。
                    if ok_app.update(cx, |this, cx| this.save_mcp_editor(cx)) {
                        window.close_dialog(cx);
                    }
                },
                lang,
            ))
            .on_ok(move |_, _, cx| enter_app.update(cx, |this, cx| this.save_mcp_editor(cx)))
            // 只在**真正关掉**时触发（`on_ok` 返回 false 时不会走这里），所以取消、ESC、
            // 点遮罩都会把草稿丢掉，不留一堆没人用的输入框实体。保存成功时草稿已经是
            // `None` 了，这里再清一次是无副作用的。
            .on_close(move |_, _, cx| close_app.update(cx, |this, cx| this.close_mcp_editor(cx)))
    });
}
