//! 设置页的「关于」页面。（「MCP 服务器」页面已经独立成 `settings_mcp.rs`。）

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::settings::{page, section, setting_row};
use super::{Palette, icon_tile};
use crate::app::AppState;
use crate::i18n::{Key, tr, tr_args};
use crate::update_ops::UpdatePhase;

// ================= 关于 =================

pub(super) fn render_about(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let lang = state.language();
    let features = [
        (IconName::HardDrive, tr(lang, Key::FeatureLocalOnly)),
        (IconName::Layers, tr(lang, Key::FeatureFourApis)),
        (IconName::Zap, tr(lang, Key::FeatureStreaming)),
        (IconName::SquareTerminal, tr(lang, Key::FeatureLocalTools)),
        (IconName::Languages, tr(lang, Key::FeatureI18n)),
    ];
    let enabled = state.config.updates_enabled;
    let busy = state.updates.phase.busy();
    let actions = h_flex()
        .gap_2()
        .child(
            Button::new("check-updates")
                .outline()
                .small()
                .icon(IconName::RefreshCw)
                .label(tr(lang, Key::UpdateCheck))
                .disabled(!enabled || busy)
                .on_click(cx.listener(|this, _, _, cx| this.check_updates(cx))),
        )
        .when(
            state.updates.release.is_some()
                && matches!(&state.updates.phase, UpdatePhase::Available | UpdatePhase::Failed(_)),
            |this| {
                this.child(
                    Button::new("download-update")
                        .primary()
                        .small()
                        .icon(IconName::Download)
                        .label(tr(lang, Key::UpdateDownload))
                        .on_click(cx.listener(|this, _, _, cx| this.download_update(cx))),
                )
            },
        )
        .when(matches!(&state.updates.phase, UpdatePhase::Downloaded), |this| {
            this.child(
                Button::new("install-update")
                    .primary()
                    .small()
                    .icon(IconName::ArrowRight)
                    .label(tr(
                        lang,
                        if cfg!(target_os = "macos") {
                            Key::UpdateShowFile
                        } else {
                            Key::UpdateInstall
                        },
                    ))
                    .on_click(cx.listener(|this, _, window, cx| this.confirm_update_install(window, cx))),
            )
        });
    let updates = section(
        tr(lang, Key::UpdateSettings),
        p,
        vec![
            setting_row(
                tr(lang, Key::UpdateEnabled),
                tr(lang, Key::UpdateEnabledHint),
                p,
                Switch::new("updates-enabled")
                    .checked(enabled)
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_updates(cx))),
            ),
            setting_row(
                tr(lang, Key::UpdateSettings),
                state.updates.status_text(lang),
                p,
                actions,
            ),
        ],
    );

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
            ))
            .child(updates),
    )
}
