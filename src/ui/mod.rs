mod analytics;
mod brand_icon;
mod chat;
mod dialogs;
mod fetch_models_dialog;
mod markdown_image;
mod model_editor_dialog;
mod model_picker;
mod params;
mod settings;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Root, Selectable as _, Sizable as _, TitleBar, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit_assets::IconName;

use crate::app::{AppState, SettingsTab, ToastLevel, ViewMode};
use crate::config::ChannelType;
use crate::i18n::{AppLanguage, tr};
use crate::{CloseSettings, NewChat, PasteIntoChat, ToggleSettings, ToggleSidebar};

/// 对话内容与输入框的最大宽度，宽屏下保持舒适的行长
pub const CONTENT_MAX_WIDTH: Pixels = px(780.);
pub const SIDEBAR_WIDTH: Pixels = px(260.);

/// 窗口的第一层是 `Root`，第二层是 `Workspace`：负责渲染弹窗、通知等浮层。
///
/// 浮层放在这一层而不是 `AppState::render` 里，是因为弹窗内容构建时需要读取 `AppState`，
/// 而 `AppState` 在自己的 `render` 期间处于借用状态。
pub struct Workspace {
    app: Entity<AppState>,
}

impl Workspace {
    pub fn new(app: Entity<AppState>) -> Self {
        Self { app }
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sheet_layer = Root::render_sheet_layer(window, cx);
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);

        let mouse_guard = canvas(
            |_, _, _| (),
            |_, (), window, _| {
                window.on_mouse_event(|event: &MouseMoveEvent, phase, window, cx| {
                    if phase.capture() && event.pressed_button != Some(MouseButton::Left) {
                        gpui_kit::base::TextSelection::end(window, cx);
                    }
                });
            },
        );

        div()
            .size_full()
            .child(self.app.clone())
            .children(sheet_layer)
            .children(dialog_layer)
            .children(notification_layer)
            .child(mouse_guard)
    }
}

/// 当前主题中常用颜色的快照，避免在构建界面时反复借用 `cx`
#[derive(Clone, Copy)]
pub struct Palette {
    pub background: Hsla,
    pub foreground: Hsla,
    pub muted: Hsla,
    pub muted_foreground: Hsla,
    pub border: Hsla,
    pub accent: Hsla,
    pub primary: Hsla,
    pub primary_foreground: Hsla,
    pub sidebar: Hsla,
    pub sidebar_foreground: Hsla,
    pub sidebar_border: Hsla,
    pub sidebar_accent: Hsla,
    pub sidebar_accent_foreground: Hsla,
    pub danger: Hsla,
    pub warning: Hsla,
    pub success: Hsla,
    pub is_dark: bool,
}

impl Palette {
    pub fn new(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            background: theme.background,
            foreground: theme.foreground,
            muted: theme.muted,
            muted_foreground: theme.muted_foreground,
            border: theme.border,
            accent: theme.accent,
            primary: theme.primary,
            primary_foreground: theme.primary_foreground,
            sidebar: theme.sidebar,
            sidebar_foreground: theme.sidebar_foreground,
            sidebar_border: theme.sidebar_border,
            sidebar_accent: theme.sidebar_accent,
            sidebar_accent_foreground: theme.sidebar_accent_foreground,
            danger: theme.danger,
            warning: theme.warning,
            success: theme.success,
            is_dark: theme.is_dark(),
        }
    }
}

/// 标题栏语言菜单中的选项
const LANGUAGES: [(AppLanguage, &str); 4] = [
    (AppLanguage::ZhCn, "简体中文"),
    (AppLanguage::ZhTw, "繁體中文"),
    (AppLanguage::EnUs, "English"),
    (AppLanguage::JaJp, "日本語"),
];

pub fn channel_icon(ct: ChannelType) -> IconName {
    match ct {
        ChannelType::OpenAiChat => IconName::Sparkles,
        ChannelType::OpenAiResponses => IconName::Layers,
        ChannelType::Gemini => IconName::Sun,
        ChannelType::Claude => IconName::Terminal,
    }
}

/// 带图标的小方块，用于应用标识、渠道图标、助手头像等
pub fn icon_tile(icon: IconName, size: Pixels, bg: Hsla, fg: Hsla) -> Div {
    div()
        .flex_none()
        .size(size)
        .rounded(size * 0.28)
        .bg(bg)
        .text_color(fg)
        .flex()
        .items_center()
        .justify_center()
        .child(Icon::new(icon).size(size * 0.58))
}

/// 可选中的小按钮：预设值、多选项都用它，选中时用主题色描边
pub fn chip(id: impl Into<ElementId>, selected: bool, p: &Palette) -> Stateful<Div> {
    h_flex()
        .id(id)
        .flex_none()
        .h(px(28.))
        .px_2p5()
        .gap_1()
        .rounded_md()
        .border_1()
        .text_sm()
        .cursor_pointer()
        .map(|this| {
            if selected {
                this.border_color(p.primary)
                    .bg(p.primary.opacity(0.08))
                    .text_color(p.primary)
                    .font_weight(FontWeight::MEDIUM)
            } else {
                this.border_color(p.border)
                    .text_color(p.foreground)
                    .hover(|style| style.bg(p.muted))
            }
        })
}

impl Render for AppState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for (level, message) in self.take_toasts() {
            let note = match level {
                ToastLevel::Info => Notification::info(message),
                ToastLevel::Success => Notification::success(message),
                ToastLevel::Error => Notification::error(message),
            };
            window.defer(cx, move |window, cx| window.push_notification(note, cx));
        }
        if self.open_model_picker {
            self.open_model_picker = false;
            self.model_fetch_query.clear();
            let search = self.model_fetch_search.clone();
            let app = cx.entity();
            window.defer(cx, move |window, cx| {
                search.update(cx, |input, cx| input.set_value("", window, cx));
                fetch_models_dialog::open_fetch_models_dialog(app, window, cx);
            });
        }

        let p = Palette::new(cx);
        let view_mode = self.view_mode;

        v_flex()
            .id("app")
            .key_context("Perch")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &NewChat, window, cx| {
                this.create_new_session(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSettings, window, cx| {
                this.toggle_settings(window, cx);
            }))
            .on_action(cx.listener(|this, _: &PasteIntoChat, window, cx| {
                this.paste_into_chat(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.toggle_sidebar(cx);
            }))
            .on_action(cx.listener(|this, _: &CloseSettings, window, cx| {
                if this.view_mode == ViewMode::Settings {
                    this.close_settings(window, cx);
                } else {
                    cx.propagate();
                }
            }))
            .size_full()
            .bg(p.background)
            .text_color(p.foreground)
            .child(self.render_title_bar(&p, cx))
            .child(div().flex().flex_1().min_h_0().w_full().map(|this| {
                match view_mode {
                    ViewMode::Chat => this
                        .when(!self.sidebar_collapsed, |this| {
                            this.child(chat::render_sidebar(self, &p, cx))
                        })
                        .child(chat::render_chat_panel(self, &p, cx)),
                    ViewMode::Settings => this.child(settings::render_settings(self, &p, cx)),
                }
            }))
    }
}

impl AppState {
    fn render_title_bar(&self, p: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let in_settings = self.view_mode == ViewMode::Settings;
        let is_dark = self.is_dark;
        let lang = self.language();
        let app = cx.entity();

        TitleBar::new()
            .child(
                h_flex()
                    .gap_2()
                    .when(!in_settings, |this| {
                        this.child(
                            Button::new("toggle-sidebar")
                                .ghost()
                                .small()
                                .icon(if self.sidebar_collapsed {
                                    IconName::PanelLeftOpen
                                } else {
                                    IconName::PanelLeftClose
                                })
                                .tooltip("显示/隐藏侧边栏 (Ctrl+B)")
                                // 标题栏整体是拖拽区，按钮需要遮挡住它，否则 Windows 会把点击当成拖动窗口
                                .occlude()
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
                        )
                    })
                    .child(icon_tile(IconName::Sparkles, px(18.), p.primary, p.primary_foreground))
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Perch")),
            )
            .child(
                h_flex()
                    .gap_1()
                    .pr_2()
                    .child(
                        Button::new("language")
                            .ghost()
                            .small()
                            .icon(IconName::Languages)
                            .tooltip(tr(lang, "language_select"))
                            .occlude()
                            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                                LANGUAGES.iter().fold(menu, |menu, &(option, label)| {
                                    let app = app.clone();
                                    menu.item(PopupMenuItem::new(label).checked(option == lang).on_click(
                                        move |_, window, cx| {
                                            app.update(cx, |this, cx| this.switch_language(option, window, cx));
                                        },
                                    ))
                                })
                            }),
                    )
                    .child(
                        Button::new("toggle-theme")
                            .ghost()
                            .small()
                            .icon(if is_dark { IconName::Sun } else { IconName::Moon })
                            .tooltip(if is_dark {
                                "切换到浅色模式"
                            } else {
                                "切换到深色模式"
                            })
                            .occlude()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.set_dark_mode(!is_dark, window, cx);
                            })),
                    )
                    .child(
                        Button::new("open-analytics")
                            .ghost()
                            .small()
                            .icon(IconName::ChartBar)
                            .tooltip("用量与费用统计看板")
                            .occlude()
                            .on_click(cx.listener(|_, _, window, cx| {
                                analytics::open_analytics_dialog(cx.entity(), window, cx);
                            })),
                    )
                    .child(
                        Button::new("toggle-settings")
                            .ghost()
                            .small()
                            .icon(IconName::Settings)
                            .selected(in_settings)
                            .tooltip(format!("{} (Ctrl+,)", tr(lang, "settings")))
                            .occlude()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_settings(window, cx);
                            })),
                    ),
            )
    }

    pub fn open_providers_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings(SettingsTab::Providers, window, cx);
    }
}
