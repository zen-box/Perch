//! 设置页「提示词」：提示词预设与消息模板的增删改。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::Palette;
use super::settings::{page, section};
use crate::app::AppState;
use crate::i18n::{Key, tr};

pub(super) fn render_prompts(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let lang = state.language();
    let presets = state.prompts.presets.clone();
    let templates = state.prompts.templates.clone();
    let preset_rows = presets
        .iter()
        .enumerate()
        .map(|(ix, preset)| {
            let use_id = preset.id.clone();
            let delete_id = preset.id.clone();
            h_flex()
                .justify_between()
                .px_4()
                .py_2()
                .child(div().text_sm().child(format!("{} {}", preset.icon, preset.name)))
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new(("use-preset", ix))
                                .ghost()
                                .xsmall()
                                .label(tr(lang, Key::Use))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.create_session_from_preset(&use_id, window, cx)
                                })),
                        )
                        .child(
                            Button::new(("delete-preset", ix))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Trash)
                                .on_click(cx.listener(move |this, _, _, cx| this.delete_prompt(&delete_id, false, cx))),
                        ),
                )
                .into_any_element()
        })
        .collect::<Vec<_>>();
    let template_rows = templates
        .iter()
        .enumerate()
        .map(|(ix, template)| {
            let id = template.id.clone();
            h_flex()
                .justify_between()
                .px_4()
                .py_2()
                .child(div().text_sm().child(template.name.clone()))
                .child(
                    Button::new(("delete-template", ix))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Trash)
                        .on_click(cx.listener(move |this, _, _, cx| this.delete_prompt(&id, true, cx))),
                )
                .into_any_element()
        })
        .collect::<Vec<_>>();
    page(
        "settings-prompts",
        tr(lang, Key::PromptTemplates),
        tr(lang, Key::PromptsIntro),
        p,
        v_flex()
            .gap_8()
            .child(section(
                tr(lang, Key::AssistantPresets),
                p,
                if preset_rows.is_empty() {
                    vec![
                        div()
                            .px_4()
                            .py_3()
                            .text_sm()
                            .child(tr(lang, Key::NoPresetYet))
                            .into_any_element(),
                    ]
                } else {
                    preset_rows
                },
            ))
            .child(section(
                tr(lang, Key::PromptTemplateList),
                p,
                if template_rows.is_empty() {
                    vec![
                        div()
                            .px_4()
                            .py_3()
                            .text_sm()
                            .child(tr(lang, Key::NoTemplateYet))
                            .into_any_element(),
                    ]
                } else {
                    template_rows
                },
            ))
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(tr(lang, Key::NewItem)),
                    )
                    .child(Input::new(&state.prompt_name_input))
                    .child(Input::new(&state.prompt_icon_input))
                    .child(Textarea::new(&state.prompt_body_input))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("save-preset")
                                    .outline()
                                    .small()
                                    .label(tr(lang, Key::SaveAsPreset))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_prompt_from_inputs(false, window, cx);
                                    })),
                            )
                            .child(
                                Button::new("save-template")
                                    .primary()
                                    .small()
                                    .label(tr(lang, Key::SaveAsTemplate))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_prompt_from_inputs(true, window, cx);
                                    })),
                            ),
                    ),
            ),
    )
}
