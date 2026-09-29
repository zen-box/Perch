//! 设置页「模型渠道」：左侧渠道列表 + 右侧详情（模型增删改、拉取、测试连接）。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, Icon, Selectable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::brand_icon::{model_avatar, model_badges, provider_avatar};
use super::settings::{PAGE_MAX_WIDTH, section};
use super::{Palette, dialogs, icon_tile, model_editor_dialog};
use crate::app::AppState;
use crate::i18n::{Key, tr, tr_args, tr_count};

// ================= 模型渠道 =================

pub(super) fn render_providers(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let lang = state.language();
    let query = state.cfg_search_provider_input.read(cx).value().to_lowercase();

    let list = v_flex()
        .w(px(248.))
        .h_full()
        .flex_none()
        .border_r_1()
        .border_color(p.border)
        .child(
            v_flex()
                .gap_2()
                .p_3()
                .child(
                    h_flex()
                        .justify_between()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(tr(lang, Key::ProviderSettings)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(p.muted_foreground)
                                        .child(state.config.providers.len().to_string()),
                                ),
                        )
                        .child(
                            Button::new("add-provider")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Plus)
                                .tooltip(tr(lang, Key::AddChannel))
                                .on_click(cx.listener(|_, _, window, cx| {
                                    dialogs::open_add_provider_dialog(cx.entity(), window, cx);
                                })),
                        ),
                )
                .child(
                    Input::new(&state.cfg_search_provider_input)
                        .small()
                        .cleanable(true)
                        .prefix(Icon::new(IconName::Search).small().text_color(p.muted_foreground)),
                ),
        )
        .child(
            v_flex()
                .flex_1()
                .px_2()
                .pb_3()
                .gap_px()
                .children(
                    state
                        .config
                        .providers
                        .iter()
                        .filter(|pr| query.is_empty() || pr.name.to_lowercase().contains(&query))
                        .map(|provider| {
                            let is_selected = provider.id == state.selected_settings_provider_id;
                            let pid = provider.id.clone();
                            h_flex()
                                .id(SharedString::from(format!("provider-{}", provider.id)))
                                .gap_2p5()
                                .px_2()
                                .py_2()
                                .rounded_md()
                                .cursor_pointer()
                                .map(|this| {
                                    if is_selected {
                                        this.bg(p.accent)
                                    } else {
                                        this.hover(|s| s.bg(p.accent.opacity(0.6)))
                                    }
                                })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.select_settings_provider(&pid, window, cx);
                                }))
                                .child(provider_avatar(provider, px(28.), p))
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .truncate()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(provider.name.clone()),
                                        )
                                        .child(div().truncate().text_xs().text_color(p.muted_foreground).child(
                                            tr_args(
                                                lang,
                                                tr_count(
                                                    lang,
                                                    provider.models.len(),
                                                    Key::ModelCountOne,
                                                    Key::ModelCountMany,
                                                ),
                                                &[&provider.models.len().to_string()],
                                            ),
                                        )),
                                )
                                .child(div().flex_none().size(px(8.)).rounded_full().bg(if provider.enabled {
                                    p.success
                                } else {
                                    p.border
                                }))
                        }),
                )
                .overflow_y_scrollbar(),
        );

    div()
        .flex()
        .flex_1()
        .min_w_0()
        .h_full()
        .child(list)
        .child(render_provider_detail(state, p, cx))
}

pub(super) fn render_provider_detail(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let lang = state.language();
    let Some(provider) = state
        .config
        .providers
        .iter()
        .find(|pr| pr.id == state.selected_settings_provider_id)
        .cloned()
    else {
        return div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .child(icon_tile(IconName::Cloud, px(44.), p.muted, p.muted_foreground))
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if state.config.providers.is_empty() {
                        tr(lang, Key::NoProviderYet)
                    } else {
                        tr(lang, Key::PickProviderHint)
                    }),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(p.muted_foreground)
                    .child(tr(lang, Key::ProviderIntro)),
            )
            .child(
                Button::new("empty-add-provider")
                    .primary()
                    .icon(IconName::Plus)
                    .label(tr(lang, Key::AddChannel))
                    .on_click(cx.listener(|_, _, window, cx| {
                        dialogs::open_add_provider_dialog(cx.entity(), window, cx);
                    })),
            )
            .into_any_element();
    };

    let mono_font = cx.theme().mono_font_family.clone();
    let toggle_pid = provider.id.clone();
    let delete_name = provider.name.clone();

    let header = h_flex()
        .gap_3()
        .child(provider_avatar(&provider, px(44.), p))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .truncate()
                        .text_xl()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(provider.name.clone()),
                )
                .child(h_flex().child(Tag::secondary().small().child(provider.channel_type.label()))),
        )
        .child(
            h_flex()
                .flex_none()
                .gap_3()
                .child(
                    Switch::new("provider-enabled")
                        .checked(provider.enabled)
                        .label(if provider.enabled {
                            tr(lang, Key::Enabled)
                        } else {
                            tr(lang, Key::Disabled)
                        })
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_provider_enabled(&toggle_pid, cx))),
                )
                .child(
                    Button::new("delete-provider")
                        .ghost()
                        .small()
                        .icon(IconName::Trash)
                        .tooltip(tr(lang, Key::DeleteProvider))
                        .on_click(cx.listener(move |_, _, window, cx| {
                            dialogs::confirm_delete_provider(cx.entity(), delete_name.clone(), window, cx);
                        })),
                ),
        );

    let connection = section(
        tr(lang, Key::ConnectionConfig),
        p,
        vec![
            v_flex()
                .gap_4()
                .px_4()
                .py_4()
                .child(
                    v_flex()
                        .gap_1p5()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(tr(lang, Key::ApiKey)),
                        )
                        .child(
                            div()
                                .py(px(4.))
                                .child(Input::new(&state.cfg_api_key_input).mask_toggle()),
                        ),
                )
                .child(
                    v_flex()
                        .gap_1p5()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(tr(lang, Key::BaseUrl)),
                        )
                        .child(div().py(px(4.)).child(Input::new(&state.cfg_base_url_input)))
                        .child(
                            div()
                                .text_xs()
                                .text_color(p.muted_foreground)
                                .child(tr(lang, Key::BaseUrlHint)),
                        ),
                )
                .child(
                    v_flex()
                        .gap_1p5()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(tr(lang, Key::Proxy)),
                        )
                        .child(div().py(px(4.)).child(Input::new(&state.cfg_proxy_input))),
                )
                .child(
                    h_flex()
                        .gap_3()
                        .child(
                            v_flex()
                                .flex_1()
                                .gap_1p5()
                                .child(div().text_sm().child(tr(lang, Key::TimeoutSeconds)))
                                .child(div().py(px(4.)).child(Input::new(&state.cfg_timeout_input))),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .gap_1p5()
                                .child(div().text_sm().child(tr(lang, Key::Retries)))
                                .child(div().py(px(4.)).child(Input::new(&state.cfg_retries_input))),
                        ),
                )
                .child(
                    v_flex()
                        .gap_1p5()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(tr(lang, Key::CustomHeaders)),
                        )
                        .child(Textarea::new(&state.cfg_headers_input))
                        .child(
                            div()
                                .text_xs()
                                .text_color(p.muted_foreground)
                                .child(tr(lang, Key::CustomHeadersHint)),
                        ),
                )
                .child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("test-provider")
                                .outline()
                                .small()
                                .icon(IconName::Plug)
                                .label(tr(lang, Key::TestConnection))
                                .on_click(cx.listener(|this, _, _, cx| this.test_provider_connection(cx))),
                        )
                        .child(
                            Button::new("save-provider")
                                .primary()
                                .small()
                                .label(tr(lang, Key::SaveConfig))
                                .on_click(cx.listener(|this, _, _, cx| this.save_current_provider_settings(cx))),
                        ),
                )
                .into_any_element(),
        ],
    );

    let model_rows: Vec<AnyElement> = if provider.models.is_empty() {
        vec![
            v_flex()
                .items_center()
                .gap_2()
                .py_8()
                .text_sm()
                .text_color(p.muted_foreground)
                .child(Icon::new(IconName::Boxes).size(px(24.)))
                .child(tr(lang, Key::NoModels))
                .into_any_element(),
        ]
    } else {
        provider
            .models
            .iter()
            .enumerate()
            .map(|(ix, model)| {
                let pin_ids = (provider.id.clone(), model.id.clone());
                let delete_ids = pin_ids.clone();
                let toggle_ids = pin_ids.clone();
                let edit_ids = pin_ids.clone();
                let row_edit_ids = pin_ids.clone();
                h_flex()
                    .gap_3()
                    .px_4()
                    .py_2p5()
                    .child(
                        // 点击模型信息打开编辑弹窗；右侧按钮是兄弟节点，不会触发这里
                        h_flex()
                            .id(("model-open", ix))
                            .flex_1()
                            .min_w_0()
                            .gap_3()
                            .cursor_pointer()
                            .when(!model.enabled, |this| this.opacity(0.55))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                model_editor_dialog::open_model_editor(cx.entity(), window, cx);
                                this.begin_edit_model(&row_edit_ids.0, &row_edit_ids.1, window, cx);
                            }))
                            .child(model_avatar(model, px(32.), p))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_0p5()
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_sm()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child(model.name.clone()),
                                            )
                                            .child(model_badges(model, p))
                                            .when(!model.tags.is_empty(), |this| {
                                                this.child(Tag::secondary().small().child(model.tags.clone()))
                                            }),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_xs()
                                            .font_family(mono_font.clone())
                                            .text_color(p.muted_foreground)
                                            .child(model.id.clone()),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .gap_1()
                            .child(
                                Button::new(("edit-model", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Pencil)
                                    .tooltip(tr(lang, Key::EditModel))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        model_editor_dialog::open_model_editor(cx.entity(), window, cx);
                                        this.begin_edit_model(&edit_ids.0, &edit_ids.1, window, cx);
                                    })),
                            )
                            .child(
                                Button::new(("pin-model", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(if model.is_pinned {
                                        IconName::PinOff
                                    } else {
                                        IconName::Pin
                                    })
                                    .selected(model.is_pinned)
                                    .tooltip(if model.is_pinned {
                                        tr(lang, Key::Unpin)
                                    } else {
                                        tr(lang, Key::PinToTop)
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.toggle_model_pin(&pin_ids.0, &pin_ids.1, cx);
                                    })),
                            )
                            .child(
                                Button::new(("delete-model", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Trash)
                                    .tooltip(tr(lang, Key::DeleteModel))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.delete_model_from_provider(&delete_ids.0, &delete_ids.1, cx);
                                    })),
                            )
                            .child(
                                Switch::new(("model-enabled", ix))
                                    .checked(model.enabled)
                                    .tooltip(if model.enabled {
                                        tr(lang, Key::Enabled)
                                    } else {
                                        tr(lang, Key::Disabled)
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.toggle_model_enabled(&toggle_ids.0, &toggle_ids.1, cx);
                                    })),
                            ),
                    )
                    .into_any_element()
            })
            .collect()
    };

    let models = v_flex()
        .gap_2()
        .child(
            h_flex()
                .justify_between()
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(tr(lang, Key::ModelList)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(p.muted_foreground)
                                .child(provider.models.len().to_string()),
                        ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("fetch-models")
                                .outline()
                                .xsmall()
                                .icon(IconName::Download)
                                .label(tr(lang, Key::FetchModels))
                                .on_click(cx.listener(|this, _, _, cx| this.fetch_models_from_provider(cx))),
                        )
                        .child(
                            Button::new("add-model")
                                .outline()
                                .xsmall()
                                .icon(IconName::Plus)
                                .label(tr(lang, Key::AddModel))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    // 先打开弹窗再准备草稿：弹窗打开时会抢走焦点，之后才能把焦点给到 ID 输入框
                                    model_editor_dialog::open_model_editor(cx.entity(), window, cx);
                                    this.begin_add_model(window, cx);
                                })),
                        ),
                ),
        )
        .child(v_flex().rounded_lg().border_1().border_color(p.border).children(
            model_rows.into_iter().enumerate().map(|(ix, row)| {
                div()
                    .when(ix > 0, |this| this.border_t_1().border_color(p.border))
                    .child(row)
            }),
        ));

    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .child(
            div()
                .size_full()
                .flex()
                .justify_center()
                .px_8()
                .py_8()
                .child(
                    v_flex()
                        .w_full()
                        .max_w(PAGE_MAX_WIDTH)
                        .gap_8()
                        .child(header)
                        .child(connection)
                        .child(models),
                )
                .overflow_y_scrollbar()
                .id("settings-provider-detail"),
        )
        .into_any_element()
}
