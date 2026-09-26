use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use super::Palette;
use super::{settings_general, settings_misc, settings_prompts, settings_providers};
use crate::app::{AppState, SettingsTab};
use crate::i18n::tr;

pub(super) const PAGE_MAX_WIDTH: Pixels = px(720.);

pub fn render_settings(state: &mut AppState, p: &Palette, cx: &mut Context<AppState>) -> impl IntoElement {
    div()
        .flex()
        .size_full()
        .child(render_nav(state, p, cx))
        .child(match state.settings_tab {
            SettingsTab::General => settings_general::render_general(state, p, cx).into_any_element(),
            SettingsTab::Providers => settings_providers::render_providers(state, p, cx).into_any_element(),
            SettingsTab::Prompts => settings_prompts::render_prompts(state, p, cx).into_any_element(),
            SettingsTab::McpServers => settings_misc::render_mcp(p).into_any_element(),
            SettingsTab::About => settings_misc::render_about(p).into_any_element(),
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
pub(super) fn page(
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

pub(super) fn section(title: &'static str, p: &Palette, rows: Vec<AnyElement>) -> impl IntoElement {
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

pub(super) fn setting_row(
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

pub(super) fn segmented<T: Copy + PartialEq + 'static>(
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
