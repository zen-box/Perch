//! 设置页「通用」：语言、主题、温度、本地工具等外观与对话偏好。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::*;

use super::Palette;
use super::settings::{page, section, segmented, setting_row};
use crate::app::AppState;
use crate::i18n::{Key, tr, tr_args};

// ================= 通用设置 =================

pub(super) fn render_general(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let lang = state.language();
    let app = cx.entity();
    let temperature = state.config.temperature;
    // 预设档位之外的温度值不高亮任何档位
    let temp_preset = [0.2_f32, 0.7, 1.0]
        .into_iter()
        .find(|t| (temperature - t).abs() < 0.05)
        .unwrap_or(-1.0);

    let appearance = section(
        tr(lang, Key::Appearance),
        p,
        vec![setting_row(tr(lang, Key::Theme), tr(lang, Key::ThemeHint), p, {
            let app = app.clone();
            segmented(
                "theme",
                vec![(false, tr(lang, Key::Light).into()), (true, tr(lang, Key::Dark).into())],
                state.is_dark,
                p,
                move |is_dark, window, cx| app.update(cx, |this, cx| this.set_dark_mode(is_dark, window, cx)),
            )
        })],
    );

    let conversation = section(
        tr(lang, Key::Chat),
        p,
        vec![
            setting_row(
                tr(lang, Key::DefaultModelForNewChat),
                tr(lang, Key::DefaultModelHint),
                p,
                {
                    let (provider_id, model_id) = state.config.default_model_selection();
                    let label = state
                        .config
                        .providers
                        .iter()
                        .find(|provider| provider.id == provider_id)
                        .and_then(|provider| provider.models.iter().find(|model| model.id == model_id))
                        .map(|model| model.name.clone())
                        .unwrap_or_else(|| tr(lang, Key::SelectModel).to_string());
                    let providers = state.config.providers.clone();
                    let app = app.clone();
                    Button::new("default-model")
                        .outline()
                        .small()
                        .label(label)
                        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                            providers
                                .iter()
                                .filter(|provider| provider.enabled)
                                .fold(menu, |menu, provider| {
                                    provider
                                        .models
                                        .iter()
                                        .filter(|model| model.enabled)
                                        .fold(menu, |menu, model| {
                                            let app = app.clone();
                                            let pid = provider.id.clone();
                                            let mid = model.id.clone();
                                            menu.item(
                                                PopupMenuItem::new(format!("{} / {}", provider.name, model.name))
                                                    .checked(pid == provider_id && mid == model_id)
                                                    .on_click(move |_, _, cx| {
                                                        app.update(cx, |this, cx| {
                                                            this.set_default_model(&pid, &mid, cx);
                                                        });
                                                    }),
                                            )
                                        })
                                })
                        })
                },
            ),
            setting_row(
                tr(lang, Key::Temperature),
                tr_args(lang, Key::TemperatureHint, &[&format!("{temperature:.1}")]),
                p,
                {
                    let app = app.clone();
                    segmented(
                        "temperature",
                        vec![
                            (0.2, tr(lang, Key::Precise).into()),
                            (0.7, tr(lang, Key::Balanced).into()),
                            (1.0, tr(lang, Key::Creative).into()),
                        ],
                        temp_preset,
                        p,
                        move |t, _, cx| app.update(cx, |this, cx| this.set_temperature(t, cx)),
                    )
                },
            ),
            v_flex()
                .gap_3()
                .px_4()
                .py_3p5()
                .child(
                    v_flex()
                        .gap_0p5()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(tr(lang, Key::SystemPrompt)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(p.muted_foreground)
                                .child(tr(lang, Key::SystemPromptHint)),
                        ),
                )
                .child(Textarea::new(&state.cfg_system_prompt_input))
                .child(
                    h_flex().justify_end().child(
                        Button::new("save-system-prompt")
                            .primary()
                            .small()
                            .label(tr(lang, Key::Save))
                            .on_click(cx.listener(|this, _, _, cx| this.save_system_prompt(cx))),
                    ),
                )
                .into_any_element(),
        ],
    );

    let local_tools = section(
        tr(lang, Key::LocalTools),
        p,
        vec![
            setting_row(
                tr(lang, Key::EnableLocalTools),
                tr(lang, Key::LocalToolsHint),
                p,
                Switch::new("local-tools-enabled")
                    .checked(state.config.local_tools_enabled)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_local_tools(cx))),
            ),
            setting_row(tr(lang, Key::ToolTimeout), tr(lang, Key::ToolTimeoutHint), p, {
                let app = app.clone();
                segmented(
                    "command-timeout",
                    // 档位之外的值（手改过配置）不高亮任何一档，和温度一致
                    vec![
                        (60_u64, tr_args(lang, Key::ToolTimeoutMinutes, &["1"]).into()),
                        (300, tr_args(lang, Key::ToolTimeoutMinutes, &["5"]).into()),
                        (600, tr_args(lang, Key::ToolTimeoutMinutes, &["10"]).into()),
                        (1800, tr_args(lang, Key::ToolTimeoutMinutes, &["30"]).into()),
                    ],
                    state.config.command_timeout_secs,
                    p,
                    move |secs, _, cx| app.update(cx, |this, cx| this.set_command_timeout(secs, cx)),
                )
            }),
        ],
    );

    let audit = section(
        tr(lang, Key::AuditLog),
        p,
        vec![
            setting_row(
                tr(lang, Key::EnableAuditLog),
                tr(lang, Key::AuditLogHint),
                p,
                Switch::new("audit-log-enabled")
                    .checked(state.config.audit_log_enabled)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_audit_log(cx))),
            ),
            setting_row(
                tr(lang, Key::AuditLogFolder),
                tr(lang, Key::AuditLogFolderHint),
                p,
                Button::new("open-audit-log-dir")
                    .outline()
                    .small()
                    .label(tr(lang, Key::AuditLogOpenFolder))
                    .on_click(cx.listener(|this, _, _, cx| this.open_audit_log_dir(cx))),
            ),
        ],
    );

    page(
        "settings-general",
        tr(lang, Key::GeneralSettings),
        tr(lang, Key::GeneralSectionDesc),
        p,
        v_flex()
            .gap_8()
            .child(appearance)
            .child(conversation)
            .child(local_tools)
            .child(audit),
    )
}
