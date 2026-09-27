//! composer 工具栏上的两个控件：模式开关（对话 / 智能体）和「本次对话的工具」选择器。
//!
//! 这两个控件是同一件事的两个入口——模式开关决定「能不能碰本机文件」，选择器决定
//! 「带哪些来源的工具」。**都由用户显式指定**，模型自己既不决定要不要用工具，
//! 也不决定用哪些（产品决策，见 AGENTS.md §11）。
//!
//! **勾的粒度是来源，不是工具**：勾一台 MCP 服务器就是把它整台交给模型，用哪个功能由
//! 模型按用户的问题自己挑。逐个工具勾对普通人是门槛，想精细控制的展开「高级」。
//!
//! 状态只有一份，在 `ChatSession::tools`；这里只负责渲染和转发点击，读写都在 `tool_ops.rs`。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::popover::{Popover, PopoverState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::Palette;
use super::chat::preview;
use super::settings::{SegmentedSize, segmented_sized};
use crate::app::AppState;
use crate::i18n::{Key, tr, tr_args};
use crate::model::Permission;
use crate::tool_ops::{SourceLabel, SourceNote};

/// 「对话 / 智能体」分段开关。
///
/// 用紧凑尺寸：它和模型、参数这些 xsmall 按钮排在同一行，设置页那种 28px 高、
/// 14px 字的分段块放进来会比旁边所有控件都重，一眼看过去全是它。
pub(super) fn render_mode_switch(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let lang = state.language();
    let app = cx.entity();
    segmented_sized(
        "session-mode",
        vec![
            (false, Some(IconName::MessageSquare), tr(lang, Key::Chat).into()),
            (true, Some(IconName::Bot), tr(lang, Key::AgentMode).into()),
        ],
        state.session_is_agent(),
        SegmentedSize::Compact,
        p,
        move |agent, _, cx| app.update(cx, |this, cx| this.set_session_mode(agent, cx)),
    )
}

/// 「本次对话的工具」选择器。
pub(super) fn render_tool_picker(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let lang = state.language();
    let app = cx.entity();
    let close_app = app.clone();
    let count = state.picked_tool_count();
    Popover::new("session-tools")
        .anchor(Anchor::BottomRight)
        .trigger(
            Button::new("session-tools-trigger")
                .ghost()
                .xsmall()
                .icon(IconName::Wrench)
                // 数字**一直在**，包括 0：它同时是"现在一个工具都不会带"的提示。
                // 只在非零时显示的话，用户会分不清"没带工具"和"这个控件没在计数"。
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if count > 0 { p.primary } else { p.muted_foreground })
                        .child(count.to_string()),
                )
                .tooltip(tr(lang, Key::SessionTools)),
        )
        // 面板一关，那次「要不要开完全权限」的询问就作废。不清掉的话，
        // 下次打开面板看到的还是警告和两个按钮，得先点一次「取消」才看得到开关本身。
        .on_open_change(move |open: &bool, _, cx: &mut App| {
            if !*open {
                close_app.update(cx, |this, cx| this.dismiss_full_permission(cx));
            }
        })
        .content(move |_, _, cx| render_tool_panel(&app, cx))
}

fn render_tool_panel(app: &Entity<AppState>, cx: &mut Context<PopoverState>) -> impl IntoElement + use<> {
    let p = Palette::new(cx);
    let lang = app.read(cx).language();
    let (groups, agent, master_on, model_supports, workspace_missing, permission) = {
        let state = app.read(cx);
        (
            state.source_groups(),
            state.session_is_agent(),
            state.config.local_tools_enabled,
            state.tools_supported_by_model(),
            state.workspace_missing(),
            state.session_permission(),
        )
    };

    let all_app = app.clone();
    let none_app = app.clone();
    let has_groups = !groups.is_empty();
    // 高级那一层只在有东西可停用时出现：一条来源都没勾，列一堆单个工具只是噪声。
    // 「有 note 的来源」也算没有东西可停用——那种情况它的工具表本来就是空的。
    let picked: Vec<usize> = groups
        .iter()
        .enumerate()
        .filter(|(_, group)| !group.tools.is_empty() && app.read(cx).is_source_picked(&group.source))
        .map(|(ix, _)| ix)
        .collect();

    let rows: Vec<AnyElement> = groups
        .iter()
        .map(|group| {
            let checked = app.read(cx).is_source_picked(&group.source);
            let row_app = app.clone();
            let source = group.source.clone();
            let label = match &group.label {
                SourceLabel::Local => tr(lang, Key::LocalTools).to_string(),
                SourceLabel::Skill => tr(lang, Key::ToolSourceSkills).to_string(),
                SourceLabel::McpServer(name) => name.clone(),
            };
            // 有 note 就把「N 个工具」换成原因：这一行现在一个工具都给不出来，
            // 还显示个数字只会让人以为勾了就能用。
            let trailing = match group.note {
                Some(SourceNote::LocalToolsOff) => tr(lang, Key::LocalToolsOffNote).to_string(),
                Some(SourceNote::NeedsWorkspace) => tr(lang, Key::WorkspaceMissing).to_string(),
                Some(SourceNote::NoSkills) => tr(lang, Key::ToolSourceNoSkills).to_string(),
                None => tr_args(lang, Key::ToolCount, &[group.tool_count().to_string().as_str()]),
            };
            let trailing_color = if group.note.is_some() {
                p.warning
            } else {
                p.muted_foreground
            };
            h_flex()
                .id(SharedString::from(format!("tool-source-{label}")))
                .items_center()
                .gap_2()
                .px_2()
                .py_1p5()
                .rounded_md()
                .cursor_pointer()
                .when(checked, |this| this.bg(p.accent))
                .hover(|this| this.bg(p.accent))
                .on_click(cx.listener(move |_, _, _, cx| {
                    let source = source.clone();
                    row_app.update(cx, |this, cx| this.toggle_session_source(&source, cx));
                }))
                .child(checkbox(checked, &p))
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .justify_between()
                        .items_center()
                        .gap_2()
                        .child(div().truncate().text_sm().text_color(p.foreground).child(label))
                        .child(div().flex_none().text_xs().text_color(trailing_color).child(trailing)),
                )
                .into_any_element()
        })
        .collect();

    // 高级那一层：把已勾来源下的单个工具列出来，取消勾选 = 这次不带它。
    // 用 `disabled_tools` 表达（黑名单），所以"勾上"才是不停用。
    let advanced: Vec<AnyElement> = picked
        .iter()
        .flat_map(|ix| {
            let group = &groups[*ix];
            let label = match &group.label {
                SourceLabel::Local => tr(lang, Key::LocalTools).to_string(),
                SourceLabel::Skill => tr(lang, Key::ToolSourceSkills).to_string(),
                SourceLabel::McpServer(name) => name.clone(),
            };
            let mut block = vec![
                div()
                    .text_xs()
                    .text_color(p.muted_foreground)
                    .child(label)
                    .into_any_element(),
            ];
            block.extend(group.tools.iter().map(|tool| {
                let enabled = !app.read(cx).session_has_disabled(&tool.name);
                let row_app = app.clone();
                let name = tool.name.clone();
                h_flex()
                    .id(SharedString::from(format!("tool-item-{}", tool.name)))
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|this| this.bg(p.accent))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        let name = name.clone();
                        row_app.update(cx, |this, cx| this.toggle_disabled_tool(&name, cx));
                    }))
                    .child(checkbox(enabled, &p))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(p.foreground)
                                    // 本机工具的名字是英文标识符，MCP 的是暴露名，
                                    // 都不适合再砍一刀——宽度靠描述那一行控制
                                    .child(tool.name.clone()),
                            )
                            .when(!tool.description.is_empty(), |this| {
                                this.child(
                                    div()
                                        .truncate()
                                        .text_xs()
                                        .text_color(p.muted_foreground)
                                        // 描述来自服务器，长度不受我们控制。这里先按字符砍一刀，
                                        // 是因为**弹层的宽度由内容最宽的一行决定**（见函数末尾的说明）。
                                        .child(preview(&tool.description, 30)),
                                )
                            }),
                    )
                    .into_any_element()
            }));
            block
        })
        .collect();
    let has_advanced = !advanced.is_empty();

    // 权限档。**只在智能体模式下出现**：对话模式压根碰不到本机文件，
    // 摆一个"权限"开关只会让人以为这里能放开什么。
    //
    // ⚠️ 二次确认做成**面板内联**，不是弹窗。`Popover` 走的是 `deferred()` 浮层，
    // 而弹窗层（`ui/mod.rs` 里的 `Root::render_dialog_layer`）只是普通子元素——
    // deferred 永远画在普通元素**上面**，所以从面板里弹出来的确认框会被面板自己盖住，
    // 连按钮都点不着。要么关掉面板再弹（得把面板的开合状态提到 `AppState`），
    // 要么就地确认。选了后者：警告和「仍然打开」就摆在被拨的那个开关旁边。
    let permission_block: Option<AnyElement> = agent.then(|| {
        let full = permission == Permission::Full;
        if app.read(cx).agent.permission_prompt && !full {
            let cancel_app = app.clone();
            let ok_app = app.clone();
            return v_flex()
                .gap_2()
                .pt_2()
                .border_t_1()
                .border_color(p.border)
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.danger)
                        .child(tr(lang, Key::FullPermissionTitle)),
                )
                .child(warning(p.danger, tr(lang, Key::FullPermissionDesc)))
                .child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("full-permission-cancel")
                                .ghost()
                                .xsmall()
                                .label(tr(lang, Key::Cancel))
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cancel_app.update(cx, |this, cx| this.dismiss_full_permission(cx));
                                })),
                        )
                        .child(
                            Button::new("full-permission-ok")
                                .danger()
                                .xsmall()
                                .label(tr(lang, Key::FullPermissionEnable))
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    ok_app.update(cx, |this, cx| this.set_session_permission(Permission::Full, cx));
                                })),
                        ),
                )
                .into_any_element();
        }

        let toggle_app = app.clone();
        v_flex()
            .gap_1p5()
            .pt_2()
            .border_t_1()
            .border_color(p.border)
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .when(full, |this| {
                                this.child(Icon::new(IconName::ShieldAlert).size(px(13.)).text_color(p.danger))
                            })
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if full { p.danger } else { p.muted_foreground })
                                    .child(tr(lang, Key::SessionPermission)),
                            ),
                    )
                    .child(
                        Switch::new("session-permission")
                            .checked(full)
                            .tooltip(tr(lang, Key::SessionPermissionHint))
                            .on_click(cx.listener(move |_, on: &bool, _, cx| {
                                if *on {
                                    toggle_app.update(cx, |this, cx| this.ask_full_permission(cx));
                                } else {
                                    // 往回收不需要确认——少给权限永远安全
                                    toggle_app
                                        .update(cx, |this, cx| this.set_session_permission(Permission::Default, cx));
                                }
                            })),
                    ),
            )
            // 已经是完全权限：开关本身变红，再把代价写在下面。收不掉，除非拨回去。
            .when(full, |this| {
                this.child(warning(p.danger, tr(lang, Key::FullPermissionWarning)))
                    .child(warning(p.muted_foreground, tr(lang, Key::NotASandbox)))
            })
            .into_any_element()
    });

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
                                        all_app.update(cx, |this, cx| this.set_all_session_sources(true, cx));
                                    })),
                            )
                            .child(
                                Button::new("tools-select-none")
                                    .ghost()
                                    .xsmall()
                                    .label(tr(lang, Key::ClearAllTools))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        none_app.update(cx, |this, cx| this.set_all_session_sources(false, cx));
                                    })),
                            ),
                    )
                }),
        )
        // 提示按「离用户最近的原因」排：模型不支持 > 没有可用的工具 > 本机开关关着。
        // 三条都摆出来只会让人不知道该先处理哪个。
        .when(!model_supports, |this| {
            this.child(hint(p.warning, tr(lang, Key::SessionToolsModelHint)))
        })
        .when(model_supports && !has_groups, |this| {
            this.child(hint(p.muted_foreground, tr(lang, Key::SessionToolsEmpty)))
        })
        // 智能体模式 + 本机工具关着：这时候「本机工具」那一行勾了也是空的，得说清楚。
        // （对话模式不提示——它本来就不该有本机工具，不是"被关掉了"。）
        .when(model_supports && agent && !master_on, |this| {
            this.child(hint(p.warning, tr(lang, Key::LocalToolsDisabledHint)))
        })
        // 智能体模式 + 还没选项目目录：本机工具一个都给不出来，告诉用户下一步做什么
        .when(model_supports && agent && master_on && workspace_missing, |this| {
            this.child(hint(p.warning, tr(lang, Key::WorkspaceHint)))
        })
        .when(model_supports && !agent, |this| {
            this.child(hint(p.muted_foreground, tr(lang, Key::SessionToolsChatHint)))
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
        .children(permission_block)
        .when(has_advanced, |this| {
            this.child(
                v_flex()
                    .gap_1()
                    .pt_1()
                    .border_t_1()
                    .border_color(p.border)
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.muted_foreground)
                            .child(tr(lang, Key::AdvancedTools)),
                    )
                    .child(
                        div()
                            .max_h(px(180.))
                            .child(v_flex().gap_1().children(advanced).overflow_y_scrollbar()),
                    ),
            )
        })
}

/// 面板里的一条说明。
fn hint(color: Hsla, text: &'static str) -> impl IntoElement {
    h_flex()
        .gap_1p5()
        .items_center()
        .child(Icon::new(IconName::Info).size(px(13.)).text_color(color))
        .child(div().text_xs().text_color(color).child(text))
}

/// 警示段的限宽。这一层的宽度由内容最宽的一行决定（见 `render_tool_panel` 末尾的
/// 说明），而权限那几句话都很长——不限宽会把整个面板撑到 600px 以上。
const WARNING_MAX_WIDTH: f32 = 250.;

/// 面板里的一段警示：图标 + **会换行**的说明。
///
/// 和 `hint` 的区别就在这个限宽：`hint` 是单行的，只适合短句。
fn warning(color: Hsla, text: &'static str) -> impl IntoElement {
    h_flex()
        .items_start()
        .gap_1p5()
        .child(
            Icon::new(IconName::TriangleAlert)
                .size(px(13.))
                .flex_none()
                .text_color(color),
        )
        .child(div().w(px(WARNING_MAX_WIDTH)).text_xs().text_color(color).child(text))
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
