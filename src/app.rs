use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::message_scroller::MessageScrollerState;
use gpui_kit::*;
use tokio::sync::oneshot;

use crate::agent_loop::AgentState;
use crate::backup::BackupFile;
use crate::config::{AppConfig, ChannelType};
use crate::i18n::{AppLanguage, Key, apply_locale, set_current, tr, tr_args};
use crate::mcp_ops::McpState;
use crate::model::{Attachment, ChatMessage, StorageData};
use crate::model_ops::{ModelEditor, TokenField};
use crate::prompts::PromptLibrary;
use crate::skills::SkillCatalog;
use crate::theme::apply_theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    Chat,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsTab {
    General,
    Providers,
    Prompts,
    Skills,
    McpServers,
    About,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastLevel {
    Info,
    Success,
    Error,
}

/// 只建一次。缓存的是 `Result`——建不起来是环境问题（线程资源），重试没有意义，
/// 而且要让启动时的预检（[`preflight_runtime`]）和之后的 [`runtime`] 看到同一个结论。
static RUNTIME: OnceLock<Result<tokio::runtime::Runtime, String>> = OnceLock::new();

fn build_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
}

/// 启动时提前把运行时建起来，好让"建不起来"变成错误页而不是崩溃。
fn preflight_runtime() -> Result<(), String> {
    RUNTIME
        .get_or_init(build_runtime)
        .as_ref()
        .map(|_| ())
        .map_err(|error| error.clone())
}

/// reqwest 与流式请求依赖 tokio，而 GPUI 自己的执行器不是 tokio，
/// 直接在 `cx.spawn` 里调用 `tokio::spawn` 会 panic，所以单独起一个运行时。
pub fn runtime() -> &'static tokio::runtime::Runtime {
    match RUNTIME.get_or_init(build_runtime) {
        Ok(runtime) => runtime,
        // 启动时 [`preflight_runtime`] 已经试建过一次，能走到这里说明是**运行期**
        // 资源耗尽。这时没有可回退的动作——网络和流式响应全都要靠它，只能崩掉
        // 并留下原因；启动期的同类失败会变成错误页，不会走到这里。
        Err(error) => panic!("failed to start tokio runtime: {error}"),
    }
}

/// 后台任务里更新 `AppState`，实体已经不在了就静默跳过。
///
/// `WeakEntity::update` 的 `Err` 只有"实体已释放"一个含义（窗口关了、程序正在退出），
/// 这时候更新状态没有任何意义，也没有补救动作可做——正是 §6 说的"失败了也无所谓"。
/// 把这条 `let _ =` 收在这里解释一次，省得十来个后台任务各写一遍。
pub(crate) fn update_state<C, R>(
    this: &WeakEntity<AppState>,
    cx: &mut C,
    update: impl FnOnce(&mut AppState, &mut Context<AppState>) -> R,
) where
    C: AppContext,
{
    let _ = this.update(cx, update);
}

pub struct AppState {
    pub storage: StorageData,
    pub config: AppConfig,
    pub view_mode: ViewMode,
    pub settings_tab: SettingsTab,
    pub selected_settings_provider_id: String,
    pub is_dark: bool,
    pub sidebar_collapsed: bool,

    // 统计看板状态
    pub analytics_range: crate::analytics::TimeRange,
    pub analytics_tab: crate::analytics::AnalyticsTab,

    // 添加渠道弹窗中当前选中的渠道类型
    pub add_channel_type: ChannelType,
    pub rename_target_session_id: Option<String>,

    /// Agent 循环与本地工具：等授权的调用、正在执行的工具、循环进行到哪了（见 `agent_loop.rs`）
    pub agent: AgentState,

    /// MCP 服务器：连接、工具清单、连接状态（见 `mcp_ops.rs`）
    pub mcp: McpState,

    /// 助手正在忙：流式生成回答，或者在后台执行工具。
    /// 忙的时候输入框显示停止按钮，不能发新消息、不能重新生成。
    pub is_streaming: bool,
    pub(crate) active_streams: HashMap<String, oneshot::Sender<()>>,
    pending_toasts: Vec<(ToastLevel, String)>,
    pub prompts: PromptLibrary,
    /// 装进数据目录的 Skills 快照。**启动时扫一次，导入/开关之后再扫**——
    /// 渲染期间不能做文件 I/O（见 `skills.rs` 的模块注释）。
    pub skills: SkillCatalog,
    pub folder_filter: String,
    pub favorites_only: bool,
    pub pending_quote: Option<String>,
    pub pending_attachments: Vec<Attachment>,
    pub pending_import: Option<BackupFile>,
    pub compare_selection: Vec<(String, String)>,
    pub edit_message_id: Option<String>,
    pub folder_target_id: Option<String>,
    pub prompt_edit_id: Option<String>,
    pub pending_models: Vec<(String, String)>,
    pub pending_model_selection: HashSet<String>,
    pub model_fetch_query: String,
    pub(crate) open_model_picker: bool,

    /// 展开了“思考过程”的消息 id
    pub expanded_reasoning: HashSet<String>,

    // 输入框状态
    pub chat_input: Entity<TextareaState>,
    pub search_session_input: Entity<InputState>,
    pub(crate) sidebar_search_query: String,
    pub(crate) sidebar_search_revision: u64,
    pub(crate) sidebar_search_ids: HashSet<String>,
    pub rename_input: Entity<InputState>,

    // 渠道设置面板输入框
    pub cfg_api_key_input: Entity<InputState>,
    pub cfg_base_url_input: Entity<InputState>,
    pub cfg_search_provider_input: Entity<InputState>,
    pub model_picker_search_input: Entity<InputState>,
    pub cfg_system_prompt_input: Entity<TextareaState>,

    // 添加渠道表单
    pub new_provider_name_input: Entity<InputState>,
    pub new_provider_base_url_input: Entity<InputState>,
    pub new_provider_api_key_input: Entity<InputState>,

    // 添加 / 编辑模型弹窗
    pub model_editor: Option<ModelEditor>,
    pub model_edit_id_input: Entity<InputState>,
    pub model_edit_name_input: Entity<InputState>,
    pub model_edit_context_input: Entity<InputState>,
    pub model_edit_output_input: Entity<InputState>,
    pub edit_message_input: Entity<TextareaState>,
    pub params_prompt_input: Entity<TextareaState>,
    pub cfg_proxy_input: Entity<InputState>,
    pub cfg_timeout_input: Entity<InputState>,
    pub cfg_retries_input: Entity<InputState>,
    pub cfg_headers_input: Entity<TextareaState>,
    pub prompt_name_input: Entity<InputState>,
    pub prompt_icon_input: Entity<InputState>,
    pub prompt_body_input: Entity<TextareaState>,
    pub folder_name_input: Entity<InputState>,
    pub model_fetch_search: Entity<InputState>,

    // 消息列表（虚拟滚动 + 自动跟随到底部）
    pub message_list: Entity<MessageScrollerState>,
    message_list_session: String,
    pub(crate) scroll_to_end_pending: bool,

    pub focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

/// 启动时要读进来的两样东西：会话数据与配置。
///
/// 单独拆一步，是因为**它们失败时 `AppState` 根本构造不出来**——界面还没建、
/// 界面语言也还没定，没法靠 `AppState` 自己渲染错误页。所以先试读，失败就把
/// 原因交给 `main.rs`，由它换一个根视图来显示（见 `ui::ErrorPage`）。
pub struct Bootstrap {
    pub storage: StorageData,
    pub config: AppConfig,
}

/// 启动阶段读不出数据的原因。
///
/// 和 [`crate::paths::MigrationFailure`] 一样**只带结构化数据、不带现成文案**：
/// 出错时配置很可能根本没读出来，界面语言也就无从得知，所以文案留到渲染错误页时
/// 按当时的语言生成。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartupFailure {
    /// tokio 运行时建不起来
    Runtime(String),
    /// 配置文件读不出来
    Config(String),
    /// 会话库打不开，或者回填后存不回去
    Storage(String),
}

impl StartupFailure {
    /// 渲染成给用户看的一句话。
    pub fn message(&self, lang: AppLanguage) -> String {
        let (key, detail) = match self {
            Self::Runtime(detail) => (Key::StartupRuntimeFailed, detail),
            Self::Config(detail) => (Key::StartupConfigFailed, detail),
            Self::Storage(detail) => (Key::StartupStorageFailed, detail),
        };
        tr_args(lang, key, &[detail.as_str()])
    }
}

/// 给没记渠道 id 的老会话按模型名回填一次。返回是否改过（改过就要存回去）。
fn backfill_session_providers(storage: &mut StorageData, config: &AppConfig) -> bool {
    let (default_provider_id, default_model) = config.default_model_selection();
    let mut changed = false;
    for session in &mut storage.sessions {
        if session.provider_id.is_empty() {
            if let Some(provider) = config
                .providers
                .iter()
                .find(|provider| provider.models.iter().any(|model| model.id == session.model))
            {
                session.provider_id = provider.id.clone();
                changed = true;
            } else if !default_provider_id.is_empty() {
                session.provider_id = default_provider_id.clone();
                session.model = default_model.clone();
                changed = true;
            }
        }
    }
    changed
}

impl AppState {
    /// 试读启动数据。失败时返回原因，由 `main.rs` 渲染错误页。
    ///
    /// 这一步只做"读"，不碰界面——所以它能在窗口里、也能在窗口外调用。
    pub fn bootstrap() -> Result<Bootstrap, StartupFailure> {
        // 先把 tokio 运行时建起来：它平时是懒加载的，但建不起来的话后面每次网络操作
        // 都会崩在 `runtime()` 里，那时候连错误页都来不及显示。
        preflight_runtime().map_err(StartupFailure::Runtime)?;
        let mut storage = StorageData::try_load_or_init().map_err(StartupFailure::Storage)?;
        let config = AppConfig::try_load().map_err(StartupFailure::Config)?;
        // 两处一次性回填，都是「老数据要跟上新模型」：
        // ① 会话没记渠道 id 的，按当前默认渠道补上；
        // ② 老格式的会话工具状态（`enabled` + `picked`）展开成「模式 + 来源」。
        //    这一步**必须在配置读完之后**：老数据里 `picked: null` 表示「当时能用的全带」，
        //    要知道配置里有哪些服务器才展开得出来，所以只能放在这里，不能塞进反序列化。
        // 用 `|` 而不是 `||`：两处都要跑，不能短路。
        let migrated = backfill_session_providers(&mut storage, &config)
            | crate::tool_ops::migrate_legacy_tool_state(&mut storage.sessions, &config);
        if migrated {
            storage
                .save()
                .map_err(|error| StartupFailure::Storage(error.to_string()))?;
        }
        Ok(Bootstrap { storage, config })
    }

    pub fn new(bootstrap: Bootstrap, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let Bootstrap { storage, config } = bootstrap;
        let is_dark = config.is_dark;
        apply_theme(is_dark, Some(window), cx);
        let lang = AppLanguage::from_str(&config.language);
        apply_locale(lang);
        set_current(cx, lang);
        // 审计日志的开关是全局的（理由见 `audit.rs` 里的静态量注释），启动时同步一次；
        // 用户改设置时由设置页再同步。
        crate::audit::set_enabled(config.audit_log_enabled);

        let selected_provider_id = config.active_provider_id.clone();
        let initial_api_key = config.get_active_api_key();
        let initial_base_url = config.get_active_base_url();

        let chat_input = cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 8).submit_on_enter(true));

        let search_session_input = cx.new(|cx| InputState::new(window, cx));
        let rename_input = cx.new(|cx| InputState::new(window, cx));

        let cfg_api_key_input = cx.new(|cx| {
            let mut inp = InputState::new(window, cx).masked(true).placeholder("sk-...");
            inp.set_value(&initial_api_key, window, cx);
            inp
        });
        let cfg_base_url_input = cx.new(|cx| {
            let mut inp = InputState::new(window, cx).placeholder("https://api.openai.com/v1");
            inp.set_value(&initial_base_url, window, cx);
            inp
        });
        let cfg_search_provider_input = cx.new(|cx| InputState::new(window, cx));
        let model_picker_search_input = cx.new(|cx| InputState::new(window, cx));

        let sys_prompt = config.system_prompt.clone();
        let cfg_system_prompt_input = cx.new(|cx| {
            let mut inp = TextareaState::new(window, cx).auto_grow(3, 12);
            inp.set_value(&sys_prompt, window, cx);
            inp
        });

        let new_provider_name_input = cx.new(|cx| {
            let mut inp = InputState::new(window, cx);
            inp.set_value(ChannelType::OpenAiChat.label(), window, cx);
            inp
        });
        let new_provider_base_url_input = cx.new(|cx| {
            let mut inp = InputState::new(window, cx).placeholder("https://api.openai.com/v1");
            inp.set_value(ChannelType::OpenAiChat.default_base_url(), window, cx);
            inp
        });
        let new_provider_api_key_input = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("sk-..."));

        let model_edit_id_input = cx.new(|cx| InputState::new(window, cx));
        let model_edit_name_input = cx.new(|cx| InputState::new(window, cx));
        let model_edit_context_input = cx.new(|cx| InputState::new(window, cx));
        let model_edit_output_input = cx.new(|cx| InputState::new(window, cx));

        let (message_list_session, message_count) = storage
            .get_active_session()
            .map(|s| (s.id.clone(), s.messages.len()))
            .unwrap_or_default();
        let message_list = cx.new(|cx| MessageScrollerState::new(message_count, cx));

        let initial_prompt = storage
            .get_active_session()
            .and_then(|session| session.params.as_ref())
            .and_then(|params| params.system_prompt.clone())
            .unwrap_or_default();
        let params_prompt_input = cx.new(|cx| {
            let mut input = TextareaState::new(window, cx).auto_grow(2, 6);
            input.set_value(&initial_prompt, window, cx);
            input
        });
        let edit_message_input = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 10));
        let cfg_proxy_input = cx.new(|cx| InputState::new(window, cx).placeholder("http://127.0.0.1:7890"));
        let cfg_timeout_input = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder("90");
            input.set_value("90", window, cx);
            input
        });
        let cfg_retries_input = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder("0");
            input.set_value("0", window, cx);
            input
        });
        let cfg_headers_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 6)
                .placeholder("X-Title: Perch")
        });
        let prompt_name_input = cx.new(|cx| InputState::new(window, cx));
        let prompt_icon_input = cx.new(|cx| InputState::new(window, cx));
        let prompt_body_input = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 8));
        let folder_name_input = cx.new(|cx| InputState::new(window, cx));
        let model_fetch_search = cx.new(|cx| InputState::new(window, cx));
        let prompts = PromptLibrary::load(lang);
        let skills = SkillCatalog::reload();

        let subscriptions = vec![
            cx.subscribe_in(&chat_input, window, |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { shift: false, .. } = event {
                    if !this.apply_slash_template(window, cx) {
                        this.send_message(window, cx);
                    }
                } else if let InputEvent::Change = event {
                    // 粘贴已由输入框的 on_paste 钩子接管（见 ui/chat.rs），这里只需要重绘
                    cx.notify();
                }
            }),
            cx.subscribe_in(&model_fetch_search, window, |this, input, event: &InputEvent, _, cx| {
                if let InputEvent::Change = event {
                    this.model_fetch_query = input.read(cx).value().to_string();
                    cx.notify();
                }
            }),
            cx.subscribe_in(&params_prompt_input, window, |this, _, event: &InputEvent, _, cx| {
                if let InputEvent::Change = event {
                    let value = this.params_prompt_input.read(cx).value().to_string();
                    this.set_session_system_prompt(value, cx);
                }
            }),
            cx.subscribe_in(&model_edit_id_input, window, |this, _, event: &InputEvent, _, cx| {
                if let InputEvent::Change = event {
                    this.sync_model_draft_id(cx);
                }
            }),
            cx.subscribe_in(
                &model_edit_context_input,
                window,
                |this, _, event: &InputEvent, _, cx| {
                    if let InputEvent::Change = event {
                        this.sync_model_draft_tokens(TokenField::Context, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &model_edit_output_input,
                window,
                |this, _, event: &InputEvent, _, cx| {
                    if let InputEvent::Change = event {
                        this.sync_model_draft_tokens(TokenField::Output, cx);
                    }
                },
            ),
        ];

        chat_input.update(cx, |input, cx| input.focus(window, cx));

        let mut state = Self {
            storage,
            config,
            view_mode: ViewMode::Chat,
            settings_tab: SettingsTab::General,
            selected_settings_provider_id: selected_provider_id,
            is_dark,
            sidebar_collapsed: false,
            analytics_range: crate::analytics::TimeRange::Days30,
            analytics_tab: crate::analytics::AnalyticsTab::Overview,
            add_channel_type: ChannelType::OpenAiChat,
            rename_target_session_id: None,
            agent: AgentState::default(),
            mcp: McpState::default(),
            is_streaming: false,
            active_streams: HashMap::new(),
            pending_toasts: Vec::new(),
            prompts,
            skills,
            folder_filter: String::new(),
            favorites_only: false,
            pending_quote: None,
            pending_attachments: Vec::new(),
            pending_import: None,
            compare_selection: Vec::new(),
            edit_message_id: None,
            folder_target_id: None,
            prompt_edit_id: None,
            pending_models: Vec::new(),
            pending_model_selection: HashSet::new(),
            model_fetch_query: String::new(),
            open_model_picker: false,
            expanded_reasoning: HashSet::new(),
            chat_input,
            search_session_input,
            sidebar_search_query: String::new(),
            sidebar_search_revision: 0,
            sidebar_search_ids: HashSet::new(),
            rename_input,
            cfg_api_key_input,
            model_fetch_search,
            cfg_base_url_input,
            cfg_search_provider_input,
            model_picker_search_input,
            cfg_system_prompt_input,
            new_provider_name_input,
            new_provider_base_url_input,
            new_provider_api_key_input,
            model_editor: None,
            model_edit_id_input,
            model_edit_name_input,
            model_edit_context_input,
            model_edit_output_input,
            edit_message_input,
            params_prompt_input,
            cfg_proxy_input,
            cfg_timeout_input,
            cfg_retries_input,
            cfg_headers_input,
            prompt_name_input,
            prompt_icon_input,
            prompt_body_input,
            folder_name_input,
            message_list,
            message_list_session,
            scroll_to_end_pending: false,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        // 旧数据迁移是在 data_dir() 里做的，那会儿还没有 AppState，失败信息先攒着，
        // 到这里才弹得出来。迁移失败会让程序当成全新安装，必须让用户看到。
        for failure in crate::paths::take_migration_failures() {
            state.toast(
                ToastLevel::Error,
                tr_args(lang, Key::MigrationFailed, &[&failure.message(lang)]),
            );
        }
        state.refresh_placeholders(window, cx);
        state
    }

    pub fn language(&self) -> AppLanguage {
        AppLanguage::from_str(&self.config.language)
    }

    /// 所有带**本地化占位符**的单行输入框。
    ///
    /// 占位符是界面文案，而 `InputState` 把它存成状态，所以切换语言时要重设一遍。
    /// 集中在这里定义，`AppState::new` 与 [`Self::refresh_placeholders`] 共用一份，
    /// 加新的本地化占位符只要往这里加一行，不会出现「创建处改了、切换处忘了」。
    /// 写死内容的占位符（`sk-...`、`https://api.openai.com/v1` 之类）不进这个列表，
    /// 它们本来就不随语言变。
    fn localized_inputs(&self) -> Vec<(&Entity<InputState>, Key)> {
        vec![
            (&self.search_session_input, Key::SearchChat),
            (&self.rename_input, Key::PhSessionName),
            (&self.cfg_search_provider_input, Key::PhSearchProvider),
            (&self.model_picker_search_input, Key::PhSearchModelOrProvider),
            (&self.new_provider_name_input, Key::PhProviderName),
            (&self.model_edit_id_input, Key::PhModelId),
            (&self.model_edit_name_input, Key::PhModelDisplayName),
            (&self.model_edit_context_input, Key::PhCustomContextLimit),
            (&self.model_edit_output_input, Key::PhCustomOutputLimit),
            (&self.prompt_name_input, Key::PhName),
            (&self.prompt_icon_input, Key::PhIcon),
            (&self.folder_name_input, Key::FolderName),
            (&self.model_fetch_search, Key::PhModelSearch),
        ]
    }

    /// 同上，多行输入框。
    fn localized_textareas(&self) -> Vec<(&Entity<TextareaState>, Key)> {
        vec![
            (&self.chat_input, Key::InputPlaceholder),
            (&self.cfg_system_prompt_input, Key::PhSystemPrompt),
            (&self.params_prompt_input, Key::PhSessionSystemPrompt),
            (&self.prompt_body_input, Key::PhTemplateVars),
        ]
    }

    /// 按当前语言重设所有本地化占位符。启动时与切换语言时各调一次。
    fn refresh_placeholders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let lang = self.language();
        for (input, key) in self.localized_inputs() {
            input.update(cx, |input, cx| {
                input.set_placeholder(tr(lang, key), window, cx);
            });
        }
        for (input, key) in self.localized_textareas() {
            input.update(cx, |input, cx| {
                input.set_placeholder(tr(lang, key), window, cx);
            });
        }
    }

    // ================= 提示 =================

    pub fn toast(&mut self, level: ToastLevel, msg: impl Into<String>) {
        self.pending_toasts.push((level, msg.into()));
    }

    pub(crate) fn persist_storage(&mut self, cx: &mut Context<Self>) {
        let lang = self.language();
        if let Err(error) = self.storage.save() {
            self.toast(
                ToastLevel::Error,
                tr_args(lang, Key::ChatSaveFailed, &[&error.to_string()]),
            );
            cx.notify();
        }
    }

    /// 保存配置。返回是否成功，调用方要弹成功提示时用它决定。
    /// §6：保存失败必须让用户看到，不能 `let _ = self.config.save()` 一吞了事。
    pub(crate) fn persist_config(&mut self, cx: &mut Context<Self>) -> bool {
        let lang = self.language();
        match self.config.save() {
            Ok(()) => true,
            Err(error) => {
                self.toast(
                    ToastLevel::Error,
                    tr_args(lang, Key::ConfigSaveFailed, &[&error.to_string()]),
                );
                cx.notify();
                false
            }
        }
    }

    /// 业务方法里只有 `Context`，拿不到 `Window`，所以先排队，渲染时再统一弹出。
    pub fn take_toasts(&mut self) -> Vec<(ToastLevel, String)> {
        std::mem::take(&mut self.pending_toasts)
    }

    // ================= 视图切换 =================

    pub fn open_settings(&mut self, tab: SettingsTab, window: &mut Window, cx: &mut Context<Self>) {
        self.view_mode = ViewMode::Settings;
        self.settings_tab = tab;
        // 对话输入框在设置页不渲染，把焦点移到根节点上，Esc 才能返回对话
        self.focus_handle.focus(window, cx);
        if tab == SettingsTab::Providers {
            self.ensure_settings_provider_selected(window, cx);
        }
        cx.notify();
    }

    pub fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.view_mode = ViewMode::Chat;
        self.chat_input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.view_mode == ViewMode::Settings {
            self.close_settings(window, cx);
        } else {
            self.open_settings(self.settings_tab, window, cx);
        }
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        cx.notify();
    }

    pub fn toggle_local_tools(&mut self, cx: &mut Context<Self>) {
        let lang = self.language();
        self.config.local_tools_enabled = !self.config.local_tools_enabled;
        if !self.config.local_tools_enabled {
            // 关掉开关就把挂着的授权请求一并撤销——否则卡片还留着，
            // 用户点同意会走到"工具未启用"的分支，看起来像点了个空按钮。
            // 正在执行的工具不打断：它是用户已经同意过的，让它跑完。
            self.agent.pending = None;
            self.agent.origin = None;
            self.agent.round = 0;
        }
        if let Err(error) = self.config.save() {
            self.toast(
                ToastLevel::Error,
                tr_args(lang, Key::LocalToolSettingsSaveFailed, &[&error.to_string()]),
            );
        }
        cx.notify();
    }

    pub fn set_dark_mode(&mut self, is_dark: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.is_dark = is_dark;
        self.config.is_dark = is_dark;
        self.persist_config(cx);
        apply_theme(is_dark, Some(window), cx);
        cx.notify();
    }

    pub fn toggle_reasoning(&mut self, message_id: &str, cx: &mut Context<Self>) {
        if !self.expanded_reasoning.remove(message_id) {
            self.expanded_reasoning.insert(message_id.to_string());
        }
        cx.notify();
    }

    // ================= 会话 =================

    /// 让消息列表的行数与当前会话保持一致；在渲染前调用。
    pub fn sync_message_list(&mut self, cx: &mut Context<Self>) {
        let (session_id, count) = self
            .storage
            .get_active_session()
            .map(|s| (s.id.clone(), s.messages.len()))
            .unwrap_or_default();
        let session_changed = session_id != self.message_list_session;
        self.message_list_session = session_id;
        let scroll_to_end = std::mem::take(&mut self.scroll_to_end_pending);

        self.message_list.update(cx, |list, cx| {
            let current = list.item_count();
            if session_changed || count < current {
                list.reset(count, cx);
            } else if count > current {
                list.append(count - current, cx);
            }
            if scroll_to_end {
                list.scroll_to_end(cx);
            }
        });
    }

    // ================= 对话 =================

    pub(crate) fn find_message_mut(&mut self, id: &str) -> Option<&mut ChatMessage> {
        self.storage
            .sessions
            .iter_mut()
            .flat_map(|s| s.messages.iter_mut())
            .find(|m| m.id == id)
    }

    /// 把快捷指令填入输入框（例如 "/bash "），方便用户继续补全
    pub fn fill_chat_input(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.chat_input.update(cx, |i, cx| {
            // 多行输入框的 set_value 会把光标放回开头，用 insert 让光标停在末尾
            i.set_value("", window, cx);
            i.insert(text, window, cx);
            i.focus(window, cx);
        });
    }

    pub fn copy_to_clipboard(&mut self, text: &str, cx: &mut Context<Self>) {
        let lang = self.language();
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        self.toast(ToastLevel::Success, tr(lang, Key::CopiedToClipboard));
        cx.notify();
    }

    pub fn switch_language(&mut self, lang: AppLanguage, window: &mut Window, cx: &mut Context<Self>) {
        self.config.language = lang.as_str().to_string();
        let saved = self.persist_config(cx);
        apply_locale(lang);
        // 先同步全局再弹提示，否则「已切换为 xx」这条提示还是旧语言
        set_current(cx, lang);
        self.refresh_placeholders(window, cx);
        // 图片客户端是长生命周期对象，拿不到 `App`，语言在构造时就定下来了
        // （见 `image_http::ImageHttpClient`），所以换语言要重建一个。
        cx.set_http_client(crate::image_http::client_for_config(&self.config));
        // 存不下来就别报"已切换"，免得用户以为下次启动还是这个语言
        if saved {
            self.toast(ToastLevel::Success, tr(lang, Key::LangSwitched));
        }
        cx.notify();
    }

    pub fn set_temperature(&mut self, temp: f32, cx: &mut Context<Self>) {
        self.config.temperature = temp;
        self.persist_config(cx);
        cx.notify();
    }

    /// 改本地命令的超时。正在跑的命令不受影响——它拿到的是开始执行那一刻的快照。
    pub fn set_command_timeout(&mut self, secs: u64, cx: &mut Context<Self>) {
        self.config.command_timeout_secs = secs;
        self.persist_config(cx);
        cx.notify();
    }

    pub fn save_system_prompt(&mut self, cx: &mut Context<Self>) {
        let lang = self.language();
        let prompt = self.cfg_system_prompt_input.read(cx).value().trim().to_string();
        self.config.system_prompt = prompt;
        if self.persist_config(cx) {
            self.toast(ToastLevel::Success, tr(lang, Key::SystemPromptSaved));
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    // `#[std::prelude::v1::test]` 而不是 `#[test]`：`gpui_kit::*` 带进来了一个同名的
    // `test` 属性宏，会顶掉内置的，展开时自我递归。见 AGENTS.md §10 第 9 条。
    use super::*;

    /// 启动失败页要靠这几条文案把"哪里坏了"讲清楚，所以四种语言都得有实际内容，
    /// 而且都要带上底层的原因（不然用户只看到一句"启动失败"，无从下手）。
    #[std::prelude::v1::test]
    fn startup_failures_explain_themselves_in_every_language() {
        let cases = [
            StartupFailure::Runtime("no threads".into()),
            StartupFailure::Config("bad json".into()),
            StartupFailure::Storage("file is not a database".into()),
        ];
        for failure in cases {
            for lang in [
                AppLanguage::ZhCn,
                AppLanguage::EnUs,
                AppLanguage::JaJp,
                AppLanguage::ZhTw,
            ] {
                let message = failure.message(lang);
                assert!(!message.is_empty(), "{failure:?} 在 {lang:?} 下是空的");
                assert!(
                    message.contains("no threads")
                        || message.contains("bad json")
                        || message.contains("file is not a database"),
                    "{failure:?} 在 {lang:?} 下没带上底层原因：{message}"
                );
            }
        }
    }

    /// 三种失败不能长一个样——用户和开发者都要能一眼分出是配置、会话库还是运行时的问题。
    #[std::prelude::v1::test]
    fn startup_failures_read_differently_from_each_other() {
        let lang = AppLanguage::ZhCn;
        let messages = [
            StartupFailure::Runtime("x".into()).message(lang),
            StartupFailure::Config("x".into()).message(lang),
            StartupFailure::Storage("x".into()).message(lang),
        ];
        let unique: std::collections::HashSet<_> = messages.iter().collect();
        assert_eq!(unique.len(), messages.len(), "三种失败的文案重复了：{messages:?}");
    }
}
