use gpui_kit::component::{Root, TitleBar};
use gpui_kit::*;

mod agent;
mod agent_loop;
mod analytics;
mod app;
mod attachment_ops;
mod backup;
mod backup_ops;
mod brand;
mod clipboard;
mod config;
mod file_store;
mod i18n;
mod image_http;
mod llm;
mod llm_request;
mod llm_stream;
mod llm_tools;
mod local_tools;
mod model;
mod model_info;
mod model_ops;
mod models_dev;
mod params_ops;
mod paths;
mod prompt_ops;
mod prompts;
mod provider_api;
mod provider_ops;
mod reply_ops;
mod session_folder_ops;
mod session_list_ops;
mod session_ops;
mod storage;
mod theme;
mod ui;

use app::AppState;
use ui::Workspace;

actions!(
    perch,
    [NewChat, ToggleSettings, ToggleSidebar, CloseSettings, PasteIntoChat]
);

fn main() {
    models_dev::sync_cache_background(false);
    let config = config::AppConfig::load();
    let app = gpui_kit::application()
        .with_assets(brand::AppAssets)
        .with_http_client(image_http::client_for_config(&config));

    app.run(|cx: &mut App| {
        gpui_kit::init(cx);
        cx.bind_keys([
            KeyBinding::new("secondary-n", NewChat, None),
            KeyBinding::new("secondary-,", ToggleSettings, None),
            KeyBinding::new("secondary-b", ToggleSidebar, None),
            KeyBinding::new("escape", CloseSettings, Some("Perch")),
            // 焦点不在输入框时的 Ctrl+V。输入框有自己的 Ctrl+V（上下文更深，优先生效），
            // 所以这条只在点了对话区空白处之类的情况下起作用。
            KeyBinding::new("secondary-v", PasteIntoChat, Some("Perch")),
        ]);

        let bounds = Bounds::centered(None, size(px(1180.0), px(780.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Perch".into()),
                    ..TitleBar::title_bar_options()
                }),
                window_min_size: Some(size(px(900.0), px(600.0))),
                ..TitleBar::window_options()
            },
            |window, cx| {
                let app = cx.new(|cx| AppState::new(window, cx));
                let workspace = cx.new(|_| Workspace::new(app));
                cx.new(|cx| Root::new(workspace, window, cx))
            },
        )
        .unwrap();
    });
}
