//! composer 工具栏上的「项目目录」按钮。
//!
//! **只在智能体模式下出现**：对话模式碰不到本机文件，摆一个目录按钮只会让人以为能用。
//!
//! 按钮上只放目录名（项目名可以很长），全路径放悬停提示里，点开才是「更换 / 清除」。
//! 还没设目录时点一下**直接开文件夹对话框**——这时候没有「更换 / 清除」可言，
//! 弹一层只有两项的菜单纯属多一步。

use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::*;
use gpui_kit_assets::IconName;

use crate::app::AppState;
use crate::i18n::{Key, tr};

/// 按钮上目录名的最大宽度。项目名可以很长，不限宽会把同一行的模式开关和工具角标挤走。
const WORKSPACE_LABEL_MAX_WIDTH: f32 = 120.;

/// 「项目目录」按钮。对话模式下返回 `None`，由调用方跳过渲染。
pub(super) fn render_workspace_picker(state: &AppState, cx: &mut Context<AppState>) -> Option<AnyElement> {
    if !state.session_is_agent() {
        return None;
    }
    let lang = state.language();
    let Some(dir) = state.session_workspace() else {
        // 还没设：一个按钮，点了直接开文件夹对话框
        return Some(
            Button::new("session-workspace-pick")
                .ghost()
                .xsmall()
                .icon(IconName::FolderPlus)
                .label(tr(lang, Key::WorkspaceChoose))
                .tooltip(tr(lang, Key::WorkspaceHint))
                .on_click(cx.listener(|this, _, _, cx| this.pick_session_workspace(cx)))
                .into_any_element(),
        );
    };

    let app = cx.entity();
    let label = dir.display_name();
    let full_path = dir.root().display().to_string();
    let change_app = app.clone();
    let clear_app = app.clone();

    Some(
        Button::new("session-workspace-trigger")
            .ghost()
            .xsmall()
            .icon(IconName::Folder)
            .child(
                div()
                    .max_w(px(WORKSPACE_LABEL_MAX_WIDTH))
                    .truncate()
                    .child(label.clone()),
            )
            .dropdown_caret(true)
            // 按钮上只有目录名，全路径放悬停提示里
            .tooltip(full_path)
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                menu.item(
                    PopupMenuItem::new(tr(lang, Key::WorkspaceChange))
                        .icon(IconName::FolderOpen)
                        .on_click({
                            let app = change_app.clone();
                            move |_, _, cx| app.update(cx, |this, cx| this.pick_session_workspace(cx))
                        }),
                )
                .item(
                    PopupMenuItem::new(tr(lang, Key::WorkspaceClear))
                        .icon(IconName::Trash)
                        .on_click({
                            let app = clear_app.clone();
                            move |_, _, cx| app.update(cx, |this, cx| this.clear_session_workspace(cx))
                        }),
                )
            })
            .into_any_element(),
    )
}
