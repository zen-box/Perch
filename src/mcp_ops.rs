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
    /// 服务器 id → 最近一次失败的原因。
    ///
    /// 和 `ServerStatus::Failed` 里那份的区别是**重连期间这份还在**：失败那一行如果
    /// 在点「重连」的瞬间消失，整行就矮一截、下面所有服务器跟着往上跳，连上/再失败时
    /// 又跳回来——看着就是页面在闪。留着上一次的原因，行高就稳了。
    last_error: HashMap<String, String>,
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
    /// 仅供测试：不连服务器，直接塞一份工具清单。
    ///
    /// 真实路径是连接成功后把 `tools/list` 的结果填进来。测试没必要为了一份清单
    /// 去起一个子进程，也不该依赖真的能跑起来某个 MCP 服务器。
    #[cfg(test)]
    pub(crate) fn with_tools(entries: Vec<(&str, Vec<ExposedTool>)>) -> Self {
        let mut state = Self::default();
        for (server_id, tools) in entries {
            state.tools.insert(server_id.to_string(), tools);
        }
        state
    }

    /// 当前能交给模型的工具**名字**。
    ///
    /// 只给「模型调了一个不存在的 MCP 工具名」那条提示用——只要名字，没必要把整套
    /// JSON Schema 也建出来。
    pub fn available_names(&self, servers: &[McpServerConfig]) -> Vec<String> {
        self.usable(servers)
            .into_iter()
            .map(|tool| tool.exposed.clone())
            .collect()
    }

    /// 真正能交给模型的工具：跳过停用的服务器和单独停用的工具。
    ///
    /// 顺序由 `config.mcp_servers` 决定，**不靠 HashMap 的遍历顺序**：工具清单的顺序
    /// 一变，同一个会话序列化出来的请求体就变，prompt 缓存全失效。
    ///
    /// 两个出口（`specs` / `available_names`）共用这一处过滤，免得哪天改了过滤条件
    /// 只改了其中一个——那样「交给模型的清单」和「提示里列的清单」就对不上了。
    fn usable<'a>(&'a self, servers: &'a [McpServerConfig]) -> Vec<&'a ExposedTool> {
        self.usable_by_server(servers)
            .into_iter()
            .flat_map(|(_, tools)| tools)
            .collect()
    }

    /// 同上，但把「属于哪台服务器」也带出来，并按服务器分组。
    ///
    /// 会话级的工具选择器要按来源分组显示——用户得知道每个工具是从哪来的，光给一个
    /// 拍平的名字列表不够。过滤口径与 [`Self::usable`] 完全一致（它就是这份实现派生的），
    /// 免得出现「选择器里勾得上、实际发不出去」。
    pub fn usable_by_server<'a>(
        &'a self,
        servers: &'a [McpServerConfig],
    ) -> Vec<(&'a McpServerConfig, Vec<&'a ExposedTool>)> {
        servers
            .iter()
            .filter(|server| server.enabled)
            .filter_map(|server| {
                let tools = self.tools.get(&server.id)?;
                let kept: Vec<&ExposedTool> = tools
                    .iter()
                    .filter(|tool| !server.disabled_tools.contains(&tool.raw))
                    .collect();
                // 一个工具都没有的服务器不进选择器：空分组只是噪声
                (!kept.is_empty()).then_some((server, kept))
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

    /// 只丢掉连接，工具清单留着。
    ///
    /// 重连走这条路：连接一定要换（旧子进程得收掉），但清单没必要跟着一起没。
    /// 清单一空，设置页的详情区就塌成一行「暂无工具」，连上再撑回来——看着像闪了一下。
    /// 顺带一个好处：重连期间交给模型的工具清单不变，同一个会话的请求体前缀不变，
    /// prompt 缓存不会因为用户点了一下「重连」就全失效。
    fn close_connection(&mut self, server_id: &str) {
        self.connections.remove(server_id);
    }

    /// 丢掉某台服务器的工具清单。
    ///
    /// 只在**确定这批工具再也用不上**时调：用户断开、停用、删除，或者重连失败。
    /// 连接没了但清单还在的话，模型会看到一个调不动的清单。
    fn forget_tools(&mut self, server_id: &str) {
        self.tools.remove(server_id);
    }

    /// 最近一次失败的原因。没失败过是 `None`。
    pub fn last_error_of(&self, server_id: &str) -> Option<&str> {
        self.last_error.get(server_id).map(String::as_str)
    }

    /// 断开一台服务器并清掉它的工具清单。**配置不动**——用户只是停用，不是删除。
    fn drop_server(&mut self, server_id: &str) {
        self.close_connection(server_id);
        self.forget_tools(server_id);
        // 用户主动断开：上一次的失败原因也该跟着走，不然重新连上之前那行会一直挂着
        self.last_error.remove(server_id);
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
        // 换一台新的：旧连接在这里被丢掉，`Connection::drop` 会把整棵进程树收掉。
        // 工具清单**不清**（见 `close_connection`）：清掉的话设置页的详情区会先塌成
        // 一行「暂无工具」再撑回来，看着就是闪了一下。状态和清单的改动都在下面这一次
        // `cx.notify()` 之前做完，界面只渲染一次，中间态不会漏出去。
        self.mcp.close_connection(server_id);
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
        // 连上了，上一次的失败原因就不该再显示
        self.mcp.last_error.remove(server_id);
        cx.notify();
    }

    fn mcp_failed(&mut self, server_id: &str, error: String, cx: &mut Context<Self>) {
        // 重连失败时清单里是**上一版**的残留，连接已经没了，那些工具一个也调不动；
        // 留着只会让模型看到一个用不了的清单。首次连接失败时清单本来就是空的。
        self.mcp.forget_tools(server_id);
        // 失败的原因可能很长（协议错误里带着整帧），留一句够看的
        let error = local_tools_truncate(&error);
        self.mcp.last_error.insert(server_id.to_string(), error.clone());
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

    /// 工具在界面上显示的名字。
    ///
    /// 本地工具就是它自己。MCP 工具返回**服务器给的原始工具名**：暴露名里的清洗和
    /// 哈希是给模型区分撞名用的（`a_b_b2c9276d`），拿去给用户看是纯噪音；而且设置页
    /// 里列的就是原始名，两边显示成不一样的东西会让人以为调错了工具。
    ///
    /// 服务器已经断开或删掉时查不到，退回暴露名去掉前缀的那一段——历史消息还得显示，
    /// 而那时清单已经没了，只能尽力而为。
    pub fn tool_display_name(&self, exposed: &str) -> String {
        match self.mcp.locate(exposed) {
            Some((_, tool)) => tool.raw.clone(),
            None => mcp::display_name(exposed).to_string(),
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
    ///
    /// 返回**是否真的存下去了**。校验不过、密钥写失败都返回 `false`——调用方（弹窗）
    /// 靠它决定关不关窗；关掉的话用户刚填的一整页就没了，只能从头再来。
    /// 配置写盘失败仍然算成功（服务器已经在内存里生效了，用户也收到了提示）。
    pub fn save_mcp_editor(&mut self, cx: &mut Context<Self>) -> bool {
        let lang = self.language();
        let Some(editor) = self.mcp.editor.as_ref() else {
            return false;
        };
        let name = editor.name.read(cx).value().trim().to_string();
        let command = editor.command.read(cx).value().trim().to_string();
        if name.is_empty() || command.is_empty() {
            self.toast(ToastLevel::Error, tr(lang, Key::McpNameRequired));
            cx.notify();
            return false;
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
        let previous = self.config.mcp_servers.iter().find(|server| server.id == id);
        // 编辑时保留原来的启用状态。用户明明把它停用了，进来改个参数、点保存，
        // 不该顺手又给它打开——那是个会让人莫名其妙多出一台在跑的服务器。
        let enabled = previous.map(|server| server.enabled).unwrap_or(true);
        let disabled_tools = previous.map(|server| server.disabled_tools.clone()).unwrap_or_default();
        let server = McpServerConfig {
            id: id.clone(),
            name,
            // 新建出来的默认就是开的：用户刚填完命令，显然想让它跑起来
            enabled,
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
            return false;
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
        true
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn exposed(exposed: &str, raw: &str) -> ExposedTool {
        ExposedTool {
            exposed: exposed.to_string(),
            raw: raw.to_string(),
            description: String::new(),
            parameters: json!({"type": "object"}),
        }
    }

    fn server(id: &str, enabled: bool, disabled_tools: &[&str]) -> McpServerConfig {
        McpServerConfig {
            id: id.to_string(),
            name: id.to_string(),
            enabled,
            transport: McpTransport::Stdio {
                command: "noop".to_string(),
                args: Vec::new(),
                cwd: None,
            },
            secret_ref: String::new(),
            disabled_tools: disabled_tools.iter().map(|name| name.to_string()).collect(),
        }
    }

    /// 两个出口必须用同一套过滤：`specs` 是交给模型的清单，`available_names` 是
    /// 「这个工具名不认识」提示里列的清单。对不上就等于告诉模型一套、实际给它另一套。
    #[std::prelude::v1::test]
    fn specs_and_names_share_the_same_filter() {
        let mut state = McpState::default();
        state.tools.insert(
            "on".to_string(),
            vec![exposed("mcp__on__a", "a"), exposed("mcp__on__b", "b")],
        );
        state.tools.insert("off".to_string(), vec![exposed("mcp__off__c", "c")]);
        let servers = vec![server("on", true, &["b"]), server("off", false, &[])];

        let names = state.available_names(&servers);
        // 选择器按来源分组拿到的工具（`usable_by_server`）和「提示里列的名字」
        // （`available_names`）必须过同一道过滤，否则会出现「选择器里勾得上、
        // 模型却调不动」或者反过来的情况。
        let grouped: Vec<String> = state
            .usable_by_server(&servers)
            .into_iter()
            .flat_map(|(_, tools)| tools)
            .map(|tool| tool.exposed.clone())
            .collect();
        assert_eq!(names, vec!["mcp__on__a".to_string()]);
        assert_eq!(grouped, names);
    }

    /// 界面要的是**服务器给的原始名**：暴露名里的哈希是给模型区分撞名用的，
    /// 设置页里列的是原始名，两边显示成不一样的东西会让人以为调错了工具。
    #[std::prelude::v1::test]
    fn locate_recovers_the_raw_name_behind_an_exposed_one() {
        let mut state = McpState::default();
        state
            .tools
            .insert("srv".to_string(), vec![exposed("mcp__srv__a_b_b2c9276d", "a.b")]);

        let located = state.locate("mcp__srv__a_b_b2c9276d");
        assert_eq!(
            located.map(|(server, tool)| (server, tool.raw.as_str())),
            Some(("srv", "a.b"))
        );
        assert!(state.locate("mcp__srv__gone").is_none());

        // 服务器断开或删掉之后再看历史消息：清单里已经查不到了，
        // `tool_display_name` 会退回去掉前缀的那一段（尽力而为，总比显示全名强）
        assert_eq!(mcp::display_name("mcp__srv__a_b_b2c9276d"), "a_b_b2c9276d");
        assert_eq!(mcp::display_name("read_file"), "read_file");
    }

    /// 重连只换连接、不清清单——清了界面就闪（详情区塌成一行「暂无工具」再撑回来）。
    /// 但重连**失败**时必须清：连接已经没了，留着就是给模型看一个调不动的清单。
    #[std::prelude::v1::test]
    fn reconnecting_keeps_the_tool_list_until_it_fails() {
        let mut state = McpState::default();
        state.tools.insert("srv".to_string(), vec![exposed("mcp__srv__a", "a")]);
        state.status.insert("srv".to_string(), ServerStatus::Ready);

        state.close_connection("srv");
        assert_eq!(state.tools_of("srv").len(), 1);

        state.forget_tools("srv");
        assert!(state.tools_of("srv").is_empty());
    }

    /// 用户主动断开是另一回事：连接和清单一起走，状态回 `Idle`（按钮文案要变回「连接」）。
    #[std::prelude::v1::test]
    fn dropping_a_server_clears_both_the_connection_and_the_tools() {
        let mut state = McpState::default();
        state.tools.insert("srv".to_string(), vec![exposed("mcp__srv__a", "a")]);
        state.status.insert("srv".to_string(), ServerStatus::Ready);

        state.drop_server("srv");
        assert!(state.tools_of("srv").is_empty());
        assert_eq!(state.status("srv"), ServerStatus::Idle);
    }

    /// 重连期间上一次的失败原因要留着。那一行是「点重连页面闪动」的根子：
    /// 一消失整行就矮一截，下面几台服务器跟着往上跳，连上或者再失败时又跳回来。
    #[std::prelude::v1::test]
    fn the_last_error_survives_a_reconnect_but_not_a_disconnect() {
        let mut state = McpState::default();
        state.last_error.insert("srv".to_string(), "boom".to_string());

        // 重连只收连接，原因继续挂着
        state.close_connection("srv");
        assert_eq!(state.last_error_of("srv"), Some("boom"));

        // 用户主动断开就该清掉，不然重新连上之前那行会一直挂着
        state.drop_server("srv");
        assert_eq!(state.last_error_of("srv"), None);
    }
}
