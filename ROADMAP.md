# Perch 开发路线图

> 写于 2026-09-25，基于 UI 重构后的代码。
>
> 核心原则：**先把普通对话做扎实，再做多模态（识图、附件），最后做 MCP / Skills / Agent。**
>
> ⚠️ **2026-09-26 更新**：程序已从 `personal-control` 改名为 **Perch**（数据目录 `%APPDATA%\Perch`、凭据管理器服务名 `Perch`，旧数据首次启动自动迁移）。
> 界面国际化已全部完成（0 处待迁中文 / **544 个 key**，见 `I18N_PLAN.md`）。
> ⚠️ **key 数和代码规模这两个数字一直在变，用的时候现数**：`python tools/stats.py`
> ——它们各自错过两次（key 数停过 427 和 496，规模停过 63 文件 / 26145 行）。
>
> ⚠️ **2026-09-27 晚更新**：代码在这一天推进了 **P3-3 MCP 全套**（stdio）+ **附件入口三轮迭代**，
> 本文件当时落后了，这次把进度总览、P2 明细、第六节 P3 各子项、第七节顺序表都改正了。
> **新增 `AGENT_MODE_PLAN.md`**（Chat/Agent 重新定义方案），它会影响第六节第 5 项和第七节的顺序。
>
> ⚠️ **2026-09-27 深夜更新**：Chat/Agent 重新定义的 **A~G 七个阶段全部做完并推送**
> （数据模型 / 选择器改服务器级 / 项目目录 / 权限档位 / Skills / 审计日志 / MCP Streamable HTTP）。
> 第三节 MCP 那节和第七节顺序表已同步；唯一漏项是 **MCP HTTP 走渠道代理**，已单列为待办。

## 进度总览（2026-09-27 晚实测核对）

| 阶段 | 状态 | 说明 |
| --- | --- | --- |
| P0 小调整 | ✅ **完成** | 标题栏语言入口、Agent 开关已删、数据目录迁移与 `write_atomic` |
| P1 普通对话体验 | ✅ **完成** | 对话级参数、提示词库、消息操作、多模型对比、置顶/收藏/文件夹、内容搜索、自动标题、JSON 备份、代码高亮、测试连接、凭据管理器 |
| P2 多模态 | ✅ **基本完成** | 见下方明细，只差"拖拽文件"与"图片压缩"两项 |
| P3 工具生态 | ✅ **P3-1 / P3-2 / P3-3 / P3-4 完成** | **P3-1 工具调用协议、P3-2 Agent 循环**（2026-09-26）、**P3-3 MCP（stdio + Streamable HTTP）**（2026-09-27）、**P3-4 Skills**（2026-09-27）；**P3-5 Agent 安全收尾已完成**（权限档位、项目目录、审计日志） |
| — Chat/Agent 重新定义 | ✅ **A~G 七阶段全部完成**（2026-09-27） | 见 `AGENT_MODE_PLAN.md` 与 `TODO.md` 第十四节。唯一漏项：**MCP HTTP 走渠道代理** |
| P4 打磨（技术债） | 📋 **已立项** | 6 条（原 9 条：Responses 那条已在 P3-1 结清，本地工具阻塞那条已在 P3-2 结清、含异步化，启动 panic 那条已在 2026-09-27 结清），见 `TECH_DEBT.md` |

代码规模（2026-09-27 深夜实测）：**70 个 rs 文件 / 29763 行**（`src/` 43 个 + `src/ui/` 27 个）。
用 `python tools/stats.py` 现数（这个数也一直在变）。
质量关：`cargo test --bin perch` **180 个通过 / 1 个 `#[ignore]`**，clippy 零警告。

### P2 明细（2026-09-26 实测）

| 子项 | 状态 |
| --- | --- |
| `Attachment` / `AttachmentKind` 数据结构、`ChatMessage.attachments` | ✅ `model.rs` |
| 四渠道图片与文档格式（`image_url` / `input_image` / `image` / `inline_data`） | ✅ `llm_request.rs`（早期在 `llm.rs`，P3-1 拆出），含 PDF（Claude `document` 块）的测试 |
| 模型能力标记 `Capability::{Vision, Files, ...}` | ✅ `model_info.rs`，输入框会拦"当前模型不支持识图" |
| 粘贴图片（`Textarea::on_paste`）、粘贴复制的文件、文件选择对话框 | ✅ `clipboard.rs` + `attachment_ops.rs` |
| 输入框上方的附件卡片、可删除、点击定位 | ✅ `ui/composer.rs` + `ui/chat.rs::attachment_badge` |
| **附件入口按模型能力分类型** | ✅ 三轮迭代完成：`890abad` 闸门地基 → `01f6da9`+`ea27483` 按类型分菜单 → `ead4514` 菜单动态化（只摆模型收得下的项，「所有文件 (\*.\*)」兜底已去掉） |
| 消息内图片缩略图、点击看大图 | ✅ `ui/markdown_image.rs` |
| 文本类附件读成"文件名 + 代码块"拼进提示词 | ✅ `llm_request.rs::effective_message_text` |
| 单附件大小限制 | ✅ `file_store::MAX_ATTACHMENT_BYTES` |
| **拖拽文件进窗口** | ❌ 未做（`on_drop::<ExternalPaths>` 全项目搜不到） |
| **图片压缩** | ❌ 未做（超限直接拒绝，不压缩） |
| **PDF/Word 文本提取** | ⬜ 计划内就没排前期（PDF 走原生 `document` 块） |
| **Word / Excel / PPT 附件** | ⚠️ **2026-09-27 改成入口拒收**（它们不会进请求体，静默丢掉更糟）。旧消息里已存下的这类附件仍会在请求体里消失，见 `TODO.md` 待办 |

---

## 一、为什么这样排优先级

1. **普通对话是每天都走的核心路径。** 对“API 聚合对话”这类产品来说，用户最直接感受到的是：切模型快不快、参数好不好调、回答好不好读、历史好不好找。
2. **多模态要改消息的数据结构，越早改迁移成本越低。** 现在 `ChatMessage.content` 和 `ChatMessageReq.content` 都是纯 `String`。支持图片和附件后要变成“内容分段”，等历史数据多了再改会更麻烦。
3. **Agent / MCP 依赖工具调用协议，而且有安全风险。** 现在的 "Agent" 实际上只是斜杠命令（`/ls` `/read` `/git` `/bash`），并没有让模型自己调用工具。输入框上的 Agent 开关（`agent_mode_enabled`）也没有接入任何逻辑。这部分要做就得把工具调用协议、权限、沙箱一起设计，适合放到后面。

   > **2026-09-27 注**：这一段是 2026-09-25 的判断，现在**已经不成立**——
   > P3-1 建好了工具调用协议、P3-2 建好了 Agent 循环和权限分级、P3-3 接上了 MCP，
   > 上面说的 `agent_mode_enabled` 字段早已删除（P0），现在的开关是会话级的 `SessionTools`。
   > 保留这段只为记录"当初为什么把它排后面"。**唯一还成立的是"沙箱"那半句**——
   > 沙箱的结论见第六节第 5 项。

---

## 二、现状速览

**已完成（UI 重构）**
- 标题栏、会话侧边栏（日期分组、搜索、右键菜单）、居中消息列、虚拟滚动与“回到最新”
- Markdown 渲染（表格、行内代码、代码块复制）、思考过程折叠、错误提示
- 设置页：通用、模型渠道（列表加详情）、MCP 占位页、关于
- 深浅色主题、Dialog / Notification、快捷键（Ctrl+N / Ctrl+B / Ctrl+, / Esc）

**已知问题 / 技术债**：已迁到 **`TECH_DEBT.md`**（含实测规模、改法与排期建议）。
AGENTS.md §13 仍是权威清单（写给写代码的人），本文只留进度。

---

## 三、P0：小调整 ✅ 完成

### 1. 语言切换移到标题栏 ✅
标题栏右侧语言按钮 + `dropdown_menu_with_anchor`，切换时同步 `gpui_kit::component::set_locale`。
⚠️ 踩过的坑：`.occlude()` 打在按钮自己身上会导致祖先 Popup 的命中框不算 hovered、
下拉开关监听被跳过——遮挡要放在外层容器上。

### 2. 收起 Agent 相关入口 ✅
输入框的 Agent 开关已删（`agent_mode_enabled` 字段已从代码中移除）；
本地工具改为设置里的开关、默认关闭。

### 3. 数据目录迁移 ✅
`%APPDATA%\Perch\`；旧目录 `PersonalControl` 首次启动自动复制（**复制不移动**，旧目录保留作回退）；
所有写入走 `paths::write_atomic`（先写临时文件再 rename）。

---

## 四、P1：普通对话体验（核心） ✅ 完成

> 2026-09-26 实测：全部子项都已实现，分布在
> `params_ops.rs` / `prompt_ops.rs` / `reply_ops.rs` / `session_ops.rs` / `session_folder_ops.rs` /
> `provider_ops.rs` / `ui/params.rs` / `ui/prompt_*.rs` 等模块。下面保留原始设计意图供参考。

### 1. 对话级参数 ✅
- **按对话记忆模型**：`ChatSession.model`
- **参数面板**：`ui/params.rs`，含系统提示词覆盖、temperature、top_p、max_tokens、
  上下文条数、推理强度（OpenAI `reasoning_effort` / Claude thinking budget / Gemini `thinkingConfig`）、流式开关
- **数据结构**：`ChatSession.params: Option<ChatParams>`，为空时用全局默认值

### 2. 提示词（Prompt）管理 ✅
- **提示词库 / 助手预设**：`prompts.rs` + `ui/prompt_*.rs`，每个预设含名称、图标、正文
- **快捷插入**：输入框 `{{` 触发模板补全
- **模板变量**：`{{date}}`、`{{clipboard}}`、`{{selection}}`
- 存在独立的 `prompts.json`

### 3. 消息操作 ✅
- **重新生成** + **用其他模型重答**
- **编辑用户消息并重新发送**
- 删除单条消息、引用回复
- 操作栏在消息悬停时出现

### 4. 多模型对比 ✅
`send_compare`，最多 2 个模型并排，可选用其中一个的结果继续会话。

### 5. 对话管理 ✅
置顶、收藏、文件夹（`session_folder_ops.rs`）、按标题与消息正文搜索、自动标题（用便宜模型起）、
Markdown 导出 + JSON 全量备份与恢复（`backup.rs` / `backup_ops.rs`）。

### 6. 渲染 ✅
- **代码高亮**：gpui-kit 的 tree-sitter feature 已开（rust / python / javascript / typescript / bash）
- 数学公式（LaTeX）、Mermaid：未做，属计划外
- 长对话性能：虚拟列表已用

### 7. 渠道与网络 ✅
- **拉取模型按渠道类型区分**：`provider_api.rs`，OpenAI `Bearer` / Claude `x-api-key` + `anthropic-version` / Gemini `?key=`
- 渠道详情页的「测试连接」（`provider_ops.rs::test_provider_connection`）
- 自定义请求头、代理、超时、失败重试 ✅
  ⚠️ 例外：**拉取模型与测试连接不走代理**，见 `TECH_DEBT.md` #3

### 8. 数据与安全 ✅
- API Key 存系统凭据管理器（`keyring`），JSON 里只留 `api_key_ref`
- 流式写入按消息 id 定位（修复了"生成中删除对话后 `is_streaming` 不复位"）

---

## 五、P2：多模态（识图、附件） ✅ 基本完成

> 2026-09-26 实测：除"拖拽文件"与"图片压缩"外均已实现。明细见开头「P2 明细」。

### 1. 数据结构 ✅
```rust
#[serde(default)]
pub attachments: Vec<Attachment>,

pub struct Attachment {
    pub id: String,
    pub kind: AttachmentKind, // Image / Text / Document / Other
    pub name: String,
    pub mime: String,
    pub path: String,         // 复制到数据目录下的 attachments/
    pub size: u64,
    pub hash: String,         // 按内容去重
}
```

### 2. 各渠道格式 ✅

| 渠道 | 图片 | 文档 |
| --- | --- | --- |
| OpenAI Chat | `{"type":"image_url",...}` | 文本类附件拼接进文本 |
| OpenAI Responses | `{"type":"input_image",...}` | `input_file` |
| Claude | `{"type":"image","source":{...}}` | `{"type":"document",...}`（PDF） |
| Gemini | `{"inline_data":{...}}` | 同为 `inline_data` |

### 3. 模型能力标记 ✅
`ModelConfig::capabilities`（Vision / Files / Tools / WebSearch / ImageOutput）。
当前模型不支持识图而用户加了图片时，输入框上方会提示。

### 4. 界面 ✅（缺 2 项）
- 粘贴图片与粘贴复制的文件 ✅；**拖拽文件 ❌ 未做**
- 输入框上方附件卡片、可删除 ✅
- 消息中图片缩略图、点击看大图 ✅
- 文本类附件读成"文件名 + 代码块" ✅
- 单个附件大小限制 ✅；**图片压缩 ❌ 未做**（超限直接拒绝）
- PDF 走各渠道原生 `document` / `inline_data` 块；
  **Word / Excel / PPT 2026-09-27 改成入口拒收**（它们不会进请求体，静默丢掉更糟）

---

## 六、P3：工具生态（MCP / Skills / Agent）

### 1. 工具调用协议（地基）—— ✅ 2026-09-26 P3-1 完成

在 `llm_tools.rs` 里为各渠道实现工具调用，并统一成内部的 `ToolCall` / `ToolResult` 事件：

| 渠道 | 工具声明 | 工具调用 | 结果回传 |
| --- | --- | --- | --- |
| OpenAI Chat | `tools[].function` | `tool_calls` | `role: "tool"` + `tool_call_id` |
| OpenAI Responses | 同上（声明） | `output[].type = "function_call"` | 同上 |
| Claude | `tools[].input_schema` | `tool_use` 内容块 | user 消息里的 `tool_result` 块 |
| Gemini | `tools[].functionDeclarations` | `parts[].functionCall` | `parts[].functionResponse` |

**已完成的部分（P3-1）**：

- 内部类型 `ToolSpec` / `ToolCall` / `ToolResult`；`ChatRequest.tools`、
  `ChatMessageReq.tool_calls` / `tool_call_id` / `tool_name`。
- 四渠道请求体序列化（`openai_tools` / `claude_tools` / `gemini_tools`、
  `openai_tool_calls`、`claude_message`）。
- **`handle_sse_line` 重写**：保留 `event:` 行（跨行保存、空行清空），
  工具参数按渠道分片攒齐再解析（`ToolCallState`）。
- `StreamEvent::ToolCall` 事件；`emit_delta` / `emit_complete` 逐渠道加工具分支，
  并把 `OpenAiResponses` 拆成独立分支（**顺带结清技术债原 #7**）。
- **无 tools 时请求体逐字段不变**（有测试锁住），老会话的 prompt 缓存不受影响。

**还没做的**：`openai_body()` 对 Responses 仍发 `messages`（规范要 `input`）——
真要用该渠道时单独拆 body 函数，已记进 `TECH_DEBT.md` 第五节。

### 2. Agent 循环 —— ✅ 2026-09-26 P3-2 完成

"模型请求工具 → 用户授权 → 执行 → 回传结果 → 模型继续"这条链子已经跑通。

**新增两个文件**：

| 文件 | 层 | 职责 |
| --- | --- | --- |
| `local_tools.rs` | 数据层 | 工具清单（`specs`）、参数校验、执行（`execute`）、权限分级（`Guard`） |
| `agent_loop.rs` | 应用层 | 循环推进（`drive_agent_loop`）、续跑（`continue_agent`）、轮数上限 |

**关键设计**：

- **判断与动作分开。** `next_action()` 是纯函数（不碰界面、不落盘、不发请求），
  返回 `RoundAction::{Finish, HitLimit, AskApproval, RunTools}`；
  `drive_agent_loop()` 只负责把结论变成动作。纯函数才能在单测里覆盖所有分支。
- **斜杠命令保留**，且和模型调用**共用同一套执行器**。手打 `/read a.rs` 不经模型、
  不花 token；模型自己调 `read_file` 走同一条执行路径。
- **授权分两级**：只读（`list_directory` / `git_status` / 普通 `read_file`）直接执行，
  危险操作（`write_file` / `run_command` / 敏感 `read_file`）每次弹卡片。口径见 `AGENTS.md §11`。
- **一轮里只弹一张卡片**：同时要授权多条时只挂第一条，其余的靠 `answered` 过滤
  在下一轮补上；已回过结果的调用不会重复执行。
- **12 轮上限兜底**（`MAX_AGENT_ROUNDS`），撞上就停下并提示。
- **模型编造的工具名也要回结果**（`There is no tool named ...`），静默忽略会让它一直重试。
- **工具结果独立成消息块**（`role: "tool"`），不再拼进助手的 Markdown 正文。
  免确认的工具会自动执行，用户看到的就是一串「调用 → 结果」的块。

**顺带结清技术债原 #4**「本地工具同步执行」——`execute_agent_tool` 整体重写。
执行也一并挪出了界面线程：`cx.spawn` + `runtime().spawn_blocking`，
配 `ExecControl` 的超时与取消（「停止」会结束进程树），不再冻界面。
**这条已完整结清，不残留新债。**

**还没做的**：Skills 还没接（MCP 已在 2026-09-27 接上，见下面第 3 项）。
两者都只是"工具的来源"——接进来之后循环一行不用改，只是 `ChatRequest.tools` 里多一批声明。
设计循环时已经考虑过"每轮重新取工具清单"这个问题（`make_job` 每轮都会重新组装请求），
**MCP 接进来时印证了这条判断：循环确实一行没改。**

### 3. MCP —— ✅ stdio + Streamable HTTP 都已完成（2026-09-27）

用官方 Rust SDK `rmcp`。**两条传输链路都已完整落地**：

- 配置：`AppConfig.mcp_servers` + `McpServerConfig` +
  `McpTransport::{Stdio { command, args, cwd }, Http { url }}`，
  全部带 `#[serde(default)]`，老配置不受影响；密钥（stdio 的环境变量 / HTTP 的请求头）
  统一走凭据管理器。
- 连接：`mcp.rs`（服务层）+ `mcp_ops.rs`（应用层）；后台启动、握手、拉工具清单并缓存；
  断了按退避重连；退出时结束子进程。两种传输共用同一个 `handshake`。
- 工具合并：`mcp__<服务器 id>__<工具>` 前缀 + 名字清洗 + Gemini 的 schema 清洗。
- 设置页：服务器列表 + 添加/编辑弹窗（**连接方式两选一：本地命令 / HTTP 端点**）+
  工具清单（每个可单独停用）+ 错误输出。
- 权限：**逐次授权**（拍板结果：不做"信任服务器"）。
- 实测：`tools/p3-test/mcp_server.py`（13 个工具，覆盖撞名/超长/中文名/分页/stderr 噪声等易错路径）
  + `tools/p3-test/mcp_http_server.py`（Streamable HTTP 靶子）
  + 真实 npx 服务器（文件系统 / 顺序思考 / 记忆图谱）。

⚠️ 当初预判的那条**成立并且已解决**：MCP 工具清单是运行时才拿到的，
而 `ChatRequest.tools` 是构造请求时一次性填好的——解法是 `make_job` 每轮重新组装请求，
把当时的清单挂上去，循环本身一行没改。

⚠️ HTTP 那条路的两个坑（都记在 `AGENT_MODE_PLAN.md` 第十节）：
`rmcp` 自己的 reqwest **不带 TLS**，光开 transport feature 连不上 https 服务器；
`custom_headers` 要的 `http::HeaderName` / `HeaderValue` **rmcp 没有 re-export**，
得自己加一个 `http` 直接依赖。

**❌ 没做**：MCP HTTP 传输**走渠道代理**（`AppConfig.proxy`）；
MCP 的 resources / prompts 两类能力。方案见 `AGENT_MODE_PLAN.md` 第十节。

### 4. Skills —— ✅ 已完成（2026-09-27，阶段 E）

- 采用 SKILL.md 文件夹格式：YAML 头部写名称和描述，正文是说明，也可以附带脚本和资源。
- 渐进加载：默认只把名称和描述放进上下文，需要时再读取全文。
- 落在 `%APPDATA%\Perch\skills\`（一个子目录一个技能），设置页有「技能」一节可停用。
- ✅ **对话模式也能用**（它是"提示词增强"，零文件系统风险），而且**免确认**——
  两个工具（`load_skill` / `read_skill_file`）读的只有 Perch 自己的技能目录，
  `safe_join` 只接受普通路径组件，`../` 会被拒。
- ❌ 未做：skill 附带脚本的**执行**（那要走 `run_command`，只有智能体模式能用）。

### 5. Agent 模式与安全 —— ✅ 已完成（2026-09-27，阶段 C/D/F）

- ✅ 现有的本地工具（`/ls`、`/read`、`/git`、`/bash`）已改写为标准工具，由模型调用。
  **但斜杠命令保留**（与用户确认过）：手打仍然直接执行，不花 token。
- ✅ 工具执行放到后台线程，不阻塞界面，并且可以中断（`spawn_blocking` + 超时 + 停止按钮）。
- ✅ 权限分级：只读工具自动执行、写文件与执行命令逐次确认。
- ✅ **权限档位（默认 / 完全）**：完全权限跳过全部三道确认，**唯一例外**是 Perch 自己的
  数据目录（`touches_the_data_dir`）——让模型改自己的配置等于让模型控制程序行为。
- ✅ **项目目录**：智能体可指定项目目录，相对路径按它算；目录之外一律弹卡片（含只读操作）。
- ✅ **审计日志**：`%APPDATA%\Perch\logs\audit-YYYY-MM-DD.jsonl`，一行一条，
  记录工具、参数摘要、是自动放行还是用户点的头、耗时、退出码。默认开，设置页可关。
- ⚠️ **不做进程沙盒**（2026-09-27 结论，未变）。Windows 桌面上没有便宜的进程级沙盒
  （AppContainer 要 manifest + 能力声明且大量正常命令跑不起来；低完整性级别会打断正常命令；
  WSL/容器要求用户额外安装）。只做「项目目录 + 授权 + 审计日志 + 完全权限开关」这套边界，
  **并且已在界面和 `AGENTS.md` 里写明「智能体不是沙盒」**——"用户以为有沙盒"本身就是安全风险。
  各方案的实际成本见 `AGENT_MODE_PLAN.md` 第七节。

---

## 七、建议的迭代顺序（2026-09-27 晚重排）

原计划把 P3 拆成 v0.6 / v0.7 两个里程碑。因为 P1 / P2 已经做完，这里改成"接下来做什么"。

| 顺序 | 内容 | 状态 | 规模 |
| --- | --- | --- | --- |
| ← 已完成 | v0.2 ~ v0.5（P0 / P1 / P2） | ✅ | — |
| ✅ 已完成 | **P3-1 工具调用协议**：四渠道统一 `ToolCall` / `ToolResult`，重写 `handle_sse_line` 认识 event 名 | ✅ 2026-09-26 | 大 |
| ✅ 已完成 | **P3-2 Agent 循环**：模型请求工具 → 用户授权 → 执行 → 回传 → 继续；本地工具迁过去 | ✅ 2026-09-26 | 中 |
| ✅ 已完成 | **P3-3 MCP（stdio）**：`rmcp` 客户端 + 设置页服务器列表 + 工具合并 + 逐次授权 | ✅ 2026-09-27 | 中 |
| ✅ 已完成 | 附件入口按模型能力分类型（三轮迭代） | ✅ 2026-09-27 | 中 |
| ✅ 已完成 | **Chat/Agent 重新定义 A~G 七个阶段**：数据模型 / 选择器改服务器级 / 项目目录 / 权限档位 / Skills / 审计日志 / MCP Streamable HTTP | ✅ 2026-09-27 | 约 3200 行 |
| **下一步 1** | **P4 技术债**：6 条，见 `TECH_DEBT.md` | 📋 | 约 225 行（不含 #1/#2/#6 需拍板/架构的） |
| **下一步 2** | **MCP 的 HTTP 传输接渠道代理**（阶段 G 唯一漏项） | ❌ | 小 |
| | **P3-4 Skills 的收尾**：技能附带的脚本执行、更多内置技能 | ❌ | 中 |

**为什么当初把 Chat/Agent 重新定义排到 P3-4 前面**（事后看是对的）：阶段 B（选择器改服务器级）、
C（项目目录）、D（权限档位）都建在新的 `SessionTools` 上。先做后面三个，
等于在旧结构上改一遍、将来再改一遍。七个阶段已于 2026-09-27 全部做完并推送。
方案见 `AGENT_MODE_PLAN.md`，可执行条目见 `TODO.md` 第十四节。

### 为什么建议先做 P3/P4 的功能、技术债插空清（2026-09-26 结论）

1. **P3 的地基是零。** P3-1 动工前核实过 `llm.rs` 里 `"tools"` 出现 0 次，
   `tool_calls` / `tool_use` / `functionCall` / `functionDeclarations` / `tool_result` / `functionResponse`
   一个都没有。这是从零新建的一层，不受现有技术债拖累。
2. **技术债里只有少数会被 P3 碰到**（原 #7「Responses 格式」确实是 P3 前置，
   已在 P3-1 一并结清，印证了这条判断）：
   - 本地工具同步执行（原 `TECH_DEBT.md` #4）——P3 要把本地工具改写成"模型调用 + 权限分级 + 可中断"，
     那个函数会整体重写。现在改一遍，P3 再改一遍。**已在 P3-2 结清**，改成了
     `runtime().spawn_blocking` + 超时 + 可停止。
   - #6（消息常驻内存）——P3 会给消息加上工具调用相关字段、体积变大，
     但数据结构那时才定型。现在改是在旧结构上改一遍、将来再改一遍。
3. **另外 5 条与 P3/P4 无关，合计约 305 行**，建议**跟着功能顺手清**
   （做渠道功能时清 #3、改消息渲染时清 #4、改设置页时清 #5），比专门排一批划算。

**当初"唯一建议现在就做的"是启动失败 panic 那条**——约 120 行，与任何功能都不冲突，
且它是"配置损坏时程序闪退、不给任何提示"，属用户会真实遇到的问题。**2026-09-27 已做掉**
（即当时的 #3，结清后其余条目上移为 #3~#6；做法与实测见 `TECH_DEBT.md` 第三节）。

---

## 八、关键代码位置

| 文件 | 内容 |
| --- | --- |
| `src/main.rs` | 入口、快捷键、窗口创建（Root 加 Workspace 两层） |
| `src/app.rs` | `AppState`：状态、界面偏好、tokio 运行时（`runtime()`） |
| `src/theme.rs` | 主题色（在 shadcn 默认主题上叠加靛蓝主色） |
| `src/i18n.rs` | 界面文案表（`i18n!` 宏生成 `Key` 枚举），见 `AGENTS.md` §9.10 |
| `src/paths.rs` | 数据目录与文件名、原子写、旧数据迁移 |
| `src/storage.rs` | SQLite 持久化（会话 / 消息 / 元数据） |
| `src/ui/mod.rs` | `Workspace`（弹窗和通知层）、标题栏、`Palette`、通用小组件 |
| `src/ui/chat.rs` | 对话主区域、消息分发；侧边栏在 `ui/sidebar.rs` |
| `src/ui/composer.rs` | 输入框与附件区 |
| `src/ui/markdown_image.rs` | Markdown 里的图片渲染（含远程图片与磁盘缓存） |
| `src/ui/settings*.rs` | 设置页（通用 / 渠道 / 提示词 / 其他） |
| `src/llm.rs` | 发送入口：`stream_chat` 重试循环、SSE 读取、`StreamEvent` 定义 |
| `src/llm_request.rs` | 各渠道的请求体构造（含 tools 序列化） |
| `src/llm_stream.rs` | 各渠道的流式响应解析 |
| `src/llm_tools.rs` | 工具调用协议（`ToolSpec` / `ToolCall` / `ToolResult`） |
| `src/local_tools.rs` | 本机工具：清单（`specs`）、参数校验、执行（`execute`）、权限分级（`Guard`）、超时与杀进程树 |
| `src/agent_loop.rs` | Agent 循环（`drive_agent_loop`）、续跑（`continue_agent`）、授权（`approve_pending_tool`）、轮数上限 |
| `src/mcp.rs` | MCP 客户端（服务层）：连接、握手、工具清单、重连退避 |
| `src/mcp_ops.rs` | MCP 的应用层：连接状态、设置页交互、服务器增删改 |
| `src/tool_ops.rs` | 会话级工具状态（`SessionTools` 的读写）、工具分组（`ToolGroup`） |
| `src/attachment_ops.rs` | 附件入口：按模型能力分类型（`AttachmentFilter`）、闸门（`AttachmentGate`）、拒收文案 |
| `src/file_store.rs` | 附件落盘：类型与 mime 识别、文件名清理、大小上限 |
| `src/reply_ops.rs` | 发消息 / 对比 / 重答的编排；`make_job` 组装请求（含工具清单四道闸） |
| `src/image_http.rs` | 图片下载客户端（拦截本机与局域网地址） |
| `src/model_info.rs` | 模型能力标记（`Capability`）、models.dev 缓存 |
| `AGENT_MODE_PLAN.md` | Chat/Agent 重新定义方案（**未动工**，动手前先读） |
| `TECH_DEBT.md` | 技术债工单（含实测规模与排期建议） |

**开发时的注意事项**
- 标题栏里的可点击元素都要加 `.occlude()`，**且遮挡要放在外层容器上**——打在按钮自己身上会导致
  祖先 Popup 的命中框不算 hovered、下拉开关被跳过（踩过一次）。
- 弹窗内容在 `Workspace` 渲染时构建，可以读取 `AppState`；**但不要在 `AppState` 的 `render` 或
  `update` 期间再去 `read` 它自己**，否则会 panic。`cx.listener` 回调里同理。
- 调用 reqwest 或其他依赖 tokio 的代码时，要通过 `app::runtime().spawn(..)`，不能直接用 `tokio::spawn`。
- 业务方法拿不到 `Window`，要弹提示就调用 `self.toast(..)`，渲染时会统一弹出。
- 完整的开发规范见 **`AGENTS.md`**（权威文档，动手前必读）。
