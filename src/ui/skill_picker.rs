//! composer 工具栏上的「技能」入口：这次对话用哪些技能，逐个勾。
//!
//! 和 MCP 分开放：技能是一段一段的做法说明，用户要的是「这次用哪几个」，
//! 一整条「技能」开关管不到这个粒度。勾上的技能才会出现在给模型的技能清单里，
//! 用不用、什么时候用，由模型按问题自己判断。
//!
//! 状态只有一份，在 `ChatSession::tools.skills`；这里只负责渲染和转发点击，读写都在 `tool_ops.rs`。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::popover::{Popover, PopoverState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::Palette;
use super::chat::preview;
use super::tool_picker::{checkbox, hint};
use crate::app::{AppState, SettingsTab};
use crate::i18n::{Key, tr};

/// 「技能」按钮和它的弹层。
pub(super) fn render_skill_picker(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let lang = state.language();
    let app = cx.entity();
    let open_app = app.clone();
    let count = state.picked_skill_count();
    Popover::new("session-skills")
        .anchor(Anchor::BottomRight)
        .trigger(
            Button::new("session-skills-trigger")
                .ghost()
                .xsmall()
                .icon(IconName::Sparkles)
                .child(tr(lang, Key::SkillsButton))
                // 数字一直在，包括 0：和旁边的工具角标一个道理，0 也是「这次一个都不用」的提示
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if count > 0 { p.primary } else { p.muted_foreground })
                        .child(count.to_string()),
                )
                .tooltip(tr(lang, Key::SessionSkills)),
        )
        // 打开时先重扫一遍技能目录：用户手工往里拷的技能，程序自己不会知道
        // （没有监听文件变化），不扫的话新装的技能根本不会出现在列表里
        .on_open_change(move |open: &bool, _, cx: &mut App| {
            if *open {
                open_app.update(cx, |this, cx| this.reload_skills(cx));
            }
        })
        .content(move |_, _, cx| render_skill_panel(&app, cx))
}

/// 列表里的一行要显示的东西。只抄这几样，不整个克隆技能：正文可能有几 KB，每次渲染都复制没必要。
struct SkillRow {
    id: String,
    title: String,
    description: String,
}

fn render_skill_panel(app: &Entity<AppState>, cx: &mut Context<PopoverState>) -> impl IntoElement + use<> {
    let p = Palette::new(cx);
    let (lang, rows, any_installed, model_supports) = {
        let state = app.read(cx);
        (
            state.language(),
            // 只列没被全局停用的：设置里关掉的技能这次也用不了，列出来只会让人以为勾了就行
            state
                .skills
                .enabled(&state.config.disabled_skills)
                .map(|skill| SkillRow {
                    id: skill.id.clone(),
                    title: skill.title.clone(),
                    description: skill.description.clone(),
                })
                .collect::<Vec<_>>(),
            !state.skills.all().is_empty(),
            state.tools_supported_by_model(),
        )
    };
    let has_rows = !rows.is_empty();
    let all_app = app.clone();
    let none_app = app.clone();
    let manage_app = app.clone();

    let rows: Vec<AnyElement> = rows
        .into_iter()
        .map(|row| {
            let checked = app.read(cx).is_skill_picked(&row.id);
            let row_app = app.clone();
            let id = row.id.clone();
            h_flex()
                .id(SharedString::from(format!("skill-pick-{}", row.id)))
                .items_center()
                .gap_2()
                .px_2()
                .py_1p5()
                .rounded_md()
                .cursor_pointer()
                .when(checked, |this| this.bg(p.accent))
                .hover(|this| this.bg(p.accent))
                .on_click(cx.listener(move |_, _, _, cx| {
                    let id = id.clone();
                    row_app.update(cx, |this, cx| this.toggle_session_skill(&id, cx));
                }))
                .child(checkbox(checked, &p))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .child(
                                    div()
                                        .truncate()
                                        .text_sm()
                                        .text_color(p.foreground)
                                        .child(row.title.clone()),
                                )
                                // 显示名和目录名不一样时把目录名也带上：模型认的是目录名
                                .when(row.title != row.id, |this| {
                                    this.child(div().text_xs().text_color(p.muted_foreground).child(row.id))
                                }),
                        )
                        .when(!row.description.is_empty(), |this| {
                            this.child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(p.muted_foreground)
                                    // 描述是用户手写的，长度不受控制；弹层宽度又由最宽的一行决定
                                    // （见 `tool_picker.rs` 里 `render_tool_panel` 末尾的说明），先按字符砍一刀
                                    .child(preview(&row.description, 30)),
                            )
                        }),
                )
                .into_any_element()
        })
        .collect();

    v_flex()
        .gap_2()
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .gap_4()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(tr(lang, Key::SessionSkills)),
                )
                .when(has_rows, |this| {
                    this.child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("skills-select-all")
                                    .ghost()
                                    .xsmall()
                                    .label(tr(lang, Key::SelectAllTools))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        all_app.update(cx, |this, cx| this.set_all_session_skills(true, cx));
                                    })),
                            )
                            .child(
                                Button::new("skills-select-none")
                                    .ghost()
                                    .xsmall()
                                    .label(tr(lang, Key::ClearAllTools))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        none_app.update(cx, |this, cx| this.set_all_session_skills(false, cx));
                                    })),
                            ),
                    )
                }),
        )
        // 技能是靠 `load_skill` 这个工具读的，模型不支持工具调用就一个都用不上
        .when(!model_supports, |this| {
            this.child(hint(p.warning, tr(lang, Key::SessionToolsModelHint)))
        })
        .map(|this| {
            if has_rows {
                this.child(hint(p.muted_foreground, tr(lang, Key::SessionSkillsHint)))
                    .child(
                        div()
                            .max_h(px(320.))
                            .child(v_flex().gap_1().children(rows).overflow_y_scrollbar()),
                    )
            } else {
                // 空的时候告诉用户下一步去哪：没装就去装，全停用了就去打开
                this.child(
                    v_flex()
                        .items_start()
                        .gap_2()
                        .child(div().text_xs().text_color(p.muted_foreground).child(if any_installed {
                            tr(lang, Key::SkillsAllDisabled)
                        } else {
                            tr(lang, Key::SkillNoneYet)
                        }))
                        .child(
                            Button::new("skills-manage")
                                .outline()
                                .xsmall()
                                .icon(IconName::Settings)
                                .label(tr(lang, Key::SkillManage))
                                .on_click(cx.listener(move |popover, _, window, cx| {
                                    manage_app
                                        .update(cx, |this, cx| this.open_settings(SettingsTab::Skills, window, cx));
                                    popover.dismiss(window, cx);
                                })),
                        ),
                )
            }
        })
}
