//! MCP 服务器的运行时状态：连接、工具清单、连接状态。
//!
//! 服务层（`mcp.rs`）只回答「怎么连、怎么调」；这一层管的是**这些东西在应用里活多久、
//! 放在哪、界面怎么读**：连接挂在这台服务器上，直到用户断开或者程序退出；工具清单
//! 跟着连接一起更新；连不上的原因留在状态里给设置页显示。
//!
//! 全部状态集中成一个字段塞进 `AppState`（§4.1），不往顶层散加。
//!
//! **总开关**：MCP 工具和本机那 5 个工具共用设置里的「本地工具」开关。
//! 两个开关（总开关 + 每台服务器的开关）已经够绕了，再来第三个「MCP 总开关」
//! 只会让"为什么模型不调工具"更难查。设置页在总开关关着时会明确提示这一点。

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::*;

use crate::app::{AppState, ToastLevel, runtime, update_state};
use crate::config::{self, McpServerConfig, McpTransport};
use crate::i18n::{Key, tr, tr_args};
use crate::llm_tools::ToolSpec;
use crate::mcp::{self, Connection, ExposedTool};

/// 一台服务器当前的连接状态。设置页照它渲染。
#[derive(Clone, Debug, PartialEq)]
pub enum ServerStatus {
    /// 没连：用户停用了，或者还没轮到它
    Idle,
    /// 正在启动子进程、握手
    Connecting,
    /// 连上了，工具清单也拿到了
    Ready,
    /// 连不上。里面是**技术细节原文**（io 错误、协议错误），不带界面文案——
    /// 措辞和语言由设置页决定。
    Failed(String),
}

impl ServerStatus {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// 一条调用该走哪条路。执行器（`agent_loop.rs`）按它分派。
pub(crate) enum Route {
    /// 本机工具。也包括模型编出来的名字：本地执行器会回一句「没有这个工具」
    /// 并把可用清单列给它，比专门为这种情况写一条分支省事。
    Local,
    /// 发给某台 MCP 服务器。`raw` 是**服务器给的原始工具名**，不是暴露给模型的那个。
    Mcp { connection: Arc<Connection>, raw: String },
}

/// MCP 的全部运行时状态。
#[derive(Default)]
pub struct McpState {
    /// 活着的连接。用 `Arc` 是因为后台任务要在 await 期间拿着它——一次调用可能跑
    /// 几十秒，期间用户完全可能把这台服务器删掉，那时连接得由最后一个持有者收掉。
    connections: HashMap<String, Arc<Connection>>,
    /// 服务器 id → 连接状态
    status: HashMap<String, ServerStatus>,
    /// 服务器 id → 它暴露出来的工具。
    ///
    /// 交给模型的顺序由 `config.mcp_servers` 决定，**不靠 HashMap 的遍历顺序**：
    /// 工具清单的顺序一变，同一个会话序列化出来的请求体就变，prompt 缓存全失效。
    tools: HashMap<String, Vec<ExposedTool>>,
    /// 设置页里正在看哪台服务器的详情
    pub selected_server_id: Option<String>,
    /// 正在编辑的服务器。`None` 表示没在编辑。
    pub editor: Option<McpEditor>,
}

/// 编辑中的服务器草稿。
///
/// 输入框**懒创建**：只有打开编辑器时才建（建 `Entity` 要窗口），关掉就丢掉。
/// 放进 `McpState` 而不是 `AppState` 顶层，是因为它只在设置页里活着（§4.1）。
pub struct McpEditor {
    /// 正在编辑哪台服务器；`None` 表示新增
    pub editing_id: Option<String>,
    pub name: Entity<InputState>,
    pub command: Entity<InputState>,
    pub args: Entity<TextareaState>,
    pub cwd: Entity<InputState>,
    pub env: Entity<TextareaState>,
}

impl McpState {
    /// 交给模型的工具清单：按配置里的服务器顺序排，跳过停用的服务器和单独停用的工具。
    pub fn specs(&self, servers: &[McpServerConfig]) -> Vec<ToolSpec> {
        servers
            .iter()
            .filter(|server| server.enabled)
            .filter_map(|server| self.tools.get(&server.id).map(|tools| (server, tools)))
            .flat_map(|(server, tools)| {
                tools
                    .iter()
                    .filter(|tool| !server.disabled_tools.contains(&tool.raw))
                    .map(ExposedTool::spec)
            })
            .collect()
    }

    /// 按暴露名找工具：它属于哪台服务器、服务器给的原始定义是什么。
    pub fn locate(&self, exposed: &str) -> Option<(&str, &ExposedTool)> {
        self.tools.iter().find_map(|(server_id, tools)| {
            tools
                .iter()
                .find(|tool| tool.exposed == exposed)
                .map(|tool| (server_id.as_str(), tool))
        })
    }

    pub fn status(&self, server_id: &str) -> ServerStatus {
        self.status.get(server_id).cloned().unwrap_or(ServerStatus::Idle)
    }

    pub fn tools_of(&self, server_id: &str) -> &[ExposedTool] {
        self.tools.get(server_id).map(Vec::as_slice).unwrap_or_default()
    }

    pub fn connection(&self, server_id: &str) -> Option<Arc<Connection>> {
        self.connections.get(server_id).cloned()
    }

    /// 服务器 stderr 的最近若干行。没连上时是空的。
    pub fn logs_of(&self, server_id: &str) -> Vec<String> {
        self.connections
            .get(server_id)
            .map(|connection| connection.logs())
            .unwrap_or_default()
    }

    /// 断开一台服务器并清掉它的工具清单。**配置不动**——用户只是停用，不是删除。
    fn drop_server(&mut self, server_id: &str) {
        self.connections.remove(server_id);
        self.tools.remove(server_id);
        self.status.insert(server_id.to_string(), ServerStatus::Idle);
    }
}

impl AppState {
    /// 启动时把所有启用的服务器连起来。
    ///
    /// 连不上的**只记状态、不弹提示**：启动瞬间弹一串错误没有意义，用户还没进设置页；
    /// 设置页里那台服务器会显示「连接失败」和原因。
    pub fn connect_mcp_servers(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<String> = self
            .config
            .mcp_servers
            .iter()
            .filter(|server| server.enabled)
            .map(|server| server.id.clone())
            .collect();
        for id in ids {
            self.connect_mcp_server(&id, cx);
        }
    }

    /// 连一台服务器（重连也走这里）。已经在连的直接返回，免得连点两下起两个子进程。
    pub fn connect_mcp_server(&mut self, server_id: &str, cx: &mut Context<Self>) {
        if self.mcp.status(server_id) == ServerStatus::Connecting {
            return;
        }
        let Some(server) = self
            .config
            .mcp_servers
            .iter()
            .find(|server| server.id == server_id)
            .filter(|server| server.enabled)
            .cloned()
        else {
            return;
        };
        // 换一台新的：旧连接在这里被丢掉，`Connection::drop` 会把整棵进程树收掉
        self.mcp.drop_server(server_id);
        self.mcp.status.insert(server_id.to_string(), ServerStatus::Connecting);
        cx.notify();

        let id = server_id.to_string();
        cx.spawn(async move |this, cx| {
            // 起子进程和握手都在 tokio 运行时里做：GPUI 自己的执行器不是 tokio，
            // 在 `cx.spawn` 的 future 里直接跑 tokio 的东西会 panic（§7）。
            let result = runtime()
                .spawn(async move {
                    let connection = Arc::new(Connection::connect(&server).await?);
                    let tools = connection.list_tools().await?;
                    Ok::<_, String>((connection, tools))
                })
                .await;
            update_state(&this, cx, |state, cx| match result {
                Ok(Ok((connection, tools))) => state.mcp_connected(&id, connection, tools, cx),
                Ok(Err(error)) => state.mcp_failed(&id, error, cx),
                // 后台任务自己 panic 或被取消：当成连接失败，别让界面一直转圈
                Err(error) => state.mcp_failed(&id, error.to_string(), cx),
            });
        })
        .detach();
    }

    /// 断开一台服务器（配置留着）。
    pub fn disconnect_mcp_server(&mut self, server_id: &str, cx: &mut Context<Self>) {
        self.mcp.drop_server(server_id);
        cx.notify();
    }

    fn mcp_connected(
        &mut self,
        server_id: &str,
        connection: Arc<Connection>,
        tools: Vec<rmcp::model::Tool>,
        cx: &mut Context<Self>,
    ) {
        // 连接期间用户可能把这台服务器停用或删掉了。这时直接丢掉连接
        // （`Arc` 最后一个持有者析构，进程树跟着收掉），不要挂上去。
        if !self
            .config
            .mcp_servers
            .iter()
            .any(|server| server.id == server_id && server.enabled)
        {
            return;
        }
        let exposed = mcp::expose_tools(server_id, &tools);
        self.mcp.tools.insert(server_id.to_string(), exposed);
        self.mcp.connections.insert(server_id.to_string(), connection);
        self.mcp.status.insert(server_id.to_string(), ServerStatus::Ready);
        cx.notify();
    }

    fn mcp_failed(&mut self, server_id: &str, error: String, cx: &mut Context<Self>) {
        // 失败的原因可能很长（协议错误里带着整帧），留一句够看的
        let error = local_tools_truncate(&error);
        self.mcp
            .status
            .insert(server_id.to_string(), ServerStatus::Failed(error));
        cx.notify();
    }

    /// 这条调用该发给谁。
    pub(crate) fn route_tool(&self, exposed: &str) -> Route {
        let Some((server_id, tool)) = self.mcp.locate(exposed) else {
            return Route::Local;
        };
        // 服务器被停用/删掉、或者这个工具被单独停用：按「不认识」处理。
        // 本地执行器会回一句可用工具清单，模型下一轮就知道该换一个了。
        let usable = self
            .config
            .mcp_servers
            .iter()
            .any(|server| server.id == server_id && server.enabled && !server.disabled_tools.contains(&tool.raw));
        match (usable, self.mcp.connection(server_id)) {
            (true, Some(connection)) => Route::Mcp {
                connection,
                raw: tool.raw.clone(),
            },
            _ => Route::Local,
        }
    }
}

/// 失败原因太长时从中间截掉，和工具结果的截断规则保持一致。
fn local_tools_truncate(text: &str) -> String {
    crate::local_tools::truncate_middle(text, 2_000)
}

impl AppState {
    /// 设置页里选中 / 取消选中一台服务器。
    pub fn select_mcp_server(&mut self, server_id: Option<&str>, cx: &mut Context<Self>) {
        self.mcp.selected_server_id = server_id.map(str::to_string);
        cx.notify();
    }

    /// 打开编辑器。`server_id` 为 `None` 表示新增。
    pub fn open_mcp_editor(&mut self, server_id: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        let existing = server_id
            .and_then(|id| self.config.mcp_servers.iter().find(|server| server.id == id))
            .cloned();
        let (name, command, args, cwd) = match existing.as_ref() {
            Some(server) => {
                let McpTransport::Stdio { command, args, cwd } = &server.transport;
                (
                    server.name.clone(),
                    command.clone(),
                    args.join("\n"),
                    cwd.clone().unwrap_or_default(),
                )
            }
            None => (String::new(), String::new(), String::new(), String::new()),
        };
        // 环境变量的**值**在凭据管理器里，这里只是把它读出来显示
        let env = existing
            .as_ref()
            .map(|server| config::format_header_lines(&server.secrets()))
            .unwrap_or_default();

        let text_input = |window: &mut Window, cx: &mut Context<Self>, value: &str| {
            let value = value.to_string();
            cx.new(|cx| {
                let mut input = InputState::new(window, cx);
                input.set_value(&value, window, cx);
                input
            })
        };
        let text_area = |window: &mut Window, cx: &mut Context<Self>, value: &str| {
            let value = value.to_string();
            cx.new(|cx| {
                let mut input = TextareaState::new(window, cx).auto_grow(2, 6);
                input.set_value(&value, window, cx);
                input
            })
        };
        let name_input = text_input(window, cx, &name);
        let command_input = text_input(window, cx, &command);
        let args_input = text_area(window, cx, &args);
        let cwd_input = text_input(window, cx, &cwd);
        let env_input = text_area(window, cx, &env);

        self.mcp.editor = Some(McpEditor {
            editing_id: server_id.map(str::to_string),
            name: name_input,
            command: command_input,
            args: args_input,
            cwd: cwd_input,
            env: env_input,
        });
        cx.notify();
    }

    pub fn close_mcp_editor(&mut self, cx: &mut Context<Self>) {
        self.mcp.editor = None;
        cx.notify();
    }

    /// 把编辑器里的内容存下来。新增和修改走同一条路。
    pub fn save_mcp_editor(&mut self, cx: &mut Context<Self>) {
        let lang = self.language();
        let Some(editor) = self.mcp.editor.as_ref() else {
            return;
        };
        let name = editor.name.read(cx).value().trim().to_string();
        let command = editor.command.read(cx).value().trim().to_string();
        if name.is_empty() || command.is_empty() {
            self.toast(ToastLevel::Error, tr(lang, Key::McpNameRequired));
            cx.notify();
            return;
        }
        let args: Vec<String> = editor
            .args
            .read(cx)
            .value()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();
        let cwd = editor.cwd.read(cx).value().trim().to_string();
        let secrets = config::parse_header_lines(&editor.env.read(cx).value());
        let editing_id = editor.editing_id.clone();

        // 改的时候 id 不动：工具名里带着 id（`mcp__<id>__<工具>`），
        // 改一次名字就让历史记录里的调用和用户「停用某个工具」的设置全部失配
        let id = editing_id.clone().unwrap_or_else(|| new_server_id(&name));
        let disabled_tools = self
            .config
            .mcp_servers
            .iter()
            .find(|server| server.id == id)
            .map(|server| server.disabled_tools.clone())
            .unwrap_or_default();
        let server = McpServerConfig {
            id: id.clone(),
            name,
            // 新建出来的默认就是开的：用户刚填完命令，显然想让它跑起来
            enabled: true,
            transport: McpTransport::Stdio {
                command,
                args,
                cwd: (!cwd.is_empty()).then_some(cwd),
            },
            // 留空即可，`secret_reference()` 会回落到 `mcp/<id>`
            secret_ref: String::new(),
            disabled_tools,
        };

        // 密钥先写。写不进去就别动配置——否则会留下一个配好了但连不上的服务器，
        // 用户还得自己猜是哪一步没成功。
        if let Err(error) = server.store_secrets(&secrets) {
            self.toast(
                ToastLevel::Error,
                tr_args(lang, Key::McpSecretFailed, &[&error.to_string()]),
            );
            cx.notify();
            return;
        }
        match self.config.mcp_servers.iter_mut().find(|slot| slot.id == server.id) {
            Some(slot) => *slot = server,
            None => self.config.mcp_servers.push(server),
        }
        // 这里不用 `persist_config`：保存 MCP 服务器失败时提示里带上「MCP」更好定位
        if let Err(error) = self.config.save() {
            self.toast(
                ToastLevel::Error,
                tr_args(lang, Key::McpSaveFailed, &[&error.to_string()]),
            );
        }

        self.mcp.editor = None;
        self.mcp.selected_server_id = Some(id.clone());
        // 存完就按新配置连一次：用户改完命令最想看的就是它能不能起来
        self.connect_mcp_server(&id, cx);
    }

    /// 启用 / 停用一台服务器。停用会把连接断掉（工具也就不再交给模型）。
    pub fn set_mcp_server_enabled(&mut self, server_id: &str, enabled: bool, cx: &mut Context<Self>) {
        let Some(server) = self.config.mcp_servers.iter_mut().find(|server| server.id == server_id) else {
            return;
        };
        server.enabled = enabled;
        self.persist_config(cx);
        if enabled {
            self.connect_mcp_server(server_id, cx);
        } else {
            self.disconnect_mcp_server(server_id, cx);
        }
    }

    /// 单独停用 / 启用服务器里的某个工具。存的是**服务器给的原始工具名**。
    pub fn set_mcp_tool_enabled(&mut self, server_id: &str, raw_tool: &str, enabled: bool, cx: &mut Context<Self>) {
        let Some(server) = self.config.mcp_servers.iter_mut().find(|server| server.id == server_id) else {
            return;
        };
        if enabled {
            server.disabled_tools.retain(|name| name != raw_tool);
        } else if !server.disabled_tools.iter().any(|name| name == raw_tool) {
            server.disabled_tools.push(raw_tool.to_string());
        }
        self.persist_config(cx);
        cx.notify();
    }

    /// 删除一台服务器：断连接、清凭据、改配置。
    pub fn remove_mcp_server(&mut self, server_id: &str, cx: &mut Context<Self>) {
        let Some(index) = self.config.mcp_servers.iter().position(|server| server.id == server_id) else {
            return;
        };
        let server = self.config.mcp_servers.remove(index);
        // 凭据也要清掉：留着就是一条没人再引用的密钥
        if let Err(error) = config::store_secret(&server.secret_reference(), "") {
            self.toast(
                ToastLevel::Error,
                tr_args(self.language(), Key::McpSecretFailed, &[&error.to_string()]),
            );
        }
        // 连接在这里被丢掉，`Connection::drop` 会结束整棵进程树
        self.disconnect_mcp_server(server_id, cx);
        if self.mcp.selected_server_id.as_deref() == Some(server_id) {
            self.mcp.selected_server_id = None;
        }
        if self
            .mcp
            .editor
            .as_ref()
            .is_some_and(|editor| editor.editing_id.as_deref() == Some(server_id))
        {
            self.mcp.editor = None;
        }
        self.persist_config(cx);
        cx.notify();
    }
}

/// 从名称生成一个服务器 id。
///
/// 只用 `[a-z0-9]` 加一段短后缀：id 会进工具名（`mcp__<id>__<工具>`），
/// 而工具名要满足三个渠道对函数名的字符要求。中文名筛完什么都不剩，退回 `server`。
fn new_server_id(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|ch| ch.to_ascii_lowercase())
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(16)
        .collect();
    let slug = if slug.is_empty() { "server".to_string() } else { slug };
    let unique = uuid::Uuid::new_v4().simple().to_string();
    format!("{slug}-{}", &unique[..6])
}
