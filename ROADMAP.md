# Perch 开发路线图

> 写于 2026-09-25，基于 UI 重构后的代码。
>
> 核心原则：**先把普通对话做扎实，再做多模态（识图、附件），最后做 MCP / Skills / Agent。**
>
> ⚠️ **2026-09-26 更新**：程序已从 `personal-control` 改名为 **Perch**（数据目录 `%APPDATA%\Perch`、凭据管理器服务名 `Perch`，旧数据首次启动自动迁移）。
> 另外，下面"当前进度"已经**落后于代码**：P1 的参数面板、消息操作、多模型对比、提示词库、渠道测试连接其实都已实现，
> 实际未做的只剩 **MCP** 与**模型自主工具调用**。后续以代码为准，别照抄本文档的待办清单。

## 当前进度（2026-09-25）

- **已完成**：数据目录迁移；标题栏语言入口与组件库语言同步；收起输入框 Agent 开关；本地工具开关默认关闭；流式消息按 ID 更新；SQLite 会话和消息持久化、旧 JSON 导入与保留；API Key 改存系统凭据管理器；按对话记忆模型及新对话默认模型；按标题和消息正文搜索会话；常用语言代码高亮依赖；按渠道类型拉取模型和测试连接。
- **部分完成**：界面国际化（仍有硬编码中文）；存储改造（SQLite 已接入，界面仍把全部会话消息载入内存，尚未按需加载）。
- **未完成**：下文 P1 的参数面板、提示词管理、消息操作、多模型对比、对话管理其余功能、渠道网络功能；P2 多模态；P3 工具生态。以下章节仍是待办清单，不代表已交付。

---

## 一、为什么这样排优先级

1. **普通对话是每天都走的核心路径。** 对“API 聚合对话”这类产品来说，用户最直接感受到的是：切模型快不快、参数好不好调、回答好不好读、历史好不好找。
2. **多模态要改消息的数据结构，越早改迁移成本越低。** 现在 `ChatMessage.content` 和 `ChatMessageReq.content` 都是纯 `String`。支持图片和附件后要变成“内容分段”，等历史数据多了再改会更麻烦。
3. **Agent / MCP 依赖工具调用协议，而且有安全风险。** 现在的 “Agent” 实际上只是斜杠命令（`/ls` `/read` `/git` `/bash`），并没有让模型自己调用工具。输入框上的 Agent 开关（`agent_mode_enabled`）也没有接入任何逻辑。这部分要做就得把工具调用协议、权限、沙箱一起设计，适合放到后面。

---

## 二、现状速览

**已完成（UI 重构）**
- 标题栏、会话侧边栏（日期分组、搜索、右键菜单）、居中消息列、虚拟滚动与“回到最新”
- Markdown 渲染（表格、行内代码、代码块复制）、思考过程折叠、错误提示
- 设置页：通用、模型渠道（列表加详情）、MCP 占位页、关于
- 深浅色主题、Dialog / Notification、快捷键（Ctrl+N / Ctrl+B / Ctrl+, / Esc）

**已知问题 / 技术债**（建议在 P0、P1 中顺手解决）

| 问题 | 位置 | 影响 |
| --- | --- | --- |
| 会话消息仍全部加载到内存 | `model.rs`、`app.rs`、`ui/chat.rs` | SQLite 已接入，但长历史的启动和内存成本仍随消息量增长 |
| 本地工具同步执行，会卡住界面 | `app.rs` 中的 `execute_agent_tool` 直接调用 `execute_local_tool` | `/bash` 执行慢命令时界面无响应 |
| 拉取模型只支持 OpenAI 风格的 `/v1/models` 加 Bearer 鉴权 | `app.rs` 中的 `fetch_models_from_provider` | Claude、Gemini 渠道拉取会失败 |
| ~~国际化只覆盖约 30 个词条，大部分文案写死为中文~~ | 已完成（2026-09-26）：界面文案全部走 `i18n::tr` / `tr_args`，白名单外的硬编码中文由 `i18n::tests::no_hardcoded_chinese_outside_whitelist` 守住 | 不再有半翻译状态 |
| `ChatSession.folder` 字段尚未用于管理界面 | `model.rs` | 还不能按文件夹整理对话 |

---

## 三、P0：小调整（约半天）

### 1. 语言切换移到标题栏
- 在标题栏右侧（主题按钮旁）加一个语言按钮：`Button` 配合 `dropdown_menu`，菜单项使用 `PopupMenuItem::new(..).checked(..)`。
- **注意**：标题栏整体是拖拽区，按钮必须加 `.occlude()`，否则在 Windows 上点击会被当成拖动窗口。
- 切换时同时调用 `gpui_kit::component::set_locale(..)`，让组件库自带的文案（弹窗的确定/取消等）一起切换。
- **前置条件**：先把 `ui/*.rs` 里写死的中文抽到 `i18n.rs`（或改用组件库已经在用的 `rust-i18n` + `locales/*.yml`），否则切换后会是半翻译状态。
- 设置页里的语言选项可以保留，也可以删掉，避免同一个功能出现在两处。

### 2. 收起 Agent 相关入口
- 去掉输入框工具栏上目前没有实际作用的 Agent 开关。
- `/ls`、`/git` 等快捷按钮和空状态里的工具卡片改为：在输入框里输入 `/` 时弹出命令菜单，或者在设置里提供“启用本地工具”开关，默认关闭。
- 空状态的建议卡片换成普通对话场景，例如“解释代码”“润色文字”“翻译”“总结长文”。

### 3. 数据目录迁移
- 把数据放到用户目录，例如 Windows 上的 `%APPDATA%\Perch\`，可以用 `dirs` 或 `directories` crate 获取路径。
- 首次启动时，如果当前目录下存在旧的 JSON 文件，就自动迁移过去。
- 写文件改为先写临时文件再 rename，避免写到一半时崩溃把数据弄坏。

---

## 四、P1：普通对话体验（核心）

### 1. 对话级参数
- **按对话记忆模型**：启用 `ChatSession.model`，在输入框切换模型时只改当前对话；全局设置里的模型作为新对话的默认值。
- **参数面板**：在对话标题栏或输入框加一个“参数”按钮，打开 Popover 或侧边 Sheet，包含：
  - 系统提示词（覆盖全局设置）
  - temperature、top_p、max_tokens
  - 上下文条数（只发送最近 N 条消息，控制成本）
  - 推理强度：OpenAI 的 `reasoning_effort`、Claude 的 thinking budget、Gemini 的 `thinkingConfig`，只对支持的模型显示
  - 流式输出开关
- **数据结构**：在 `ChatSession` 里加一个 `#[serde(default)] params: Option<ChatParams>`，为空时使用全局默认值。
- `llm.rs` 的 `stream_chat` 改为接收一个参数结构体，不再逐个传参，由各渠道自己决定哪些参数可用。

### 2. 提示词（Prompt）管理
- **提示词库 / 助手预设**：每个预设包含名称、图标、系统提示词、默认模型和参数。新建对话时可以选择预设，类似 Cherry Studio 的“助手”或 LobeChat 的 Agent，但不涉及工具调用。
- **快捷插入**：在输入框输入 `/` 时搜索提示词模板。
- **模板变量**：支持 `{{date}}`、`{{clipboard}}`、`{{selection}}` 等变量。
- 数据可以单独存成一个 `prompts.json`。

### 3. 消息操作
- **重新生成**，并支持“用其他模型重答”。这是聚合类产品很有特色的能力。
- **编辑用户消息并重新发送**：先做“截断后重发”，分支对话可以后置。
- 删除单条消息、引用回复、继续生成。
- 以上操作都放在消息悬停时出现的操作栏里（和现在的复制按钮放在一起）。

### 4. 多模型对比（可选，差异化功能）
- 同一个问题同时发给 2 到 3 个模型，结果并排显示，方便比较后选一个继续对话。
- 依赖第 3 项“用其他模型重答”的基础设施，可以放在 P1 的最后。

### 5. 对话管理
- 置顶、收藏、文件夹（`ChatSession.folder` 字段已经存在）。
- 搜索范围从标题扩展到消息内容。
- 用便宜的模型自动生成标题，目前是截取第一句话。
- 导入和导出：已有 Markdown 导出，再加 JSON 全量备份和恢复。

### 6. 渲染
- **代码高亮**：开启 gpui-kit 的 tree-sitter 功能即可，界面代码不用改（首次编译需要下载语法包）：
  ```toml
  gpui-kit = { version = "0.6.6", features = ["tree-sitter-rust", "tree-sitter-python", "tree-sitter-javascript", "tree-sitter-typescript", "tree-sitter-bash"] }
  ```
- 数学公式（LaTeX）、Mermaid：先确认 `TextView` 的支持情况，不支持的话放到后面。
- 长对话性能：已经使用虚拟列表，后续需要关注超长单条消息的渲染耗时。

### 7. 渠道与网络
- **拉取模型按渠道类型区分**：
  - Claude：`GET /v1/models`，请求头为 `x-api-key` 和 `anthropic-version`
  - Gemini：`GET /v1beta/models?key=...`
- 渠道详情页加一个“测试连接”按钮。
- 支持自定义请求头、代理、超时时间（目前固定为 90 秒）、失败自动重试。

### 8. 数据与安全
- API Key 改存到系统凭据管理器（`keyring` crate，Windows 上对应“凭据管理器”），JSON 里只保留引用。
- 流式写入改为按消息 id 定位，不再使用 `last_mut()`，同时修复“生成中删除对话后 `is_streaming` 无法复位”的问题。

---

## 五、P2：多模态（识图、附件）

### 1. 数据结构（先做）
```rust
// ChatMessage 新增字段，旧数据默认为空
#[serde(default)]
pub attachments: Vec<Attachment>,

pub struct Attachment {
    pub id: String,
    pub kind: AttachmentKind, // Image / Text / Pdf / Other
    pub name: String,
    pub mime: String,
    pub path: String,         // 复制到数据目录下的 attachments/，不直接内嵌 base64，避免 JSON 过大
    pub size: u64,
}
```
`ChatMessageReq.content` 从 `String` 改成“内容分段”（文本、图片、文档），由各渠道分别序列化。

### 2. 各渠道格式

| 渠道 | 图片 | 文档 |
| --- | --- | --- |
| OpenAI Chat | `{"type":"image_url","image_url":{"url":"data:image/png;base64,..."}}` | 文本类附件拼接进文本 |
| OpenAI Responses | `{"type":"input_image","image_url":"data:..."}` | `input_file` |
| Claude | `{"type":"image","source":{"type":"base64","media_type":"image/png","data":"..."}}` | `{"type":"document",...}`（PDF） |
| Gemini | `{"inline_data":{"mime_type":"image/png","data":"..."}}` | 同为 `inline_data` |

### 3. 模型能力标记
- `ModelConfig` 增加 `capabilities`（vision、reasoning、tools 等）。目前的 `tags` 是自由文本，不适合用于逻辑判断。
- 如果当前模型不支持识图，而用户添加了图片，就在输入框上方提示，并提供“切换到支持识图的模型”的入口。

### 4. 界面
- 支持粘贴图片（`Textarea::on_paste`）和拖拽文件（GPUI 的 `on_drop::<ExternalPaths>`）。
- 在输入框上方以缩略图或文件卡片的形式展示附件，可以删除。gpui-kit 自带 `attachment` 组件，可以先评估是否能用。
- 消息中显示图片缩略图（`img()`），点击查看大图。
- 文本类附件（txt、md、代码文件）读取内容后，以“文件名 + 代码块”的形式拼进提示词。
- PDF、Word 的文本提取放到后面。
- 限制单个附件大小；图片过大时先压缩（`image` crate）。

---

## 六、P3：工具生态（MCP / Skills / Agent）

### 1. 工具调用协议（地基）
在 `llm.rs` 里为各渠道实现工具调用，并统一成内部的 `ToolCall` / `ToolResult` 事件：
- OpenAI：请求带 `tools`，响应里是 `tool_calls`，结果以 `role: "tool"` 回传
- Claude：请求带 `tools`，响应里是 `tool_use` 块，结果以 `tool_result` 块回传
- Gemini：请求带 `functionDeclarations`，响应里是 `functionCall`，结果以 `functionResponse` 回传

然后实现“模型请求工具 → 用户授权 → 执行 → 回传结果 → 模型继续”的循环。现有的权限卡片可以复用。

### 2. MCP
- 使用官方 Rust SDK `rmcp`，支持 stdio 和 Streamable HTTP 两种传输。
- 设置页（目前是“即将推出”占位页）改为服务器列表：添加、启用、查看工具列表、查看连接状态和日志。
- MCP 提供的工具统一接入上面的工具调用协议。

### 3. Skills
- 采用 SKILL.md 文件夹格式：YAML 头部写名称和描述，正文是说明，也可以附带脚本和资源。
- 渐进加载：默认只把名称和描述放进上下文，需要时再读取全文。
- 界面上可以和提示词库放在一起管理。

### 4. Agent 模式与安全
- 现有的本地工具（`/ls`、`/read`、`/git`、`/bash`）改写为标准工具，由模型调用。
- 工具执行放到后台线程，不阻塞界面，并且可以中断。
- 权限分级：只读工具可以自动执行；写文件、执行命令需要逐次确认；提供“本会话内始终允许”的选项。
- 限定工作目录，所有操作记录审计日志。

---

## 七、建议的迭代顺序

| 里程碑 | 内容 | 预计工作量 |
| --- | --- | --- |
| v0.2 | P0 全部：标题栏语言切换（含文案抽取）、收起 Agent 入口、数据目录迁移 | 1～2 天 |
| v0.3 | 对话级参数、按对话记忆模型、消息操作（重新生成、编辑、删除）、代码高亮 | 3～5 天 |
| v0.4 | 提示词库 / 助手预设、对话管理（置顶、文件夹、内容搜索）、渠道测试连接、API Key 存入系统凭据 | 3～5 天 |
| v0.5 | 多模态：数据结构、图片粘贴和拖拽、各渠道图片格式、文本类附件 | 4～6 天 |
| v0.6 | 工具调用协议加 Agent 循环，本地工具迁移过去 | 4～6 天 |
| v0.7 | MCP 客户端与管理界面、Skills | 5～8 天 |

多模型对比可以根据精力插在 v0.4 之后。

---

## 八、关键代码位置

| 文件 | 内容 |
| --- | --- |
| `src/main.rs` | 入口、快捷键、窗口创建（Root 加 Workspace 两层） |
| `src/app.rs` | `AppState`：状态与全部业务逻辑、tokio 运行时 |
| `src/theme.rs` | 主题色（在 shadcn 默认主题上叠加靛蓝主色） |
| `src/ui/mod.rs` | `Workspace`（弹窗和通知层）、标题栏、`Palette`、通用小组件 |
| `src/ui/chat.rs` | 侧边栏、消息列表、空状态、授权卡片、输入框 |
| `src/ui/model_picker.rs` | 模型选择弹层 |
| `src/ui/settings.rs` | 设置页（通用、渠道、MCP、关于） |
| `src/ui/dialogs.rs` | 各类弹窗 |
| `src/llm.rs` | 各渠道的流式请求 |
| `src/config.rs`、`src/model.rs` | 配置与对话数据结构、持久化 |

**开发时的注意事项**
- 标题栏里的可点击元素都要加 `.occlude()`，否则在 Windows 上点击会被当成拖动窗口。
- 弹窗内容在 `Workspace` 渲染时构建，可以读取 `AppState`；但不要在 `AppState` 的 `render` 或 `update` 期间再去 `read` 它自己，否则会 panic。
- 调用 reqwest 或其他依赖 tokio 的代码时，要通过 `app::runtime().spawn(..)`，不能直接用 `tokio::spawn`。
- 业务方法拿不到 `Window`，要弹提示就调用 `self.toast(..)`，渲染时会统一弹出。
