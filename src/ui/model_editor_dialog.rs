//! 添加 / 编辑模型的弹窗。
//!
//! 它比 `dialogs.rs` 里那些"问一句就关"的弹窗重得多：一个表单卡片、一个图标选择浮层、
//! 加上规格/思考/能力三组控件，所以单独一个文件。`field` / `footer` 仍留在 `dialogs.rs`，
//! 那是所有弹窗共用的。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::popover::{Popover, PopoverState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::brand_icon::{brand_avatar, capability_color, capability_icon};
use super::dialogs::{field, footer};
use super::{Palette, chip, icon_tile};
use crate::app::AppState;
use crate::brand;
use crate::config::ModelConfig;
use crate::model::ReasoningLevel;
use crate::model_info::{Capability, format_tokens};
use crate::model_ops::TokenField;

const CONTEXT_PRESETS: [(u32, &str); 5] = [
    (32_000, "32K"),
    (128_000, "128K"),
    (200_000, "200K"),
    (256_000, "256K"),
    (1_000_000, "1M"),
];
const OUTPUT_PRESETS: [(u32, &str); 6] = [
    (4_000, "4K"),
    (8_000, "8K"),
    (16_000, "16K"),
    (32_000, "32K"),
    (64_000, "64K"),
    (128_000, "128K"),
];

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
                        this.child(field(
                            "模型 ID",
                            Some("调用接口时使用的名字，填好后会自动识别下面的规格"),
                            Input::new(&id_input),
                            &p,
                        ))
                    })
                    .child(field("显示名称", None, Input::new(&name_input), &p))
                    .when(!is_new, |this| {
                        this.child(
                            h_flex()
                                .gap_1p5()
                                .text_xs()
                                .text_color(p.muted_foreground)
                                .child("模型 ID")
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .font_family(mono_font.clone())
                                        .child(draft.id.clone()),
                                )
                                .when(!provider_name.is_empty(), |this| {
                                    this.child(format!("· {provider_name}"))
                                }),
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
            Some(if draft.reasoning_levels.is_some() {
                "已手动设置"
            } else {
                "自动识别"
            }),
            &p,
            vec![
                v_flex()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .child(row_title(
                        "支持的强度",
                        "对话参数里只会列出这里选中的档位，不支持调节就都不选",
                        &p,
                    ))
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_1p5()
                            .children(ReasoningLevel::ALL.into_iter().map(|level| {
                                let selected = levels.contains(&level);
                                let app = app.clone();
                                chip(SharedString::from(format!("model-level-{level:?}")), selected, &p)
                                    .on_click(move |_, _, cx| {
                                        app.update(cx, |this, cx| this.toggle_model_draft_level(level, cx))
                                    })
                                    .when(selected, |this| this.child(Icon::new(IconName::Check).size(px(12.))))
                                    .child(level.label())
                            })),
                    )
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
                    .child(row_title(
                        "默认强度",
                        "对话没有单独设置时使用；「不指定」表示不发送，由接口决定",
                        &p,
                    ))
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
                            .children(
                                std::iter::once(None)
                                    .chain(levels.iter().copied().map(Some))
                                    .map(|level| {
                                        let app = app.clone();
                                        chip(
                                            SharedString::from(format!("model-default-level-{level:?}")),
                                            draft.default_reasoning == level,
                                            &p,
                                        )
                                        .on_click(move |_, _, cx| {
                                            app.update(cx, |this, cx| {
                                                this.update_model_draft(cx, |draft| draft.default_reasoning = level)
                                            })
                                        })
                                        .child(level.map(ReasoningLevel::label).unwrap_or("不指定"))
                                    }),
                            )
                            .into_any_element()
                    })
                    .into_any_element(),
            ],
        );

        let capabilities = draft.effective_capabilities();
        let abilities = form_card(
            "能力",
            Some(if draft.capabilities.is_some() {
                "已手动设置"
            } else {
                "自动识别"
            }),
            &p,
            vec![
                v_flex()
                    .gap_2()
                    .p_3()
                    .child(
                        div()
                            .grid()
                            .grid_cols(3)
                            .gap_2()
                            .children(Capability::ALL.into_iter().map(|capability| {
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
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .text_sm()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child(capability.label()),
                                            )
                                            .child(
                                                Icon::new(if selected {
                                                    IconName::CircleCheck
                                                } else {
                                                    IconName::Circle
                                                })
                                                .size(px(14.))
                                                .text_color(if selected { p.primary } else { p.border }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(p.muted_foreground)
                                            .child(capability.description()),
                                    )
                            })),
                    )
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
            .child(
                v_flex()
                    .gap_5()
                    .pb_1()
                    .child(header)
                    .child(specs)
                    .child(thinking)
                    .child(abilities),
            )
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
                            .on_click(move |_, window, cx| {
                                reset_app.update(cx, |this, cx| this.reset_model_draft(window, cx))
                            }),
                    )
                    .child(footer(
                        if is_new { "添加模型" } else { "保存" },
                        move |window, cx| {
                            if ok_app.update(cx, |this, cx| this.confirm_model_editor(cx)) {
                                window.close_dialog(cx);
                            }
                        },
                    )),
            )
            .on_ok(move |_, _, cx| enter_app.update(cx, |this, cx| this.confirm_model_editor(cx)))
    });
}

/// 弹窗里的分组卡片：标题 + 右侧状态说明 + 带分隔线的行
fn form_card(
    title: &'static str,
    status: Option<&'static str>,
    p: &Palette,
    rows: Vec<AnyElement>,
) -> impl IntoElement {
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
                    div()
                        .when(ix > 0, |this| this.border_t_1().border_color(p.border))
                        .child(row)
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
    let auto_label = format!(
        "自动 · {}",
        detected.map(format_tokens).unwrap_or_else(|| "未知".into())
    );
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
                    chip(
                        SharedString::from(format!("{field:?}-{preset}")),
                        value == Some(preset),
                        p,
                    )
                    .on_click(move |_, window, cx| {
                        app.update(cx, |this, cx| {
                            this.set_model_draft_tokens(field, Some(preset), window, cx)
                        })
                    })
                    .child(label)
                }))
                .child(div().flex_1().min_w(px(96.)).child(Input::new(input).small())),
        )
        .when_some(error, |this, error| {
            this.child(div().text_xs().text_color(p.danger).child(error))
        })
        .into_any_element()
}

/// 头像和「更换图标」按钮：自动匹配不对时可以手动选择品牌图标
fn render_icon_picker(app: &Entity<AppState>, draft: &ModelConfig, p: &Palette) -> impl IntoElement {
    let label = if draft.name.trim().is_empty() {
        draft.id.clone()
    } else {
        draft.name.clone()
    };
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
        .child(
            div()
                .text_xs()
                .text_color(p.muted_foreground)
                .child("默认按模型 ID 自动匹配，匹配不对时可以手动指定"),
        )
        .child(
            div().max_h(px(300.)).child(
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
