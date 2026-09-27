//! MCP（Model Context Protocol）客户端。
//!
//! 把外部 MCP 服务器（stdio 子进程）暴露的工具接进来，和本机那 5 个工具并列交给模型。
//! 这一层**不碰 GPUI**——不引用 `Context`、不读界面状态，只回答三件事：
//! 「怎么连上去」（[`Connection::connect`]）、「它有哪些工具」（[`Connection::list_tools`]）、
//! 「照着这份参数调一下」（[`Connection::call`]）。
//!
//! 什么时候连、连不上怎么提示、连接活多久，都是 `mcp_ops.rs` 的事。
//!
//! 三个关键取舍：
//!
//! - **工具名要重新起。** MCP 服务器给的名字只保证在它自己内部唯一，而且可能是
//!   `github.create_issue` 这种写法——里面的 `.` 会被 OpenAI 直接 400 拒掉。
//!   统一改成 `mcp__<服务器>__<工具>`，清洗到 `[A-Za-z0-9_-]`、最长 64 字符
//!   （三个渠道对函数名的上限都是 64）。原始名字记在 [`ExposedTool::raw`] 里，
//!   调用时用它。
//! - **schema 不在这里裁。** Gemini 只认 OpenAPI Schema 的一个子集，多一个关键字
//!   整个请求就失败；但 OpenAI 和 Claude 吃得下完整 schema。裁剪放在
//!   `llm_tools::gemini_tools` 那条序列化路径上做，别为了迁就一个渠道把另外三个的
//!   表达力一起削掉。
//! - **stderr 收进日志。** 子进程的 stderr 一旦 piped 就必须有人一直读，否则管道写满
//!   服务器会卡在 write 上；而「npx 找不到包」这类真正有用的报错恰好都在 stderr 里。

use std::collections::{BTreeSet, VecDeque};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::RoleClient;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock, Tool};
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use serde_json::{Value, json};

use crate::config::{McpServerConfig, McpTransport};
use crate::llm_tools::{ToolResult, ToolSpec};

/// 暴露给模型的 MCP 工具名前缀。
pub const TOOL_PREFIX: &str = "mcp__";
/// 服务器 id 与工具名之间的分隔符。
const TOOL_SEP: &str = "__";
/// 三个渠道对函数名长度上限的共识值（OpenAI / Claude / Gemini 都是 64）。
const MAX_TOOL_NAME: usize = 64;
/// 服务器 id 在名字里最多占多少字符。留出余量给工具名和哈希，见 [`compose_name`]。
const MAX_SERVER_PART: usize = 32;
/// 一台服务器的 stderr 日志最多留多少行。用户要看的是「为什么连不上」，
/// 不是完整日志；无限攒着只会白占内存。
const MAX_LOG_LINES: usize = 200;
/// 启动子进程并完成握手最多等多久。npx 要现下包时会慢，但也不能无限等：
/// 卡住的时候界面得能给出结论。
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(60);
/// 拉一次工具清单最多等多久。
pub const LIST_TIMEOUT: Duration = Duration::from_secs(30);

/// 一台服务器拉回来的工具，加上我们给它起的外号。
#[derive(Clone, Debug, PartialEq)]
pub struct ExposedTool {
    /// 交给模型的函数名，也是模型回传时我们认的那个名字。
    pub exposed: String,
    /// 服务器给的原始工具名。**调用时必须用它**，不是 `exposed`。
    pub raw: String,
    pub description: String,
    pub parameters: Value,
}

impl ExposedTool {
    /// 交给模型的那份声明。
    pub fn spec(&self) -> ToolSpec {
        ToolSpec::new(self.exposed.clone(), self.description.clone(), self.parameters.clone())
    }
}

/// 把服务器报的原始工具名清洗成可以交给模型的一组声明。
///
/// 顺序跟着服务器给的顺序走（不排序）：服务器自己通常是稳定的，而且这里刻意做成
/// 「谁先谁后都不影响结果」，见下面撞名的处理。
pub fn expose_tools(server_id: &str, tools: &[Tool]) -> Vec<ExposedTool> {
    let server = server_part(server_id);
    let plain: Vec<String> = tools.iter().map(|tool| compose_name(&server, &tool.name, 0)).collect();
    // 两个不同的原始名清洗之后可能撞在一起（`a.b` 与 `a-b`）。撞了的**全都要**加哈希，
    // 而不是「先到的用干净名字、后到的加哈希」——后者在服务器换个返回顺序时，
    // 两个工具的名字会互换，用户「停用了某个工具」的设置和历史里的调用就对不上了。
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut duplicated: BTreeSet<String> = BTreeSet::new();
    for name in &plain {
        if !seen.insert(name.as_str()) {
            duplicated.insert(name.clone());
        }
    }

    let mut taken: BTreeSet<String> = BTreeSet::new();
    tools
        .iter()
        .zip(plain)
        .map(|(tool, candidate)| {
            let raw = tool.name.to_string();
            let mut exposed = if duplicated.contains(&candidate) {
                compose_name(&server, &raw, 1)
            } else {
                candidate
            };
            // 哈希理论上也可能再撞，兜一圈；上限纯粹是防死循环
            let mut attempt = 2;
            while !taken.insert(exposed.clone()) && attempt < 8 {
                exposed = compose_name(&server, &raw, attempt);
                attempt += 1;
            }

            let description = tool
                .description
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .or(tool.title.as_deref())
                .unwrap_or_default()
                .to_string();
            // 服务器没给 schema 时补一个空对象。三个渠道都要求 `parameters` 存在，
            // 缺了字段整个请求会被拒——为了一个工具把整轮对话弄挂不值得。
            let parameters = if tool.input_schema.is_empty() {
                json!({"type": "object", "properties": {}})
            } else {
                Value::Object((*tool.input_schema).clone())
            };
            ExposedTool {
                exposed,
                raw,
                description,
                parameters,
            }
        })
        .collect()
}

/// 这个名字是不是 MCP 工具（用来决定执行时走哪条路）。
pub fn is_mcp_tool(name: &str) -> bool {
    name.starts_with(TOOL_PREFIX)
}

/// 从暴露名里反推 (服务器 id, 工具名)。
///
/// **只当提示用**：清洗和截断都可能让名字不可逆（原名里有 `.`、或者太长被截过）。
/// 真正决定「这条调用发给哪台服务器、用哪个原始工具名」的是 `mcp_ops` 里那张清单。
pub fn parse_tool_name(exposed: &str) -> Option<(&str, &str)> {
    let rest = exposed.strip_prefix(TOOL_PREFIX)?;
    let (server, tool) = rest.split_once(TOOL_SEP)?;
    if server.is_empty() || tool.is_empty() {
        return None;
    }
    Some((server, tool))
}

/// 界面上显示的名字。
///
/// 交给模型的名字得带服务器前缀（否则两台服务器各有 `read_file` 就分不清了），
/// 但界面上不需要——`mcp__filesystem-3f8a21__read_text_file` 又长又难读。
/// 只留工具名那一段。
pub fn display_name(exposed: &str) -> &str {
    match parse_tool_name(exposed) {
        Some((_, tool)) => tool,
        None => exposed,
    }
}

/// 按 `mcp__<服务器>__<工具>` 组名。
///
/// `attempt` 为 0 就是朴素拼接；需要区分开（撞名、或者太长被截过）时用 1、2、3…
/// 再算一遍，每次多带一段原始名的哈希，所以同一个原始名永远得到同一个结果。
fn compose_name(server: &str, raw_tool: &str, attempt: usize) -> String {
    let tool = sanitize_or_hash(raw_tool);
    let prefix = format!("{TOOL_PREFIX}{server}{TOOL_SEP}");
    // 截断过的名字必须带哈希：`aaaa…a` 和 `aaaa…ab` 截完前 55 个字符是一样的，
    // 不带哈希就分不出来了。撞名要区分也是同一套处理。
    let needs_hash = attempt > 0 || prefix.len() + tool.len() > MAX_TOOL_NAME;
    if !needs_hash {
        return format!("{prefix}{tool}");
    }
    let suffix = format!("_{}", short_hash(&format!("{raw_tool}#{attempt}")));
    let budget = MAX_TOOL_NAME.saturating_sub(prefix.len() + suffix.len());
    let head: String = tool.chars().take(budget).collect();
    format!("{prefix}{head}{suffix}")
}

/// 名字里服务器那一段。
fn server_part(server_id: &str) -> String {
    let cleaned = sanitize_or_hash(server_id);
    // 服务器 id 是本机生成的短 slug，正常远到不了 32 字符；这里只是防着有人手改
    // 配置文件塞进一个超长 id，把工具名挤得只剩哈希。
    if cleaned.len() > MAX_SERVER_PART {
        let head: String = cleaned.chars().take(MAX_SERVER_PART).collect();
        format!("{head}_{}", short_hash(server_id))
    } else {
        cleaned
    }
}

/// 清洗成三个渠道都接受的函数名字符。
///
/// 只留 `[A-Za-z0-9_-]`，其余一律换成 `_`；连续下划线压成一个——`__` 是服务器和
/// 工具之间的分隔符，工具名里再出现就会让 [`parse_tool_name`] 切错位置。/// 首尾的下划线也去掉，免得拼出 `mcp__files___read` 这种三个下划线连在一起的名字。
fn sanitize(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_underscore = false;
    for ch in raw.chars() {
        let ch = if ch.is_ascii_alphanumeric() || ch == '-' {
            ch
        } else {
            '_'
        };
        if ch == '_' {
            if last_underscore {
                continue;
            }
            last_underscore = true;
        } else {
            last_underscore = false;
        }
        out.push(ch);
    }
    out.trim_matches('_').to_string()
}

/// 清洗后如果什么都不剩（工具名全是中文、emoji 之类），用哈希顶上一个名字——
/// 名字是空的会让整个 `tools` 字段不合法。
fn sanitize_or_hash(raw: &str) -> String {
    let cleaned = sanitize(raw);
    if cleaned.is_empty() {
        format!("t{}", short_hash(raw))
    } else {
        cleaned
    }
}

/// 8 位十六进制的短哈希。
///
/// 用 sha2 而不是 `DefaultHasher`：后者不保证跨 Rust 版本稳定。同一个工具换个
/// 版本就换个名字的话，用户「停用这个工具」的设置和历史里的调用记录全都会失配。
fn short_hash(raw: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(raw.as_bytes());
    digest.iter().take(4).map(|byte| format!("{byte:02x}")).collect()
}

/// 把服务器返回的内容块拼成一段文本。
///
/// 图片 / 音频 / 资源块**只留一句说明**：我们的 `ToolResult::content` 是字符串，
/// 四个渠道回传 tool_result 时也都按纯文本发（OpenAI 的 `tool` 角色根本不接受图片），
/// 与其塞一段 base64 把上下文撑爆，不如告诉模型「这儿有个图，多大」。
fn content_text(blocks: &[ContentBlock]) -> String {
    let mut parts = Vec::new();
    for block in blocks {
        match block {
            ContentBlock::Text(text) => parts.push(text.text.clone()),
            ContentBlock::Image(image) => parts.push(format!(
                "[image: {}, {} bytes of base64 data]",
                image.mime_type,
                image.data.len()
            )),
            ContentBlock::Audio(audio) => parts.push(format!(
                "[audio: {}, {} bytes of base64 data]",
                audio.mime_type,
                audio.data.len()
            )),
            ContentBlock::Resource(embedded) => {
                // 文本资源直接展开；二进制资源 `get_text` 返回空串，只留一句说明
                let text = embedded.get_text();
                parts.push(if text.is_empty() {
                    "[binary resource]".to_string()
                } else {
                    text
                });
            }
            ContentBlock::ResourceLink(link) => parts.push(format!("[resource: {}]", link.uri)),
            // `ContentBlock` 是 `#[non_exhaustive]`：协议以后加新块类型时，
            // 这里少认一种，总比编译不过强
            _ => parts.push("[unsupported content]".to_string()),
        }
    }
    parts.join("\n")
}

/// 把服务器返回的结果转成我们内部的结果类型。
pub fn to_tool_result(id: &str, exposed_name: &str, result: &CallToolResult, duration_ms: u64) -> ToolResult {
    let mut content = content_text(&result.content);
    if content.trim().is_empty() {
        // 有些服务器只回 `structuredContent`。有就把它当正文——总比回一个空块强，
        // 模型看到空结果只会原样重试同一个调用。
        content = match &result.structured_content {
            Some(value) => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
            // 和 `local_tools` 用同一句话，界面上两条路看起来才一致
            None => "(没有输出)".to_string(),
        };
    }
    ToolResult {
        id: id.to_string(),
        name: exposed_name.to_string(),
        content,
        is_error: result.is_error.unwrap_or(false),
        duration_ms,
        // MCP 调用不是本机的子进程，没有退出码
        exit_code: None,
    }
}

/// 一条活着的 MCP 服务器连接。
///
/// 协议会话本身 `&self` 就能发请求，所以整条连接可以放进 `Arc` 里，
/// 后台任务和界面两边同时拿着用。
pub struct Connection {
    client: RunningService<RoleClient, ()>,
    /// 子进程 pid。rmcp 只负责结束它自己启动的那个进程，服务器再往下起的子进程
    /// （`npx` → `node`）要我们自己按 pid 收整棵树。
    pid: Option<u32>,
    /// 服务器 stderr 的最后若干行（新的在后面）
    logs: Arc<Mutex<VecDeque<String>>>,
}

impl Connection {
    /// 启动子进程并完成握手。
    ///
    /// 失败时返回的是**技术细节**（io 错误、协议错误原文），不带任何界面文案：
    /// 怎么措辞、用哪种语言提示用户，是界面层的事。
    ///
    /// 必须在 tokio 运行时里调用（它内部要 `tokio::spawn` 一个读 stderr 的任务）。
    pub async fn connect(server: &McpServerConfig) -> Result<Self, String> {
        let McpTransport::Stdio { command, args, cwd } = &server.transport;
        let program = resolve_program(command);
        let mut cmd = tokio::process::Command::new(&program);
        cmd.args(args);
        if let Some(dir) = cwd.as_deref().filter(|dir| !dir.trim().is_empty()) {
            cmd.current_dir(dir);
        }
        // 环境变量的值不在配置文件里，在凭据管理器里，见 `McpServerConfig::secrets`
        for pair in server.secrets() {
            cmd.env(&pair.name, &pair.value);
        }
        hide_console_window(&mut cmd);

        let (transport, stderr) = TokioChildProcess::builder(cmd)
            // stderr 必须 piped 而不是 inherit：Windows 上的图形界面进程没有控制台，
            // 继承下去子进程拿到的可能是个无效句柄；而且有用的报错全在 stderr 里。
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("failed to start `{program}`: {error}"))?;

        let pid = transport.id();
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        if let Some(stderr) = stderr {
            spawn_stderr_reader(stderr, logs.clone());
        }

        // 握手可能卡住（服务器起来了但不肯说协议版本、或者有别的东西往 stdout 写脏了
        // 帧）。超时要自己把进程收掉——这时候还没构造出 `Connection`，Drop 兜不住。
        let handshake = tokio::time::timeout(HANDSHAKE_TIMEOUT, ().serve(transport)).await;
        let client = match handshake {
            Ok(Ok(client)) => client,
            Ok(Err(error)) => {
                kill(pid);
                return Err(error.to_string());
            }
            Err(_) => {
                kill(pid);
                return Err(format!(
                    "timed out after {} seconds while connecting",
                    HANDSHAKE_TIMEOUT.as_secs()
                ));
            }
        };
        Ok(Self { client, pid, logs })
    }

    /// 拉这台服务器的全部工具（自己处理分页）。
    pub async fn list_tools(&self) -> Result<Vec<Tool>, String> {
        match tokio::time::timeout(LIST_TIMEOUT, self.client.list_all_tools()).await {
            Ok(Ok(tools)) => Ok(tools),
            Ok(Err(error)) => Err(error.to_string()),
            Err(_) => Err(format!(
                "timed out after {} seconds while listing tools",
                LIST_TIMEOUT.as_secs()
            )),
        }
    }

    /// 调一个工具。`raw_tool` 是**服务器给的原始工具名**，不是暴露给模型的那个。
    ///
    /// 超时和取消由调用方决定：这里只发请求。工具能跑多久（跑一次浏览器、编译一次
    /// 项目）差别太大，写死一个上限总有人不够用。
    pub async fn call(&self, raw_tool: &str, arguments: Value) -> Result<CallToolResult, String> {
        let mut params = CallToolRequestParams::new(raw_tool.to_string());
        if let Value::Object(map) = arguments {
            params = params.with_arguments(map);
        }
        // 参数不是对象（模型发错了）时就不带参数发过去，让服务器自己报缺哪个字段——
        // 硬塞一个数组进去只会在协议层被拒，报错信息还看不出是参数格式的问题
        self.client.call_tool(params).await.map_err(|error| error.to_string())
    }

    /// 服务器 stderr 的最近若干行（新的在后面）。
    pub fn logs(&self) -> Vec<String> {
        self.logs
            .lock()
            .map(|logs| logs.iter().cloned().collect())
            .unwrap_or_default()
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // 连接没了，服务器进程也得跟着走：Windows 上父进程退出不会带走子进程，
        // 不主动收的话 `npx` 起的 node 会一直挂在后台。
        kill(self.pid);
    }
}

/// 按 pid 结束整棵进程树。进程早就退出了的话这一步会失败，属于预期情况。
fn kill(pid: Option<u32>) {
    if let Some(pid) = pid {
        crate::local_tools::kill_process_tree_by_pid(pid);
    }
}

/// 一直读子进程的 stderr，收进环形缓冲。
///
/// **必须一直读**：管道写满之后服务器会卡在 write 上，连工具调用也跟着停。
fn spawn_stderr_reader(stderr: tokio::process::ChildStderr, logs: Arc<Mutex<VecDeque<String>>>) {
    use tokio::io::{AsyncBufReadExt, BufReader};
    // 这里用 `tokio::spawn` 而不是 `app::runtime().spawn`：本模块是服务层，
    // 引用 `app` 就违反分层（§3.1）。调用方本来就跑在那个运行时里，当前上下文
    // 就是它。
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = strip_ansi(line.trim_end_matches('\r'));
            if line.trim().is_empty() {
                continue;
            }
            // 锁被毒化了也不能让整个连接挂掉：日志丢了就丢了
            let Ok(mut logs) = logs.lock() else {
                return;
            };
            if logs.len() >= MAX_LOG_LINES {
                logs.pop_front();
            }
            logs.push_back(format!("[{}] {line}", chrono::Local::now().format("%H:%M:%S")));
        }
    });
}

/// 去掉 ANSI 颜色控制序列。
///
/// 经 `npx` 启动的 Node 程序在 stderr 上基本都带颜色，原样显示就是一堆 `[32m`。
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        // ESC 后面通常是 `[` 开头的 CSI 序列，一直吃到终止字节（0x40~0x7e）
        if chars.next() != Some('[') {
            continue;
        }
        for next in chars.by_ref() {
            if ('\u{40}'..='\u{7e}').contains(&next) {
                break;
            }
        }
    }
    out
}

/// 在 PATH 里找可执行文件，补上 Windows 上的 `.cmd` / `.bat` 后缀。
///
/// Windows 的 `CreateProcess` 只会自动补 `.exe`，而 MCP 服务器的启动命令十有八九
/// 是 `npx`——真正要跑的是 `npx.cmd`。不补这一下，用户照着服务器文档填 `npx` 会直接
/// 报「系统找不到指定的文件」，而且这个报错很难看出是后缀的问题。
///
/// 找不到时原样返回，让 spawn 去报错：错误信息里至少还有用户填的那个名字。
fn resolve_program(command: &str) -> String {
    let trimmed = command.trim();
    // 已经带路径分隔符的交给系统自己找，别再拼后缀
    if trimmed.contains('/') || trimmed.contains('\\') {
        return trimmed.to_string();
    }
    #[cfg(target_os = "windows")]
    if let Some(found) = find_in_path(trimmed) {
        return found;
    }
    trimmed.to_string()
}

#[cfg(target_os = "windows")]
fn find_in_path(program: &str) -> Option<String> {
    // 顺序照 Windows 自己的来：先看没后缀的，再按 PATHEXT 里最常见的几种
    const EXTS: [&str; 4] = ["", ".exe", ".cmd", ".bat"];
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for ext in EXTS {
            let candidate = dir.join(format!("{program}{ext}"));
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// Windows 上图形界面程序启动控制台程序时，默认会弹出一个黑色的命令行窗口。
fn hide_console_window(command: &mut tokio::process::Command) {
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = command;
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;

    fn tool(name: &str, description: &str, schema: Value) -> Tool {
        let Value::Object(schema) = schema else {
            panic!("schema 必须是对象");
        };
        Tool::new(name.to_string(), description.to_string(), schema)
    }

    /// 单测只关心名字，不想为了算一个名字先造一个 `Tool`。
    fn exposed_name_for_test(server_id: &str, raw_tool: &str) -> String {
        compose_name(&server_part(server_id), raw_tool, 0)
    }

    /// `CallToolResult` 是 `#[non_exhaustive]` 的，没法直接写字面量，也没提供改
    /// `structured_content` 的构造器——那就走反序列化，顺便验证了字段名。
    fn call_result(value: Value) -> CallToolResult {
        serde_json::from_value(value).expect("这是合法的 CallToolResult")
    }

    #[std::prelude::v1::test]
    fn plain_names_are_composed_verbatim() {
        assert_eq!(exposed_name_for_test("files", "read_file"), "mcp__files__read_file");
        assert_eq!(parse_tool_name("mcp__files__read_file"), Some(("files", "read_file")));
    }

    #[std::prelude::v1::test]
    fn unsupported_characters_are_replaced() {
        // `.` `/` `:` 都会被 OpenAI 拒掉，中划线是允许的
        let name = exposed_name_for_test("gh", "repos.create/issue:fix");
        assert_eq!(name, "mcp__gh__repos_create_issue_fix");
        assert!(
            name.chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        );
        assert_eq!(parse_tool_name(&name), Some(("gh", "repos_create_issue_fix")));
    }

    #[std::prelude::v1::test]
    fn underscores_never_double_up_inside_a_tool_name() {
        // 工具名里原本的 `__` 要压成一个，否则解析会切到错误的位置
        let name = exposed_name_for_test("srv", "read__file");
        assert_eq!(name, "mcp__srv__read_file");
        assert_eq!(parse_tool_name(&name), Some(("srv", "read_file")));
    }

    #[std::prelude::v1::test]
    fn long_names_are_shortened_but_stay_unique() {
        let long = "a".repeat(200);
        let other = format!("{}b", "a".repeat(199));
        let first = exposed_name_for_test("srv", &long);
        assert_eq!(first.len(), MAX_TOOL_NAME, "截到上限长度");
        assert_eq!(
            first,
            exposed_name_for_test("srv", &long),
            "同样的输入必须得到同样的名字"
        );
        assert_ne!(first, exposed_name_for_test("srv", &other), "截断之后还要靠哈希区分开");
        assert!(first.starts_with("mcp__srv__"));
    }

    #[std::prelude::v1::test]
    fn overlong_server_ids_do_not_squeeze_the_tool_name_out() {
        let name = exposed_name_for_test(&"s".repeat(200), "read_file");
        assert!(name.len() <= MAX_TOOL_NAME, "得到 {name}");
        assert!(name.contains("read_file"), "工具名要留得下：{name}");
    }

    #[std::prelude::v1::test]
    fn names_that_clean_to_nothing_still_get_one() {
        // 全中文的工具名清洗完什么都不剩，不能拼出 `mcp__srv__` 这种半截名字
        let name = exposed_name_for_test("srv", "读取文件");
        assert_eq!(name, exposed_name_for_test("srv", "读取文件"));
        assert!(name.starts_with("mcp__srv__t"), "得到 {name}");
        assert!(name.len() > "mcp__srv__".len());
    }

    #[std::prelude::v1::test]
    fn tools_that_collide_after_cleaning_get_distinct_names() {
        let tools = vec![
            tool("a.b", "点号", json!({"type": "object"})),
            tool("a-b", "中划线", json!({"type": "object"})),
        ];
        let exposed = expose_tools("srv", &tools);
        assert_ne!(exposed[0].exposed, exposed[1].exposed, "撞名了就得区分开");
        // 原始名字要原样留着，调用时靠它
        assert_eq!(exposed[0].raw, "a.b");
        assert_eq!(exposed[1].raw, "a-b");

        // 服务器换个返回顺序，两个工具的名字不能跟着互换——否则历史记录里的调用
        // 会指向另一个工具
        let reversed = expose_tools("srv", &[tools[1].clone(), tools[0].clone()]);
        assert_eq!(exposed[0].exposed, reversed[1].exposed);
        assert_eq!(exposed[1].exposed, reversed[0].exposed);
    }

    #[std::prelude::v1::test]
    fn tools_without_a_schema_still_get_one() {
        let tools = vec![tool("ping", "", Value::Object(Map::new()))];
        let exposed = expose_tools("srv", &tools);
        assert_eq!(exposed[0].parameters["type"], "object");
        assert_eq!(exposed[0].spec().parameters["type"], "object");
    }

    #[std::prelude::v1::test]
    fn a_missing_description_falls_back_to_the_title() {
        let mut only_title = tool("ping", "", json!({"type": "object"}));
        only_title.title = Some("Ping 一下".to_string());
        let exposed = expose_tools("srv", &[only_title]);
        assert_eq!(exposed[0].description, "Ping 一下");
    }

    #[std::prelude::v1::test]
    fn only_prefixed_names_count_as_mcp() {
        assert!(is_mcp_tool("mcp__files__read_file"));
        assert!(!is_mcp_tool("read_file"));
        assert!(!is_mcp_tool("mcp_files_read_file"));
        // 前缀不完整 / 少一段都不算
        assert_eq!(parse_tool_name("mcp__files__"), None);
        assert_eq!(parse_tool_name("mcp__files"), None);
        assert_eq!(parse_tool_name("read_file"), None);
    }

    #[std::prelude::v1::test]
    fn text_blocks_are_joined_and_other_blocks_are_summarised() {
        let blocks = vec![
            ContentBlock::text("第一行"),
            ContentBlock::image("AAAA", "image/png"),
            ContentBlock::resource_link(rmcp::model::Resource::new("file:///a.txt", "a.txt")),
        ];
        let text = content_text(&blocks);
        assert!(text.starts_with("第一行\n"));
        assert!(text.contains("image/png"));
        // base64 本身不能出现在结果里——那会把上下文撑爆
        assert!(!text.contains("AAAA"));
        assert!(text.contains("file:///a.txt"));
    }

    #[std::prelude::v1::test]
    fn an_empty_result_falls_back_to_the_structured_content() {
        let result = call_result(json!({"content": [], "structuredContent": {"count": 3}}));
        let converted = to_tool_result("c1", "mcp__srv__count", &result, 12);
        assert!(converted.content.contains("\"count\""), "得到 {}", converted.content);
        assert!(!converted.is_error);
        assert_eq!(converted.duration_ms, 12);
        assert_eq!(converted.exit_code, None);
    }

    #[std::prelude::v1::test]
    fn a_completely_empty_result_says_so() {
        let result = call_result(json!({"content": []}));
        let converted = to_tool_result("c1", "mcp__srv__ping", &result, 1);
        assert_eq!(converted.content, "(没有输出)");
    }

    #[std::prelude::v1::test]
    fn error_results_keep_their_flag_and_text() {
        let result = call_result(json!({
            "content": [{"type": "text", "text": "boom"}],
            "isError": true
        }));
        let converted = to_tool_result("c1", "mcp__srv__ping", &result, 5);
        assert!(converted.is_error);
        assert_eq!(converted.content, "boom");
    }

    #[std::prelude::v1::test]
    fn ansi_escapes_are_stripped_from_logs() {
        assert_eq!(strip_ansi("\u{1b}[32mok\u{1b}[0m"), "ok");
        // 没有 ESC 的行原样保留
        assert_eq!(strip_ansi("plain"), "plain");
        // 半个序列（被行尾截断）也不能把后面的内容吃掉
        assert_eq!(strip_ansi("\u{1b}[32"), "");
    }
}
