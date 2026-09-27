//! Agent 循环：模型要求调用工具之后，怎么把这一轮接着跑下去。
//!
//! 一次用户提问可能对应不止一轮请求：
//!
//! ```text
//! 发请求（带工具清单）
//!   → 模型回 ToolCall（可能一次好几个）
//!   → 免确认的先在后台执行；要确认的挂授权卡片，一次一张
//!   → 结果作为 tool 消息落进会话
//!   → 这条消息的调用全部有了结果，再带着结果发一轮（回到第一行）
//!   → 模型这次没要求调用工具，给正文 → 结束
//! ```
//!
//! 单独一个文件而不是塞进 `reply_ops.rs`，是因为「怎么发一轮请求」和「发完之后
//! 要不要再来一轮」是两件事，混在一起两个都读不清（§3.3 的规模限制也拦着）。
//!
//! 几条硬规矩：
//!
//! - **循环绑定在发起它的对话上**（[`AgentOrigin::session_id`]）。结果写回那个对话，
//!   续跑用那个对话的历史，授权卡片也只在那个对话里显示——用户中途切到别的对话，
//!   不能把工具结果和请求带进去。
//! - **工具在后台执行**，界面不卡；执行期间「停止」按钮可以打断。
//! - **手打的斜杠命令不经过模型**：同意之后只执行、不续跑，一个请求都不发。
//! - **授权一律由用户当面点**：这一层只负责挂起和判断级别，不做任何"记住上次同意"
//!   之类的自动放行——那会把 §11 的「危险操作每次授权」悄悄改掉。

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gpui_kit::*;

use crate::app::{AppState, ToastLevel, runtime, update_state};
use crate::i18n::{Key, tr};
use crate::llm_tools::{ToolCall, ToolResult};
use crate::local_tools::{self, ExecControl, PendingTool};
use crate::mcp::{self, Connection};
use crate::mcp_ops::Route;
use crate::model::{ChatMessage, ChatSession};
use crate::reply_ops::JobSpec;

/// 一轮用户提问最多允许模型连着续跑几次。
///
/// 这是个**兜底**不是功能：模型正常会在拿到结果后给出结论。但模型卡住时
/// （比如反复读同一个文件、或者参数一直发错）没有上限就会一直转，
/// 每轮都在花 token，用户只能看着它转。
pub(crate) const MAX_AGENT_ROUNDS: usize = 12;

/// 到了轮数上限、没来得及执行的调用回传给模型的理由。
const LIMIT_REASON: &str = "the tool-call limit for this turn was reached.";
/// 用户点了停止时回传给模型的理由。
const STOPPED_REASON: &str = "stopped by the user.";
/// 后台执行任务整个没了（panic 之类）时回传给模型的理由。正常不会出现，
/// 但没有它的话占位块会永远停在「正在执行」上。
const RUN_LOST_REASON: &str = "the tool runner stopped unexpectedly.";

/// 一轮流式请求是替哪个对话、哪条消息、哪个模型发的。
///
/// 续跑时原样再用一次，不能重新去读 `session.provider_id`：用户可能在生成期间
/// 换了模型，续跑应当仍在原来那个模型上做，否则一个回答会横跨两个模型。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AgentOrigin {
    pub(crate) session_id: String,
    pub(crate) message_id: String,
    pub(crate) provider_id: String,
    pub(crate) model: String,
}

/// 授权卡片上那条调用是谁发起的。
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ApprovalSource {
    /// 用户手打的斜杠命令：同意后只执行，不请求模型。
    /// `local_only` 为真时结果只在本机显示（读敏感文件）。
    Manual { local_only: bool },
    /// 模型在循环里发起的：处理完回到循环。
    Agent(AgentOrigin),
}

/// 一条卡在授权卡片上、等用户点头的调用。
#[derive(Clone, Debug, PartialEq)]
pub struct PendingToolApproval {
    /// 卡片属于哪个对话：只在这个对话里显示，结果也只写回这里
    pub session_id: String,
    pub tool: PendingTool,
    pub(crate) source: ApprovalSource,
}

impl PendingToolApproval {
    /// 结果是否只在本机显示
    pub fn is_local_only(&self) -> bool {
        matches!(self.source, ApprovalSource::Manual { local_only: true })
    }
}

/// 正在后台执行的一批调用。
pub(crate) struct ToolRun {
    /// 区分被停止的旧任务：停止之后它的结果可能还会回来，得认出来丢掉
    id: u64,
    session_id: String,
    /// 每条调用对应的占位消息 id，执行完按它回填
    placeholders: Vec<(String, PendingTool)>,
    cancel: Arc<AtomicBool>,
    /// 执行完回到哪个循环；`None` 是手打命令，执行完就结束
    then: Option<AgentOrigin>,
}

/// Agent 相关的全部状态。集中成一个字段，不往 `AppState` 顶层散加（AGENTS.md §4.1）。
#[derive(Default)]
pub struct AgentState {
    /// 等用户点「允许 / 拒绝」的调用
    pub pending: Option<PendingToolApproval>,
    /// 当前这轮流式请求属于哪个循环；`None` 表示没有循环在跑
    pub(crate) origin: Option<AgentOrigin>,
    /// 这一轮用户提问里已经续跑了几次（和 [`MAX_AGENT_ROUNDS`] 比）
    pub(crate) round: usize,
    /// 正在后台执行的调用
    pub(crate) running: Option<ToolRun>,
    next_run_id: u64,
    /// 界面上展开了全文的工具结果（消息 id）
    pub expanded_results: HashSet<String>,
}

impl AgentState {
    /// 这个对话里等授权的调用
    pub fn pending_in(&self, session_id: &str) -> Option<&PendingToolApproval> {
        self.pending.as_ref().filter(|pending| pending.session_id == session_id)
    }

    /// 结束当前循环（不影响等授权的卡片和正在执行的工具）
    fn end_loop(&mut self) {
        self.origin = None;
        self.round = 0;
    }
}

/// 这一轮该怎么走。把判断从 `AppState` 里拆出来是因为它是**纯函数**：
/// 不碰界面、不落盘、不发请求，所以可以直接写测试。`drive_agent_loop`
/// 只负责把结论变成动作。
#[derive(Debug, PartialEq)]
pub(crate) enum RoundAction {
    /// 这条消息没有工具调用：这一轮正常结束
    Finish,
    /// 调用都有结果了：带着结果再请求一次模型
    Continue,
    /// 到了轮数上限：没回的调用不再执行
    HitLimit(Vec<PendingTool>),
    /// 本地工具被关掉了：没回的调用直接回"已停用"，不再请求模型
    Disabled(Vec<PendingTool>),
    /// 先把这些免确认的调用执行了，完了再回来看
    RunTools(Vec<PendingTool>),
    /// 剩下的都要用户点头，先问这一条
    AskApproval(PendingTool),
}

/// 决定这一轮接下来怎么走。
///
/// - `calls`：这条助手消息要求的调用
/// - `answered`：会话里已经有结果的调用 id
/// - `round`：已经续跑过几次
pub(crate) fn next_action(
    calls: &[ToolCall],
    answered: &HashSet<String>,
    round: usize,
    tools_enabled: bool,
) -> RoundAction {
    if calls.is_empty() {
        return RoundAction::Finish;
    }
    let waiting: Vec<PendingTool> = calls
        .iter()
        .filter(|call| !answered.contains(&call.id))
        .map(PendingTool::from_call)
        .collect();
    if waiting.is_empty() {
        return RoundAction::Continue;
    }
    if round >= MAX_AGENT_ROUNDS {
        return RoundAction::HitLimit(waiting);
    }
    // 工具没开：不必挂起也不必真实执行，直接把"已停用"回传就行。
    // 这种情况也要给每个调用一个结果，否则历史里留着没回的调用。
    if !tools_enabled {
        return RoundAction::Disabled(waiting);
    }
    // 免确认的先跑掉，用户就不用盯着卡片等它们；要确认的**一次只问一条**，
    // 一次弹好几张卡片用户会不知道先点哪个。处理完一条回到这里，再看下一条。
    //
    // 模型编出来的工具名也算免确认：执行器对它什么都不做，只回一句「没有这个工具、
    // 可用的有哪些」。为一个不存在的工具弹授权卡片，用户只会莫名其妙。
    let free: Vec<PendingTool> = waiting
        .iter()
        .filter(|tool| {
            if mcp::is_mcp_tool(&tool.name) {
                // MCP 工具**一律每次确认**（§11 已确认的决策）：调用跑在别人的服务器上，
                // 我们既不知道它到底做什么，也没法像本机工具那样按"读/写/执行"分级。
                // 注意不能走下面的「不认识的名字当免确认」那条——MCP 工具名在本机
                // 工具表里当然查不到，那样会被误判成"模型编的名字"直接放行。
                false
            } else if local_tools::is_known(&tool.name) {
                !tool.needs_approval()
            } else {
                true
            }
        })
        .cloned()
        .collect();
    if !free.is_empty() {
        return RoundAction::RunTools(free);
    }
    match waiting.into_iter().next() {
        Some(first) => RoundAction::AskApproval(first),
        None => RoundAction::Continue,
    }
}

/// 会话里已经有结果的调用 id。调用 id 在会话里是唯一的（见 `llm_tools::fresh_call_id`）。
fn answered_call_ids(session: &ChatSession) -> HashSet<String> {
    session
        .messages
        .iter()
        .filter(|message| message.role == "tool" && !message.tool_call_id.is_empty())
        .map(|message| message.tool_call_id.clone())
        .collect()
}

impl AppState {
    /// 当前流结束后，如果它属于 Agent 循环，就把循环推下去。
    ///
    /// 返回 `true` 表示循环已经接手（在执行工具、挂着授权、或又发了一轮），
    /// 调用方就不要再做"整轮结束"的收尾（自动起标题）。
    pub(crate) fn advance_agent_loop(&mut self, message_id: &str, cx: &mut Context<Self>) -> bool {
        let Some(origin) = self.agent.origin.clone() else {
            return false;
        };
        // 只有循环自己发起的那一轮才续跑；别的请求（对比、起标题）不该被卷进来
        if origin.message_id != message_id {
            return false;
        }
        self.drive_agent_loop(origin, cx)
    }

    /// 看这条消息的调用处理到哪了，决定下一步。返回值同 [`Self::advance_agent_loop`]。
    pub(crate) fn drive_agent_loop(&mut self, origin: AgentOrigin, cx: &mut Context<Self>) -> bool {
        let snapshot = self.session(&origin.session_id).and_then(|session| {
            let message = session
                .messages
                .iter()
                .find(|message| message.id == origin.message_id)?;
            Some((
                message.called_tools.clone(),
                answered_call_ids(session),
                message.error.is_some(),
            ))
        });
        // 对话或消息已经不在了（被删、被清空、重新生成截掉了），或者这一轮请求本身出错了
        // （可能只吐了半截调用）：都不再往下跑
        let Some((calls, answered, false)) = snapshot else {
            self.agent.end_loop();
            return false;
        };

        match next_action(&calls, &answered, self.agent.round, self.config.local_tools_enabled) {
            RoundAction::Finish => {
                self.agent.end_loop();
                false
            }
            RoundAction::Continue => {
                self.continue_agent(origin, cx);
                true
            }
            RoundAction::HitLimit(waiting) => {
                for tool in &waiting {
                    let result = local_tools::not_executed_result(tool, LIMIT_REASON);
                    self.push_to_session(&origin.session_id, ChatMessage::new_tool(&result));
                }
                self.agent.end_loop();
                self.toast(ToastLevel::Error, tr(self.language(), Key::AgentRoundLimit));
                self.persist_storage(cx);
                false
            }
            RoundAction::Disabled(waiting) => {
                for tool in &waiting {
                    let result = local_tools::disabled_result(tool);
                    self.push_to_session(&origin.session_id, ChatMessage::new_tool(&result));
                }
                // 工具没开就不必再发一轮：模型什么都做不了，只会重复要求调用
                self.agent.end_loop();
                self.persist_storage(cx);
                false
            }
            RoundAction::RunTools(tools) => {
                let session_id = origin.session_id.clone();
                self.start_tool_run(&session_id, tools, Some(origin), false, cx);
                true
            }
            RoundAction::AskApproval(tool) => {
                self.agent.pending = Some(PendingToolApproval {
                    session_id: origin.session_id.clone(),
                    tool,
                    source: ApprovalSource::Agent(origin),
                });
                self.persist_storage(cx);
                cx.notify();
                true
            }
        }
    }

    /// 带着已有的工具结果再发一轮请求。
    ///
    /// 复用 `make_job` + `spawn_jobs`，不另写一套发送逻辑：重试、取消、
    /// 指标统计这些都在那条路上，抄一份必然会走偏。
    fn continue_agent(&mut self, origin: AgentOrigin, cx: &mut Context<Self>) {
        self.agent.round += 1;
        let history = self.history_messages(&origin.session_id);
        let mut assistant = ChatMessage::new_assistant();
        assistant.model = origin.model.clone();
        let message_id = assistant.id.clone();
        self.push_to_session(&origin.session_id, assistant);

        let spec = JobSpec {
            session_id: &origin.session_id,
            message_id: &message_id,
            variant_id: None,
            provider_id: &origin.provider_id,
            model_id: &origin.model,
            with_tools: true,
        };
        let Some(job) = self.make_job(spec, history) else {
            let error = tr(self.language(), Key::ErrNoModelInChannel).to_string();
            if let Some(message) = self.find_message_mut(&message_id) {
                message.is_streaming = false;
                message.error = Some(error);
            }
            self.agent.end_loop();
            self.persist_storage(cx);
            cx.notify();
            return;
        };
        self.scroll_to_end_pending = true;
        self.spawn_jobs(vec![job], cx);
    }

    /// 在后台执行一批调用：先插占位块（界面显示「正在执行」），执行完回填结果。
    ///
    /// `then` 为 `Some` 时执行完回到那个循环；`None` 是手打命令，执行完就结束。
    pub(crate) fn start_tool_run(
        &mut self,
        session_id: &str,
        tools: Vec<PendingTool>,
        then: Option<AgentOrigin>,
        local_only: bool,
        cx: &mut Context<Self>,
    ) {
        let mut placeholders = Vec::new();
        for tool in &tools {
            let message = ChatMessage::tool_placeholder(&tool.id, &tool.name, local_only);
            placeholders.push((message.id.clone(), tool.clone()));
            self.push_to_session(session_id, message);
        }
        // 超时跟着设置走：编译、装依赖这类命令要多久因项目而异，
        // 写死在代码里总有人不够用（也总有人嫌久）
        let control = ExecControl::with_timeout_secs(self.config.command_timeout_secs);
        self.agent.next_run_id += 1;
        let run_id = self.agent.next_run_id;
        self.agent.running = Some(ToolRun {
            id: run_id,
            session_id: session_id.to_string(),
            placeholders,
            cancel: control.cancel.clone(),
            then,
        });
        // 工具执行期间和生成回答一样算「忙」：输入框换成停止按钮，不能再发新消息
        self.is_streaming = true;
        self.scroll_to_end_pending = true;
        self.persist_storage(cx);
        cx.notify();

        // 每条调用走哪条路要在挪进后台任务之前定下来：`route_tool` 要读 `self`，
        // 而 MCP 的连接句柄也得先克隆出来（后台任务只拿得到 `'static` 的东西）
        let routes: Vec<Route> = tools.iter().map(|tool| self.route_tool(&tool.name)).collect();

        cx.spawn(async move |this, cx| {
            // 真正干活的在 tokio 运行时里：GPUI 自己的执行器不是 tokio，
            // 在 `cx.spawn` 的 future 里直接跑 tokio 的定时器或子进程会 panic（§7）。
            // 结果用 channel 递回来——`tokio::sync` 这套不依赖运行时上下文，
            // 在 GPUI 这边 await 它没问题。
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            runtime().spawn(async move {
                let _ = tx.send(run_tool_batch(tools, routes, control).await);
            });
            // 后台任务整个没了（panic 之类）时不能就这么算了：占位块会永远停在
            // 「正在执行」，界面也一直卡在"忙"上。这时结果为空，`finish_tool_run`
            // 会按占位块补一批"没跑成"的结果收尾。
            let results = rx.recv().await.unwrap_or_default();
            update_state(&this, cx, |state, cx| state.finish_tool_run(run_id, results, cx));
        })
        .detach();
    }

    /// 后台执行完了：回填结果，然后按来源决定下一步。
    fn finish_tool_run(&mut self, run_id: u64, results: Vec<ToolResult>, cx: &mut Context<Self>) {
        // 已经被停止的旧任务：占位块在停止时就写好了「已停止」，这里的结果不要了
        let Some(run) = self.agent.running.take_if(|run| run.id == run_id) else {
            return;
        };
        for (ix, (placeholder_id, tool)) in run.placeholders.iter().enumerate() {
            // 缺结果时补一条说明，而不是把占位块留在"正在执行"上
            let result = results
                .get(ix)
                .cloned()
                .unwrap_or_else(|| local_tools::not_executed_result(tool, RUN_LOST_REASON));
            if let Some(message) = self.find_message_mut(placeholder_id) {
                message.content = result.content;
                message.tool_is_error = result.is_error;
                message.tool_duration_ms = result.duration_ms;
                message.tool_exit_code = result.exit_code;
                message.is_streaming = false;
            }
        }
        self.is_streaming = !self.active_streams.is_empty();
        self.persist_storage(cx);
        if let Some(origin) = run.then {
            let session_id = origin.session_id.clone();
            if !self.drive_agent_loop(origin, cx) {
                self.maybe_autotitle(&session_id, cx);
            }
        }
        cx.notify();
    }

    /// 用户点了「停止」：结束正在执行的工具，并让循环不再往下跑。
    ///
    /// 还没执行完的调用在原地写上「已停止」。不写的话历史里就留着没回结果的调用——
    /// 请求时虽然有兜底配对，但界面上会一直是个空块。
    pub(crate) fn stop_agent(&mut self) {
        if let Some(run) = self.agent.running.take() {
            run.cancel.store(true, Ordering::Relaxed);
            for (placeholder_id, tool) in &run.placeholders {
                let result = local_tools::not_executed_result(tool, STOPPED_REASON);
                if let Some(message) = self.find_message_mut(placeholder_id)
                    && message.is_streaming
                {
                    message.content = result.content;
                    message.tool_is_error = true;
                    message.is_streaming = false;
                }
            }
        }
        self.agent.end_loop();
    }

    /// 对话被删除或清空时，丢掉和它有关的循环状态。
    pub(crate) fn forget_agent_session(&mut self, session_id: &str) {
        if self.agent.pending_in(session_id).is_some() {
            self.agent.pending = None;
        }
        if self
            .agent
            .origin
            .as_ref()
            .is_some_and(|origin| origin.session_id == session_id)
        {
            self.agent.end_loop();
        }
        if let Some(run) = self.agent.running.take_if(|run| run.session_id == session_id) {
            run.cancel.store(true, Ordering::Relaxed);
            self.is_streaming = !self.active_streams.is_empty();
        }
    }

    /// 用户在授权卡片上点了「允许一次」。
    pub fn approve_pending_tool(&mut self, cx: &mut Context<Self>) {
        let Some(approval) = self.agent.pending.take() else {
            return;
        };
        if self.session(&approval.session_id).is_none() {
            cx.notify();
            return;
        }
        if !self.config.local_tools_enabled {
            // 卡片挂着的时候用户把本地工具关了：按"已停用"处理，别执行
            if matches!(approval.source, ApprovalSource::Agent(_)) {
                let result = local_tools::disabled_result(&approval.tool);
                self.push_to_session(&approval.session_id, ChatMessage::new_tool(&result));
                self.agent.end_loop();
                self.persist_storage(cx);
            }
            self.toast(ToastLevel::Error, tr(self.language(), Key::LocalToolsDisabled));
            cx.notify();
            return;
        }
        let local_only = approval.is_local_only();
        let then = match approval.source {
            ApprovalSource::Manual { .. } => None,
            ApprovalSource::Agent(origin) => Some(origin),
        };
        self.start_tool_run(&approval.session_id, vec![approval.tool], then, local_only, cx);
    }

    /// 用户在授权卡片上点了「拒绝」。
    ///
    /// 模型发起的调用要把「用户拒绝了」回传给模型——闭嘴比假装执行成功好：
    /// 模型看到拒绝的理由，下一轮可以换个思路；直接静默结束的话，用户会以为卡住了。
    /// 手打的命令拒绝了就是不执行，什么都不用回。
    pub fn deny_pending_tool(&mut self, cx: &mut Context<Self>) {
        let Some(approval) = self.agent.pending.take() else {
            return;
        };
        if let ApprovalSource::Agent(origin) = approval.source {
            let result = local_tools::denial_result(&approval.tool);
            self.push_to_session(&approval.session_id, ChatMessage::new_tool(&result));
            self.persist_storage(cx);
            let session_id = origin.session_id.clone();
            if !self.drive_agent_loop(origin, cx) {
                self.maybe_autotitle(&session_id, cx);
            }
        }
        cx.notify();
    }

    /// 当前对话里挂着授权卡片时，拦下会改动这个对话的操作（发消息、重新生成、删消息……）。
    /// 返回 `true` 表示拦下了。
    pub(crate) fn block_while_approval_pending(&mut self, cx: &mut Context<Self>) -> bool {
        let active_id = self.storage.active_session_id.clone();
        if self.agent.pending_in(&active_id).is_none() {
            return false;
        }
        self.toast(ToastLevel::Info, tr(self.language(), Key::ApprovalPendingFirst));
        cx.notify();
        true
    }

    /// 往指定对话末尾追加一条消息（工具结果、占位块、续跑的助手消息）。
    ///
    /// 必须指定对话，不能用「当前打开的对话」：循环跑着的时候用户可能已经切走了。
    pub(crate) fn push_to_session(&mut self, session_id: &str, message: ChatMessage) {
        if let Some(session) = self
            .storage
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.messages.push(message);
        }
        if self.storage.active_session_id == session_id {
            self.scroll_to_end_pending = true;
        }
    }

    pub(crate) fn session(&self, session_id: &str) -> Option<&ChatSession> {
        self.storage.sessions.iter().find(|session| session.id == session_id)
    }

    /// 展开 / 收起一条工具结果的全文
    pub fn toggle_tool_result(&mut self, message_id: &str, cx: &mut Context<Self>) {
        if !self.agent.expanded_results.remove(message_id) {
            self.agent.expanded_results.insert(message_id.to_string());
        }
        cx.notify();
    }
}

/// 执行一批调用，返回的顺序和传进来的一致——结果要按顺序回填到占位块上。
///
/// 两条路差别很大，所以分开跑：本机工具是**阻塞**的（读文件、等命令跑完），整批
/// 丢给后台线程；MCP 调用是异步的，一条条 await——它的瓶颈在对端，占着线程没用。
async fn run_tool_batch(tools: Vec<PendingTool>, routes: Vec<Route>, control: ExecControl) -> Vec<ToolResult> {
    let mut slots: Vec<Option<ToolResult>> = tools.iter().map(|_| None).collect();
    let mut local: Vec<(usize, PendingTool)> = Vec::new();
    let mut remote: Vec<(usize, PendingTool, Arc<Connection>, String)> = Vec::new();
    for (ix, (tool, route)) in tools.iter().zip(routes).enumerate() {
        match route {
            Route::Local => local.push((ix, tool.clone())),
            Route::Mcp { connection, raw } => remote.push((ix, tool.clone(), connection, raw)),
        }
    }

    if !local.is_empty() {
        let blocking_control = control.clone();
        let done = runtime()
            .spawn_blocking(move || {
                local
                    .into_iter()
                    .map(|(ix, tool)| (ix, local_tools::execute(&tool, &blocking_control)))
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default();
        for (ix, result) in done {
            if let Some(slot) = slots.get_mut(ix) {
                *slot = Some(result);
            }
        }
    }

    for (ix, tool, connection, raw) in remote {
        let result = run_remote_tool(&tool, &connection, &raw, &control).await;
        if let Some(slot) = slots.get_mut(ix) {
            *slot = Some(result);
        }
    }

    slots
        .into_iter()
        .zip(tools)
        .map(|(slot, tool)| slot.unwrap_or_else(|| local_tools::not_executed_result(&tool, RUN_LOST_REASON)))
        .collect()
}

/// 调一次 MCP 工具。三种结束方式：正常返回、用户点「停止」、超时。
async fn run_remote_tool(
    tool: &PendingTool,
    connection: &Arc<Connection>,
    raw: &str,
    control: &ExecControl,
) -> ToolResult {
    let started = Instant::now();
    let call = connection.call(raw, tool.arguments.clone());
    let outcome = tokio::select! {
        result = tokio::time::timeout(control.command_timeout, call) => match result {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(format!(
                "timed out after {} seconds",
                control.command_timeout.as_secs()
            )),
        },
        // 用户点了停止。MCP 协议里没有「取消这一个请求」的约定，能做的是**不再等它**：
        // 这个 future 被丢掉，服务器那边照旧会跑完，只是结果没人接了。
        _ = wait_for_stop(control.cancel.clone()) => {
            return local_tools::not_executed_result(tool, STOPPED_REASON);
        }
    };
    match outcome {
        Ok(result) => mcp::to_tool_result(&tool.id, &tool.name, &result, started.elapsed().as_millis() as u64),
        Err(error) => ToolResult {
            id: tool.id.clone(),
            name: tool.name.clone(),
            content: format!("MCP call failed: {error}"),
            is_error: true,
            duration_ms: started.elapsed().as_millis() as u64,
            exit_code: None,
        },
    }
}

/// 等用户点「停止」。
///
/// 本机工具靠 [`ExecControl::cancel`] 结束子进程；MCP 调用打断不了对端，只能不再等。
/// 轮询 100 毫秒一次：比再引入一套通知机制省事，而人对「停止」的感知也到不了
/// 100 毫秒这个精度。
async fn wait_for_stop(cancel: Arc<AtomicBool>) {
    while !cancel.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn call(id: &str, name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: name.into(),
            arguments,
        }
    }

    #[std::prelude::v1::test]
    fn no_calls_means_finish() {
        assert_eq!(next_action(&[], &HashSet::new(), 0, true), RoundAction::Finish);
    }

    #[std::prelude::v1::test]
    fn all_answered_means_continue() {
        // 调用都有结果了，就该带着结果再请求一次模型——而不是停下
        let calls = vec![call("c1", "read_file", json!({"path": "a.rs"}))];
        let answered: HashSet<String> = ["c1".to_string()].into_iter().collect();
        assert_eq!(next_action(&calls, &answered, 0, true), RoundAction::Continue);
    }

    #[std::prelude::v1::test]
    fn read_only_tools_run_without_asking() {
        let calls = vec![call("c1", "read_file", json!({"path": "src/main.rs"}))];
        match next_action(&calls, &HashSet::new(), 0, true) {
            RoundAction::RunTools(tools) => {
                assert_eq!(tools.len(), 1);
                assert_eq!(tools[0].name, "read_file");
            }
            other => panic!("只读工具应当直接跑，得到 {other:?}"),
        }
    }

    #[std::prelude::v1::test]
    fn dangerous_tools_stop_for_approval() {
        let calls = vec![call("c1", "run_command", json!({"command": "rm -rf /"}))];
        match next_action(&calls, &HashSet::new(), 0, true) {
            RoundAction::AskApproval(tool) => assert_eq!(tool.name, "run_command"),
            other => panic!("危险工具应当挂起等授权，得到 {other:?}"),
        }
    }

    #[std::prelude::v1::test]
    fn free_tools_run_before_asking_about_the_rest() {
        // 模型一次要了「读文件 + 执行命令」：读文件先跑掉，命令再问。
        // 以前的写法是只问命令、同意后直接续跑——读文件那个调用就永远没有结果了。
        let calls = vec![
            call("c1", "run_command", json!({"command": "cargo test"})),
            call("c2", "read_file", json!({"path": "a.rs"})),
        ];
        match next_action(&calls, &HashSet::new(), 0, true) {
            RoundAction::RunTools(tools) => {
                assert_eq!(tools.len(), 1);
                assert_eq!(tools[0].id, "c2");
            }
            other => panic!("应当先跑免确认的，得到 {other:?}"),
        }
        // 读文件有结果之后，才轮到命令
        let answered: HashSet<String> = ["c2".to_string()].into_iter().collect();
        match next_action(&calls, &answered, 0, true) {
            RoundAction::AskApproval(tool) => assert_eq!(tool.id, "c1"),
            other => panic!("接着应当问命令，得到 {other:?}"),
        }
    }

    #[std::prelude::v1::test]
    fn only_one_approval_card_shows_at_a_time() {
        let calls = vec![
            call("c1", "run_command", json!({"command": "echo 1"})),
            call("c2", "run_command", json!({"command": "echo 2"})),
        ];
        match next_action(&calls, &HashSet::new(), 0, true) {
            RoundAction::AskApproval(tool) => assert_eq!(tool.id, "c1"),
            other => panic!("应当只挂第一条，得到 {other:?}"),
        }
        let answered: HashSet<String> = ["c1".to_string()].into_iter().collect();
        match next_action(&calls, &answered, 0, true) {
            RoundAction::AskApproval(tool) => assert_eq!(tool.id, "c2"),
            other => panic!("第一条处理完应当轮到第二条，得到 {other:?}"),
        }
    }

    #[std::prelude::v1::test]
    fn sensitive_reads_also_stop_for_approval() {
        let calls = vec![call("c1", "read_file", json!({"path": "~/.ssh/id_rsa"}))];
        assert!(matches!(
            next_action(&calls, &HashSet::new(), 0, true),
            RoundAction::AskApproval(_)
        ));
    }

    #[std::prelude::v1::test]
    fn round_limit_stops_the_loop() {
        let calls = vec![call("c1", "read_file", json!({"path": "a.rs"}))];
        match next_action(&calls, &HashSet::new(), MAX_AGENT_ROUNDS, true) {
            RoundAction::HitLimit(waiting) => assert_eq!(waiting.len(), 1, "没执行的调用要交出来补结果"),
            other => panic!("到上限应当停下，得到 {other:?}"),
        }
        // 差一轮的时候还能继续
        assert!(matches!(
            next_action(&calls, &HashSet::new(), MAX_AGENT_ROUNDS - 1, true),
            RoundAction::RunTools(_)
        ));
    }

    #[std::prelude::v1::test]
    fn disabled_tools_still_report_back() {
        // 工具没开也要回结果：不回的话历史里留着没回的调用，下一次请求会被拒绝
        let calls = vec![call("c1", "read_file", json!({"path": "a.rs"}))];
        match next_action(&calls, &HashSet::new(), 0, false) {
            RoundAction::Disabled(tools) => assert_eq!(tools.len(), 1),
            other => panic!("工具停用时也该产出结果，得到 {other:?}"),
        }
    }

    #[std::prelude::v1::test]
    fn unknown_tools_are_answered_without_asking() {
        // 模型编了个不存在的工具名：执行器什么都不做、只回「没有这个工具」，不必问用户
        let calls = vec![call("c1", "delete_everything", json!({}))];
        assert!(!local_tools::is_known("delete_everything"));
        assert!(matches!(
            next_action(&calls, &HashSet::new(), 0, true),
            RoundAction::RunTools(_)
        ));
        let result = local_tools::execute(&PendingTool::from_call(&calls[0]), &ExecControl::default());
        assert!(result.is_error);
        assert!(result.content.contains("delete_everything"));
        assert!(
            result.content.contains("read_file"),
            "要告诉模型有哪些工具可用：{}",
            result.content
        );
    }

    #[std::prelude::v1::test]
    fn mcp_tools_always_stop_for_approval() {
        // MCP 工具名在本机工具表里当然查不到，但**不能**因此被当成「模型编的名字」放行——
        // 调用跑在别人的服务器上，只能每次确认
        let calls = vec![call("c1", "mcp__files__read_file", json!({"path": "a.rs"}))];
        assert!(!local_tools::is_known("mcp__files__read_file"));
        match next_action(&calls, &HashSet::new(), 0, true) {
            RoundAction::AskApproval(tool) => assert_eq!(tool.name, "mcp__files__read_file"),
            other => panic!("MCP 工具应当每次都问，得到 {other:?}"),
        }
    }
}
