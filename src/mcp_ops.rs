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

use gpui_kit::*;

use crate::app::{AppState, runtime, update_state};
use crate::config::McpServerConfig;
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
