//! 设置页「提示词」：提示词预设与消息模板的增删改。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::Palette;
use super::settings::{page, section};
use crate::app::AppState;

pub(super) fn render_prompts(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
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
                        .child(Button::new(("use-preset", ix)).ghost().xsmall().label("使用").on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.create_session_from_preset(&use_id, window, cx)
                            }),
                        ))
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
        "提示词",
        "助手预设用于新建对话，模板可在输入框输入 /名称 后回车插入",
        p,
        v_flex()
            .gap_8()
            .child(section(
                "助手预设",
                p,
                if preset_rows.is_empty() {
                    vec![div().px_4().py_3().text_sm().child("还没有预设").into_any_element()]
                } else {
                    preset_rows
                },
            ))
            .child(section(
                "提示词模板",
                p,
                if template_rows.is_empty() {
                    vec![div().px_4().py_3().text_sm().child("还没有模板").into_any_element()]
                } else {
                    template_rows
                },
            ))
            .child(
                v_flex()
                    .gap_3()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("新建"))
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
                                    .label("保存为预设")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_prompt_from_inputs(false, window, cx);
                                    })),
                            )
                            .child(
                                Button::new("save-template")
                                    .primary()
                                    .small()
                                    .label("保存为模板")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_prompt_from_inputs(true, window, cx);
                                    })),
                            ),
                    ),
            ),
    )
}
