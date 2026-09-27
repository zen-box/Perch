//! composer 工具栏上的两个控件：模式开关（对话 / 智能体）和「本次对话的工具」选择器。
//!
//! 这两个控件是同一件事的两个入口——模式开关决定「这一轮要不要带工具」，选择器决定
//! 「带哪些」。**都由用户显式指定**，模型自己既不决定要不要用工具，也不决定用哪些
//! （产品决策，见 AGENTS.md §11）。
//!
//! 状态只有一份，在 `ChatSession::tools`；这里只负责渲染和转发点击，读写都在 `tool_ops.rs`。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::popover::{Popover, PopoverState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::Palette;
use super::chat::preview;
use super::settings::segmented;
use crate::app::AppState;
use crate::i18n::{Key, tr, tr_args};
use crate::tool_ops::ToolGroupLabel;

/// 「对话 / 智能体」分段开关。
pub(super) fn render_mode_switch(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let lang = state.language();
    let app = cx.entity();
    segmented(
        "session-mode",
        vec![
            (false, tr(lang, Key::Chat).into()),
            (true, tr(lang, Key::AgentMode).into()),
        ],
        state.session_tools_enabled(),
        p,
        move |enabled, _, cx| app.update(cx, |this, cx| this.set_session_tools_enabled(enabled, cx)),
    )
}

/// 「本次对话的工具」选择器。
pub(super) fn render_tool_picker(state: &AppState, cx: &mut Context<AppState>) -> impl IntoElement {
    let lang = state.language();
    let app = cx.entity();
    let enabled = state.session_tools_enabled();
    let count = state.picked_tool_count();
    Popover::new("session-tools")
        .anchor(Anchor::BottomLeft)
        .trigger(
            Button::new("session-tools-trigger")
                .ghost()
                .xsmall()
                .icon(IconName::Wrench)
                // 角标只在智能体模式下出现：对话模式下显示「3 个工具」会让人以为已经生效了
                .when(enabled, |this| {
                    this.primary()
                        .label(tr_args(lang, Key::McpToolCount, &[&count.to_string()]))
                })
                .tooltip(tr(lang, Key::SessionTools)),
        )
        .content(move |_, _, cx| render_tool_panel(&app, cx))
}

fn render_tool_panel(app: &Entity<AppState>, cx: &mut Context<PopoverState>) -> impl IntoElement + use<> {
    let p = Palette::new(cx);
    let lang = app.read(cx).language();
    let (groups, enabled, master_on, model_supports) = {
        let state = app.read(cx);
        (
            state.tool_groups(),
            state.session_tools_enabled(),
            state.config.local_tools_enabled,
            state.tools_supported_by_model(),
        )
    };

    let all_app = app.clone();
    let none_app = app.clone();
    let has_groups = !groups.is_empty();

    let rows: Vec<AnyElement> = groups
        .into_iter()
        .map(|group| {
            let label = match group.label {
                ToolGroupLabel::Local => tr(lang, Key::LocalTools).to_string(),
                ToolGroupLabel::McpServer(name) => name,
            };
            v_flex()
                .gap_1()
                .child(div().text_xs().text_color(p.muted_foreground).child(label))
                .child(v_flex().children(group.tools.into_iter().map(|tool| {
                    let checked = app.read(cx).is_tool_picked(&tool.name);
                    let row_app = app.clone();
                    let name = tool.name.clone();
                    h_flex()
                        .id(SharedString::from(format!("tool-{}", tool.name)))
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1p5()
                        .rounded_md()
                        .cursor_pointer()
                        .when(checked, |this| this.bg(p.accent))
                        .hover(|this| this.bg(p.accent))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            row_app.update(cx, |this, cx| this.toggle_session_tool(&name, cx));
                        }))
                        .child(checkbox(checked, &p))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(div().truncate().text_sm().text_color(p.foreground).child(tool.name))
                                .when(!tool.description.is_empty(), |this| {
                                    this.child(
                                        div()
                                            .truncate()
                                            .text_xs()
                                            .text_color(p.muted_foreground)
                                            // 描述来自服务器，长度不受我们控制。这里先按字符砍一刀，
                                            // 是因为**弹层的宽度由内容最宽的一行决定**（见函数末尾的说明），
                                            // 真实服务器那种整段文档式的描述会把面板撑得极宽。
                                            .child(preview(&tool.description, 36)),
                                    )
                                }),
                        )
                        .into_any_element()
                })))
                .into_any_element()
        })
        .collect();

    v_flex()
        .gap_2()
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(tr(lang, Key::SessionTools)),
                )
                .when(has_groups, |this| {
                    this.child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("tools-select-all")
                                    .ghost()
                                    .xsmall()
                                    .label(tr(lang, Key::SelectAllTools))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        all_app.update(cx, |this, cx| this.set_all_session_tools(true, cx));
                                    })),
                            )
                            .child(
                                Button::new("tools-select-none")
                                    .ghost()
                                    .xsmall()
                                    .label(tr(lang, Key::ClearAllTools))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        none_app.update(cx, |this, cx| this.set_all_session_tools(false, cx));
                                    })),
                            ),
                    )
                }),
        )
        // 提示按「离用户最近的原因」排：模型不支持 > 总开关关着 > 模式是对话。
        // 三条都摆出来只会让人不知道该先处理哪个。
        .when(!model_supports, |this| {
            this.child(hint(p.warning, tr(lang, Key::SessionToolsModelHint)))
        })
        .when(model_supports && !master_on, |this| {
            this.child(hint(p.warning, tr(lang, Key::McpDisabledByMaster)))
        })
        .when(model_supports && master_on && !enabled, |this| {
            this.child(hint(p.muted_foreground, tr(lang, Key::SessionToolsChatHint)))
        })
        .when(!has_groups, |this| {
            this.child(hint(p.muted_foreground, tr(lang, Key::SessionToolsEmpty)))
        })
        // ⚠️ 宽度**不要**写 `.w(px(..))` / `.max_w(px(..))`：实测这一层里两个都不生效，
        // 面板宽度永远等于「内容最宽的一行 + 内边距」——同一个弹层组件里的「参数」面板
        // 也设了 `Popover::w(px(360.))`，实测出来同样宽了约 150px。定宽改不动，就反过来
        // 控制内容的宽度（上面给描述加了字符上限）。高度同理，所以列表用一个外层 `div`
        // 的 `max_h` 兜住，超出部分靠滚动。
        .child(
            div()
                .max_h(px(320.))
                .child(v_flex().gap_2().children(rows).overflow_y_scrollbar()),
        )
}

/// 面板里的一条说明。
fn hint(color: Hsla, text: &'static str) -> impl IntoElement {
    h_flex()
        .gap_1p5()
        .items_center()
        .child(Icon::new(IconName::Info).size(px(13.)).text_color(color))
        .child(div().text_xs().text_color(color).child(text))
}

/// 勾选框。整行都能点，所以它自己不接事件。
fn checkbox(checked: bool, p: &Palette) -> impl IntoElement {
    div()
        .flex_none()
        .size(px(16.))
        .rounded_sm()
        .border_1()
        .border_color(if checked { p.primary } else { p.border })
        .when(checked, |this| this.bg(p.primary))
        .flex()
        .items_center()
        .justify_center()
        .when(checked, |this| {
            this.child(
                Icon::new(IconName::Check)
                    .size(px(12.))
                    .text_color(p.primary_foreground),
            )
        })
}
