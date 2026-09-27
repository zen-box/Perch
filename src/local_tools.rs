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
}

impl Default for ExecControl {
    fn default() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            command_timeout: COMMAND_TIMEOUT,
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
    fn string_arg(&self, key: &str) -> Option<&str> {
        // 模型偶尔会把参数整个发成字符串（`{"_raw": "..."}`），那种情况下参数不是对象，
        // 这里取不到就是取不到，让执行器报一条清楚的错误，别 panic。
        self.arguments.as_object()?.get(key)?.as_str()
    }

    /// 这条调用执行前要不要用户确认。
    ///
    /// 除了工具本身的级别，读文件还有一条额外规则：**路径落在当前工作目录之外，
    /// 且看起来是敏感文件时也要确认**。否则模型可以直接把 `~/.ssh/id_rsa` 读走，
    /// 而这些都是免确认的只读工具。
    pub fn needs_approval(&self) -> bool {
        match guard_for(&self.name) {
            Guard::NeedsApproval => true,
            Guard::Free => self.name == "read_file" && self.reads_a_sensitive_path(),
        }
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

/// 敏感文件判断。**这是防呆不是防线**：模型可以换条路径绕过去，
/// 真正的防线是"读取要用户点一次同意"，这里只负责把明显该问的挑出来。
pub fn is_sensitive_path(path: &Path) -> bool {
    let outside_cwd = !is_inside_cwd(path);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let sensitive_name = matches!(
        name.as_str(),
        "id_rsa" | "id_ed25519" | "id_ecdsa" | "id_dsa" | ".env" | ".env.local" | "credentials" | "credentials.json"
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

    sensitive_name || sensitive_ext || inside_data_dir || (outside_cwd && name.starts_with(".env"))
}

/// 路径是否在当前工作目录下（不做 canonicalize，允许文件还不存在）。
fn is_inside_cwd(path: &Path) -> bool {
    let Ok(cwd) = std::env::current_dir() else {
        // 拿不到工作目录就没法判断"在外"，保守起见当成在外——多问一次不致命
        return false;
    };
    let absolute = if path.is_absolute() {
        normalize(path)
    } else {
        normalize(&cwd.join(path))
    };
    absolute.starts_with(normalize(&cwd))
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
            let (content, is_error) = list_directory(pending);
            (content, is_error, None)
        }
        "read_file" => {
            let (content, is_error) = read_file(pending);
            (content, is_error, None)
        }
        "git_status" => git_status(control),
        "write_file" => {
            let (content, is_error) = write_file(pending);
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

fn list_directory(pending: &PendingTool) -> (String, bool) {
    let dir = pending
        .string_arg("path")
        .filter(|path| !path.trim().is_empty())
        .unwrap_or(".");
    match fs::read_dir(dir) {
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
                return (format!("目录 {dir} 是空的。"), false);
            }
            let total = items.len();
            if total > MAX_LIST_ENTRIES {
                items.truncate(MAX_LIST_ENTRIES);
                items.push(format!("... ({} more entries not shown)", total - MAX_LIST_ENTRIES));
            }
            (items.join("\n"), false)
        }
        Err(error) => (format!("无法列出目录 {dir}：{error}"), true),
    }
}

fn read_file(pending: &PendingTool) -> (String, bool) {
    let Some(path) = pending.string_arg("path") else {
        return ("缺少参数 path，或者它不是字符串。".to_string(), true);
    };
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) => return (format!("无法读取文件 {path}：{error}"), true),
    };
    let total = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    // 只读开头一段：几百 MB 的日志整个读进内存，界面和模型都受不了
    let mut bytes = Vec::new();
    if let Err(error) = file.take(MAX_READ_BYTES).read_to_end(&mut bytes) {
        return (format!("无法读取文件 {path}：{error}"), true);
    }
    // 文本文件里不会有 NUL 字节。二进制内容转成文字只是一堆乱码，还白白占上下文
    if bytes.contains(&0) {
        return (
            format!("{path} looks like a binary file; only text files can be read."),
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
    let mut command = Command::new("git");
    command.args(["status", "--short"]);
    match run_process(command, control, GIT_TIMEOUT) {
        Ok(output) if output.exit_code() == Some(0) => {
            let text = String::from_utf8_lossy(&output.stdout);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                ("工作区干净，没有改动。".to_string(), false, Some(0))
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
        Err(error) => (format!("无法执行 git：{error}"), true, None),
    }
}

fn write_file(pending: &PendingTool) -> (String, bool) {
    let Some(path) = pending.string_arg("path") else {
        return ("缺少参数 path，或者它不是字符串。".to_string(), true);
    };
    let Some(content) = pending.string_arg("content") else {
        return ("缺少参数 content，或者它不是字符串。".to_string(), true);
    };
    match fs::write(path, content) {
        Ok(()) => (format!("已写入 {path}（{} 字节）。", content.len()), false),
        Err(error) => (format!("无法写入文件 {path}：{error}"), true),
    }
}

fn run_command(pending: &PendingTool, control: &ExecControl) -> (String, bool, Option<i32>) {
    let Some(command) = pending.string_arg("command") else {
        return ("缺少参数 command，或者它不是字符串。".to_string(), true, None);
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

    match run_process(process, control, control.command_timeout) {
        Ok(output) => {
            let code = output.exit_code();
            let (content, is_error) = describe_process_output(output, control.command_timeout);
            (content, is_error, code)
        }
        Err(error) => (format!("无法执行命令：{error}"), true, None),
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
    // 只结束 powershell 本身的话，它启动的程序（比如 ping -t）会继续在后台跑
    #[cfg(target_os = "windows")]
    {
        let mut taskkill = Command::new("taskkill");
        taskkill
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_console_window(&mut taskkill);
        // 进程可能刚好自己退出了，taskkill 失败无所谓，下面还有 kill 兜底
        let _ = taskkill.status();
    }
    // 同上：进程已经退出时这两步会失败，属于预期情况
    let _ = child.kill();
    let _ = child.wait();
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
        (true, true) => "(没有输出)".to_string(),
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

    fn execute_default(pending: &PendingTool) -> ToolResult {
        execute(pending, &ExecControl::default())
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
        };
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
        let control = ExecControl::default();
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
        assert!(!pending("list_directory", json!({"path": "src"})).needs_approval());
        assert!(!pending("read_file", json!({"path": "src/main.rs"})).needs_approval());
        assert!(!pending("git_status", json!({})).needs_approval());
        assert!(pending("write_file", json!({"path": "a.txt", "content": "x"})).needs_approval());
        assert!(pending("run_command", json!({"command": "ls"})).needs_approval());
        // 读敏感文件也要点头
        assert!(pending("read_file", json!({"path": "~/.ssh/id_rsa"})).needs_approval());
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
    fn arguments_summary_reads_like_a_call() {
        let summary = pending("read_file", json!({"path": "src/main.rs"})).arguments_summary();
        assert!(summary.starts_with("read_file("), "得到 {summary}");
        assert!(summary.contains("path=src/main.rs"));
        assert_eq!(pending("git_status", json!({})).arguments_summary(), "git_status");
    }
}
