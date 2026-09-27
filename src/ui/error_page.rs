//! 启动失败时显示的错误页。
//!
//! 它**不是 `AppState` 的视图**——会走到这里，正是因为 `AppState` 构造不出来
//! （配置或会话库读不出来）。所以它只依赖 `cx`，由 `main.rs` 直接挂到 `Root` 底下。
//!
//! 这一页要回答用户的三个问题：出了什么事、数据在哪、我还能做什么。所以除了原因，
//! 还要把数据目录路径原样写出来（用户可以自己去看），并给一个「打开数据目录」按钮。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::{Palette, icon_tile};
use crate::app::StartupFailure;
use crate::i18n::{Key, current, tr, tr_args};
use crate::paths::{MigrationFailure, data_dir, take_migration_failures};

/// 启动失败页：标题栏 + 一张卡片（原因、数据目录、两个按钮）。
pub struct ErrorPage {
    failure: StartupFailure,
    /// 顺带把迁移失败的记录也列出来——它们同样是「读不到数据」，分两处显示会让人
    /// 以为只有一个问题。
    migrations: Vec<MigrationFailure>,
    /// 「打开数据目录」失败时的原因。按钮点了没反应是最糟的反馈，所以留在页面上。
    reveal_error: Option<String>,
}

impl ErrorPage {
    pub fn new(failure: StartupFailure) -> Self {
        Self {
            failure,
            migrations: take_migration_failures(),
            reveal_error: None,
        }
    }
}

impl Render for ErrorPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lang = current(cx);
        let p = Palette::new(cx);
        let mono_font = cx.theme().mono_font_family.clone();
        let dir = data_dir().display().to_string();
        let reveal_error = self.reveal_error.clone();

        // 主原因排在最前，迁移失败跟在后面——都当成「一行原因」渲染，用户不用分辨来源
        let details: Vec<String> = std::iter::once(self.failure.message(lang))
            .chain(self.migrations.iter().map(|failure| failure.message(lang)))
            .collect();

        v_flex()
            .size_full()
            .bg(p.background)
            .text_color(p.foreground)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .gap_2()
                        .child(icon_tile(IconName::Sparkles, px(18.), p.primary, p.primary_foreground))
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Perch")),
                ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .items_center()
                    .justify_center()
                    .gap_5()
                    .p_6()
                    .child(icon_tile(
                        IconName::TriangleAlert,
                        px(48.),
                        p.danger.opacity(0.12),
                        p.danger,
                    ))
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(tr(lang, Key::StartupFailedTitle)),
                    )
                    .child(
                        div()
                            .max_w(px(560.))
                            .text_center()
                            .text_sm()
                            .text_color(p.muted_foreground)
                            .child(tr(lang, Key::StartupFailedHint)),
                    )
                    .child(
                        v_flex()
                            .id("startup-errors")
                            .w_full()
                            .max_w(px(560.))
                            .max_h(px(220.))
                            .overflow_y_scroll()
                            .gap_2()
                            .p_3()
                            .rounded_lg()
                            .border_1()
                            .border_color(p.border)
                            .bg(p.muted)
                            .children(details.into_iter().map(|detail| {
                                div()
                                    .text_xs()
                                    .font_family(mono_font.clone())
                                    .text_color(p.foreground)
                                    .child(detail)
                            })),
                    )
                    .child(
                        div()
                            .max_w(px(560.))
                            .text_xs()
                            .text_color(p.muted_foreground)
                            .child(tr_args(lang, Key::StartupDataDir, &[&dir])),
                    )
                    .when(reveal_error.is_some(), |this| {
                        let error = reveal_error.unwrap_or_default();
                        this.child(div().max_w(px(560.)).text_xs().text_color(p.danger).child(tr_args(
                            lang,
                            Key::StartupOpenDataDirFailed,
                            &[&error],
                        )))
                    })
                    .child(
                        h_flex()
                            .gap_2()
                            .when(cfg!(target_os = "windows"), |this| {
                                this.child(
                                    Button::new("startup-open-data-dir")
                                        .outline()
                                        .icon(IconName::FolderOpen)
                                        .label(tr(lang, Key::StartupOpenDataDir))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Err(error) = crate::paths::reveal(data_dir()) {
                                                this.reveal_error = Some(error.to_string());
                                                cx.notify();
                                            }
                                        })),
                                )
                            })
                            .child(
                                Button::new("startup-quit")
                                    .primary()
                                    .icon(IconName::Power)
                                    .label(tr(lang, Key::StartupQuit))
                                    .on_click(|_, _, cx| cx.quit()),
                            ),
                    ),
            )
    }
}
