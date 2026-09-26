use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, Icon, Selectable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::brand_icon::{model_avatar, model_badges, provider_avatar};
use super::{Palette, dialogs, icon_tile};
use crate::app::{AppState, SettingsTab};
use crate::i18n::tr;

const PAGE_MAX_WIDTH: Pixels = px(720.);

pub fn render_settings(state: &mut AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    div()
        .flex()
        .size_full()
        .child(render_nav(state, p, cx))
         .child(match state.settings_tab {
             SettingsTab::General => render_general(state, p, cx).into_any_element(),
             SettingsTab::Providers => render_providers(state, p, cx).into_any_element(),
             SettingsTab::Prompts => render_prompts(state, p, cx).into_any_element(),
             SettingsTab::McpServers => render_mcp(p).into_any_element(),
             SettingsTab::About => render_about(p).into_any_element(),
         })
}

fn render_nav(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    let lang = state.language();
    let tabs = [
        (SettingsTab::General, IconName::Settings2, tr(lang, "general_settings")),
        (SettingsTab::Providers, IconName::Cloud, tr(lang, "provider_settings")),
         (SettingsTab::Prompts, IconName::BookOpen, "提示词"),
         (SettingsTab::McpServers, IconName::Plug, tr(lang, "mcp_settings")),
        (SettingsTab::About, IconName::Info, tr(lang, "about_settings")),
    ];

    v_flex()
        .w(px(220.))
        .h_full()
        .flex_none()
        .gap_1()
        .p_3()
        .bg(p.sidebar)
        .border_r_1()
        .border_color(p.sidebar_border)
        .child(
            h_flex()
                .id("back-to-chat")
                .h(px(32.))
                .gap_2()
                .px_2p5()
                .rounded_md()
                .cursor_pointer()
                .text_sm()
                .text_color(p.sidebar_foreground)
                .hover(|s| s.bg(p.sidebar_accent.opacity(0.6)))
                .on_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx)))
                .child(Icon::new(IconName::ArrowLeft).size(px(16.)))
                .child(div().flex_1().child(tr(lang, "back_to_chat")))
                .child(div().text_xs().text_color(p.muted_foreground).child("Esc")),
        )
        .child(
            div()
                .px_2p5()
                .pt_4()
                .pb_1()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.muted_foreground)
                .child(tr(lang, "settings")),
        )
        .children(tabs.into_iter().map(|(tab, icon, label)| {
            let is_active = state.settings_tab == tab;
            h_flex()
                .id(SharedString::from(format!("settings-tab-{:?}", tab)))
                .h(px(32.))
                .gap_2()
                .px_2p5()
                .rounded_md()
                .cursor_pointer()
                .text_sm()
                .map(|this| {
                    if is_active {
                        this.bg(p.sidebar_accent)
                            .text_color(p.sidebar_accent_foreground)
                            .font_weight(FontWeight::MEDIUM)
                    } else {
                        this.text_color(p.sidebar_foreground)
                            .hover(|s| s.bg(p.sidebar_accent.opacity(0.6)))
                    }
                })
                .on_click(cx.listener(move |this, _, window, cx| this.open_settings(tab, window, cx)))
                .child(Icon::new(icon).size(px(16.)))
                .child(label)
        }))
}

// ================= 页面骨架 =================

/// 可滚动的设置页：居中的内容列 + 标题与说明
fn page(
    id: &'static str,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    p: &Palette,
    content: impl IntoElement,
) -> impl IntoElement {
    div().flex_1().min_w_0().h_full().child(
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
                    .gap_6()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child(title.into()))
                            .child(div().text_sm().text_color(p.muted_foreground).child(description.into())),
                    )
                    .child(content),
            )
            .overflow_y_scrollbar()
            .id(id),
    )
}

/// 带标题的分组卡片，行与行之间用细线分隔
fn section(title: &'static str, p: &Palette, rows: Vec<AnyElement>) -> impl IntoElement {
    v_flex()
        .gap_2()
        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(title))
        .child(
            v_flex()
                .rounded_lg()
                .border_1()
                .border_color(p.border)
                .bg(p.background)
                .children(rows.into_iter().enumerate().map(|(ix, row)| {
                    div()
                        .when(ix > 0, |this| this.border_t_1().border_color(p.border))
                        .child(row)
                })),
        )
}

/// 左侧标题说明、右侧控件的设置行
fn setting_row(
    title: &'static str,
    description: impl Into<SharedString>,
    p: &Palette,
    control: impl IntoElement,
) -> AnyElement {
    h_flex()
        .justify_between()
        .gap_6()
        .px_4()
        .py_3p5()
        .child(
            v_flex()
                .min_w_0()
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
                .child(div().text_xs().text_color(p.muted_foreground).child(description.into())),
        )
        .child(div().flex_none().child(control))
        .into_any_element()
}

/// 分段选择器
fn segmented<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(T, SharedString)>,
    selected: T,
    p: &Palette,
    on_select: impl Fn(T, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_select = std::rc::Rc::new(on_select);
    h_flex()
        .gap_0p5()
        .p_0p5()
        .rounded_lg()
        .bg(p.muted)
        .children(options.into_iter().enumerate().map(|(ix, (value, label))| {
            let is_selected = value == selected;
            let on_select = on_select.clone();
            div()
                .id((id, ix))
                .h(px(28.))
                .px_3()
                .flex()
                .items_center()
                .rounded_md()
                .cursor_pointer()
                .text_sm()
                .map(|this| {
                    if is_selected {
                        this.bg(p.background)
                            .text_color(p.foreground)
                            .font_weight(FontWeight::MEDIUM)
                            .shadow_sm()
                    } else {
                        this.text_color(p.muted_foreground)
                            .hover(|s| s.text_color(p.foreground))
                    }
                })
                .on_click(move |_, window, cx| on_select(value, window, cx))
                .child(label)
        }))
}

// ================= 通用设置 =================

fn render_general(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
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
        vec![
            setting_row("主题", "选择浅色或深色界面", p, {
                let app = app.clone();
                segmented(
                    "theme",
                    vec![(false, "浅色".into()), (true, "深色".into())],
                    state.is_dark,
                    p,
                    move |is_dark, window, cx| app.update(cx, |this, cx| this.set_dark_mode(is_dark, window, cx)),
                )
            }),
        ],
    );

    let conversation = section(
        "对话",
        p,
        vec![
            setting_row(
                "新对话默认模型",
                "只影响之后创建的对话",
                p,
                {
                    let (provider_id, model_id) = state.config.default_model_selection();
                    let label = state.config.providers.iter()
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
                            providers.iter().filter(|provider| provider.enabled).fold(menu, |menu, provider| {
                                provider.models.iter().filter(|model| model.enabled).fold(menu, |menu, model| {
                                    let app = app.clone();
                                    let pid = provider.id.clone();
                                    let mid = model.id.clone();
                                    menu.item(PopupMenuItem::new(format!("{} / {}", provider.name, model.name))
                                        .checked(pid == provider_id && mid == model_id)
                                        .on_click(move |_, _, cx| {
                                            app.update(cx, |this, cx| {
                                                this.config.select_model(&pid, &mid);
                                                cx.notify();
                                            });
                                        }))
                                })
                            })
                        })
                },
            ),
            setting_row(
                tr(lang, "temperature"),
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
                        .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(tr(lang, "system_prompt")))
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
                            .label(tr(lang, "save"))
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
        tr(lang, "general_settings"),
        "外观与默认的对话参数",
        p,
        v_flex().gap_8().child(appearance).child(conversation).child(local_tools),
    )
}

// ================= 模型渠道 =================

fn render_providers(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
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
                                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("模型渠道"))
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
                                .tooltip("添加渠道")
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
                                        .child(
                                            div()
                                                .truncate()
                                                .text_xs()
                                                .text_color(p.muted_foreground)
                                                .child(format!("{} 个模型", provider.models.len())),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .size(px(8.))
                                        .rounded_full()
                                        .bg(if provider.enabled { p.success } else { p.border }),
                                )
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

fn render_provider_detail(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
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
            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(if state.config.providers.is_empty() {
                "还没有模型渠道"
            } else {
                "选择一个渠道查看配置"
            }))
            .child(
                div()
                    .text_sm()
                    .text_color(p.muted_foreground)
                    .child("支持 OpenAI、Gemini、Claude 等接口规范，也可以接入兼容 OpenAI 的中转服务"),
            )
            .child(
                Button::new("empty-add-provider")
                    .primary()
                    .icon(IconName::Plus)
                    .label("添加渠道")
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
                .child(div().truncate().text_xl().font_weight(FontWeight::SEMIBOLD).child(provider.name.clone()))
                .child(h_flex().child(Tag::secondary().small().child(provider.channel_type.label()))),
        )
        .child(
            h_flex()
                .flex_none()
                .gap_3()
                .child(
                    Switch::new("provider-enabled")
                        .checked(provider.enabled)
                        .label(if provider.enabled { "已启用" } else { "已停用" })
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_provider_enabled(&toggle_pid, cx))),
                )
                .child(
                    Button::new("delete-provider")
                        .ghost()
                        .small()
                        .icon(IconName::Trash)
                        .tooltip("删除渠道")
                        .on_click(cx.listener(move |_, _, window, cx| {
                            dialogs::confirm_delete_provider(cx.entity(), delete_name.clone(), window, cx);
                        })),
                ),
        );

    let connection = section(
        "连接配置",
        p,
        vec![
            v_flex()
                .gap_4()
                .px_4()
                .py_4()
                 .child(
                     v_flex()
                         .gap_1p5()
                         .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(tr(lang, "api_key")))
                         .child(div().py(px(4.)).child(Input::new(&state.cfg_api_key_input).mask_toggle())),
                 )
                .child(
                    v_flex()
                        .gap_1p5()
                        .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(tr(lang, "base_url")))
                         .child(div().py(px(4.)).child(Input::new(&state.cfg_base_url_input)))
                        .child(
                            div()
                                .text_xs()
                                .text_color(p.muted_foreground)
                                .child("一般以 /v1 结尾，例如 https://api.openai.com/v1"),
                        ),
                )
                 .child(v_flex().gap_1p5().child(div().text_sm().font_weight(FontWeight::MEDIUM).child("代理")).child(div().py(px(4.)).child(Input::new(&state.cfg_proxy_input))))
                 .child(h_flex().gap_3()
                     .child(v_flex().flex_1().gap_1p5().child(div().text_sm().child("超时（秒）")).child(div().py(px(4.)).child(Input::new(&state.cfg_timeout_input))))
                     .child(v_flex().flex_1().gap_1p5().child(div().text_sm().child("失败重试")).child(div().py(px(4.)).child(Input::new(&state.cfg_retries_input)))))
                 .child(v_flex().gap_1p5().child(div().text_sm().font_weight(FontWeight::MEDIUM).child("自定义请求头")).child(Textarea::new(&state.cfg_headers_input)).child(div().text_xs().text_color(p.muted_foreground).child("每行一个 Name: Value")))
                .child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("test-provider")
                                .outline()
                                .small()
                                .icon(IconName::Plug)
                                .label("测试连接")
                                .on_click(cx.listener(|this, _, _, cx| this.test_provider_connection(cx))),
                        )
                        .child(
                            Button::new("save-provider")
                                .primary()
                                .small()
                                .label("保存配置")
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
                .child(tr(lang, "no_models"))
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
                                dialogs::open_model_editor(cx.entity(), window, cx);
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
                                    .tooltip("编辑模型")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        dialogs::open_model_editor(cx.entity(), window, cx);
                                        this.begin_edit_model(&edit_ids.0, &edit_ids.1, window, cx);
                                    })),
                            )
                            .child(
                                Button::new(("pin-model", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(if model.is_pinned { IconName::PinOff } else { IconName::Pin })
                                    .selected(model.is_pinned)
                                    .tooltip(if model.is_pinned { "取消置顶" } else { "置顶到模型列表顶部" })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.toggle_model_pin(&pin_ids.0, &pin_ids.1, cx);
                                    })),
                            )
                            .child(
                                Button::new(("delete-model", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Trash)
                                    .tooltip("删除模型")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.delete_model_from_provider(&delete_ids.0, &delete_ids.1, cx);
                                    })),
                            )
                            .child(
                                Switch::new(("model-enabled", ix))
                                    .checked(model.enabled)
                                    .tooltip(if model.enabled { "已启用" } else { "已停用" })
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
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("模型"))
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
                                .label(tr(lang, "fetch_models"))
                                .on_click(cx.listener(|this, _, _, cx| this.fetch_models_from_provider(cx))),
                        )
                        .child(
                            Button::new("add-model")
                                .outline()
                                .xsmall()
                                .icon(IconName::Plus)
                                .label(tr(lang, "add_model"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    // 先打开弹窗再准备草稿：弹窗打开时会抢走焦点，之后才能把焦点给到 ID 输入框
                                    dialogs::open_model_editor(cx.entity(), window, cx);
                                    this.begin_add_model(window, cx);
                                })),
                        ),
                ),
        )
        .child(
            v_flex()
                .rounded_lg()
                .border_1()
                .border_color(p.border)
                .children(model_rows.into_iter().enumerate().map(|(ix, row)| {
                    div()
                        .when(ix > 0, |this| this.border_t_1().border_color(p.border))
                        .child(row)
                })),
        );

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

 fn render_prompts(state: &AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
     let presets = state.prompts.presets.clone();
     let templates = state.prompts.templates.clone();
     let preset_rows = presets.iter().enumerate().map(|(ix, preset)| {
         let use_id = preset.id.clone();
         let delete_id = preset.id.clone();
         h_flex().justify_between().px_4().py_2().child(div().text_sm().child(format!("{} {}", preset.icon, preset.name))).child(
             h_flex().gap_1()
                 .child(Button::new(("use-preset", ix)).ghost().xsmall().label("使用").on_click(cx.listener(move |this, _, window, cx| this.create_session_from_preset(&use_id, window, cx))))
                 .child(Button::new(("delete-preset", ix)).ghost().xsmall().icon(IconName::Trash).on_click(cx.listener(move |this, _, _, cx| this.delete_prompt(&delete_id, false, cx)))),
         ).into_any_element()
     }).collect::<Vec<_>>();
     let template_rows = templates.iter().enumerate().map(|(ix, template)| {
         let id = template.id.clone();
         h_flex().justify_between().px_4().py_2().child(div().text_sm().child(template.name.clone())).child(
             Button::new(("delete-template", ix)).ghost().xsmall().icon(IconName::Trash).on_click(cx.listener(move |this, _, _, cx| this.delete_prompt(&id, true, cx))),
         ).into_any_element()
     }).collect::<Vec<_>>();
     page(
         "settings-prompts",
         "提示词",
         "助手预设用于新建对话，模板可在输入框输入 /名称 后回车插入",
         p,
         v_flex().gap_8()
             .child(section("助手预设", p, if preset_rows.is_empty() { vec![div().px_4().py_3().text_sm().child("还没有预设").into_any_element()] } else { preset_rows }))
             .child(section("提示词模板", p, if template_rows.is_empty() { vec![div().px_4().py_3().text_sm().child("还没有模板").into_any_element()] } else { template_rows }))
             .child(v_flex().gap_3().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("新建"))
                 .child(Input::new(&state.prompt_name_input))
                 .child(Input::new(&state.prompt_icon_input))
                 .child(Textarea::new(&state.prompt_body_input))
                 .child(h_flex().gap_2()
                     .child(Button::new("save-preset").outline().small().label("保存为预设").on_click(cx.listener(|this, _, window, cx| { this.save_prompt_from_inputs(false, window, cx); })))
                     .child(Button::new("save-template").primary().small().label("保存为模板").on_click(cx.listener(|this, _, window, cx| { this.save_prompt_from_inputs(true, window, cx); }))))),
     )
 }
 
 // ================= MCP / 关于 =================
 
// ================= MCP / 关于 =================

fn render_mcp(p: &Palette) -> impl IntoElement {
    page(
        "settings-mcp",
        "MCP 服务器",
        "通过 Model Context Protocol 为 Agent 接入外部工具",
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
            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child("即将推出"))
            .child(
                div()
                    .max_w(px(420.))
                    .text_center()
                    .text_sm()
                    .text_color(p.muted_foreground)
                    .child("后续将支持接入本地 Stdio 与远程 SSE 类型的 MCP 服务器，让 Agent 可以使用文件系统、GitHub、数据库等工具。"),
            ),
    )
}

fn render_about(p: &Palette) -> impl IntoElement {
    let features = [
        (IconName::HardDrive, "数据只保存在本地，没有任何云端遥测"),
        (IconName::Layers, "支持 OpenAI Chat、OpenAI Responses、Gemini、Claude 四种接口规范"),
        (IconName::Zap, "原生 SSE 流式解析，Markdown 实时渲染"),
        (IconName::SquareTerminal, "内置本地工具：/ls、/read、/git、/bash（执行前需授权）"),
        (IconName::Languages, "界面支持简体中文、繁體中文、English、日本語"),
    ];

    page(
        "settings-about",
        "关于",
        "纯 Rust + GPUI 构建的桌面 AI 工作台",
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
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(p.muted_foreground)
                                    .child(format!("版本 {}", env!("CARGO_PKG_VERSION"))),
                            ),
                    ),
            )
            .child(section(
                "特性",
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
