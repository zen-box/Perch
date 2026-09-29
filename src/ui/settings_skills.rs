//! 设置页「技能」：看装了哪些、逐个开关、导入、打开目录。
//!
//! 技能目录是 `%APPDATA%\Perch\skills\`，一个子目录一个技能。**这个页面不是唯一的入口**：
//! 用户完全可以自己把目录丢进去——切到本页、打开输入框上的「技能」时都会先重扫一遍
//! （`skill_ops::reload_skills`），不用知道有「重新扫描」这个按钮；那个按钮只留给
//! 本页开着时往目录里丢东西的情况。所以这里只做「看得见 + 能关掉」，
//! 不搞必须经过本页才能装的东西。
//!
//! 技能的正文不在这里展示：它可能很长，而且改它是编辑文件的事——想改就点「打开目录」。
//! 在设置页里做一个只读的正文预览，既不能改又要占掉半屏。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::Palette;
use super::settings::{page, section};
use crate::app::AppState;
use crate::i18n::{Key, tr, tr_args};

pub(super) fn render_skills(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let lang = state.language();
    let skills = state.skills.all().to_vec();
    let disabled = state.config.disabled_skills.clone();

    let rows: Vec<AnyElement> = if skills.is_empty() {
        vec![
            div()
                .px_4()
                .py_3()
                .text_sm()
                .text_color(p.muted_foreground)
                .child(tr(lang, Key::SkillNoneYet))
                .into_any_element(),
        ]
    } else {
        skills
            .iter()
            .enumerate()
            .map(|(ix, skill)| {
                let enabled = !disabled.iter().any(|id| id == &skill.id);
                let id = skill.id.clone();
                let file_count = skill.files.len();
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap_4()
                    .px_4()
                    .py_3p5()
                    .child(
                        v_flex()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    // 标题是 front matter 里的 `name`，缺了就退回目录名，
                                    // 所以这里可能和 id 一模一样——那就别再重复显示一遍
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(skill.title.clone()),
                                    )
                                    .when(skill.title != skill.id, |this| {
                                        this.child(
                                            div().text_xs().text_color(p.muted_foreground).child(skill.id.clone()),
                                        )
                                    })
                                    .when(file_count > 0, |this| {
                                        this.child(div().text_xs().text_color(p.muted_foreground).child(tr_args(
                                            lang,
                                            Key::SkillFileCount,
                                            &[&file_count.to_string()],
                                        )))
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.muted_foreground)
                                    .child(skill.description.clone()),
                            ),
                    )
                    .child(
                        Switch::new(("skill-toggle", ix))
                            .checked(enabled)
                            .tooltip(tr(lang, Key::SkillEnableHint))
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle_skill(&id, cx))),
                    )
                    .into_any_element()
            })
            .collect()
    };

    let buttons = h_flex()
        .gap_2()
        .child(
            Button::new("skill-import")
                .primary()
                .small()
                .icon(IconName::FolderOpen)
                .label(tr(lang, Key::SkillImport))
                .on_click(cx.listener(|this, _, _, cx| this.import_skill(cx))),
        )
        .child(
            Button::new("skill-reload")
                .outline()
                .small()
                .icon(IconName::RefreshCw)
                .label(tr(lang, Key::SkillReload))
                .on_click(cx.listener(|this, _, _, cx| this.reload_skills(cx))),
        )
        .child(
            Button::new("skill-open-folder")
                .ghost()
                .small()
                .icon(IconName::FolderOpen)
                .label(tr(lang, Key::SkillOpenFolder))
                .on_click(cx.listener(|this, _, _, cx| this.open_skills_dir(cx))),
        );

    page(
        "settings-skills",
        tr(lang, Key::SkillsSettings),
        tr(lang, Key::SkillsIntro),
        p,
        v_flex()
            .gap_6()
            .child(section(tr(lang, Key::SkillListTitle), p, rows))
            .child(buttons),
    )
}
