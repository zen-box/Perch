# 技术债与待优化清单

> 写于 2026-09-26 晚。**AGENTS.md §13 保有同样的表**，那份是"给写代码的人看的禁令"（不要照抄这些写法），
> 本文是"排期用的工单"——多了实测规模、改法、前置依赖。
> 两边都要维护：改完一项，两处一起删。

## 一、排期建议（结论先行）

**先做 P3/P4 的功能，技术债分批插空清。**

理由见第四节。简单说：P3 要新建工具调用协议，而现在这 9 条里**只有 3 条会被 P3 碰到**，
其中 2 条（#4、#6）恰好是 P3 的前置条件——提前清反而要改两遍。

## 二、总览

代码规模：**54 个 rs 文件 / 19069 行**（P3-1 拆出 `llm_request.rs` / `llm_stream.rs` / `llm_tools.rs` 后）。
下面"规模"是逐条读过代码后估的**净改动量**（不含格式化噪声）。

| # | 问题 | 性质 | 规模 | 阻塞谁 | 建议 |
| --- | --- | --- | --- | --- | --- |
| 3 | 启动/初始化失败直接 panic（7 处） | 技术债 | ~120 行 | 无 | **随时可做**，唯一两条"用户能感知"的之一 |
| 4 | 本地工具同步执行，卡住界面 | 技术债 | ~60 行 | **P3 依赖** | 等 P3 |
| 6 | 拉取模型、测试连接不走渠道代理 | 技术债（已是 bug） | ~25 行 | 无 | **随时可做**，最便宜 |
| 8 | 2 处 `too_many_arguments` | 技术债 | ~100 行 | 无 | **随时可做**，顺手拆函数 |
| 5 | 重复小组件 | 技术债（**表有错判**） | ~100 行 | 无 | 顺手修，见下 |
| 1 | 远程图片自动落盘、无上限、无清理 | **待拍板** | ~50 行 | 无 | 需你定行为 |
| 2 | models.dev 自动同步（自建线程 / 不走代理 / 静默） | **待拍板** | ~30 行 | 无 | 需你定是否保留 |
| 7 | 全部会话消息常驻内存 | **架构** | 300~700 行 | P3/P4 | 放 v2，见第四节 |

> ✅ **2026-09-26 结清：原 #7「OpenAI Responses 渠道格式不对」**（约 150 行）。
> P3-1 打通工具调用协议时一并修好——`handle_sse_line` 重写后保留 `event:` 行，
> `OpenAiResponses` 在 `emit_delta` / `emit_complete` 里拆成独立分支。
> 回归测试 `llm::tests::responses_streaming_uses_event_names`。
> **原 #8、#9 顺次上移为 #7、#8**，本文档与 `AGENTS.md §13` 已同步。

## 三、逐条详情

### #3 启动或初始化失败直接 panic —— 建议先做

7 个位置（`main.rs::main`、`app.rs::runtime`/`new`、`config.rs::load`、`model.rs::load_or_init`、
`paths.rs::data_file`、`llm.rs::claude_body`）。

现在配置损坏时程序**直接闪退、不给任何提示**，用户只能猜。这是清单里唯一"用户能感知到"的健壮性问题。

改法（关键是**不用改 UI 层任何签名**）：

1. 把 `AppState::new` 的实际初始化抽成 `fn init(...) -> Result<Self, String>`。
2. `AppState` 加 `init_error: Option<String>` 字段。
3. `AppState::new` 里 `init(...)` 失败时，构造一个只有 `init_error` 的最小状态。
4. `Workspace`（或 `AppState::render`）开头短路：有 `init_error` 就渲染一个错误页——带错误详情、
   「打开数据目录」按钮、「退出」按钮。
5. `main.rs` 里开窗口的 `.unwrap()` 单独处理（那时还没有 `cx` 可用来渲染，只能打日志 + 退出）。

`paths.rs:197` 的 `fs::copy` panic 要单独看：那是**单文件迁移失败**，本来已有
`MigrationFailure` 收集机制（见 `paths.rs`），应该改走那条路而不是 panic。

⚠️ `model_info.rs` 的 5 处 `LazyLock<Regex>` **不动**，属 AGENTS.md §6 合法例外。

### #4 本地工具同步执行 —— 等 P3

`app.rs:598-629` 的 `execute_agent_tool` 直接同步调 `execute_local_tool`，`/bash` 跑慢命令时界面冻结。

改法：`cx.spawn` 包一层，内部 `crate::app::runtime().spawn(...)`。
**难点不是异步，是消息顺序**——现在先算出结果再插入消息，改成异步后要
「先插入一条 streaming 占位消息 → 后台执行 → 回填内容」，否则用户点完没反应。

**为什么等 P3**：P3 要把这套改成"模型调用工具 + 权限分级 + 可中断"，
届时这个函数会被整体重写。现在改等于白改一遍。

### #6 拉取模型不走代理 —— 最便宜的一条

`llm.rs:143-160` 已有完整的代理套用逻辑（含 `Proxy::all` 与失败时的报错翻译），
但 `provider_api.rs:12` 的 `Client::builder()` 是裸的。

改法：把 `llm.rs` 那段抽成共用函数（例如 `http::client_builder(proxy, lang)`），两处共用。
`provider_api::fetch_models` 需要多接一个 `proxy: &str` 参数——注意它的调用点在
`provider_ops.rs`，`AppConfig` 里已有代理字段可取。

### #8 `too_many_arguments` ×2 —— 顺带拆函数

| 函数 | 参数 | 体量 | 调用点 | 做法 |
| --- | --- | --- | --- | --- |
| `ui/model_editor_dialog.rs::token_row` | 11 | 54 行 | 2 | 套 `struct TokenRow`，直接改 |
| `ui/message_assistant.rs::render_assistant_message` | 9 | **326 行** | 1 | 套结构体不难，但顺手把这 326 行拆开（远超 §3.3 的 100 行上限） |

参考 `ui/params.rs` 的 `ChoiceRow`——那是本项目已有的"用结构体收参数"范式。
改完把两处 `#[allow(clippy::too_many_arguments)]` 删掉。

### #5 重复小组件 —— ⚠️ 表里有错判

逐对读过源码，AGENTS.md §13 的第 5 条**写错了**：

- `filter_chip`（`ui/sidebar.rs:180`）与 `chip`（`ui/mod.rs:161`）**不是一个东西**：
  前者是 `Button` + xsmall/primary/ghost，后者是 `h_flex` 手绘的 28px 圆角块。
  两者视觉与交互都不同，**不该合并**，这条应从表里删掉。
- 真正重复的是另外两对半：
  - `section`（`ui/settings.rs:140`，7 处调用）与 `form_card`（`ui/model_editor_dialog.rs:375`，3 处调用）
    —— **几乎逐行相同**，差一个可选的 `status` 副标题。合并成 `ui/widgets.rs::form_card(title, status, p, rows)`，
    `status: Option<&str>` 为 `None` 时就是 `section`。
  - `labeled`（`ui/params.rs:220`）与 `row_title`（`ui/model_editor_dialog.rs:402`）
    —— 都是"标题 + 说明"，前者横排（说明在右）、后者竖排（说明在下）。
    抽成带 `axis` 参数的一个函数即可。

实际是把 4 个函数收敛成 2 个，改约 10 个调用点。

### #1 远程图片自动加载并落盘 —— 待你拍板

**现状已核实**：
- `ui/markdown_image.rs:179` 注释明写"远程网络图片：直接发起加载并显示，**不需手动点击**"。
- 缓存落在 `%APPDATA%\Perch\cache\images\`（`image_http.rs:234`，文件名是 URL 的 hash）。
- **没有任何容量上限，也没有任何清理逻辑**（全项目搜不到 evict/remove/TTL）。
- 安全侧仍有兜底：`is_blocked_host` 拦本机与局域网地址、`MAX_IMAGE_BYTES` 限制单张大小。

**这和"默认不加载、不落盘"的早期决定冲突**（AGENTS.md §11 写着"回复里的链接和图片地址，
不能在用户不知情时访问"）。需要你定一个：

- **A** 保留自动加载，加**容量上限 + LRU 清理**（例如 200MB、启动时清理）——约 50 行。
- **B** 恢复"点一下才加载"，缓存那套可留可去——约 80 行，且要重画 `image_card` 的未加载态。
- **C** 保持现状，把 AGENTS.md §11 那条改成"已确认自动加载"——0 行，但要把条目从债表删掉。

### #2 models.dev 自动同步 —— 待你拍板

**现状已核实**：
- `main.rs:44` 无条件调用 `sync_cache_background(false)`——**每次启动都访问外网**，用户不知情。
- `models_dev.rs:157` 自己 `std::thread::spawn` + `new_current_thread` runtime，
  而 `app.rs:42` 已经有现成的 `runtime()`（AGENTS.md §7 明确要求用后者）。
- `Client::builder()` 不带代理。
- 失败路径全是 `return` / `let _ =`，**全静默**。

它拉的是模型元数据（上下文窗口、价格），用于用量看板和成本计算——**删掉会丢功能**。

需要你定：
- 是否保留自动同步？（保留的话改成 `runtime()` + 走代理 + `AppConfig` 加 `models_dev_enabled` 开关
  + 失败弹一次 toast，约 30 行；参考 `ui/settings_general.rs:153` 的 `Switch` 范式）

### ~~#7 OpenAI Responses 渠道格式不对~~ —— ✅ 2026-09-26 已结清

**结清时的实际情况**（留作记录，别再按这个改）：

1. **URL 一直是对的**（`openai_url` 会拼 `/responses`）——当时列这条债时差点误判成 URL 问题。
2. 错在 `openai_body()` 发的是 `messages`；`emit_delta` / `emit_complete` 解析 `/choices/0/...`。
3. `handle_sse_line` **把 `event:` 行整个丢掉**，只取 `data:` 之后的 JSON。

**怎么修的（P3-1 顺带做掉）**：

- `handle_sse_line` 重写：`event:` 行存进跨行变量、空行清空、`data: [DONE]` 触发收尾。
  顺带修掉一个隐患——旧代码用 `line.trim()`，而 `\r\n` 行尾的 `\r` 会让事件名带上 `\r`。
- `emit_delta` / `emit_complete` 里 `OpenAiResponses` **拆成独立分支**，按事件名分派
  （`response.output_text.delta` / `response.reasoning_summary_text.delta` /
  `response.output_item.added` / `response.function_call_arguments.delta`）。
  非流式的 `emit_complete` 改读 `output[]` 数组、按 `type` 分派。
- 回归测试：`llm::tests::responses_streaming_uses_event_names`。

⚠️ **遗留一项，另记**（见文末「新发现」）：`openai_body()` 对 Responses 依旧发 `messages`。
真的要用 Responses 渠道时得单独拆 body 函数——和"协议层"不是同一个话题。

### #7 全部会话消息常驻内存 —— 放 v2

`storage.rs:79` 加载时把**所有会话的所有消息** `SELECT ... ORDER BY position` 全部读进内存。
`.messages` 字段散在 **56 处**（`session_ops.rs` 16、`llm.rs` 11、`model.rs` 10、`reply_ops.rs` 5、
`ui/chat.rs` 4、`app.rs` 4、`session_list_ops.rs` 3…）。

改它等于重新定义"消息从哪来"这一数据层协议，56 个调用点全部要跟着改。

**✅ 好消息**：`storage.rs::save` 已经做了签名比对（`session_signature` / `message_signature`），
只写变化的部分、并删除已消失的会话与消息。**那块不用动**，别被条目里"保存时全量比对"的说法误伤——
它比对的是签名而不是全量重写。

建议：v2 做「按需加载当前会话的消息 + 侧边栏只用摘要列」。
`storage.rs:44` 的 `SELECT` 其实**已经**没有带消息（`messages: Vec::new()` 占位），
所以真正要改的是 `storage.rs:79-84` 那段"逐会话把消息塞回内存"的循环，以及调用点的取数方式。

## 四、为什么建议先做 P3/P4

1. **P3 要新建工具调用协议，现在没有这块地基。** 已核实：`llm.rs` 里
   `"tools"` 出现 **0 次**，`tool_calls` / `tool_use` / `functionCall` / `functionDeclarations`
   / `tool_result` / `functionResponse` **一个都没有**。这是从零开始的一层。

2. **9 条里只有 2 条会被 P3 碰到。**（原 #7「Responses 格式」已在 P3-1 结清，见上）
   - #4（本地工具阻塞）——P3 要把本地工具改写成"模型调用 + 权限分级 + 可中断"，
     这个函数会**整体重写**。现在改成异步，P3 时再改一遍。
   - #7（消息常驻内存）——P3 会让消息多出工具调用相关字段、体积变大，
     内存压力只会更明显，但那时的数据结构才定型。现在改是在旧结构上改一遍再改一遍。

3. **另外 6 条和 P3/P4 完全无关，随时可做**，加起来约 **425 行**。
   与其现在做，不如等它们**自己浮上来**：做到哪块顺手清哪块（#6 做渠道功能时、
   #8 改消息渲染时、#5 改设置页时），这比专门排一批划算。

4. **P1/P2 已完成，P3/P4 是最后两个大块。** 做完功能再统一优化，能避免"优化完又被新功能推翻"。

**唯一建议现在就做的：#3。** 它和任何功能都不冲突，而且是"数据坏了程序闪退无提示"——
属于用户会真实遇到的问题。约 120 行，一批提交就能收。

---

## 五、新发现（P3-1 期间）

- **`openai_body()` 对 Responses 渠道仍发 `messages`，规范要的是 `input`。**
  P3-1 把响应侧的解析修对了（按 `event:` 名分派），但**请求侧没动**——因为
  拆 body 函数不属于"工具调用协议"这件事，混在一起会让这一批改动说不清。
  真要启用 Responses 渠道时单独做：把 `openai_body` 拆成 `openai_chat_body` /
  `openai_responses_body`，后者发 `input` 并且消息结构也不同（`input` 是"项"的数组）。

- **`ChatMessageReq` 的字段在长**（`role` / `content` / `attachments` / `tool_calls` /
  `tool_call_id` / `tool_name`）。P3-2 还要加"权限决定"之类的东西时不建议再往上堆，
  考虑按角色拆枚举（`user` / `assistant` / `tool`，各带自己的字段）。

---

## 六、维护约定

- 改完一项：**本文与 AGENTS.md §13 两处一起删**。
- ⚠️ **删条目会让编号顺移，两处的交叉引用必须同步改**（已经踩过三次）。
  若嫌麻烦，可以在重编号时改用带语义的名字（如 `startup-panic`）而不是数字——
  但那样引用会变长，权衡由你定。
- 新发现的问题记进 AGENTS.md §13（那份是权威），在本文补上排期信息。
- 本文里**不写行号**，行号必然过期（已经踩过）。只写函数名与文件路径。
