# Chat / Agent 重新定义与工具来源重构（方案）

> 写于 2026-09-27。**这是一份方案，不是工单**——可执行的条目在 `TODO.md` 第十四节。
> 状态：**未动工**，等拍板（第十二节列了 6 个问题）。
>
> 起因：现在的「对话 / 智能体」只是一个 `bool`——带上工具就是智能体，不带就是对话。
> 这个区分太薄：用户想在对话里挂一个搜索 MCP 提高回答质量，做不到（必须切成智能体，
> 而那同时就把本机文件权限也打开了）。

---

## 一、现状（逐行核实过，别按记忆改）

| 位置 | 现状 |
| --- | --- |
| `model.rs::SessionTools` | `{ enabled: bool, picked: Option<Vec<String>> }`，`enabled == true` 就是智能体 |
| `model.rs::ChatSession::tools_enabled` | 只读 `tools.enabled` |
| `model.rs::ChatSession::wants_tool` | `picked == None` → **全带**；`Some` → 白名单 |
| `reply_ops.rs::make_job` | `spec.with_tools && config.local_tools_enabled` 才去取清单 |
| `reply_ops.rs::tool_list_for` | 模型要有 `Capability::Tools`；再按 `session.wants_tool` 过滤 |
| `ui/tool_picker.rs` | 按来源分组（本机一组 / 每台服务器一组），**每个工具一个勾选框** |
| `config.rs::McpServerConfig.enabled` | `#[serde(default)]`（缺字段＝false），但 `mcp_ops.rs` **新建时写死 `true`** |
| `config.rs::McpTransport` | **只有 `Stdio` 一个变体**，HTTP 没做（枚举已按 `kind` 标签设计，加变体不破坏老配置） |

由此得出三条**现在就是问题**的事实：

1. **全局开关 `local_tools_enabled` 卡住的是全部工具，包括 MCP。**
   用户关掉「本地工具」（默认就是关的）、只勾一台 MCP 服务器 → 一个工具都发不出去。
   这个开关的名字叫「本地工具」，却管着 MCP，是**语义错了**。
2. **对话模式一个工具都不带。** 想用 MCP 就必须切智能体，而智能体同时把
   `read_file` / `write_file` / `run_command` 一起打开了——**用户为了要一个搜索工具，
   被迫交出了硬盘**。
3. **加一台 MCP 服务器 = 默认全选它的全部工具。**
   `mcp_ops.rs` 新建服务器时 `enabled = true`，而会话级 `picked = None` 表示"全带"，
   两者一叠，用户刚配好 fetch，它的每个工具就已经在往请求里塞了。

---

## 二、目标定义

**两条正交的轴**，而不是一个开关：

| | **对话（Chat）** | **智能体（Agent）** |
| --- | --- | --- |
| 用途 | 问答、写作、翻译、查资料 | 让它动手改东西 |
| MCP 工具 | ✅ 能用 | ✅ 能用 |
| Skills | ✅ 能用 | ✅ 能用 |
| **读 / 写 / 删 / 建本机文件** | ❌ **没有** | ✅ 有 |
| **执行本机命令** | ❌ 没有 | ✅ 有 |
| 工作目录 | 不需要 | 需要 |
| 权限档位 | 不适用 | 默认权限 / 完全权限 |

**核心区别就一句话：对话碰不到你的硬盘，智能体能碰。**

这个定义比现在的「带不带工具」好在三处：

- 用户不必为了用一个搜索 MCP 而交出硬盘；
- 「对话不会改你的文件」是**用户能一句话理解、也能一句话验证**的承诺；
- 「工具」这件事从"一个开关"变成"来源 + 权限"两件事，后面加 Skills、
  加 MCP 的 HTTP 传输、加权限档位都有地方放，不用再改语义。

---

## 三、数据模型改造

```rust
/// 这个会话要不要工具、从哪些来源要、本机权限多大。
pub struct SessionTools {
    /// 模式。对话不给本机工具，智能体给。
    pub mode: SessionMode,           // Chat | Agent
    /// 会话里启用的工具来源。`None` = 没动过选择器，用下面的默认值。
    pub sources: Option<Vec<ToolSource>>,
    /// 智能体的本机权限档。对话模式下无意义。
    pub permission: Permission,      // Default | Full
    /// 智能体的项目目录（绝对路径）。
    pub workspace: Option<String>,
}

/// 一个工具来源。选择器按这个粒度勾，不按工具勾。
pub enum ToolSource {
    /// 本机文件与命令。**只有智能体模式才生效。**
    Local,
    Mcp { server_id: String },
    Skill,
}
```

**默认值（新会话）**——这是这次改动的重点，两个"默认不带"：

| 模式 | `sources` 默认 |
| --- | --- |
| 对话 | `Some([])` —— 什么都不带。用户想用 MCP 自己勾 |
| 智能体 | `Some([Local])` —— 只带本机工具，**MCP 一台都不带** |

**结构性保证**：`ToolSource::Local` 只在 `mode == Agent` 时生效。
对话模式下就算 `sources` 里塞了 `Local` 也**必须**被忽略——
"切回对话就碰不到硬盘"要落在代码里，不能只靠界面不显示那个勾选框。

### 迁移（老数据必须能读，这是铁律）

现在 `picked: None` 的含义是**"全带"**，而新默认是**"全不带"**。
直接改语义会让老会话悄悄丢掉 MCP 工具，所以要显式迁移：

| 老 JSON | 新值 |
| --- | --- |
| 没有 `tools` 字段 | `None`（对话，不带工具） |
| `{"enabled": false}` | `mode: Chat, sources: Some([])` |
| `{"enabled": true, "picked": null}` | `mode: Agent, sources: None` → **加载时展开成"当时的全部来源"**（`Local` + 配置里现有的每台服务器 id） |
| `{"enabled": true, "picked": ["read_file", "mcp__fetch__fetch"]}` | `mode: Agent, sources: Some([Local, Mcp{fetch}])`（从工具名反推来源） |

⚠️ `picked: None` 那条**不能在反序列化时就展开**——那时拿不到配置里的服务器清单。
做法：反序列化成一个"待展开"标记，`config.rs` 加载完配置后再填。
`model.rs` 和 `config.rs` 谁先加载要理清（现在 `AppState::bootstrap` 是先读库再读配置，
顺序正好，但要把展开那一步插在两者之间）。

⚠️ 迁移测试要覆盖上面四行，特别是「老会话升级后 MCP 工具没丢」这一条。

---

## 四、工具来源闸门（`tool_list_for` 重写）

```
最终清单 =
    模型有 Capability::Tools            （现有，保留）
  && 会话 mode 不是"关闭工具"            （新）
  → 按 sources 逐个收集：
      Local     → 需要 mode == Agent && config.local_tools_enabled
      Mcp(id)   → 需要该服务器 enabled（配置里）且已连上（运行时）
      Skill     → 需要 config.skills_enabled（新开关，默认开）
  → 再按会话级"单独停用的工具"过滤
```

两处**行为变更**（都是修 bug，不是回归）：

1. **全局 `local_tools_enabled` 只管本机工具。** 现在它卡住 MCP，
   改成只卡 `ToolSource::Local`。开关文案也要跟着改成「允许智能体读写本机文件」之类，
   不能再叫「本地工具」——名字对不上就会再错一次。
2. **对话模式下 MCP 调用也进循环。** `make_job` 里 `agent: !tools.is_empty()`
   保持不变（清单非空就允许循环），所以对话模式下模型调 MCP 工具会正常续跑。
   ⚠️ 授权：MCP 工具是别人写的代码，**对话模式下也要逐次授权**，和智能体一致
   （见第十二节第 1 条）。

---

## 五、Agent 权限档位

| 档位 | 行为 | 界面 |
| --- | --- | --- |
| **默认权限** | 只读本机文件免确认；写文件 / 跑命令 / 读敏感文件 → 每次弹卡片（**＝现状**） | 普通 |
| **完全权限** | 本机工具全部直接执行，不弹卡片 | ⚠️ 醒目标记（红/橙 + 图标），首次开启要二次确认弹窗 |

实现很小：`local_tools.rs::Guard` 判定时加一道 `permission == Full → 直接放行`。
真正的工作量在**界面和文案**——要让用户明白自己开了什么。

完全权限下建议**仍然保留两条硬底线**（要拍板，见第十二节）：

- **Perch 自己的数据目录仍然拦**（`perch-config.json`、`perch.db`、凭据引用）。
  理由：让模型改自己的配置，等于让模型控制程序本身的行为。
- **工作目录之外放行，但记审计日志。** 否则"完全"两个字没意义。

⚠️ **必须写进界面和 `AGENTS.md`**：完全权限 = 模型可以在你机器上做任何事。
只在你能接受"跑一个不受信任的脚本"时开。

---

## 六、项目目录（工作目录）

**现状有多坑**：相对路径按「程序从哪个目录启动」算。装好的程序就是安装目录，
基本没法用——所以现在本机工具虽然做完了，实际上不好用。

要做：

1. `SessionTools.workspace: Option<String>`，智能体模式下在输入框工具栏显示，
   点开是文件夹选择框；
2. 相对路径按它算（`local_tools.rs` 里所有路径解析都过它）；
3. **目录之外的读写要授权**——这才是真正的边界。
   现在只有"敏感路径判断"（`.env` / `~/.ssh` 之类），那只是防呆，
   模型换个路径就绕过去了（`TECH_DEBT.md` 第五节第 ③ 条记着这条）；
4. 系统提示词里告诉模型：工作目录 + 系统环境（Windows / PowerShell）；
5. ⚠️ **没设项目目录时怎么办**——要拍板，见第十二节第 4 条。
   我倾向**不给本机工具**，并在界面上明确提示"先选项目目录"。
   因为"拿安装目录兜底"正是现在最坑的地方，不能留一个隐式默认。

---

## 七、沙盒 —— 老实说：做不了真的，所以要把边界说清楚

要求是"像别的 Agent 那样在沙盒里跑"。**Windows 桌面上没有便宜的进程级沙盒。**
把选项摊开看：

| 方案 | 能限制文件访问吗 | 成本 | 结论 |
| --- | --- | --- | --- |
| 工作目录 + 授权（第六节） | 只限制**我们自己发的工具** | 中 | ✅ 做 |
| Job Object | ❌ 只能限内存 / CPU / 进程数 | 低 | 可选，防"跑飞了把机器拖死" |
| AppContainer | ✅ 但要 manifest + 能力声明，大量正常命令直接跑不起来 | 很高 | ❌ 不做 |
| 低完整性级别（Low IL） | 部分 | 高，会打断大量正常命令 | ❌ 不做 |
| WSL / 容器 | ✅ | 要求用户装 WSL，工作目录映射复杂 | ❌ 不做 |

**第一期只做「工作目录 + 授权 + 审计日志 + 完全权限开关」这套边界，不做进程沙盒。**

⚠️ **必须在界面上和 `AGENTS.md` 里写明「智能体不是沙盒」。**
"用户以为有沙盒"本身就是安全风险——他会在装了沙盒的心理预期下让模型跑脚本。
要跑不受信任的代码，请用户自己在虚拟机 / 容器里跑。

长期（不在本期）：评估 Job Object，至少拿到"子进程树能整体结束 + 资源上限"。

---

## 八、MCP 的选择粒度改成服务器级

**现状的毛病**：43 个工具 → 选择器是一长串勾选框。用户想用 fetch，
得先搞清 fetch 有哪几个功能、再逐个决定勾不勾。对普通人是门槛。

**改成**：

```
[ ] 本机工具             （只有智能体模式才出现）
[ ] 演示服务器 · 13 个工具
[ ] 分页服务器 · 3 个工具
▸ 高级：单独停用某些工具
```

- **默认只显示服务器级勾选。** 勾一台 = 这台的全部工具都带上，
  模型按用户的问题自己挑用哪个功能——这才是模型该干的活。
- 「高级」折叠展开后才列出单个工具，保留现有能力，但降级为高级选项。
- **默认一台都不勾**（第二节的默认值）。
- 数据模型天然支持：`ToolSource::Mcp { server_id }` 就是服务器级；
  工具级停用放 `SessionTools.disabled_tools`（会话级）和
  `McpServerConfig.disabled_tools`（全局，已有）两层。

**加服务器时默认启用还是默认停用？**
建议**仍然默认启用**：不连上就不知道它有哪些工具，选择器里会是一条查不到工具的空行。
"默认不用"靠**会话里不勾**来实现，而不是靠服务器不启动。
⚠️ 副作用：配了不用的服务器会一直在后台跑子进程。本期只在设置页提示，不做"按需启动"。

---

## 九、Skills 接入

详细方案沿用 `TODO.md` 第六节，这次重新定义带来两条补充：

1. **对话模式也能用 Skills。** Skills 是"提示词增强"，零文件系统风险，
   正是对话模式该有的能力。
2. **`load_skill` / `read_skill_file` 读的是程序自己数据目录里的文件**
   （`%APPDATA%\Perch\skills\`），不碰用户的文件系统 → 所以对话模式可以用。
   但 skill 附带的**脚本要执行**时走 `run_command`，那是本机工具
   → **只有智能体模式能用**。对话模式下这两个工具在闸门处就被拦掉。

---

## 十、MCP 的第二种传输（Streamable HTTP）—— ✅ 已完成（2026-09-27）

> 实际做法与下面这版方案的差异，逐条记在末尾。方案原文保留，方便对照。

- `McpTransport` 加 `Http { url, headers }` 变体。
  枚举是按 `kind` 标签设计的，**加变体不破坏老配置**（`config.rs` 的注释里已经写了这一条）。
- 请求头里的密钥走凭据管理器，复用 `secrets()` 那套 `NAME: VALUE` 表示，
  界面也能复用同一个输入框。
- 走渠道代理（`AppConfig.proxy`）。
- 连接管理复用 stdio 那套：重连退避、工具清单缓存、`notifications/tools/list_changed`。
- ⚠️ **动工前先核实**：当前 `rmcp` 版本是否已含 Streamable HTTP 客户端。
  没有的话要么升版本、要么自己写 HTTP 那一层（JSON-RPC over HTTP 不难，但分帧规则和 stdio 不同）。

### 落地时和方案不一样的地方

1. **请求头不放进 `Http` 变体**，改成只留 `Http { url }`，头走 `secrets()`。
   方案里写的是 `Http { url, headers }`。分开存会多出一个"哪个头算密钥"的选择题，
   而答案只能靠猜；统一走凭据管理器就只有一条规则：**值都不进配置文件**。
   代价是 `secrets()` 这个名字在 HTTP 下读起来别扭（它装的是请求头），
   界面上按连接方式换标签（`环境变量` / `请求头`）把这件事说清楚了。
2. **代理没做。** 方案里写"走渠道代理"，实际没接 `AppConfig.proxy`——
   `StreamableHttpClientTransport::from_config` 用的是它自己 `default_http_client()` 造的
   reqwest client，要接代理得换成 `with_client` 自己造 client 并套 `reqwest::Proxy`。
   记在 `TODO.md` 里，别当成已完成。
3. **TLS feature 是个坑。** `rmcp` 自己的 reqwest 是 `default-features = false`、**不带 TLS**；
   只开 `transport-streamable-http-client-reqwest` 的话 https 的服务器连不上，
   报的是握手阶段的 TLS 错，看不出是缺 feature。必须再显式开一个 TLS 后端
   （这里选 `reqwest` = rustls，和渠道那条路同一个后端）。
4. **`http = "1"` 是新增的直接依赖。** `custom_headers` 收的是
   `HashMap<HeaderName, HeaderValue>`，这两个类型在 `http` 里，而 `rmcp` 没有 re-export。
   版本跟着 rmcp 自己依赖的那个走，不会编出两份。
5. `notifications/tools/list_changed` 在 HTTP 下**没单独验**。它走的是同一个
   `RunningService`，传输层已经由 `initialize` / `tools/list` / `tools/call` 三条路验过了。

### 实测证据（`tools/p3-test/`）

靶子是新写的 `mcp_http_server.py`（Streamable HTTP，端口 8770），
和 stdio 那个 `mcp_server.py` 是一对。五条链路全部验到：

| 验的什么 | 怎么看 |
| --- | --- |
| 加一台 HTTP 服务器能连上 | `shots/g2-mcp.png`：`HTTP 演示服务器 · 已连接 · 5 个工具`，摘要就是那个 URL |
| 工具清单正确 | 同上，5 个 = `echo` / `add` / `headers` / `boom` / `slow` |
| 调一次工具能跑通 | `shots/g17-http-call.png`（授权卡片）→ `shots/g19-after-allow.png`（`echo: 来自模型的问候`） |
| 自定义请求头能到达服务器 | `mcp_http_calls.jsonl`：`tools/call` 那条带着 `x-perch-test: hello-from-header` |
| 编辑器两种连接方式 | `shots/g3-editor-http.png` / `g4-editor-stdio.png`；切来切去不清空已填内容（`g6` → `g8`） |

⚠️ **查那个日志时大小写必须不区分**：`http` crate 的 `HeaderName` 会把名字规范成小写，
所以收到的是 `x-perch-test` 而不是 `X-Perch-Test`。用 `headers['X-Perch-Test']` 查会
什么都查不到，看着就像"自定义头没到"——实测被骗过一次。

---

## 十一、分阶段（建议顺序 + 规模估算）

> 规模是**净改动行数**（新增 + 修改，不含格式化噪声），2026-09-27 晚按现有代码实测估的。
> 「文件」列写「改 / 新建」。测试行数已含在各阶段里（约占 15%~20%）。

| 阶段 | 内容 | 净改动 | 文件 | 要拍板 |
| --- | --- | --- | --- | --- |
| **A** | 数据模型改造：`SessionMode` + `ToolSource` + 迁移 + `tool_list_for` 重写（含"全局开关只管本机"） | 约 400 行 | 8 改 / 0 新 | ✅ 已定 |
| **B** | 选择器改服务器级 + 默认不勾 + 「高级」折叠 | 约 300 行 | 4 改 / 0 新 | ✅ 已定 |
| **C** | 智能体项目目录（选目录、相对路径、目录外授权、提示词） | 约 460 行 | 3 改 / 2 新 | ✅ 已定 |
| **D** | 权限档位（默认 / 完全）+ 二次确认 + 界面标记 | 约 265 行 | 4 改 / 0 新 | ✅ 已定 |
| **E** | Skills（最独立，可插空做） | 约 860 行 | 4 改 / 3 新 | 否 |
| **F** | 审计日志（`%APPDATA%\Perch\logs\`） | 约 270 行 | 4 改 / 1 新 | 否 |
| **G** | MCP Streamable HTTP | 约 645 行 | 5 改 / 0 新 | ✅ 已完成（代理除外，见第十节） |
| — | ~~进程沙盒~~ | — | — | ❌ 不做，只做边界（第七节） |

**A~G 合计：约 3200 行净改动，20 个文件（14 改 + 6 新建）。**
其中 **A~D（Chat/Agent 的核心）约 1425 行、12 个文件（10 改 + 2 新建）**——
E / F / G 是三个可以独立切开、甚至并行的小块。

各阶段会碰到的文件：

| 阶段 | 修改 | 新建 |
| --- | --- | --- |
| A | `model.rs`、`config.rs`、`reply_ops.rs`、`tool_ops.rs`、`ui/tool_picker.rs`、`ui/composer.rs`、`app.rs`、`i18n.rs` | — |
| B | `tool_ops.rs`、`ui/tool_picker.rs`、`model.rs`、`i18n.rs` | — |
| C | `local_tools.rs`、`agent_loop.rs`、`reply_ops.rs`、`i18n.rs` | `workspace_ops.rs`、`ui/workspace_picker.rs` |
| D | `local_tools.rs`、`ui/tool_picker.rs`、`ui/mod.rs`（弹窗）、`i18n.rs` | — |
| E | `local_tools.rs`、`reply_ops.rs`、`config.rs`、`i18n.rs` | `skills.rs`、`skill_ops.rs`、`ui/settings_skills.rs` |
| F | `local_tools.rs`、`agent_loop.rs`、`ui/settings_general.rs`、`config.rs` | `audit.rs` |
| G | `config.rs`、`mcp.rs`、`mcp_ops.rs`、`ui/settings_mcp.rs`、`i18n.rs` | — |

**两个"省事"的实测结论**（估规模前核过代码）：

- ✅ **`storage.rs` 一行不用改。**`SessionTools` 存的是 `sessions.tools` 这个
  **JSON TEXT 列**（`serde_json::to_string`），schema 不变 → **不需要 `ALTER TABLE`、
  不需要 SQL 迁移**，只有 serde 层的形状变化。这砍掉了这类改造里通常最麻烦的一块。
- ⚠️ **`i18n.rs` 每个新文案是 1 行 × 4 种语言**，表里现在 496 个 key。
  A~G 加起来约 60 个新 key ≈ 60 行，已算进各阶段。

顺序建议 **A → B → C → D → E → F → G**。
A 是地基（B / C / D 都建在它上面）；E 最独立，可以和 C/D 并行。

---

## 十二、要拍板的问题 —— ✅ 2026-09-27 已全部定案

| # | 问题 | 结论 |
| --- | --- | --- |
| 1 | 对话模式下 MCP 工具要不要逐次授权？ | ✅ **要**，和智能体一致（MCP 工具是别人写的代码） |
| 2 | 完全权限下，Perch 自己的数据目录要不要仍然拦？ | ✅ **拦**（让模型改自己的配置 = 让模型控制程序行为） |
| 3 | 完全权限下，工作目录之外要不要放行？ | ✅ **放行**，但记审计日志（否则"完全"没意义） |
| 4 | 智能体模式下没设项目目录时：不给本机工具 / 给只读 / 用安装目录兜底？ | ✅ **不给**，界面提示"先选项目目录"（兜底正是现在最坑的地方） |
| 5 | 加 MCP 服务器时默认启用还是默认停用？ | ✅ **默认启用**，"默认不用"靠会话里不勾 |
| 6 | `rmcp` 是否已含 Streamable HTTP 客户端？ | ✅ **已核实：含**（3.4.1，feature `transport-streamable-http-client-reqwest`）。⚠️ 还要额外开一个 TLS feature，见第十节第 3 条 |

结论已回填 `AGENTS.md §11 已确认的产品决策`（那是权威）和 `TODO.md` 第十一节。
**阶段 A~D 可以直接动手，不用再等确认。**

---

## 十三、维护约定

- 本文件是**方案**，可执行的条目在 `TODO.md` 第十四节；做完一项两边一起改。
- 拍板结果要回填到 `AGENTS.md §11 已确认的产品决策`（那是权威），本文只留"为什么这么设计"。
- **不写行号**，只写文件名与函数名。
- 动工前重读第一节，那些是逐行核实过的现状——方案过期比代码过期更坑。
