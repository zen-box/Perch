use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::message_scroller::MessageScrollerState;
use gpui_kit::*;
use tokio::sync::oneshot;

use crate::agent::execute_local_tool;
use crate::backup::BackupFile;
use crate::config::{AppConfig, ChannelType};
use crate::i18n::{AppLanguage, Key, apply_locale, set_current, tr};
use crate::model::{Attachment, ChatMessage, StorageData};
use crate::model_ops::{ModelEditor, TokenField};
use crate::prompts::PromptLibrary;
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
    McpServers,
    About,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastLevel {
    Info,
    Success,
    Error,
}

/// reqwest 与流式请求依赖 tokio，而 GPUI 自己的执行器不是 tokio，
/// 直接在 `cx.spawn` 里调用 `tokio::spawn` 会 panic，所以单独起一个运行时。
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to start tokio runtime")
    })
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

    // Agent 权限确认卡片
    pub pending_tool_name: Option<String>,
    pub pending_tool_cmd: Option<String>,

    pub is_streaming: bool,
    pub(crate) active_streams: HashMap<String, oneshot::Sender<()>>,
    pending_toasts: Vec<(ToastLevel, String)>,
    pub prompts: PromptLibrary,
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

impl AppState {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut storage = StorageData::load_or_init();
        let config = AppConfig::load();
        let (default_provider_id, default_model) = config.default_model_selection();
        let mut session_selection_changed = false;
        for session in &mut storage.sessions {
            if session.provider_id.is_empty() {
                if let Some(provider) = config
                    .providers
                    .iter()
                    .find(|provider| provider.models.iter().any(|model| model.id == session.model))
                {
                    session.provider_id = provider.id.clone();
                    session_selection_changed = true;
                } else if !default_provider_id.is_empty() {
                    session.provider_id = default_provider_id.clone();
                    session.model = default_model.clone();
                    session_selection_changed = true;
                }
            }
        }
        if session_selection_changed {
            storage
                .save()
                .unwrap_or_else(|error| panic!("Unable to update chat model selection: {error}"));
        }
        let is_dark = config.is_dark;
        apply_theme(is_dark, Some(window), cx);
        let lang = AppLanguage::from_str(&config.language);
        apply_locale(lang);
        set_current(cx, lang);

        let selected_provider_id = config.active_provider_id.clone();
        let initial_api_key = config.get_active_api_key();
        let initial_base_url = config.get_active_base_url();

        let chat_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 8)
                .submit_on_enter(true)
                .placeholder(tr(lang, Key::InputPlaceholder))
        });

        let search_session_input = cx.new(|cx| InputState::new(window, cx).placeholder(tr(lang, Key::SearchChat)));
        let rename_input = cx.new(|cx| InputState::new(window, cx).placeholder("输入新的对话名称"));

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
        let cfg_search_provider_input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索渠道"));
        let model_picker_search_input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索模型或渠道"));

        let sys_prompt = config.system_prompt.clone();
        let cfg_system_prompt_input = cx.new(|cx| {
            let mut inp = TextareaState::new(window, cx)
                .auto_grow(3, 12)
                .placeholder("例如：你是一名资深的 Rust 工程师，回答简洁并给出可运行的示例。");
            inp.set_value(&sys_prompt, window, cx);
            inp
        });

        let new_provider_name_input = cx.new(|cx| {
            let mut inp = InputState::new(window, cx).placeholder("例如：OpenAI 官方 / DeepSeek / 公司代理");
            inp.set_value(ChannelType::OpenAiChat.label(), window, cx);
            inp
        });
        let new_provider_base_url_input = cx.new(|cx| {
            let mut inp = InputState::new(window, cx).placeholder("https://api.openai.com/v1");
            inp.set_value(ChannelType::OpenAiChat.default_base_url(), window, cx);
            inp
        });
        let new_provider_api_key_input = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("sk-..."));

        let model_edit_id_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("调用接口时使用的名字，例如 claude-sonnet-4-5"));
        let model_edit_name_input = cx.new(|cx| InputState::new(window, cx).placeholder("留空则显示模型 ID"));
        let model_edit_context_input = cx.new(|cx| InputState::new(window, cx).placeholder("自定义，如 128K"));
        let model_edit_output_input = cx.new(|cx| InputState::new(window, cx).placeholder("自定义，如 64K"));

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
            let mut input = TextareaState::new(window, cx)
                .auto_grow(2, 6)
                .placeholder("留空则使用全局系统提示词");
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
        let prompt_name_input = cx.new(|cx| InputState::new(window, cx).placeholder("名称"));
        let prompt_icon_input = cx.new(|cx| InputState::new(window, cx).placeholder("例如：✨"));
        let prompt_body_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 8)
                .placeholder("支持 {{date}} {{clipboard}} {{selection}}")
        });
        let folder_name_input = cx.new(|cx| InputState::new(window, cx).placeholder("文件夹名称"));
        let model_fetch_search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索模型 ID 或名称"));
        let prompts = PromptLibrary::load();

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
            pending_tool_name: None,
            pending_tool_cmd: None,
            is_streaming: false,
            active_streams: HashMap::new(),
            pending_toasts: Vec::new(),
            prompts,
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
        for message in crate::paths::take_migration_failures() {
            state.toast(
                ToastLevel::Error,
                format!("旧数据迁移失败，可能读不到历史数据：{message}"),
            );
        }
        state
    }

    pub fn language(&self) -> AppLanguage {
        AppLanguage::from_str(&self.config.language)
    }

    // ================= 提示 =================

    pub fn toast(&mut self, level: ToastLevel, msg: impl Into<String>) {
        self.pending_toasts.push((level, msg.into()));
    }

    pub(crate) fn persist_storage(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = self.storage.save() {
            self.toast(ToastLevel::Error, format!("对话保存失败: {error}"));
            cx.notify();
        }
    }

    /// 保存配置。返回是否成功，调用方要弹成功提示时用它决定。
    /// §6：保存失败必须让用户看到，不能 `let _ = self.config.save()` 一吞了事。
    pub(crate) fn persist_config(&mut self, cx: &mut Context<Self>) -> bool {
        match self.config.save() {
            Ok(()) => true,
            Err(error) => {
                self.toast(ToastLevel::Error, format!("配置保存失败: {error}"));
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
        self.config.local_tools_enabled = !self.config.local_tools_enabled;
        if !self.config.local_tools_enabled {
            self.pending_tool_name = None;
            self.pending_tool_cmd = None;
        }
        if let Err(error) = self.config.save() {
            self.toast(ToastLevel::Error, format!("本地工具设置保存失败: {error}"));
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

    pub fn execute_agent_tool(&mut self, tool_name: &str, arg: &str, cx: &mut Context<Self>) {
        if !self.config.local_tools_enabled {
            self.pending_tool_name = None;
            self.pending_tool_cmd = None;
            self.toast(ToastLevel::Error, "本地工具未启用");
            cx.notify();
            return;
        }
        // 权限卡片里展示的是 "Bash"，本地工具名是小写的 "bash"
        let result = execute_local_tool(&tool_name.to_ascii_lowercase(), arg);
        let active_id = self.storage.active_session_id.clone();
        if let Some(session) = self.storage.sessions.iter_mut().find(|s| s.id == active_id) {
            let mut assistant_msg = ChatMessage::new_assistant();
            assistant_msg.is_streaming = false;
            assistant_msg.tool_calls.push(result.display.clone());

            let formatted_content = if result.is_error {
                format!("**工具执行失败**:\n```text\n{}\n```", result.output)
            } else {
                format!("**工具执行成功**:\n```text\n{}\n```", result.output)
            };
            assistant_msg.content = formatted_content;
            session.messages.push(assistant_msg);
        }
        self.persist_storage(cx);
        self.pending_tool_name = None;
        self.pending_tool_cmd = None;
        self.scroll_to_end_pending = true;
        cx.notify();
    }

    pub fn deny_pending_tool(&mut self, cx: &mut Context<Self>) {
        self.pending_tool_name = None;
        self.pending_tool_cmd = None;
        cx.notify();
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
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        self.toast(ToastLevel::Success, "已复制到剪贴板");
        cx.notify();
    }

    pub fn switch_language(&mut self, lang: AppLanguage, window: &mut Window, cx: &mut Context<Self>) {
        self.config.language = lang.as_str().to_string();
        let saved = self.persist_config(cx);
        apply_locale(lang);
        // 先同步全局再弹提示，否则「已切换为 xx」这条提示还是旧语言
        set_current(cx, lang);
        self.chat_input.update(cx, |i, cx| {
            i.set_placeholder(tr(lang, Key::InputPlaceholder), window, cx)
        });
        self.search_session_input
            .update(cx, |i, cx| i.set_placeholder(tr(lang, Key::SearchChat), window, cx));
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

    pub fn save_system_prompt(&mut self, cx: &mut Context<Self>) {
        let prompt = self.cfg_system_prompt_input.read(cx).value().trim().to_string();
        self.config.system_prompt = prompt;
        if self.persist_config(cx) {
            self.toast(ToastLevel::Success, "系统提示词已保存");
        }
        cx.notify();
    }
}
