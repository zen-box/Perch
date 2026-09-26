use gpui_kit::component::{TITLE_BAR_HEIGHT, Theme, ThemeMode};
use gpui_kit::*;

/// 在 gpui-kit 默认主题（shadcn neutral）之上叠加品牌靛蓝色。
///
/// 所有界面颜色都从 `cx.theme()` 读取，切换深浅色时组件（输入框、按钮、弹窗等）会一起变化。
pub fn apply_theme(is_dark: bool, window: Option<&mut Window>, cx: &mut App) {
    let mode = if is_dark { ThemeMode::Dark } else { ThemeMode::Light };
    Theme::change(mode, window, cx);

    // (主色, 悬停, 按下)
    let (primary, hover, active) = if is_dark {
        (0x6366f1, 0x818cf8, 0x4f46e5)
    } else {
        (0x4f46e5, 0x4338ca, 0x3730a3)
    };
    let primary: Hsla = rgb(primary).into();
    let hover: Hsla = rgb(hover).into();
    let active: Hsla = rgb(active).into();
    let on_primary: Hsla = rgb(0xffffff).into();

    let theme = Theme::global_mut(cx);
    theme.primary = primary;
    theme.primary_hover = hover;
    theme.primary_active = active;
    theme.primary_foreground = on_primary;
    theme.button_primary = primary;
    theme.button_primary_hover = hover;
    theme.button_primary_active = active;
    theme.button_primary_foreground = on_primary;
    theme.ring = primary.opacity(0.6);
    theme.link = primary;
    theme.link_hover = hover;
    theme.link_active = active;
    theme.progress_bar = primary;
    theme.slider_bar = primary;
    theme.caret = primary;
    // 按钮等组件读取的是 tokens 里的背景，需要同步一份
    theme.tokens.primary = primary.into();
    theme.tokens.primary_hover = hover.into();
    theme.tokens.primary_active = active.into();
    theme.tokens.button_primary = primary.into();
    theme.tokens.button_primary_hover = hover.into();
    theme.tokens.button_primary_active = active.into();
    theme.tokens.progress_bar = primary.into();
    theme.tokens.slider_bar = primary.into();
    // 通知从标题栏下方弹出，避免遮住窗口按钮
    theme.notification.margins.top = TITLE_BAR_HEIGHT + px(12.);
    Theme::sync_base(cx);
}
