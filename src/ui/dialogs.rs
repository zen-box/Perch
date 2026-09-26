use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputState, Textarea};
use gpui_kit::component::popover::{Popover, PopoverState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::brand_icon::{brand_avatar, capability_color, capability_icon, model_badges};
use super::{Palette, channel_icon, chip, icon_tile};
use crate::app::AppState;
use crate::brand;
use crate::config::{ChannelType, ModelConfig};
use crate::model::ReasoningLevel;
use crate::model_info::{Capability, format_tokens};
use crate::model_ops::TokenField;

/// 弹窗里的表单项：标签 + 输入框 + 可选说明
fn field(label: &'static str, hint: Option<&'static str>, input: impl IntoElement, p: &Palette) -> impl IntoElement {
    v_flex()
        .gap_1p5()
        .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label))
        .child(input)
        .children(hint.map(|hint| div().text_xs().text_color(p.muted_foreground).child(hint)))
}

/// 弹窗底部的「取消 / 确认」按钮
fn footer(ok_label: &'static str, on_ok: impl Fn(&mut Window, &mut App) + 'static) -> impl IntoElement {
    h_flex()
        .w_full()
        .justify_end()
        .gap_2()
        .child(
            Button::new("dialog-cancel")
                .outline()
                .label("取消")
                .on_click(|_, window, cx| window.close_dialog(cx)),
        )
        .child(
            Button::new("dialog-ok")
                .primary()
                .label(ok_label)
                .on_click(move |_, window, cx| on_ok(window, cx)),
        )
}

pub fn open_rename_dialog(app: Entity<AppState>, input: Entity<InputState>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = Palette::new(cx);
        let ok_app = app.clone();
        let enter_app = app.clone();
        dialog
            .title("重命名对话")
            .w(px(420.))
            .child(field("对话名称", None, Input::new(&input), &p))
            .footer(footer("保存", move |window, cx| {
                ok_app.update(cx, |this, cx| this.confirm_rename_session(window, cx));
                window.close_dialog(cx);
            }))
            .on_ok(move |_, window, cx| {
                enter_app.update(cx, |this, cx| this.confirm_rename_session(window, cx));
                true
            })
    });
}

pub fn open_add_provider_dialog(app: Entity<AppState>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = Palette::new(cx);
        let (current_ct, name_input, base_url_input, api_key_input) = {
            let state = app.read(cx);
            (
                state.add_channel_type,
                state.new_provider_name_input.clone(),
                state.new_provider_base_url_input.clone(),
                state.new_provider_api_key_input.clone(),
            )
        };
        let ok_app = app.clone();
        let enter_app = app.clone();

        dialog
            .title("添加模型渠道")
            .w(px(520.))
            .child(
                v_flex()
                    .gap_4()
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child("接口规范"))
                            .child(div().grid().grid_cols(2).gap_2().children(ChannelType::all().iter().map(|&ct| {
                                let is_active = ct == current_ct;
                                let select_app = app.clone();
                                h_flex()
                                    .id(SharedString::from(format!("channel-type-{:?}", ct)))
                                    .gap_2()
                                    .px_3()
                                    .py_2p5()
                                    .rounded_lg()
                                    .border_1()
                                    .cursor_pointer()
                                    .text_sm()
                                    .map(|this| {
                                        if is_active {
                                            this.border_color(p.primary)
                                                .bg(p.primary.opacity(0.08))
                                                .font_weight(FontWeight::MEDIUM)
                                        } else {
                                            this.border_color(p.border).hover(|s| s.bg(p.muted))
                                        }
                                    })
                                    .on_click(move |_, window, cx| {
                                        select_app.update(cx, |this, cx| this.select_add_channel_type(ct, window, cx));
                                    })
                                    .child(
                                        Icon::new(channel_icon(ct))
                                            .size(px(16.))
                                            .text_color(if is_active { p.primary } else { p.muted_foreground }),
                                    )
                                    .child(ct.label())
                            }))),
                    )
                    .child(field("渠道名称", None, Input::new(&name_input), &p))
                    .child(field(
                        "接口地址 (Base URL)",
                        Some("已按接口规范填入官方地址，使用代理或中转时请修改"),
                        Input::new(&base_url_input),
                        &p,
                    ))
                    .child(field("API 密钥", None, Input::new(&api_key_input).mask_toggle(), &p)),
            )
            .footer(footer("添加渠道", move |window, cx| {
                if ok_app.update(cx, |this, cx| this.confirm_add_provider(window, cx)) {
                    window.close_dialog(cx);
                }
            }))
            .on_ok(move |_, window, cx| enter_app.update(cx, |this, cx| this.confirm_add_provider(window, cx)))
    });
}

const CONTEXT_PRESETS: [(u32, &str); 5] =
    [(32_000, "32K"), (128_000, "128K"), (200_000, "200K"), (256_000, "256K"), (1_000_000, "1M")];
const OUTPUT_PRESETS: [(u32, &str); 6] =
    [(4_000, "4K"), (8_000, "8K"), (16_000, "16K"), (32_000, "32K"), (64_000, "64K"), (128_000, "128K")];

/// 添加 / 编辑模型。打开前先调用 `begin_add_model` 或 `begin_edit_model` 准备草稿。
pub fn open_model_editor(app: Entity<AppState>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let p = Palette::new(cx);
        let mono_font = cx.theme().mono_font_family.clone();
        let (editor, provider_name, id_input, name_input, context_input, output_input) = {
            let state = app.read(cx);
            let Some(editor) = state.model_editor.clone() else {
                return dialog.title("编辑模型");
            };
            let provider_name = state
                .config
                .providers
                .iter()
                .find(|provider| provider.id == editor.provider_id)
                .map(|provider| provider.name.clone())
                .unwrap_or_default();
            (
                editor,
                provider_name,
                state.model_edit_id_input.clone(),
                state.model_edit_name_input.clone(),
                state.model_edit_context_input.clone(),
                state.model_edit_output_input.clone(),
            )
        };
        let is_new = editor.is_new();
        let draft = editor.draft.clone();
        let detected = draft.detected();

        let header = h_flex()
            .gap_4()
            .items_start()
            .child(render_icon_picker(&app, &draft, &p))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_3()
                    .when(is_new, |this| {
                        this.child(field("模型 ID", Some("调用接口时使用的名字，填好后会自动识别下面的规格"), Input::new(&id_input), &p))
                    })
                    .child(field("显示名称", None, Input::new(&name_input), &p))
                    .when(!is_new, |this| {
                        this.child(
                            h_flex()
                                .gap_1p5()
                                .text_xs()
                                .text_color(p.muted_foreground)
                                .child("模型 ID")
                                .child(div().min_w_0().truncate().font_family(mono_font.clone()).child(draft.id.clone()))
                                .when(!provider_name.is_empty(), |this| this.child(format!("· {provider_name}"))),
                        )
                    }),
            );

        let specs = form_card(
            "规格",
            None,
            &p,
            vec![
                token_row(
                    &app,
                    TokenField::Context,
                    "上下文窗口",
                    "一次对话最多能带上多少 token",
                    draft.context_window,
                    detected.context_window,
                    &CONTEXT_PRESETS,
                    &context_input,
                    editor.context_error.clone(),
                    &p,
                ),
                token_row(
                    &app,
                    TokenField::Output,
                    "最大输出",
                    "单次回复的上限。Claude 必须指定，没设置对话参数时就用它",
                    draft.max_output,
                    detected.max_output,
                    &OUTPUT_PRESETS,
                    &output_input,
                    editor.output_error.clone(),
                    &p,
                ),
            ],
        );

        let levels = draft.effective_reasoning_levels();
        let thinking = form_card(
            "思考",
            Some(if draft.reasoning_levels.is_some() { "已手动设置" } else { "自动识别" }),
            &p,
            vec![
                v_flex()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .child(row_title("支持的强度", "对话参数里只会列出这里选中的档位，不支持调节就都不选", &p))
                    .child(h_flex().flex_wrap().gap_1p5().children(ReasoningLevel::ALL.into_iter().map(|level| {
                        let selected = levels.contains(&level);
                        let app = app.clone();
                        chip(SharedString::from(format!("model-level-{level:?}")), selected, &p)
                            .on_click(move |_, _, cx| app.update(cx, |this, cx| this.toggle_model_draft_level(level, cx)))
                            .when(selected, |this| this.child(Icon::new(IconName::Check).size(px(12.))))
                            .child(level.label())
                    })))
                    .when(levels.is_empty() && detected.always_thinks, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(p.muted_foreground)
                                .child("这个模型总会先思考再回答，但接口不支持调节强度"),
                        )
                    })
                    .into_any_element(),
                v_flex()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .child(row_title("默认强度", "对话没有单独设置时使用；「不指定」表示不发送，由接口决定", &p))
                    .child(if levels.is_empty() {
                        div()
                            .text_sm()
                            .text_color(p.muted_foreground)
                            .child("先在上面选择支持的强度")
                            .into_any_element()
                    } else {
                        h_flex()
                            .flex_wrap()
                            .gap_1p5()
                            .children(std::iter::once(None).chain(levels.iter().copied().map(Some)).map(|level| {
                                let app = app.clone();
                                chip(
                                    SharedString::from(format!("model-default-level-{level:?}")),
                                    draft.default_reasoning == level,
                                    &p,
                                )
                                .on_click(move |_, _, cx| {
                                    app.update(cx, |this, cx| this.update_model_draft(cx, |draft| draft.default_reasoning = level))
                                })
                                .child(level.map(ReasoningLevel::label).unwrap_or("不指定"))
                            }))
                            .into_any_element()
                    })
                    .into_any_element(),
            ],
        );

        let capabilities = draft.effective_capabilities();
        let abilities = form_card(
            "能力",
            Some(if draft.capabilities.is_some() { "已手动设置" } else { "自动识别" }),
            &p,
            vec![
                v_flex()
                    .gap_2()
                    .p_3()
                    .child(div().grid().grid_cols(3).gap_2().children(Capability::ALL.into_iter().map(|capability| {
                        let selected = capabilities.contains(&capability);
                        let app = app.clone();
                        let color = capability_color(capability);
                        v_flex()
                            .id(SharedString::from(format!("model-capability-{capability:?}")))
                            .gap_1()
                            .px_3()
                            .py_2p5()
                            .rounded_lg()
                            .border_1()
                            .cursor_pointer()
                            .map(|this| {
                                if selected {
                                    this.border_color(p.primary).bg(p.primary.opacity(0.06))
                                } else {
                                    this.border_color(p.border).hover(|style| style.bg(p.muted))
                                }
                            })
                            .on_click(move |_, _, cx| {
                                app.update(cx, |this, cx| this.toggle_model_draft_capability(capability, cx))
                            })
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Icon::new(capability_icon(capability))
                                            .size(px(16.))
                                            .text_color(if selected { color } else { p.muted_foreground }),
                                    )
                                    .child(div().flex_1().text_sm().font_weight(FontWeight::MEDIUM).child(capability.label()))
                                    .child(
                                        Icon::new(if selected { IconName::CircleCheck } else { IconName::Circle })
                                            .size(px(14.))
                                            .text_color(if selected { p.primary } else { p.border }),
                                    ),
                            )
                            .child(div().text_xs().text_color(p.muted_foreground).child(capability.description()))
                    })))
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.muted_foreground)
                            .child("目前只用于标注和筛选；图片、文档附件和联网搜索接入后会按这里判断模型能否使用"),
                    )
                    .into_any_element(),
            ],
        );

        let ok_app = app.clone();
        let enter_app = app.clone();
        let reset_app = app.clone();
        dialog
            .title(if is_new { "添加模型" } else { "编辑模型" })
            .w(px(620.))
            .margin_top(px(48.))
            .child(v_flex().gap_5().pb_1().child(header).child(specs).child(thinking).child(abilities))
            .footer(
                h_flex()
                    .w_full()
                    .justify_between()
                    .child(
                        Button::new("model-editor-reset")
                            .ghost()
                            .small()
                            .icon(IconName::RotateCcw)
                            .label("恢复自动识别")
                            .tooltip("清除手动设置的规格、能力、思考和图标")
                            .on_click(move |_, window, cx| reset_app.update(cx, |this, cx| this.reset_model_draft(window, cx))),
                    )
                    .child(footer(if is_new { "添加模型" } else { "保存" }, move |window, cx| {
                        if ok_app.update(cx, |this, cx| this.confirm_model_editor(cx)) {
                            window.close_dialog(cx);
                        }
                    })),
            )
            .on_ok(move |_, _, cx| enter_app.update(cx, |this, cx| this.confirm_model_editor(cx)))
    });
}

/// 弹窗里的分组卡片：标题 + 右侧状态说明 + 带分隔线的行
fn form_card(title: &'static str, status: Option<&'static str>, p: &Palette, rows: Vec<AnyElement>) -> impl IntoElement {
    v_flex()
        .gap_2()
        .child(
            h_flex()
                .justify_between()
                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(title))
                .children(status.map(|status| div().text_xs().text_color(p.muted_foreground).child(status))),
        )
        .child(
            v_flex()
                .rounded_lg()
                .border_1()
                .border_color(p.border)
                .children(rows.into_iter().enumerate().map(|(ix, row)| {
                    div().when(ix > 0, |this| this.border_t_1().border_color(p.border)).child(row)
                })),
        )
}

fn row_title(title: &'static str, description: &'static str, p: &Palette) -> impl IntoElement {
    v_flex()
        .gap_0p5()
        .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
        .child(div().text_xs().text_color(p.muted_foreground).child(description))
}

/// 上下文窗口 / 最大输出：自动 + 预设值 + 自定义输入
#[allow(clippy::too_many_arguments)]
fn token_row(
    app: &Entity<AppState>,
    field: TokenField,
    title: &'static str,
    description: &'static str,
    value: Option<u32>,
    detected: Option<u32>,
    presets: &[(u32, &'static str)],
    input: &Entity<InputState>,
    error: Option<String>,
    p: &Palette,
) -> AnyElement {
    let auto_label = format!("自动 · {}", detected.map(format_tokens).unwrap_or_else(|| "未知".into()));
    let auto_app = app.clone();
    v_flex()
        .gap_2()
        .px_4()
        .py_3()
        .child(row_title(title, description, p))
        .child(
            h_flex()
                .gap_1p5()
                .child(
                    chip(SharedString::from(format!("{field:?}-auto")), value.is_none(), p)
                        .on_click(move |_, window, cx| {
                            auto_app.update(cx, |this, cx| this.set_model_draft_tokens(field, None, window, cx))
                        })
                        .child(auto_label),
                )
                .children(presets.iter().map(|&(preset, label)| {
                    let app = app.clone();
                    chip(SharedString::from(format!("{field:?}-{preset}")), value == Some(preset), p)
                        .on_click(move |_, window, cx| {
                            app.update(cx, |this, cx| this.set_model_draft_tokens(field, Some(preset), window, cx))
                        })
                        .child(label)
                }))
                .child(div().flex_1().min_w(px(96.)).child(Input::new(input).small())),
        )
        .when_some(error, |this, error| this.child(div().text_xs().text_color(p.danger).child(error)))
        .into_any_element()
}

/// 头像和「更换图标」按钮：自动匹配不对时可以手动选择品牌图标
fn render_icon_picker(app: &Entity<AppState>, draft: &ModelConfig, p: &Palette) -> impl IntoElement {
    let label = if draft.name.trim().is_empty() { draft.id.clone() } else { draft.name.clone() };
    let app = app.clone();
    v_flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .pt_1()
        .child(if draft.id.trim().is_empty() && draft.icon.is_none() {
            // 还没填 ID 时显示中性的占位图标
            icon_tile(IconName::Box, px(52.), p.muted, p.muted_foreground).into_any_element()
        } else {
            brand_avatar(brand::brand_for_model(draft), &label, px(52.), p)
        })
        .child(
            Popover::new("model-icon-picker")
                .anchor(Anchor::TopLeft)
                .w(px(360.))
                .trigger(Button::new("model-icon-trigger").ghost().xsmall().label("更换图标"))
                .content(move |_, _, cx| render_icon_grid(&app, cx)),
        )
}

fn render_icon_grid(app: &Entity<AppState>, cx: &mut Context<PopoverState>) -> impl IntoElement + use<> {
    let p = Palette::new(cx);
    let (current, auto_brand) = {
        let state = app.read(cx);
        let draft = state.model_editor.as_ref().map(|editor| editor.draft.clone());
        (
            draft.as_ref().and_then(|draft| draft.icon.clone()),
            draft.as_ref().and_then(|draft| brand::brand_for_model_id(&draft.id)),
        )
    };
    let cell = |id: SharedString, selected: bool, title: &'static str, avatar: AnyElement| {
        v_flex()
            .id(id)
            .p_1()
            .rounded_md()
            .border_1()
            .cursor_pointer()
            .border_color(if selected { p.primary } else { transparent_black() })
            .hover(|style| style.bg(p.muted))
            .tooltip(move |window, cx| Tooltip::new(title).build(window, cx))
            .child(avatar)
    };
    let auto_app = app.clone();
    v_flex()
        .gap_2()
        .child(div().text_sm().font_weight(FontWeight::MEDIUM).child("选择图标"))
        .child(div().text_xs().text_color(p.muted_foreground).child("默认按模型 ID 自动匹配，匹配不对时可以手动指定"))
        .child(
            div()
                .max_h(px(300.))
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_1()
                        .child(
                            cell(
                                "icon-auto".into(),
                                current.is_none(),
                                "自动匹配",
                                brand_avatar(auto_brand, "?", px(30.), &p),
                            )
                            .on_click(cx.listener(move |popover, _, window, cx| {
                                auto_app.update(cx, |this, cx| this.update_model_draft(cx, |draft| draft.icon = None));
                                popover.dismiss(window, cx);
                            })),
                        )
                        .children(brand::BRANDS.iter().map(|brand| {
                            let app = app.clone();
                            let key = brand.key;
                            cell(
                                SharedString::from(format!("icon-{key}")),
                                current.as_deref() == Some(key),
                                brand.title,
                                brand_avatar(Some(brand), brand.title, px(30.), &p),
                            )
                            .on_click(cx.listener(move |popover, _, window, cx| {
                                app.update(cx, |this, cx| {
                                    this.update_model_draft(cx, |draft| draft.icon = Some(key.to_string()))
                                });
                                popover.dismiss(window, cx);
                            }))
                        }))
                        .overflow_y_scrollbar(),
                ),
        )
}

pub fn confirm_regenerate(
    app: Entity<AppState>,
    message_id: String,
    target: Option<(String, String)>,
    later_count: usize,
    window: &mut Window,
    cx: &mut App,
) {
    let description = format!("这条回答之后的 {later_count} 条消息会被删除，然后重新生成这条回答。");
    window.open_alert_dialog(cx, move |alert, _, _| {
        let app = app.clone();
        let message_id = message_id.clone();
        let target = target.clone();
        alert
            .title("重新生成这条回答？")
            .description(description.clone())
            .button_props(danger_props("重新生成"))
            .on_ok(move |_, _, cx| {
                let (provider_id, model_id) = target.clone().unzip();
                app.update(cx, |this, cx| this.regenerate_message(&message_id, provider_id, model_id, cx));
                true
            })
    });
}

fn danger_props(ok_text: &'static str) -> DialogButtonProps {
    DialogButtonProps::default()
        .ok_text(ok_text)
        .ok_variant(ButtonVariant::Danger)
        .cancel_text("取消")
        .show_cancel(true)
}

pub fn confirm_delete_session(
    app: Entity<AppState>,
    session_id: String,
    title: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let description = format!("「{}」中的全部消息都会被删除，且无法恢复。", title);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let app = app.clone();
        let session_id = session_id.clone();
        alert
            .title("删除这个对话？")
            .description(description.clone())
            .button_props(danger_props("删除"))
            .on_ok(move |_, _, cx| {
                app.update(cx, |this, cx| this.delete_session(session_id.clone(), cx));
                true
            })
    });
}

pub fn confirm_clear_session(app: Entity<AppState>, window: &mut Window, cx: &mut App) {
    window.open_alert_dialog(cx, move |alert, _, _| {
        let app = app.clone();
        alert
            .title("清空当前对话？")
            .description("对话中的全部消息都会被清空，且无法恢复。")
            .button_props(danger_props("清空"))
            .on_ok(move |_, _, cx| {
                app.update(cx, |this, cx| this.clear_current_session(cx));
                true
            })
    });
}

pub fn confirm_delete_provider(app: Entity<AppState>, provider_name: String, window: &mut Window, cx: &mut App) {
    let description = format!("「{}」及其下的全部模型配置都会被删除。", provider_name);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let app = app.clone();
        alert
            .title("删除这个渠道？")
            .description(description.clone())
            .button_props(danger_props("删除"))
            .on_ok(move |_, window, cx| {
                app.update(cx, |this, cx| this.delete_selected_provider(window, cx));
                true
            })
    });
}

 pub fn open_edit_message_dialog(app: Entity<AppState>, window: &mut Window, cx: &mut App) {
     window.open_dialog(cx, move |dialog, _, cx| {
         let p = Palette::new(cx);
         let input = app.read(cx).edit_message_input.clone();
         let ok_app = app.clone();
         let enter_app = app.clone();
         dialog
             .title("编辑并重新发送")
             .w(px(520.))
             .child(field(
                 "消息内容",
                 Some("保存后会删除这条消息之后的回复，并重新生成"),
                 Textarea::new(&input),
                 &p,
             ))
             .footer(footer("重新发送", move |window, cx| {
                 if ok_app.update(cx, |this, cx| this.confirm_edit_message(window, cx)) {
                     window.close_dialog(cx);
                 }
             }))
             .on_ok(move |_, window, cx| enter_app.update(cx, |this, cx| this.confirm_edit_message(window, cx)))
     });
 }

 pub fn open_folder_dialog(app: Entity<AppState>, window: &mut Window, cx: &mut App) {
     window.open_dialog(cx, move |dialog, _, cx| {
         let p = Palette::new(cx);
         let input = app.read(cx).folder_name_input.clone();
         let ok_app = app.clone();
         let enter_app = app.clone();
         dialog
             .title("移动到文件夹")
             .w(px(420.))
             .child(field("文件夹名称", Some("留空则移回「默认」"), Input::new(&input), &p))
             .footer(footer("移动", move |window, cx| {
                 if ok_app.update(cx, |this, cx| this.confirm_move_folder(window, cx)) {
                     window.close_dialog(cx);
                 }
             }))
             .on_ok(move |_, window, cx| enter_app.update(cx, |this, cx| this.confirm_move_folder(window, cx)))
     });
 }

 pub fn open_fetch_models_dialog(app: Entity<AppState>, window: &mut Window, cx: &mut App) {
     window.open_dialog(cx, move |dialog, _, cx| {
         let p = Palette::new(cx);
         let (models, selected, existing, query, search) = {
             let state = app.read(cx);
             let existing = state
                 .config
                 .providers
                 .iter()
                 .find(|provider| provider.id == state.selected_settings_provider_id)
                 .map(|provider| provider.models.iter().map(|model| model.id.clone()).collect::<Vec<_>>())
                 .unwrap_or_default();
             (
                 state.pending_models.clone(),
                 state.pending_model_selection.clone(),
                 existing,
                 state.model_fetch_query.to_lowercase(),
                 state.model_fetch_search.clone(),
             )
         };
         let available: Vec<_> = models
             .iter()
             .filter(|(id, _)| !existing.iter().any(|old| old == id))
             .filter(|(id, name)| query.is_empty() || id.to_lowercase().contains(&query) || name.to_lowercase().contains(&query))
             .cloned()
             .collect();
         let visible_ids: Vec<String> = available.iter().map(|(id, _)| id.clone()).collect();
         let selected_count = selected.len();
         let already = models.iter().filter(|(id, _)| existing.iter().any(|old| old == id)).count();
         let rows = available.iter().enumerate().map(|(ix, (id, name))| {
             let checked = selected.contains(id);
             let toggle_id = id.clone();
             let box_app = app.clone();
             h_flex()
                 .id(SharedString::from(format!("fetch-row-{ix}")))
                 .gap_2()
                 .px_2()
                 .py_1p5()
                 .rounded_md()
                 .items_center()
                 .child(
                     Checkbox::new(SharedString::from(format!("fetch-model-{ix}")))
                         .checked(checked)
                         .on_click({
                             let id = toggle_id;
                             move |checked, _, cx| {
                                 let checked = *checked;
                                 box_app.update(cx, |this, cx| this.set_pending_model(&id, checked, cx));
                             }
                         }),
                 )
                 .child(
                     v_flex()
                         .flex_1()
                         .min_w_0()
                         .child(div().truncate().text_sm().child(name.clone()))
                         .child(div().truncate().text_xs().text_color(p.muted_foreground).child(id.clone())),
                 )
                 .child(model_badges(&ModelConfig::new(id.clone(), name.clone()), &p))
                 .into_any_element()
         });
         let select_app = app.clone();
         let select_ids = visible_ids.clone();
         let clear_app = app.clone();
         let clear_ids = visible_ids;
         let ok_app = app.clone();
         let cancel_app = app.clone();
         let dismiss_app = app.clone();
         let enter_app = app.clone();
         dialog
             .title("选择要添加的模型")
             .w(px(560.))
             .child(
                 v_flex()
                     .gap_3()
                     .child(div().text_xs().text_color(p.muted_foreground).child(format!(
                         "可添加 {} 个，已存在 {already} 个。已选 {selected_count} 个",
                         available.len()
                     )))
                     .child(Input::new(&search))
                     .child(if available.is_empty() {
                         div().py_8().text_sm().text_center().text_color(p.muted_foreground).child("没有匹配的新模型").into_any_element()
                     } else {
                         div()
                             .max_h(px(360.))
                             .border_1()
                             .border_color(p.border)
                             .rounded_lg()
                             .child(v_flex().p_1().children(rows).overflow_y_scrollbar())
                             .into_any_element()
                     })
                     .child(
                         h_flex()
                             .gap_2()
                             .child(Button::new("select-visible-models").outline().xsmall().label("全选当前").on_click(move |_, _, cx| {
                                 select_app.update(cx, |this, cx| this.select_pending_models(&select_ids, true, cx));
                             }))
                             .child(Button::new("clear-visible-models").ghost().xsmall().label("清空当前").on_click(move |_, _, cx| {
                                 clear_app.update(cx, |this, cx| this.select_pending_models(&clear_ids, false, cx));
                             })),
                     ),
             )
             .footer(
                 h_flex()
                     .w_full()
                     .justify_end()
                     .gap_2()
                     .child(Button::new("fetch-cancel").outline().label("取消").on_click(move |_, window, cx| {
                         cancel_app.update(cx, |this, cx| this.cancel_pending_models(cx));
                         window.close_dialog(cx);
                     }))
                     .child(Button::new("fetch-ok").primary().label("添加所选").on_click(move |_, window, cx| {
                         if ok_app.update(cx, |this, cx| this.confirm_pending_models(cx)) {
                             window.close_dialog(cx);
                         }
                     })),
             )
             .on_ok(move |_, _, cx| enter_app.update(cx, |this, cx| this.confirm_pending_models(cx)))
             .on_cancel(move |_, _, cx| {
                 dismiss_app.update(cx, |this, cx| this.cancel_pending_models(cx));
                 true
             })
     });
 }
