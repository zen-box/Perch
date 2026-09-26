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
use crate::i18n::{Key, tr};

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
        "外观",
        p,
        vec![setting_row("主题", "选择浅色或深色界面", p, {
            let app = app.clone();
            segmented(
                "theme",
                vec![(false, "浅色".into()), (true, "深色".into())],
                state.is_dark,
                p,
                move |is_dark, window, cx| app.update(cx, |this, cx| this.set_dark_mode(is_dark, window, cx)),
            )
        })],
    );

    let conversation = section(
        "对话",
        p,
        vec![
            setting_row("新对话默认模型", "只影响之后创建的对话", p, {
                let (provider_id, model_id) = state.config.default_model_selection();
                let label = state
                    .config
                    .providers
                    .iter()
                    .find(|provider| provider.id == provider_id)
                    .and_then(|provider| provider.models.iter().find(|model| model.id == model_id))
                    .map(|model| model.name.clone())
                    .unwrap_or_else(|| "选择模型".to_string());
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
            }),
            setting_row(
                tr(lang, Key::Temperature),
                format!("当前 {:.1}，数值越低回答越稳定，越高越有创意", temperature),
                p,
                {
                    let app = app.clone();
                    segmented(
                        "temperature",
                        vec![(0.2, "精准".into()), (0.7, "平衡".into()), (1.0, "创意".into())],
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
                                .child("每次对话都会作为第一条 system 消息发送给模型"),
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
        "本地工具",
        p,
        vec![setting_row(
            "启用本地工具",
            "允许在对话中使用 /ls、/read、/git、/bash 指令",
            p,
            Switch::new("local-tools-enabled")
                .checked(state.config.local_tools_enabled)
                .on_click(cx.listener(|this, _, _, cx| this.toggle_local_tools(cx))),
        )],
    );

    page(
        "settings-general",
        tr(lang, Key::GeneralSettings),
        "外观与默认的对话参数",
        p,
        v_flex()
            .gap_8()
            .child(appearance)
            .child(conversation)
            .child(local_tools),
    )
}
