//! 本机可以执行的那几个工具：清单、参数校验、执行、以及权限判定。
//!
//! 三个职责都放在一起，因为它们改的永远是同一件事——加一个工具就要同时加
//! 声明、执行分支和权限级别；分成三个文件反而要跳着改。
//!
//! **这一层不碰 GPUI，也不知道 Agent 循环的存在**：它只回答两个问题——
//! 「现在有哪些工具」（[`specs`]）和「照着这份参数把它跑起来」（[`execute`]）。
//! 什么时候该问用户、结果怎么回传给模型，都是 `agent_loop.rs` 的事。
//!
//! 参数从 `ToolCall::arguments` 的 JSON 值直接读，不再像以前那样用 `:::` 拼字符串——
//! 模型给的是结构化 JSON，拆字符串既脆又没法报错。
//!
//! [`execute`] 会阻塞（读文件、等命令跑完），**调用方必须放到后台线程**。
//! 它自己保证两件事：命令有超时、能被「停止」打断（[`ExecControl`]）；
//! 回传的内容有长度上限，读一个大文件或者跑一条输出刷屏的命令，都不会把界面或模型的上下文撑爆。

use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::llm_tools::{ToolCall, ToolResult, ToolSpec};
use crate::session_tools::Permission;

/// 命令最长能跑多久，到点就结束整个进程树，把"超时"回传给模型。
///
/// 给得比较宽：编译、装依赖这类命令本来就要几分钟。真卡住了（比如模型启动了一个
/// 不会自己退出的服务），用户随时可以点「停止」，不用等到超时。
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(300);
/// `git status` 这种查询超过半分钟，多半是卡在了凭据提示或者超大仓库上
const GIT_TIMEOUT: Duration = Duration::from_secs(30);
/// 一条结果最多回传这么多字符，超出的部分从中间截掉：
/// 命令的报错通常在结尾，文件和列表的结构在开头，两头都留着
pub const MAX_RESULT_CHARS: usize = 30_000;
/// `read_file` 最多读这么多字节。再大的文件模型也看不完，只读开头
const MAX_READ_BYTES: u64 = 256 * 1024;
/// 目录最多列出这么多项
const MAX_LIST_ENTRIES: usize = 500;
/// 子进程的每路输出最多保留这么多字节。超出的照样读走再丢掉——
/// 不读的话子进程写满管道就会卡住，永远不退出
const MAX_CAPTURE_BYTES: usize = 1024 * 1024;

/// 执行一条工具时的外部控制。
#[derive(Clone, Debug)]
pub struct ExecControl {
    /// 用户点了「停止」就置为 true，正在跑的命令会被结束
    pub cancel: Arc<AtomicBool>,
    /// `run_command` 的超时
    pub command_timeout: Duration,
    /// 这次执行归属的工作目录。没有它就不该执行任何本机工具——
    /// 闸门在 `tool_ops::session_tool_specs`，这里只是最后一道「万一还是被调到」的兜底。
    pub workspace: Option<ProjectDir>,
}

impl Default for ExecControl {
    fn default() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            command_timeout: COMMAND_TIMEOUT,
            workspace: None,
        }
    }
}

impl ExecControl {
    /// 按设置里的秒数构造。下限一秒——写成 0 会让每条命令刚启动就被判超时。
    pub fn with_timeout_secs(secs: u64) -> Self {
        Self {
            command_timeout: Duration::from_secs(secs.max(1)),
            ..Self::default()
        }
    }

    /// 带上这次执行的工作目录。
    pub fn in_workspace(mut self, workspace: Option<ProjectDir>) -> Self {
        self.workspace = workspace;
        self
    }
}

/// 智能体干活的那个目录。**它就是边界**。
///
/// 以前相对路径按「程序从哪个目录启动」算，装好的程序就是安装目录，基本没法用；
/// 而"工作目录之外"的判断也一直拿进程的当前目录当基准，等于没有边界。
/// 现在基准由会话指定（`SessionTools::workspace`），
/// 它同时管三件事：相对路径从哪算、命令在哪跑、什么算「外面」。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectDir {
    root: PathBuf,
}

impl ProjectDir {
    /// 从会话里存的字符串构造。
    ///
    /// 空串、以及**相对路径**都当没设：相对路径的基准本身就不确定，拿它当边界等于没有边界。
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let path = Path::new(trimmed);
        path.is_absolute().then(|| Self { root: normalize(path) })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 把工具参数里的路径解析成绝对路径：相对路径按工作目录算。
    pub fn resolve(&self, raw: &str) -> PathBuf {
        let path = Path::new(raw);
        if path.is_absolute() {
            normalize(path)
        } else {
            normalize(&self.root.join(path))
        }
    }

    /// 路径在不在边界内。**不访问文件系统**（目标可能还不存在）。
    pub fn contains(&self, raw: &str) -> bool {
        is_under(&self.resolve(raw), &self.root)
    }

    /// 给界面显示的短名：最后一段目录名。取不到就退回完整路径。
    pub fn display_name(&self) -> String {
        self.root
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| self.root.display().to_string())
    }
}

/// 进程当前目录的字符串形式，给测试当项目目录用。
///
/// 为什么不写死一个盘符：`ProjectDir::parse` 只认绝对路径，而 `C:/work` 在非 Windows
/// 上是相对路径，测试会莫名其妙地退化成「没设目录」。拿当前目录就没这个问题，
/// 而且 `a.rs` 这类相对路径照样落在边界内，测试不必改路径写法。
#[cfg(test)]
pub(crate) fn current_dir_string() -> String {
    std::env::current_dir().expect("拿不到当前目录").display().to_string()
}

/// `path` 是否就是 `root` 或者它的子孙。
///
/// 按组件比而不是字符串前缀：`E:\a` 不该匹配上 `E:\ab`。
/// Windows 上还要忽略大小写——用户和模型都可能在盘符或目录名上换个大小写，
/// 按字节比会把「明明在目录里」判成「在外面」，白问一次。
fn is_under(path: &Path, root: &Path) -> bool {
    let mut rest = path.components();
    for expected in root.components() {
        match rest.next() {
            Some(actual) if component_eq(actual, expected) => {}
            _ => return false,
        }
    }
    true
}

#[cfg(target_os = "windows")]
fn component_eq(a: Component<'_>, b: Component<'_>) -> bool {
    a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
}

#[cfg(not(target_os = "windows"))]
fn component_eq(a: Component<'_>, b: Component<'_>) -> bool {
    a == b
}

/// 工具执行前需要用户点头的程度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guard {
    /// 只读、不碰网络和系统命令，直接跑。
    Free,
    /// 会执行任意命令或改文件内容，每次都要用户确认。
    NeedsApproval,
}

/// 一个工具的完整定义：给模型看的声明、给界面看的名字、以及执行方式。
pub struct LocalTool {
    pub name: &'static str,
    pub spec: ToolSpec,
    pub guard: Guard,
}

/// 拿到当前可用的工具清单，交给 [`ChatRequest::tools`](crate::llm::ChatRequest::tools)。
///
/// 顺序固定（不是随机或 HashMap 顺序）：同样的工具集每次序列化出来逐字节一致，
/// 对端才能命中 prompt 缓存。
pub fn specs() -> Vec<ToolSpec> {
    all().into_iter().map(|tool| tool.spec).collect()
}

/// 查一个工具的权限级别。**名字不认识时按最严处理**——
/// 模型可能编出一个不存在的工具名，那种情况不该被当成"只读"放过去。
pub fn guard_for(name: &str) -> Guard {
    all()
        .into_iter()
        .find(|tool| tool.name == name)
        .map_or(Guard::NeedsApproval, |tool| tool.guard)
}

/// 名字是否是已知工具。
pub fn is_known(name: &str) -> bool {
    all().iter().any(|tool| tool.name == name)
}

fn all() -> Vec<LocalTool> {
    vec![
        LocalTool {
            name: "list_directory",
            guard: Guard::Free,
            spec: ToolSpec::new(
                "list_directory",
                "列出某个目录下的文件和子目录。目录名后面带 / 的是子目录。",
                json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "要列出的目录路径。留空表示当前工作目录。"
                        }
                    },
                    "required": []
                }),
            ),
        },
        LocalTool {
            name: "read_file",
            guard: Guard::Free,
            spec: ToolSpec::new(
                "read_file",
                "读取一个文本文件的内容。读到敏感文件（密钥、凭据、.env 之类）时需要用户确认。",
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "文件的完整路径或相对路径" }
                    },
                    "required": ["path"]
                }),
            ),
        },
        LocalTool {
            name: "git_status",
            guard: Guard::Free,
            spec: ToolSpec::no_args("git_status", "查看当前 git 仓库的改动（相当于 git status --short）。"),
        },
        LocalTool {
            name: "write_file",
            guard: Guard::NeedsApproval,
            spec: ToolSpec::new(
                "write_file",
                "把内容写入文件，会覆盖原有内容。需要用户确认。",
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "要写入的文件路径" },
                        "content": { "type": "string", "description": "写入的完整内容" }
                    },
                    "required": ["path", "content"]
                }),
            ),
        },
        LocalTool {
            name: "run_command",
            guard: Guard::NeedsApproval,
            spec: ToolSpec::new(
                "run_command",
                "在本机执行一条 shell 命令并返回输出。需要用户确认。",
                json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "要执行的命令" }
                    },
                    "required": ["command"]
                }),
            ),
        },
    ]
}

/// 一条待执行的调用：把它的参数和授权状态一起带着，防止用户在卡片上点了同意、
/// 结果执行的却是另一条（参数在流式期间还会变）。
#[derive(Clone, Debug, PartialEq)]
pub struct PendingTool {
    /// 对应 `ToolCall::id`
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

impl PendingTool {
    pub fn from_call(call: &ToolCall) -> Self {
        Self {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        }
    }

    /// 把参数补全成一条完整的 `ToolCall`——执行时要用 `arguments_text` 编回去。
    fn as_call(&self) -> ToolCall {
        ToolCall {
            id: self.id.clone(),
            name: self.name.clone(),
            arguments: self.arguments.clone(),
        }
    }

    /// 参数里的某个字符串字段。类型不对时返回 `None`，由调用方报成一条错误结果。
    pub(crate) fn string_arg(&self, key: &str) -> Option<&str> {
        // 模型偶尔会把参数整个发成字符串（`{"_raw": "..."}`），那种情况下参数不是对象，
        // 这里取不到就是取不到，让执行器报一条清楚的错误，别 panic。
        self.arguments.as_object()?.get(key)?.as_str()
    }

    /// 这条调用执行前要不要用户确认。
    ///
    /// 三道，任何一道命中都要问：
    ///
    /// 1. 工具本身的级别（写文件、跑命令一律要确认）；
    /// 2. **路径落在工作目录之外**——这才是真正的边界。以前只有下面那条"敏感文件判断"，
    ///    那只是防呆：模型换个路径就绕过去了；
    /// 3. 读的是敏感文件（`.env`、密钥之类），即使就在工作目录里也问一次。
    ///
    /// **完全权限**（`Permission::Full`）把这三道一起跳过——除了第 4 条硬底线：
    /// 带 `path` 参数、而且指向 Perch 自己的数据目录，仍然要问。数据目录里放着渠道配置、
    /// 会话库和 API Key 的引用，让模型改这些等于让模型控制程序本身的行为
    /// （产品决策，见 AGENTS.md §11 第 2 条）。
    pub fn needs_approval(&self, workspace: Option<&ProjectDir>, permission: Permission) -> bool {
        if permission == Permission::Full {
            return self.touches_the_data_dir(workspace);
        }
        match guard_for(&self.name) {
            Guard::NeedsApproval => true,
            Guard::Free => self.touches_path_outside(workspace) || self.reads_a_sensitive_path(),
        }
    }

    /// 参数里的路径是否落在 Perch 自己的数据目录里。
    ///
    /// ⚠️ 只对**带 `path` 参数的工具**有效。`run_command` 里手写一个数据目录的路径照样
    /// 绕得过去——所以「完全权限」的提示语必须说清「模型可以在你机器上做任何事」，
    /// 不能靠这一条兜底。它拦的是"顺手改配置"这类最容易被想到的做法。
    fn touches_the_data_dir(&self, workspace: Option<&ProjectDir>) -> bool {
        let Some(raw) = self.string_arg("path") else {
            return false;
        };
        let data_dir = crate::paths::data_dir();
        let resolved = match workspace {
            Some(dir) => dir.resolve(raw),
            // 没有工作目录就没法解析相对路径，按原样比——数据目录基本都是绝对路径写死的
            None => normalize(Path::new(raw)),
        };
        is_under(&resolved, data_dir)
    }

    /// 参数里的路径是否落在工作目录之外。
    ///
    /// **没有工作目录时一律算「在外」**：保守方向，多问一次不致命。
    /// `path` 留空表示工作目录自己，不算越界；没有 `path` 参数的工具（`git_status`）
    /// 取不到参数，也就不会因为这条被问。
    fn touches_path_outside(&self, workspace: Option<&ProjectDir>) -> bool {
        let Some(raw) = self.string_arg("path") else {
            return false;
        };
        if raw.trim().is_empty() {
            return false;
        }
        workspace.is_none_or(|workspace| !workspace.contains(raw))
    }

    fn reads_a_sensitive_path(&self) -> bool {
        self.string_arg("path")
            .is_some_and(|path| is_sensitive_path(Path::new(path)))
    }

    /// 参数压成一行给授权卡片和消息标签用（`path=a.rs, line=3`）。
    pub fn arguments_summary(&self) -> String {
        crate::llm_tools::tool_call_label(&self.as_call())
    }
}

/// 敏感文件判断。**这是防呆不是防线**——模型可以换条路径绕过去，
/// 真正的防线是「工作目录之外要授权」加上「读取要用户点一次同意」，
/// 这里只负责把明显该问的挑出来。
pub fn is_sensitive_path(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    // `.env` 开头的都算（`.env.production`、`.env.local` …），不止那一个名字
    let sensitive_name = name.starts_with(".env")
        || matches!(
            name.as_str(),
            "id_rsa" | "id_ed25519" | "id_ecdsa" | "id_dsa" | "credentials" | "credentials.json"
        );
    let sensitive_ext = matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "pem" | "key" | "pfx" | "p12"
    );
    // 数据目录里存着渠道配置和 API Key 的引用，一并在内
    let inside_data_dir = path.starts_with(crate::paths::data_dir());

    sensitive_name || sensitive_ext || inside_data_dir
}

/// 消掉 `.` 和 `..`，不访问文件系统。`canonicalize` 在这里不能用：文件可能还不存在。
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// 执行一条调用。无论成功失败都返回 [`ToolResult`]——
/// **执行失败不是流程失败**：把错误原文回传给模型，它下一轮能自己改。
///
/// 会阻塞，必须在后台线程调用（见模块说明）。
pub fn execute(pending: &PendingTool, control: &ExecControl) -> ToolResult {
    let started = Instant::now();
    // 文件操作不产生进程，退出码一律 `None`；只有 `git_status` / `run_command` 会给。
    let (content, is_error, exit_code) = match pending.name.as_str() {
        "list_directory" => {
            let (content, is_error) = list_directory(pending, control);
            (content, is_error, None)
        }
        "read_file" => {
            let (content, is_error) = read_file(pending, control);
            (content, is_error, None)
        }
        "git_status" => git_status(control),
        "write_file" => {
            let (content, is_error) = write_file(pending, control);
            (content, is_error, None)
        }
        "run_command" => run_command(pending, control),
        other => (unknown_tool_message(other), true, None),
    };
    ToolResult {
        id: pending.id.clone(),
        name: pending.name.clone(),
        content: truncate_middle(&content, MAX_RESULT_CHARS),
        is_error,
        duration_ms: started.elapsed().as_millis() as u64,
        exit_code,
    }
}

/// 模型调了一个不存在的工具：告诉它有哪些可用。
///
/// 只说"没有"不够——模型下一轮多半会换个名字再猜一次，列出清单它才能直接改对。
fn unknown_tool_message(name: &str) -> String {
    let available: Vec<&str> = all().iter().map(|tool| tool.name).collect();
    format!(
        "There is no tool named `{name}`. Available tools: {}.",
        available.join(", ")
    )
}

/// 没有执行的调用回传给模型的说明（用户点了停止、到了轮数上限……）。
///
/// 不回结果不行：各渠道都要求「带调用的助手消息后面跟着每个调用的结果」，
/// 缺一个整个请求就会被拒绝。原因写清楚，模型下一轮才知道该怎么接。
pub fn not_executed_result(pending: &PendingTool, reason: &str) -> ToolResult {
    ToolResult {
        id: pending.id.clone(),
        name: pending.name.clone(),
        content: format!("Not executed: {reason}"),
        is_error: true,
        // 没跑过，谈不上耗时和退出码
        duration_ms: 0,
        exit_code: None,
    }
}

/// 上层判定「这条调用不该执行」时，用一句现成的说明包成失败结果。
///
/// 和 [`not_executed_result`] 的区别：那条是「本来要跑、但没跑成」（用户点了停止、
/// 到了轮数上限），内容自带 `Not executed:` 前缀；这条是「压根不认识这个工具名」，
/// 说明由上层给全（只有上层知道该列本机的清单还是 MCP 的清单）。
pub fn error_result(pending: &PendingTool, message: String) -> ToolResult {
    ToolResult {
        id: pending.id.clone(),
        name: pending.name.clone(),
        content: truncate_middle(&message, MAX_RESULT_CHARS),
        is_error: true,
        // 没执行过，谈不上耗时和退出码
        duration_ms: 0,
        exit_code: None,
    }
}

/// 用户拒绝时回传给模型的收尾话术。
/// 用一句固定的英文而不是界面译文：**它是给模型看的内容，不是给用户看的**，
/// 和 `llm_request::effective_message_text` 的取向一致；跟着界面语言变反而会让
/// 模型在中文语境下收到一句中文、英文语境下收到英文，行为不稳定。
pub fn denial_result(pending: &PendingTool) -> ToolResult {
    ToolResult {
        id: pending.id.clone(),
        name: pending.name.clone(),
        content: format!("User denied permission to run `{}`.", pending.name),
        is_error: true,
        duration_ms: 0,
        exit_code: None,
    }
}

/// 工具没启用时回传给模型的说明。
pub fn disabled_result(pending: &PendingTool) -> ToolResult {
    ToolResult {
        id: pending.id.clone(),
        name: pending.name.clone(),
        content: format!(
            "Local tools are disabled in the user's settings, so `{}` cannot run. \
             Tell the user they can enable them in Settings → General.",
            pending.name
        ),
        is_error: true,
        duration_ms: 0,
        exit_code: None,
    }
}

/// 取这次执行的工作目录。拿不到就回一条说明。
///
/// 正常情况下走不到这里：没设项目目录时，清单里根本不会有本机工具
/// （见 `tool_ops::session_tool_specs`）。这是最后一道兜底——
/// 万一被绕到这里，宁可回一句"跑不了"，也不要拿程序自己的目录凑数。
fn workspace_or_error(control: &ExecControl) -> Result<&ProjectDir, String> {
    control.workspace.as_ref().ok_or_else(|| {
        "This session has no project folder set, so file and command tools are unavailable. \
         Ask the user to pick one first."
            .to_string()
    })
}

fn list_directory(pending: &PendingTool, control: &ExecControl) -> (String, bool) {
    let workspace = match workspace_or_error(control) {
        Ok(workspace) => workspace,
        Err(message) => return (message, true),
    };
    let raw = pending
        .string_arg("path")
        .filter(|path| !path.trim().is_empty())
        .unwrap_or(".");
    // 相对路径按工作目录算——这是这次改动最实在的一条：以前按「程序从哪个目录启动」算，
    // 装好的程序就是安装目录，模型说「看看这个项目」它会去翻 Perch 自己的目录。
    let path = workspace.resolve(raw);
    let dir = path.display().to_string();
    match fs::read_dir(&path) {
        Ok(entries) => {
            let mut items = Vec::new();
            for entry in entries.flatten() {
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    items.push(format!("{name}/"));
                } else {
                    items.push(name);
                }
            }
            items.sort();
            if items.is_empty() {
                return (format!("Directory {dir} is empty."), false);
            }
            let total = items.len();
            if total > MAX_LIST_ENTRIES {
                items.truncate(MAX_LIST_ENTRIES);
                items.push(format!("... ({} more entries not shown)", total - MAX_LIST_ENTRIES));
            }
            (items.join("\n"), false)
        }
        Err(error) => (format!("Failed to list directory {dir}: {error}"), true),
    }
}

fn read_file(pending: &PendingTool, control: &ExecControl) -> (String, bool) {
    let workspace = match workspace_or_error(control) {
        Ok(workspace) => workspace,
        Err(message) => return (message, true),
    };
    let Some(raw) = pending.string_arg("path") else {
        return ("Missing parameter `path`, or it is not a string.".to_string(), true);
    };
    let path = workspace.resolve(raw);
    let path_text = path.display().to_string();
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) => return (format!("Failed to read file {path_text}: {error}"), true),
    };
    let total = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    // 只读开头一段：几百 MB 的日志整个读进内存，界面和模型都受不了
    let mut bytes = Vec::new();
    if let Err(error) = file.take(MAX_READ_BYTES).read_to_end(&mut bytes) {
        return (format!("Failed to read file {path_text}: {error}"), true);
    }
    // 文本文件里不会有 NUL 字节。二进制内容转成文字只是一堆乱码，还白白占上下文
    if bytes.contains(&0) {
        return (
            format!("{path_text} looks like a binary file; only text files can be read."),
            true,
        );
    }
    // 不是 UTF-8 的文本（比如 GBK）也照样给出来，乱掉的字符用替换符，总比整个报错强
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    let mut truncated = total > MAX_READ_BYTES;
    if text.chars().count() > MAX_RESULT_CHARS {
        text = text.chars().take(MAX_RESULT_CHARS).collect();
        truncated = true;
    }
    if truncated {
        text.push_str(&format!(
            "\n\n[Truncated: showing the beginning of a {} KB file.]",
            total.div_ceil(1024)
        ));
    }
    (text, false)
}

fn git_status(control: &ExecControl) -> (String, bool, Option<i32>) {
    let workspace = match workspace_or_error(control) {
        Ok(workspace) => workspace,
        Err(message) => return (message, true, None),
    };
    let mut command = Command::new("git");
    command.args(["status", "--short"]);
    // 在项目目录里跑，不是程序自己的目录
    command.current_dir(workspace.root());
    match run_process(command, control, GIT_TIMEOUT) {
        Ok(output) if output.exit_code() == Some(0) => {
            let text = String::from_utf8_lossy(&output.stdout);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                ("Working tree clean; no changes.".to_string(), false, Some(0))
            } else {
                (trimmed.to_string(), false, Some(0))
            }
        }
        // 不在仓库里、超时、被停止：把 git 自己的说法原样交出去，别当成"干净"
        Ok(output) => {
            let code = output.exit_code();
            let (content, is_error) = describe_process_output(output, GIT_TIMEOUT);
            (content, is_error, code)
        }
        Err(error) => (format!("Failed to run git: {error}"), true, None),
    }
}

fn write_file(pending: &PendingTool, control: &ExecControl) -> (String, bool) {
    let workspace = match workspace_or_error(control) {
        Ok(workspace) => workspace,
        Err(message) => return (message, true),
    };
    let Some(raw) = pending.string_arg("path") else {
        return ("Missing parameter `path`, or it is not a string.".to_string(), true);
    };
    let Some(content) = pending.string_arg("content") else {
        return ("Missing parameter `content`, or it is not a string.".to_string(), true);
    };
    let path = workspace.resolve(raw);
    let path_text = path.display().to_string();
    match fs::write(&path, content) {
        Ok(()) => (format!("Wrote {path_text} ({} bytes).", content.len()), false),
        Err(error) => (format!("Failed to write file {path_text}: {error}"), true),
    }
}

fn run_command(pending: &PendingTool, control: &ExecControl) -> (String, bool, Option<i32>) {
    let workspace = match workspace_or_error(control) {
        Ok(workspace) => workspace,
        Err(message) => return (message, true, None),
    };
    let Some(command) = pending.string_arg("command") else {
        return (
            "Missing parameter `command`, or it is not a string.".to_string(),
            true,
            None,
        );
    };

    #[cfg(target_os = "windows")]
    let process = {
        let mut process = Command::new("powershell");
        // -NonInteractive：命令要是弹出确认或 Read-Host，直接报错而不是一直等输入
        process.args(["-NoProfile", "-NonInteractive", "-Command", command]);
        process
    };

    #[cfg(not(target_os = "windows"))]
    let process = {
        let mut process = Command::new("sh");
        process.args(["-c", command]);
        process
    };

    let mut process = process;
    // 命令在项目目录里跑：模型说「跑一下测试」，它该跑的是这个项目的测试
    process.current_dir(workspace.root());

    match run_process(process, control, control.command_timeout) {
        Ok(output) => {
            let code = output.exit_code();
            let (content, is_error) = describe_process_output(output, control.command_timeout);
            (content, is_error, code)
        }
        Err(error) => (format!("Failed to run command: {error}"), true, None),
    }
}

/// 子进程是怎么结束的。
#[derive(Debug, PartialEq)]
enum Outcome {
    /// 自己跑完了。值是退出码——Windows 上进程被强制结束时拿不到，所以是 `Option`。
    Exited(Option<i32>),
    TimedOut,
    Cancelled,
}

struct ProcessOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    outcome: Outcome,
}

impl ProcessOutput {
    /// 进程自己的退出码。超时、被停止、拿不到退出码时都是 `None`——
    /// 界面据此决定要不要显示「退出码」这一项。
    fn exit_code(&self) -> Option<i32> {
        match self.outcome {
            Outcome::Exited(code) => code,
            Outcome::TimedOut | Outcome::Cancelled => None,
        }
    }
}

/// 跑一个子进程，同时盯着超时和「停止」。到点或被停止就结束整个进程树。
///
/// 不用 `Command::output()`：它会一直等到进程退出，模型要是跑了一条
/// `ping -t` 或者启动了一个开发服务器，就永远等不回来。
fn run_process(mut command: Command, control: &ExecControl, timeout: Duration) -> std::io::Result<ProcessOutput> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console_window(&mut command);
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().map(read_in_background);
    let stderr = child.stderr.take().map(read_in_background);

    let started = Instant::now();
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Outcome::Exited(status.code()),
            Ok(None) => {}
            Err(error) => {
                kill_process_tree(&mut child);
                return Err(error);
            }
        }
        if control.cancel.load(Ordering::Relaxed) {
            kill_process_tree(&mut child);
            break Outcome::Cancelled;
        }
        if started.elapsed() >= timeout {
            kill_process_tree(&mut child);
            break Outcome::TimedOut;
        }
        thread::sleep(Duration::from_millis(50));
    };

    Ok(ProcessOutput {
        stdout: collect_output(stdout),
        stderr: collect_output(stderr),
        outcome,
    })
}

/// 在单独的线程里把一路输出读完，只留前 [`MAX_CAPTURE_BYTES`] 字节。
fn read_in_background(mut pipe: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut kept = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let room = MAX_CAPTURE_BYTES.saturating_sub(kept.len());
                    kept.extend_from_slice(&buffer[..read.min(room)]);
                }
            }
        }
        // 接收端可能已经不等了（见 collect_output 的超时），发不出去也无所谓
        let _ = tx.send(kept);
    });
    rx
}

/// 取回后台读到的输出。
///
/// 进程结束后最多再等两秒：它启动的孙进程可能还拿着管道不放（比如后台服务），
/// 那样读线程永远等不到结尾。宁可少几行输出，也不能让整个工具调用卡在这里。
fn collect_output(receiver: Option<mpsc::Receiver<Vec<u8>>>) -> Vec<u8> {
    receiver
        .and_then(|receiver| receiver.recv_timeout(Duration::from_secs(2)).ok())
        .unwrap_or_default()
}

/// 结束进程和它启动的所有子进程。
fn kill_process_tree(child: &mut Child) {
    kill_process_tree_by_pid(child.id());
    // 同上：进程已经退出时这两步会失败，属于预期情况
    let _ = child.kill();
    let _ = child.wait();
}

/// 按 pid 结束整棵进程树。
///
/// 只杀父进程的话，它启动的程序（`ping -t`、开发服务器）会继续在后台跑。
/// 单独抽出来是因为 MCP 服务器的子进程不归本模块管（句柄在 rmcp 手里），
/// 但用户关掉那台服务器时同样要把整棵树收掉，而那时手上只剩一个 pid。
///
/// **同步实现**：调用点可能在界面线程上（点「删除服务器」），那里没有 tokio 上下文，await 不了。
pub fn kill_process_tree_by_pid(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let mut taskkill = Command::new("taskkill");
        taskkill
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_console_window(&mut taskkill);
        // 进程可能刚好自己退出了，taskkill 失败无所谓
        let _ = taskkill.status();
    }
    // 非 Windows 上暂时什么都不做：这里没有拿到子进程句柄，也就没法 kill。
    // 需要时再按平台补（Linux 可以用进程组，macOS 同）。
    #[cfg(not(target_os = "windows"))]
    let _ = pid;
}

/// Windows 上图形界面程序启动控制台程序时，默认会弹出一个黑色的命令行窗口。
fn hide_console_window(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = command;
}

/// 把子进程的输出和结束方式整理成一条结果。
fn describe_process_output(output: ProcessOutput, timeout: Duration) -> (String, bool) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut combined = match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (true, true) => "(No output)".to_string(),
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (false, false) => format!("{stdout}\n{stderr}"),
    };
    let is_error = match output.outcome {
        Outcome::Exited(code) => code != Some(0),
        Outcome::TimedOut => {
            combined.push_str(&format!(
                "\n\n[Stopped: the command did not finish within {} seconds.]",
                timeout.as_secs()
            ));
            true
        }
        Outcome::Cancelled => {
            combined.push_str("\n\n[Stopped by the user.]");
            true
        }
    };
    (combined, is_error)
}

/// 超过上限时保留开头和结尾，中间注明省略了多少。
pub fn truncate_middle(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }
    let head = max_chars * 2 / 3;
    let tail = max_chars - head;
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(total - tail).collect();
    format!(
        "{start}\n\n[... {} characters omitted ...]\n\n{end}",
        total - head - tail
    )
}

/// 给模型看的「这次在哪儿干活」说明，拼进 system 消息。
///
/// 只在**真的带了本机工具**时才加（见 `reply_ops::make_job`）：没给工具却告诉它有个
/// 项目目录，它会以为能读文件，白跑一轮再被拒。
///
/// 内容不跟着界面语言走——它和工具描述、工具输出一样是请求体的一部分，
/// 同一个会话换个界面语言就变成另一个请求体，prompt 缓存会失效
/// （取向见 `i18n_skip.txt` 第三类）。
pub fn environment_preamble(workspace: &ProjectDir) -> String {
    let shell = if cfg!(target_os = "windows") {
        "PowerShell (invoked as `powershell -NoProfile -NonInteractive -Command`)"
    } else {
        "the system shell (invoked as `sh -c`)"
    };
    format!(
        "# Environment\n\
         \n\
         - Operating system: {os}\n\
         - Shell used by `run_command`: {shell}\n\
         - Project folder (working directory): {dir}\n\
         \n\
         Relative paths in tool arguments are resolved against the project folder. \
         Keep your work inside it unless the user asks otherwise; touching anything \
         outside it may require the user's approval.",
        os = std::env::consts::OS,
        dir = workspace.root().display(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(name: &str, arguments: Value) -> PendingTool {
        PendingTool {
            id: "call_1".into(),
            name: name.into(),
            arguments,
        }
    }

    /// 测试用的项目目录：拿进程当前目录当根（就是 crate 根）。
    ///
    /// 这样 `src/main.rs`、`.` 这类相对路径都落在边界内，测试不必改路径写法；
    /// 要造"在外面"的路径就拼临时目录或者上一级。
    fn test_dir() -> ProjectDir {
        ProjectDir::parse(&current_dir_string()).expect("当前目录是绝对路径")
    }

    fn execute_default(pending: &PendingTool) -> ToolResult {
        let control = ExecControl::default().in_workspace(Some(test_dir()));
        execute(pending, &control)
    }

    #[test]
    fn commands_report_their_exit_code() {
        // 非零退出码要原样带出来：界面显示「退出码 3」比笼统一个"失败"有用得多
        let failed = execute_default(&pending("run_command", json!({"command": "exit 3"})));
        assert!(failed.is_error, "非零退出码要标成错误");
        assert_eq!(failed.exit_code, Some(3));

        let ok = execute_default(&pending("run_command", json!({"command": "exit 0"})));
        assert!(!ok.is_error);
        assert_eq!(ok.exit_code, Some(0));
    }

    #[test]
    fn file_tools_have_a_duration_but_no_exit_code() {
        // 文件操作不产生进程，没有退出码可报；耗时则每条都记
        let result = execute_default(&pending("list_directory", json!({"path": "."})));
        assert_eq!(result.exit_code, None);
        assert!(
            result.duration_ms < 10_000,
            "耗时不该是个荒唐的值：{}",
            result.duration_ms
        );
    }

    #[test]
    fn calls_that_never_ran_carry_no_timing() {
        // 停止 / 拒绝：没执行过就不该有耗时和退出码，否则界面会显示一个假的「0ms」
        let call = pending("run_command", json!({"command": "echo hi"}));
        let stopped = not_executed_result(&call, "stopped by the user.");
        assert_eq!(stopped.duration_ms, 0);
        assert_eq!(stopped.exit_code, None);

        let denied = denial_result(&call);
        assert_eq!(denied.duration_ms, 0);
        assert_eq!(denied.exit_code, None);
    }

    #[test]
    fn command_timeout_never_drops_below_a_second() {
        // 配置里写成 0 会让每条命令刚启动就被判超时
        assert_eq!(
            ExecControl::with_timeout_secs(0).command_timeout,
            Duration::from_secs(1)
        );
        assert_eq!(
            ExecControl::with_timeout_secs(120).command_timeout,
            Duration::from_secs(120)
        );
    }

    #[test]
    fn long_output_keeps_both_ends() {
        let text = format!("HEAD{}TAIL", "x".repeat(100));
        let cut = truncate_middle(&text, 30);
        assert!(cut.starts_with("HEAD"), "开头要留着：{cut}");
        assert!(cut.ends_with("TAIL"), "结尾要留着：{cut}");
        assert!(cut.contains("characters omitted"));
        assert_eq!(truncate_middle("short", 30), "short");
    }

    #[test]
    fn large_files_are_cut_to_the_beginning() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.log");
        let line = "0123456789abcdefghij\n";
        fs::write(&path, line.repeat(40_000)).unwrap();

        let result = execute_default(&pending("read_file", json!({"path": path.to_str().unwrap()})));
        assert!(!result.is_error);
        assert!(result.content.starts_with("0123456789"));
        assert!(result.content.contains("[Truncated"), "要告诉模型只看到了开头");
        assert!(result.content.chars().count() <= MAX_RESULT_CHARS + 200);
    }

    #[test]
    fn binary_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image.bin");
        fs::write(&path, [0x89u8, b'P', b'N', b'G', 0, 0, 1, 2]).unwrap();
        let result = execute_default(&pending("read_file", json!({"path": path.to_str().unwrap()})));
        assert!(result.is_error);
        assert!(result.content.contains("binary"));
    }

    #[test]
    fn huge_directories_are_capped() {
        let dir = tempfile::tempdir().unwrap();
        for ix in 0..MAX_LIST_ENTRIES + 20 {
            fs::write(dir.path().join(format!("f{ix:04}.txt")), "").unwrap();
        }
        let result = execute_default(&pending(
            "list_directory",
            json!({"path": dir.path().to_str().unwrap()}),
        ));
        assert!(!result.is_error);
        assert_eq!(result.content.lines().count(), MAX_LIST_ENTRIES + 1);
        assert!(result.content.ends_with("(20 more entries not shown)"));
    }

    /// 不会自己结束的命令必须能被超时结束，而且不能把调用方一直挂着。
    #[test]
    fn commands_stop_at_the_timeout() {
        #[cfg(target_os = "windows")]
        let command = "Start-Sleep -Seconds 30";
        #[cfg(not(target_os = "windows"))]
        let command = "sleep 30";
        let control = ExecControl {
            command_timeout: Duration::from_secs(1),
            ..ExecControl::default()
        }
        .in_workspace(Some(test_dir()));
        let started = Instant::now();
        let result = execute(&pending("run_command", json!({ "command": command })), &control);
        assert!(started.elapsed() < Duration::from_secs(15), "超时之后应当马上返回");
        assert!(result.is_error);
        assert!(
            result.content.contains("did not finish within 1 seconds"),
            "{}",
            result.content
        );
    }

    #[test]
    fn commands_stop_when_the_user_cancels() {
        #[cfg(target_os = "windows")]
        let command = "Start-Sleep -Seconds 30";
        #[cfg(not(target_os = "windows"))]
        let command = "sleep 30";
        let control = ExecControl::default().in_workspace(Some(test_dir()));
        let cancel = control.cancel.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            cancel.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        let result = execute(&pending("run_command", json!({ "command": command })), &control);
        assert!(started.elapsed() < Duration::from_secs(15), "点了停止应当马上返回");
        assert!(result.is_error);
        assert!(result.content.contains("Stopped by the user"), "{}", result.content);
    }

    #[test]
    fn not_executed_results_explain_why() {
        let result = not_executed_result(
            &pending("run_command", json!({"command": "ls"})),
            "stopped by the user.",
        );
        assert!(result.is_error);
        assert_eq!(result.id, "call_1");
        assert!(result.content.starts_with("Not executed: stopped by the user."));
    }

    #[test]
    fn specs_are_stable_and_well_formed() {
        let first = specs();
        let second = specs();
        assert_eq!(first.len(), 5);
        // 顺序必须稳定，否则每次请求体都不一样，prompt 缓存全失效
        assert_eq!(
            first.iter().map(|spec| spec.name.as_str()).collect::<Vec<_>>(),
            second.iter().map(|spec| spec.name.as_str()).collect::<Vec<_>>()
        );
        for spec in &first {
            assert!(!spec.description.trim().is_empty(), "{} 缺说明", spec.name);
            assert_eq!(spec.parameters["type"], "object", "{} 的参数不是对象 schema", spec.name);
        }
    }

    #[test]
    fn unknown_tools_are_treated_as_risky() {
        // 模型可能编出不存在的工具名，不能因为"查不到"就当成只读放过去
        assert_eq!(guard_for("rm_rf_everything"), Guard::NeedsApproval);
        assert!(!is_known("rm_rf_everything"));
        assert!(is_known("read_file"));
    }

    #[test]
    fn sensitive_paths_need_approval() {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_else(|_| "C:/Users/nobody".to_string());
        let key = format!("{home}/.ssh/id_rsa");
        assert!(is_sensitive_path(Path::new(&key)), "私钥应当算敏感");
        assert!(is_sensitive_path(Path::new("E:/proj/.env")));
        assert!(is_sensitive_path(Path::new("E:/proj/server.pem")));
    }

    #[test]
    fn ordinary_project_files_do_not_need_approval() {
        assert!(!is_sensitive_path(Path::new("src/main.rs")));
        assert!(!is_sensitive_path(Path::new("README.md")));
        assert!(!is_sensitive_path(Path::new("config/app.toml")));
    }

    #[test]
    fn read_and_list_are_free_but_execution_is_not() {
        let dir = test_dir();
        let inside = Some(&dir);
        assert!(!pending("list_directory", json!({"path": "src"})).needs_approval(inside, Permission::Default));
        assert!(!pending("read_file", json!({"path": "src/main.rs"})).needs_approval(inside, Permission::Default));
        assert!(!pending("git_status", json!({})).needs_approval(inside, Permission::Default));
        assert!(
            pending("write_file", json!({"path": "a.txt", "content": "x"})).needs_approval(inside, Permission::Default)
        );
        assert!(pending("run_command", json!({"command": "ls"})).needs_approval(inside, Permission::Default));
        // 读敏感文件也要点头，哪怕它就在项目目录里
        assert!(pending("read_file", json!({"path": "~/.ssh/id_rsa"})).needs_approval(inside, Permission::Default));
    }

    #[test]
    fn paths_outside_the_project_folder_need_approval() {
        // **这是真正的边界**：以前只有 `is_sensitive_path` 那道防呆，模型换个路径就绕过去了
        let dir = test_dir();
        let inside = Some(&dir);
        let outside = std::env::temp_dir().join("perch-outside.txt");
        let outside = outside.to_str().expect("临时目录是 UTF-8");

        assert!(
            !pending("read_file", json!({ "path": "src/main.rs" })).needs_approval(inside, Permission::Default),
            "目录里的普通文件不该问"
        );
        assert!(
            pending("read_file", json!({ "path": outside })).needs_approval(inside, Permission::Default),
            "目录之外的文件该问"
        );
        assert!(
            pending("list_directory", json!({ "path": outside })).needs_approval(inside, Permission::Default),
            "列目录同样是在看你机器上的东西，也要问"
        );
    }

    #[test]
    fn without_a_project_folder_every_path_counts_as_outside() {
        // 没有工作目录就没有基准，保守方向：一律当成在外
        assert!(pending("read_file", json!({"path": "src/main.rs"})).needs_approval(None, Permission::Default));
        // `path` 留空表示"就是工作目录自己"，这种情况不算越界
        assert!(!pending("list_directory", json!({"path": ""})).needs_approval(None, Permission::Default));
    }

    #[test]
    fn full_permission_skips_every_approval_except_the_data_dir() {
        let dir = test_dir();
        let inside = Some(&dir);
        let outside = std::env::temp_dir().join("perch-outside.txt");
        let outside = outside.to_str().expect("临时目录是 UTF-8");

        // 默认档下要问的，完全权限下都直接放行
        for (name, args) in [
            ("write_file", json!({"path": "a.txt", "content": "x"})),
            ("run_command", json!({"command": "ls"})),
            ("read_file", json!({"path": outside})),
            ("read_file", json!({"path": "~/.ssh/id_rsa"})),
        ] {
            assert!(
                !pending(name, args.clone()).needs_approval(inside, Permission::Full),
                "完全权限下 {name} 不该再问：{args}"
            );
        }

        // 硬底线：Perch 自己的数据目录仍然拦
        let config = crate::paths::data_dir().join("perch-config.json");
        assert!(
            pending(
                "write_file",
                json!({"path": config.display().to_string(), "content": "{}"})
            )
            .needs_approval(inside, Permission::Full),
            "完全权限下改 Perch 自己的配置仍然要问"
        );
        assert!(
            pending(
                "read_file",
                json!({"path": crate::paths::data_dir().join("perch.db").display().to_string()})
            )
            .needs_approval(inside, Permission::Full),
            "读会话库也要问"
        );
    }

    #[test]
    fn a_project_folder_must_be_absolute() {
        // 相对路径的基准本身就不确定，拿它当边界等于没有边界
        assert!(ProjectDir::parse("").is_none());
        assert!(ProjectDir::parse("   ").is_none());
        assert!(ProjectDir::parse("proj").is_none());
        assert!(ProjectDir::parse("./proj").is_none());
        assert!(ProjectDir::parse("E:/proj").is_some() || ProjectDir::parse("/tmp").is_some());
    }

    #[test]
    fn relative_paths_are_resolved_against_the_project_folder() {
        let dir = ProjectDir::parse(if cfg!(target_os = "windows") {
            "C:/work"
        } else {
            "/work"
        })
        .expect("绝对路径");
        let resolved = dir.resolve("src/main.rs");
        assert!(resolved.is_absolute());
        assert!(dir.contains("src/main.rs"));
        assert!(dir.contains("."), "工作目录自己也算在里面");
        // `..` 要能爬出去，否则模型写个 `../../x` 就绕过边界了
        assert!(!dir.contains("../../x"));
        assert!(!dir.contains(".."));
    }

    #[test]
    fn path_containment_compares_whole_components() {
        // 前缀比字符串会出事：`/work2` 不该被当成 `/work` 里面
        let dir = ProjectDir::parse(if cfg!(target_os = "windows") {
            "C:/work"
        } else {
            "/work"
        })
        .expect("绝对路径");
        let sibling = if cfg!(target_os = "windows") {
            "C:/work2/a"
        } else {
            "/work2/a"
        };
        assert!(!dir.contains(sibling));
    }

    #[test]
    fn file_tools_refuse_to_run_without_a_project_folder() {
        // 兜底：没设项目目录时清单里根本不会有本机工具，正常走不到这里。
        // 万一被绕进来，宁可回一句"跑不了"，也不要拿程序自己的目录凑数。
        let control = ExecControl::default();
        for name in ["list_directory", "read_file", "write_file"] {
            let args = if name == "write_file" {
                json!({"path": "a.txt", "content": "x"})
            } else {
                json!({"path": "a.txt"})
            };
            let result = execute(&pending(name, args), &control);
            assert!(result.is_error, "{name} 没有工作目录时应当拒绝执行");
            assert!(
                result.content.contains("project folder"),
                "{name} 的说明要提到缺项目目录：{}",
                result.content
            );
        }
    }

    #[test]
    fn commands_run_inside_the_project_folder() {
        // 以前按「程序从哪个目录启动」算，装好的程序就是安装目录——
        // 模型说「看看这个项目」，它会去翻 Perch 自己的目录
        let temp = tempfile::tempdir().expect("建临时目录");
        let dir = ProjectDir::parse(&temp.path().display().to_string()).expect("绝对路径");
        let control = ExecControl::default().in_workspace(Some(dir));
        let result = execute(&pending("run_command", json!({"command": "pwd"})), &control);
        assert!(!result.is_error, "{}", result.content);
        // 临时目录名是一串随机字符：它出现在输出里，就说明命令确实在那个目录里跑
        let marker = temp
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .expect("临时目录名");
        assert!(
            result.content.contains(marker),
            "命令该在项目目录里跑，得到 {}",
            result.content.trim()
        );
    }

    #[test]
    fn the_environment_preamble_names_the_project_folder() {
        let dir = ProjectDir::parse(if cfg!(target_os = "windows") {
            "C:/work"
        } else {
            "/work"
        })
        .expect("绝对路径");
        let text = environment_preamble(&dir);
        // `normalize` 会把分隔符统一成平台的形式，所以比对也用 `root()` 的显示形式
        assert!(text.contains(&dir.root().display().to_string()));
        assert!(text.contains("run_command"), "要说明命令走哪个 shell");
        assert!(text.contains("Operating system"));
    }

    #[test]
    fn bad_arguments_report_an_error_instead_of_panicking() {
        let result = execute_default(&pending("read_file", json!({})));
        assert!(result.is_error);
        assert!(result.content.contains("path"), "错误里要提到缺哪个参数");

        // 参数整个是字符串（模型发歪了）也不该炸
        let result = execute_default(&pending("run_command", json!("ls -la")));
        assert!(result.is_error);

        let result = execute_default(&pending("no_such_tool", json!({})));
        assert!(result.is_error);
        assert!(result.content.contains("no_such_tool"));
    }

    #[test]
    fn list_directory_validates_the_path() {
        let missing = execute_default(&pending(
            "list_directory",
            json!({"path": "E:/definitely/not/here/xyz"}),
        ));
        assert!(missing.is_error);
        assert!(!missing.content.is_empty());
    }

    #[test]
    fn denial_and_disabled_results_carry_the_tool_name() {
        let target = pending("run_command", json!({"command": "rm -rf /"}));
        let denied = denial_result(&target);
        assert!(denied.is_error);
        assert!(denied.content.contains("run_command"));
        // 拒绝理由是给模型看的，不跟着界面语言走
        assert!(denied.content.is_ascii());

        let disabled = disabled_result(&target);
        assert!(disabled.is_error);
        assert!(disabled.content.contains("run_command"));
    }

    #[test]
    fn error_results_keep_the_message_and_the_tool_name() {
        // 上层判定「不该执行」时走这条：说明是上层给全的，这里只负责装配
        let target = pending("mcp__srv__nope", json!({}));
        let result = error_result(&target, "There is no MCP tool named `mcp__srv__nope`.".to_string());
        assert!(result.is_error);
        assert_eq!(result.name, "mcp__srv__nope");
        assert!(result.content.contains("no MCP tool named"));
        // 没执行过，谈不上耗时和退出码
        assert_eq!(result.duration_ms, 0);
        assert_eq!(result.exit_code, None);
    }

    #[test]
    fn arguments_summary_reads_like_a_call() {
        let summary = pending("read_file", json!({"path": "src/main.rs"})).arguments_summary();
        assert!(summary.starts_with("read_file("), "得到 {summary}");
        assert!(summary.contains("path=src/main.rs"));
        assert_eq!(pending("git_status", json!({})).arguments_summary(), "git_status");
    }
}
