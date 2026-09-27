use gpui_kit::component::{Root, TitleBar};
use gpui_kit::*;

mod agent;
mod agent_loop;
mod analytics;
mod app;
mod attachment_ops;
mod audit;
mod audit_ops;
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
mod mcp;
mod mcp_ops;
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
mod skill_ops;
mod skills;
mod storage;
mod theme;
mod tool_ops;
mod ui;
mod workspace_ops;

use app::AppState;
use ui::{ErrorPage, Workspace};

actions!(
    perch,
    [NewChat, ToggleSettings, ToggleSidebar, CloseSettings, PasteIntoChat]
);

fn main() {
    models_dev::sync_cache_background(false);
    // 配置读不出来也得能把窗口开起来——错误页要用它定主题和语言。所以这里退回默认值，
    // 真正的读取和报错交给 `AppState::bootstrap`（它失败时显示错误页，而不是闪退）。
    let config = config::AppConfig::try_load().unwrap_or_default();
    // 这两个值要在窗口开出来之前用，先取出来，省得把整份配置搬进闭包
    let is_dark = config.is_dark;
    let language = config.language.clone();
    let app = gpui_kit::application()
        .with_assets(brand::AppAssets)
        .with_http_client(image_http::client_for_config(&config));

    app.run(move |cx: &mut App| {
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

        // 主题和语言先设好：错误页也要有颜色、有文案（`AppState::new` 会再设一次，
        // 那次带上窗口，用来触发重绘）。
        let lang = i18n::AppLanguage::from_str(&language);
        theme::apply_theme(is_dark, None, cx);
        i18n::apply_locale(lang);
        i18n::set_current(cx, lang);

        let bounds = Bounds::centered(None, size(px(1180.0), px(780.0)), cx);
        let opened = cx.open_window(
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
                // 启动数据先试读。读不出来就换一个根视图显示原因——这一步必须在
                // 建 `AppState` 之前，因为它失败时 `AppState` 根本构造不出来。
                match AppState::bootstrap() {
                    Ok(bootstrap) => {
                        let app = cx.new(|cx| AppState::new(bootstrap, window, cx));
                        // 后台连接启用的 MCP 服务器。放在这里而不是 `AppState::new`
                        // 里：`new` 执行期间实体还没挂到 app 上，那时候起后台任务不合适。
                        app.update(cx, |state, cx| state.connect_mcp_servers(cx));
                        let workspace = cx.new(|_| Workspace::new(app));
                        cx.new(|cx| Root::new(workspace, window, cx))
                    }
                    Err(failure) => {
                        let page = cx.new(|_| ErrorPage::new(failure));
                        cx.new(|cx| Root::new(page, window, cx))
                    }
                }
            },
        );
        if let Err(error) = opened {
            // 走到这里连窗口都没开出来，没有任何界面能显示错误了，只能留下记录再退出
            eprintln!("Perch 启动失败：无法创建窗口：{error}");
            cx.quit();
        }
    });
}
