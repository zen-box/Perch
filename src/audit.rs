//! 审计日志：模型在什么时候、对什么、做了什么，以及那是自动放行还是你点的头。
//!
//! 写在 `%APPDATA%\Perch\logs\audit-YYYY-MM-DD.jsonl`，一行一条 JSON。
//! 按天分文件：攒成一个大文件用编辑器打不开，而按天切分还能直接删掉某一天。
//!
//! **为什么要有它**：`AGENTS.md` §11 已经定了「Perch 不做进程沙盒」，
//! 那"事后查得到"就是安全模型里还站得住的那一环——完全权限下越界执行是**放行**的，
//! 放行之后总得留下痕迹。
//!
//! 三条纪律：
//!
//! - **不碰 GPUI**，也不弹任何提示。写日志失败不能影响工具执行本身
//!   （磁盘满了、目录权限不对，都不该让一次 `read_file` 失败）。
//! - **参数只记摘要**。`write_file` 的 `content` 可能是一整篇用户文档、
//!   `run_command` 的 `command` 可能带着一段内联脚本——全量抄进日志会让文件涨得很快，
//!   而且那本来就是把用户的正文又存了一份。留个头 + 长度，够回答"它动了哪个文件"。
//! - **被拒的调用也要记**。「用户点了几次拒绝」和「模型试了几次越界」只有在对得上号时
//!   才有意义，只记执行成功的那部分等于把最有用的信息丢了。

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};

/// 参数摘要里单个值最多留这么多字符。
const MAX_VALUE_CHARS: usize = 300;
/// 失败原因最多留这么多字符。
const MAX_DETAIL_CHARS: usize = 300;

/// 日志开着吗。**默认开**：它是"不做沙盒"这个决定的配套，见模块注释。
///
/// 用全局开关而不是在每个调用点查配置：`record` 的调用点散在 `agent_loop` 的
/// 七八个分支里（包括"被拒绝"和"到了轮数上限"这些），每处都写一遍 `if enabled`
/// 迟早会漏一处，而漏掉的那处恰恰可能最该记。设置一变由 `set_enabled` 同步过来。
static ENABLED: AtomicBool = AtomicBool::new(true);

/// 跟着设置开关走。启动时和用户改设置时各调一次。
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// 这条调用是怎么被放行的（或者为什么没放行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// 免确认，直接跑：只读、在项目目录里、或者 Skills 工具。
    Auto,
    /// 用户点了「允许一次」。
    Approved,
    /// 用户点了「拒绝」。
    Denied,
    /// 用户点了「停止」，还没跑完的算这个。
    Stopped,
    /// 到了轮数上限，没执行。
    Limit,
    /// 工具来源被关掉了，没执行。
    Disabled,
    /// 执行器整个没了（panic 之类），结果不是跑出来的。
    Lost,
}

impl Decision {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Stopped => "stopped",
            Self::Limit => "limit",
            Self::Disabled => "disabled",
            Self::Lost => "lost",
        }
    }
}

/// 一条要落盘的记录。
///
/// 用借用而不是 `String`：调用点手上就有这些字段，构造一次记录不该再分配一堆字符串——
/// 日志是"顺手记一下"，不该在工具执行那条路上再添开销。
pub struct Record<'a> {
    pub session_id: &'a str,
    pub tool: &'a str,
    /// 原始参数。`None` 表示没有参数（手打命令那种）。
    pub arguments: Option<&'a Value>,
    pub decision: Decision,
    /// 跑成功了吗。没执行的调用（拒绝 / 上限 / 停用）一律 `false`。
    pub ok: bool,
    pub duration_ms: u64,
    pub exit_code: Option<i32>,
    /// 失败原因 / 结果摘要。太长会被截断。
    pub detail: Option<&'a str>,
}

/// 记一条。**失败就算了**——见模块注释里的第一条纪律。
pub fn record(entry: Record<'_>) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let _ = append(entry);
}

fn append(entry: Record<'_>) -> std::io::Result<()> {
    let now = chrono::Local::now();
    let path = crate::paths::logs_dir().join(format!("audit-{}.jsonl", now.format("%Y-%m-%d")));
    let line = json!({
        "at": now.format("%Y-%m-%d %H:%M:%S").to_string(),
        "session": entry.session_id,
        "tool": entry.tool,
        "decision": entry.decision.as_str(),
        "args": summarize_args(entry.arguments),
        "ok": entry.ok,
        "ms": entry.duration_ms,
        "exit": entry.exit_code,
        "detail": entry.detail.map(|text| truncate(text, MAX_DETAIL_CHARS)),
    });
    // append 模式：多开几个窗口同时写也不会互相覆盖（一行一次 `write`，
    // 在 Windows 上小于管道缓冲区的追加写是原子的）。
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")
}

/// 参数摘要：`键=值` 拼成一行，单个值太长就留个头加长度。
///
/// 非字符串的值直接 `to_string()`：数字、布尔、嵌套对象都短得了，而嵌套对象
/// 恰好是 MCP 工具的参数形状，原样留着比拆开更有用。
fn summarize_args(arguments: Option<&Value>) -> String {
    let Some(Value::Object(map)) = arguments else {
        return String::new();
    };
    let mut parts = Vec::new();
    for (key, value) in map {
        let text = match value {
            Value::String(text) => summarize_text(text),
            other => other.to_string(),
        };
        parts.push(format!("{key}={text}"));
    }
    parts.join(" ")
}

fn summarize_text(text: &str) -> String {
    let total = text.chars().count();
    if total <= MAX_VALUE_CHARS {
        return text.to_string();
    }
    // 我们自己拼的那几段用**英文**：日志是给人翻、给脚本 grep 的机器可读文件，
    // 而它的格式不该跟着界面语言变——同一个 `audit-2026-09-27.jsonl` 里
    // 前半段中文后半段英文，谁看都得先愣一下。
    //
    // 这里**不走** `truncate`：它会在尾巴上补一个 `…`，再拼 `... ({total} chars)`
    // 就成了 `…... (1000 chars)` 两个省略号。参数摘要的读者要的是"有多长"，
    // 长度本身就是信号，不用再补一个"被切过"的记号。
    let head: String = text.chars().take(MAX_VALUE_CHARS).collect();
    format!("{head}... ({total} chars)")
}

/// 按**字符**截断，不是按字节。
///
/// 按字节切会把一个多字节字符切成两半，`String` 装不下半字符——那里只能 panic，
/// 而日志不该有把程序搞崩的能力。
fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cut: String = text.chars().take(max_chars).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[std::prelude::v1::test]
    fn short_arguments_are_kept_as_they_are() {
        let args = json!({ "path": "src/main.rs", "limit": 20 });

        let summary = summarize_args(Some(&args));

        assert!(summary.contains("path=src/main.rs"));
        assert!(summary.contains("limit=20"));
    }

    #[std::prelude::v1::test]
    fn a_long_value_keeps_its_head_and_length() {
        // `write_file` 的 content 可能是整篇文档。日志里要能看出"它写了哪个文件"，
        // 但不该把正文再存一份。
        let body = "字".repeat(1000);
        let args = json!({ "path": "a.md", "content": body });

        let summary = summarize_args(Some(&args));

        assert!(summary.contains("path=a.md"));
        assert!(summary.contains("... (1000 chars)"), "要标出原始长度：{summary}");
        // 只在"太长"这一处收尾，别让 `…` 和 `... (N chars)` 叠在一起——
        // 实测导出的日志里见过 `…... (300 chars)` 这种两个省略号的样子。
        assert!(!summary.contains('…'), "截断记号只该有一个：{summary}");
        assert!(summary.chars().count() < 500, "不能把整篇正文抄进来");
    }

    #[std::prelude::v1::test]
    fn truncating_counts_characters_not_bytes() {
        // 按字节切会把一个汉字切成两半，那在 `String` 上是非法状态。
        let text = "编辑器测试".repeat(100);

        let cut = truncate(&text, 10);

        assert_eq!(cut.chars().count(), 11, "10 个字 + 省略号");
        assert!(cut.starts_with("编辑器测试"));
    }

    #[std::prelude::v1::test]
    fn a_missing_or_non_object_argument_is_an_empty_summary() {
        assert_eq!(summarize_args(None), "");
        assert_eq!(summarize_args(Some(&json!("x"))), "");
    }

    #[std::prelude::v1::test]
    fn every_decision_has_its_own_name() {
        // 名字撞了的话日志里就分不出"用户拒绝"和"被停用"——那正是最该分清的两件事
        let all = [
            Decision::Auto,
            Decision::Approved,
            Decision::Denied,
            Decision::Stopped,
            Decision::Limit,
            Decision::Disabled,
            Decision::Lost,
        ];
        let mut names: Vec<&str> = all.iter().map(|decision| decision.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all.len());
    }
}
