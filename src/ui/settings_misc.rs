//! 设置页的「MCP 服务器」与「关于」两个页面。

use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::settings::{page, section};
use super::{Palette, icon_tile};
use crate::i18n::{AppLanguage, Key, tr, tr_args};

// ================= MCP / 关于 =================

pub(super) fn render_mcp(p: &Palette, lang: AppLanguage) -> impl IntoElement {
    page(
        "settings-mcp",
        tr(lang, Key::McpSettings),
        tr(lang, Key::McpSettingsDesc),
        p,
        v_flex()
            .items_center()
            .gap_3()
            .py_12()
            .rounded_lg()
            .border_1()
            .border_dashed()
            .border_color(p.border)
            .child(icon_tile(IconName::Plug, px(44.), p.muted, p.muted_foreground))
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(tr(lang, Key::ComingSoon)),
            )
            .child(
                div()
                    .max_w(px(420.))
                    .text_center()
                    .text_sm()
                    .text_color(p.muted_foreground)
                    .child(tr(lang, Key::McpComingSoon)),
            ),
    )
}

pub(super) fn render_about(p: &Palette, lang: AppLanguage) -> impl IntoElement {
    let features = [
        (IconName::HardDrive, tr(lang, Key::FeatureLocalOnly)),
        (IconName::Layers, tr(lang, Key::FeatureFourApis)),
        (IconName::Zap, tr(lang, Key::FeatureStreaming)),
        (IconName::SquareTerminal, tr(lang, Key::FeatureLocalTools)),
        (IconName::Languages, tr(lang, Key::FeatureI18n)),
    ];

    page(
        "settings-about",
        tr(lang, Key::AboutSettings),
        tr(lang, Key::AboutTagline),
        p,
        v_flex()
            .gap_6()
            .child(
                h_flex()
                    .gap_4()
                    .child(icon_tile(IconName::Sparkles, px(56.), p.primary, p.primary_foreground))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("Perch"))
                            .child(div().text_sm().text_color(p.muted_foreground).child(tr_args(
                                lang,
                                Key::Version,
                                &[env!("CARGO_PKG_VERSION")],
                            ))),
                    ),
            )
            .child(section(
                tr(lang, Key::Features),
                p,
                features
                    .into_iter()
                    .map(|(icon, text)| {
                        h_flex()
                            .gap_3()
                            .px_4()
                            .py_3()
                            .text_sm()
                            .child(Icon::new(icon).size(px(16.)).text_color(p.muted_foreground))
                            .child(text)
                            .into_any_element()
                    })
                    .collect(),
            )),
    )
}
