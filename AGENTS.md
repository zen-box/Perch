# Perch 开发规范（AGENTS.md）

本文件是在本仓库工作的所有 AI 助手（Claude、Gemini、DeepSeek、Codex 等）和开发者共同遵守的规范。

- **动手前先读完本文件。**
- 现有代码里有不少历史遗留写法（见 [§13 技术债清单](#13-技术债清单)），**不要照抄**。本文件与现有代码不一致时，以本文件为准。
- 觉得某条规则不合理，先向用户提出修改本文件，不要自行绕过。
- 产品计划见 `ROADMAP.md`，本文件只管"怎么写"。

## 1. 铁律

1. **先读后改。** 改一个功能前，读完相关代码、调用方和数据结构，再读本文件对应章节。不要凭函数名猜。
2. **只做被要求的事，只改需要改的地方。** 不重构无关代码，不改无关文件；局部修改，保留原有注释和结构，不要把整个文件重新生成一遍。发现的其他问题写在最后的总结里。
3. **大改先确认方案。** 跨多个模块、调整架构、改数据格式、改产品行为（默认值、交互流程、隐私和安全策略），先把方案告诉用户，同意后再做。[§11](#11-安全与隐私) 里"已确认的产品决策"不能推翻。
4. **交付前必须通过** `cargo fmt`、`cargo build`（零警告）、`cargo clippy`（不新增警告）、`cargo test`。
5. **运行时代码不 panic。** 不写 `unwrap()`、`expect()`、`panic!()`；出错要让用户看到原因。
6. **不阻塞界面线程。** 网络、磁盘大文件、子进程、图片编解码、系统文件对话框都放到后台。
7. **颜色、尺寸、文案不写死。** 颜色从 `Palette` 取，尺寸用 [§9](#9-界面设计规范) 的档位，文案走国际化。
8. **数据必须向后兼容。** 老用户的配置、数据库、附件要能继续读取；改格式必须写迁移和测试。
9. **不碰真实数据和密钥。** 测试和调试用临时目录，不写系统凭据管理器。
10. **不擅自加依赖、不提交 git。** 新增依赖、提交、推送都要用户同意。

## 2. 项目概况

Perch 是一个 API 聚合的 AI 对话桌面客户端。

- 技术栈：Rust（edition 2024）+ GPUI（`gpui-kit` 0.6.6，组件来自 gpui-component）。主要平台是 Windows。
- 产品优先级：普通对话 → 多模态（图片、附件）→ MCP / Agent。
- 用户数据在 `%APPDATA%\Perch\`：

| 内容 | 位置 |
| --- | --- |
| 配置（不含密钥） | `perch-config.json` |
| 会话和消息 | `perch.db`（SQLite） |
| 提示词库 | `prompts.json` |
| 附件（按哈希去重） | `attachments/` |
| 模型目录缓存 | `models-dev-cache.json` |
| 图片缓存 | `cache/images/` |
| API Key | 系统凭据管理器，服务名 `Perch` |

旧版本的数据目录 `PersonalControl` 在首次启动时自动迁移，逻辑在 `paths.rs`。

### 常用命令

| 目的 | 命令 |
| --- | --- |
| 编译 / 运行 | `cargo build` / `cargo run` |
| 测试 | `cargo test` |
| 格式化 | `cargo fmt` |
| 静态检查 | `cargo clippy` |

- 程序运行时 exe 被占用，编译前先执行 `taskkill /IM perch.exe /F`。
- `build.rs` 把 Windows 主线程栈设为 8MB（Debug 构建的 GPUI 元素树很深），不要删；它还负责嵌入 `assets/brand/` 下的品牌图标。

## 3. 架构与分层

### 3.1 分层

依赖只能从上往下：

```
界面层   ui/*                              画界面，把用户操作转成 AppState 方法调用
  ↓
应用层   app.rs、*_ops.rs                   AppState：状态、业务流程、后台任务、提示
  ↓
服务层   llm.rs、provider_api.rs、          与外部通信、系统能力
         image_http.rs、models_dev.rs、agent.rs
  ↓
数据层   config.rs、model.rs、storage.rs、   数据结构、持久化、纯计算
         prompts.rs、backup.rs、paths.rs、
         file_store.rs、model_info.rs、brand.rs、
         clipboard.rs、analytics.rs
```

- 下层不能引用上层。数据层和服务层不 `use crate::app` 或 `crate::ui`；`app.rs` 不使用 `ui::` 里定义的类型。
- 数据层和服务层不依赖 GPUI 的 `Context`、`Window`、`Entity`。`brand.rs` 实现 `AssetSource`、`image_http.rs` 实现 `HttpClient` 属于接口适配，是例外；`clipboard.rs` 只用 gpui 的数据类型（`ClipboardEntry`、`Image`）做纯计算，不碰 `Context` / `Window`，同样允许。
- 界面层不读写文件、不发网络请求，也不直接改 `config` 或 `storage` 的字段，一律调用 AppState 的方法。
- 解析、计算、格式转换这类逻辑，尽量写成下层的纯函数，便于测试。

### 3.2 新代码放在哪里

| 要写的东西 | 放在哪里 |
| --- | --- |
| 配置项、渠道和模型设置 | `config.rs` |
| 对话、消息、附件的数据结构 | `model.rs` |
| 数据库表和读写 | `storage.rs`（改表结构要升版本号、写迁移） |
| 数据文件路径 | `paths.rs`：数据目录、`APP_NAME` / `LEGACY_APP_NAME`，以及全部数据文件名常量（`CONFIG_FILE`、`SESSIONS_FILE`、`DATABASE_FILE`、`PROMPTS_FILE`、`MODELS_DEV_CACHE_FILE`）和旧名对照表 `LEGACY_FILES`。写文件一律用 `write_atomic` / `write_atomic_bytes` |
| 大模型请求格式 | `llm.rs`（请求体构建写成纯函数并测试） |
| 渠道管理接口（拉取模型、测试连接） | `provider_api.rs` |
| 一组新的业务操作 | 新建 `xxx_ops.rs`，写 `impl AppState { … }`；不要再往 `app.rs`、`session_ops.rs` 里加 |
| 剪贴板内容的识别和转换 | `clipboard.rs`（纯数据层：接收 `&[ClipboardEntry]`，判断该粘贴什么、把图片转成可保存格式）。**读剪贴板本身**在 `attachment_ops.rs` 里调 `cx.read_from_clipboard()`，不要直接调 Win32 剪贴板接口 |
| 附件、粘贴 | `attachment_ops.rs` |
| 确实需要的平台 API（Win32 等） | 新建 `platform/` 模块封装，业务代码只调用封装后的函数 |
| 统计、计算、格式化 | 数据层的纯函数，不写在 `ui/` 里 |
| 新页面、面板 | `ui/` 下新建文件 |
| 弹窗 | 简单的放 `ui/dialogs.rs`，超过约 150 行的单独一个文件 |
| 多处复用的小组件 | `ui/mod.rs`，或新建 `ui/widgets.rs` |

### 3.3 规模

- 单个文件（不算测试）超过 **800 行** 就要拆。目前没有超标文件（最大的 `llm.rs` 非测试 796 行）：新功能不要再往大文件里加；改到其中某块时，顺手把那块拆成新文件。
- 界面函数超过约 100 行，或链式调用嵌套超过 4 层，拆出 `render_xxx` 子函数。
- 每个 `xxx_ops.rs` 只负责一个领域，例如会话、模型、附件、渠道。

## 4. 状态管理

### 4.1 AppState

`AppState`（`app.rs`）是唯一的顶层状态实体，持有配置、会话、提示词库和各个输入框。它已经有 60 多个字段，所以：

- **不再往 `AppState` 顶层加零散字段。** 新功能的状态合并成一个结构体，作为一个字段（如 `pub analytics: AnalyticsState`）；独立的面板可以做成单独的 `Entity`，自己持有输入框和临时状态。
- **临时状态属于弹窗或面板。** 表单用"草稿 + 保存时写回"：打开时复制一份，编辑草稿，点保存才写回配置。参考 `model_ops.rs` 的 `ModelEditor`。
- **持久数据只在 AppState 的方法里修改。** 方法负责：修改 → 保存 → 失败时 toast → `cx.notify()`。
- 方法命名以动词开头：`begin_xxx`（打开弹窗、准备草稿）、`confirm_xxx`（保存，返回 `bool` 表示能否关闭弹窗）、`cancel_xxx`、`toggle_xxx`、`set_xxx`。
- 只有要改输入框内容或焦点的方法才接收 `window: &mut Window`。

### 4.2 GPUI 实体读写（违反会直接 panic）

- `cx.listener(|this, event, window, cx| …)` 的回调执行时，AppState 已经处在更新中：直接用 `this`，**不能再调用 `app.update(cx, …)` 或 `app.read(cx)`**。
- 弹窗、Popover、菜单项的回调不在 AppState 的更新中，这时才用 `app.update(cx, |this, cx| …)`。
- `AppState::render` 里不能更新自己。渲染时要触发的动作（弹提示、开弹窗），用 `window.defer(cx, …)` 推迟，参考 `ui/mod.rs` 里 toast 的处理。
- 弹窗内容在 `Workspace` 渲染时构建，构建函数里可以 `app.read(cx)`。

```rust
// ❌ 在 listener 里再次更新同一个实体：运行时 panic
Button::new("pin").on_click(cx.listener(move |_, _, _, cx| {
    app.update(cx, |this, cx| this.toggle_session_pin(&id, cx));
}))

// ✅ listener 里直接用 this
Button::new("pin").on_click(cx.listener(move |this, _, _, cx| {
    this.toggle_session_pin(&id, cx);
}))
```

## 5. 异步、线程与 I/O

- tokio 运行时**只有一个**：`crate::app::runtime()`。不要调用 `tokio::spawn`（GPUI 线程上没有 tokio 上下文，会 panic），也不要自己创建运行时。
- 标准写法：

```rust
cx.spawn(async move |this, cx| {
    let result = runtime()
        .spawn(async move { provider_api::fetch_models(&provider).await })
        .await
        .unwrap_or_else(|error| Err(format!("拉取失败：{error}")));
    let _ = this.update(cx, |state, cx| {
        match result {
            Ok(models) => { /* 更新状态 */ }
            Err(error) => state.toast(ToastLevel::Error, error),
        }
        cx.notify();
    });
})
.detach();
```

- 会阻塞的操作放到后台：网络、大文件读写、子进程、图片编解码。`rfd` 系统文件对话框在 `std::thread::spawn` 里调用，用 `oneshot` 把结果送回界面，参考 `attachment_ops.rs` 的 `pick_attachments`。
- 渲染函数只做轻量工作。遍历全部会话、解析大 JSON、读文件这类耗时计算，要缓存结果或放到后台。
- 网络请求：
  - 访问用户配置的渠道时，遵守该渠道的代理、超时和自定义请求头。
  - 超时用 `connect_timeout` 加 `read_timeout`（空闲超时），不要用 `timeout()`：它是总时长，会截断正常输出中的长回答。
  - 流式响应按字节缓存，凑齐一行再解码，否则汉字会被网络分块切断。
- 流式回复按消息 id 写回（`apply_stream_event`），不要按"最后一条消息"定位；取消走 `active_streams`。

## 6. 错误处理

- 运行时代码不写 `unwrap()`、`expect()`、`panic!()`。只有两个例外：
  - 字面量常量的初始化，如 `LazyLock<Regex>` 里的 `Regex::new(r"…").unwrap()`；
  - 测试代码。
- 错误要让用户看到：
  - 业务方法里用 `self.toast(ToastLevel::Error, format!("模型保存失败：{error}"))`；
  - 单条消息的错误写进 `ChatMessage.error`，显示在消息卡片里。
- 不要静默吞错。`let _ = self.config.save();`、`Err(_) => return` 只能用在"失败了也无所谓"的地方（如向已关闭的 channel 发送），并加注释说明。**保存失败必须提示用户。**
- 底层函数返回具体的错误类型，或返回 `Result<T, String>`（面向用户的中文消息）；到 AppState 这一层统一转成提示。
- 错误消息里不能出现 API Key。拼接请求相关的错误时，经过 `llm.rs` 的 `redact`。
- 目前没有日志系统。不要把 `println!`、`eprintln!` 留在代码里：Windows 图形程序看不到这些输出。

## 7. 数据与兼容

- 持久化结构（`AppConfig`、`ProviderConfig`、`ModelConfig`、`ChatSession`、`ChatMessage` 等）新增字段必须加 `#[serde(default)]`；`Option` 字段再加 `skip_serializing_if = "Option::is_none"`。
- **不要改名或删除已经持久化的字段、枚举变体。** 确实要改，写迁移并加测试，参考 `ModelConfig::migrate_legacy_tags`。
- SQLite 表结构有变化，在 `storage.rs` 的 `migrate` 里升级版本号并补迁移测试，参考 `upgrades_v1_database_without_losing_sessions`。
- 写用户数据用 `paths::write_atomic`（先写临时文件再改名），不要直接 `fs::write` 覆盖。
- 数据文件名、目录名、凭据服务名都属于用户数据，改名必须兼容旧数据，参考 `paths.rs` 的 `LEGACY_FILES`。
- API Key 只通过 `AppConfig::store_provider_key` 和 `config::load_provider_key` 存取系统凭据管理器。`api_key` 字段不序列化，备份文件不含密钥，这两点都有测试，不要破坏。

## 8. 代码风格

- 格式以 `cargo fmt` 为准，配置在 `rustfmt.toml`（行宽 120，换行符 LF）。不要手工对齐，不要留奇怪的缩进。
- 文件编码 UTF-8，换行符 LF（见 `.editorconfig`、`.gitattributes`）。
- 命名：
  - 类型用 `PascalCase`，函数和变量用 `snake_case`，常量用 `SCREAMING_SNAKE_CASE`；
  - 界面构建函数叫 `render_xxx`；
  - 元素 id 用短横线连接的英文，如 `"model-picker"`；列表里的动态 id 用 `("model-row", ix)` 或 `SharedString::from(format!("model-row-{id}"))`。
- 模块：二进制程序里一律写 `mod xxx;`，不写 `pub mod`；只在模块内部用的函数不加 `pub`。
- 同样的逻辑出现第二次，就抽成函数，不要复制粘贴。
- 注释用中文，写"为什么"，不复述代码；不明显的公开函数写 `///` 文档注释。不留注释掉的代码；改了行为要同步改注释。
- 不留死代码：用不到的就删，不要用 `#[allow(dead_code)]` 掩盖。确实需要保留的，写明原因。
- 新增依赖前先确认现有依赖能不能做到，并经用户同意。不要从 GPL / AGPL 项目复制代码（例如同目录下的 AQBot）。

## 9. 界面设计规范

### 9.1 总体风格

- 基调是 shadcn 风格的中性灰加靛蓝主色（`theme.rs`）。留白充足，层级靠字号、字重和明暗区分，少用边框和色块。
- 所有界面在浅色、深色两种主题下都要正常显示。
- 图标只用 Lucide（`gpui_kit_assets::IconName`）。厂商 logo 只用于模型和渠道头像（`ui/brand_icon.rs`）。不要用 emoji 当图标。

### 9.2 布局

| 常量 | 值 | 用途 |
| --- | --- | --- |
| `SIDEBAR_WIDTH` | 260 | 对话侧边栏 |
| `CONTENT_MAX_WIDTH` | 780 | 消息列和输入框（居中） |
| `PAGE_MAX_WIDTH` | 720 | 设置页内容 |
| — | 220 | 设置页左侧导航 |
| — | 248 | 渠道列表 |

- 新的布局尺寸定义成常量，不要把数字散落在各处。
- flex 子元素里的长文字要截断时，同时写 `.min_w_0()` 和 `.truncate()`。

### 9.3 间距（1 个单位 = 4px）

| 场景 | 写法 |
| --- | --- |
| 图标与文字、紧凑元素之间 | `gap_1`、`gap_1p5` |
| 同一行的控件之间 | `gap_2` |
| 卡片里的行、表单字段之间 | `gap_3`、`gap_4` |
| 页面上的分区之间 | `gap_6`、`gap_8` |
| 卡片和设置行的内边距 | `px_4().py_3()`；紧凑卡片 `p_3()` |
| 页面边距 | 设置页 `px_8().py_8()`；对话区 `px_6()` |

### 9.4 文字

| 用途 | 写法 |
| --- | --- |
| 空状态大标题 | `text_2xl()` + `FontWeight::SEMIBOLD` |
| 页面标题 | `text_xl()` + `FontWeight::SEMIBOLD` |
| 正文、按钮、列表项（默认） | `text_sm()`；行标题加 `FontWeight::MEDIUM` |
| 说明、提示、时间、徽标 | `text_xs()` + `muted_foreground` |
| 模型 ID、代码、路径 | 等宽字体 `cx.theme().mono_font_family` |

### 9.5 颜色

颜色只从 `Palette::new(cx)` 取，不写十六进制色值。

| 颜色 | 语义 |
| --- | --- |
| `primary` | 选中状态、主操作、链接 |
| `danger` | 删除、错误 |
| `warning` | 需要用户注意，如本地命令授权 |
| `success` | 已启用、成功 |
| `muted`、`muted_foreground` | 次要背景、次要文字 |
| `border` | 分隔线、描边 |

- 常用透明度：选中背景 `primary.opacity(0.08)`；悬停背景 `muted`；错误卡片背景 `danger.opacity(if p.is_dark { 0.12 } else { 0.06 })`。
- 例外：品牌色（`brand.rs`）、能力图标色（`ui/brand_icon.rs`）、图表配色（`ui/analytics.rs`）。这些必须集中定义成常量，界面代码里不要再出现新的色值。

```rust
// ❌
.bg(rgb(0xF3F4F6)).text_color(hsla(0., 0., 0.4, 1.))
// ✅
.bg(p.muted).text_color(p.muted_foreground)
```

### 9.6 圆角与尺寸

| 元素 | 圆角 |
| --- | --- |
| 列表行、小按钮、chip | `rounded_md()` |
| 卡片、面板、图片框 | `rounded_lg()` |
| 大卡片（如空状态的建议卡） | `rounded_xl()` |
| 输入框外框 | `rounded(px(16.))` |
| 胶囊、状态圆点 | `rounded_full()` |

- 图标：12（行内极小）、14（菜单、列表）、16（按钮、卡片）。
- 头像：16–20（下拉列表）、28（消息、渠道列表）、32（模型列表）、44–52（页面或弹窗头部）。
- 列表行高 32–34。

### 9.7 组件

先用现成组件；写新组件前先搜一下有没有同类实现。同一个概念只保留一个实现。

| 需求 | 用什么 |
| --- | --- |
| 主操作（每个区域最多一个） | `Button::new(..).primary()` |
| 次要操作 | `.outline()` |
| 工具栏、行内操作 | `.ghost()`；行内用 `.xsmall()`，工具栏和表单用 `.small()` |
| 只有图标的按钮 | 必须加 `.tooltip("…")` |
| 开关类设置 | `Switch` |
| 2–4 个互斥选项 | `segmented`（`ui/settings.rs`） |
| 预设值、多选标签 | `chip`（`ui/mod.rs`） |
| 列表勾选 | `Checkbox` |
| 选择器、参数面板 | `Popover` |
| 简短操作菜单 | `dropdown_menu` + `PopupMenuItem`；右键菜单用 `context_menu` |
| 表单弹窗 | `window.open_dialog`，写在 `ui/dialogs.rs`，字段用 `field`、底部按钮用 `footer` |
| 危险操作确认 | `window.open_alert_dialog` + `danger_props` |
| 操作结果提示 | `self.toast(…)` |
| 加载中 | `Spinner` |
| 小标签 | `Tag` |
| 模型、渠道头像 | `model_avatar`、`provider_avatar`、`brand_avatar` |
| 普通图标块 | `icon_tile` |
| 设置页 | `page`、`section`、`setting_row`（`ui/settings.rs`） |

### 9.8 交互

- 删除、清空、覆盖、会丢数据的重新生成，都要先弹确认框，确认按钮用危险样式。
- 可点击的行：整行 hover 变色、`cursor_pointer()`。行上的次要按钮和"点击整行"的区域做成**兄弟元素**，不要嵌套（原因见 [§10](#10-gpui-踩坑记录) 第 1 条）。
- 悬停才出现的按钮用 `.invisible()` 加 `.group_hover(…)`，同样的操作在右键菜单里也要能找到。
- 弹窗里 Enter 提交（`on_ok`）、Esc 关闭。要给输入框设焦点时，先打开弹窗再设。
- 标题栏里的可点击元素必须加 `.occlude()`，否则在 Windows 上点击会被当成拖动窗口。
- 空状态：图标块、标题、一句说明、一个主操作按钮。
- 错误提示写清楚三件事：哪一步失败、原因、下一步怎么办。

### 9.9 文案

- 用"你"，不用"您"；句子简短，不用感叹号。
- 按钮用动词，如「保存」「添加模型」「重新生成」。完成类提示用"已……"，如「已复制到剪贴板」。
- 引用界面上的名称用「」，如：点击「从接口拉取模型」。
- 专有名词保留英文：API Key、Base URL、token、MCP。

### 9.10 国际化

- 界面文案通过 `i18n::tr(lang, key)` 获取。新增文案时，简体中文、繁体中文、英文、日文四种语言一起加。
- `tr` 遇到不存在的 key 会返回空字符串，加完一定要在界面上确认能正常显示。
- 现状：大部分文案仍是写死的中文（约 600 处）。改到哪块，就顺手把那块迁到 `tr`。

## 10. GPUI 踩坑记录

1. **点击回调里不要 `cx.stop_propagation()`。** `on_click` 在鼠标抬起时触发；拦截后，窗口级的文字选择收不到鼠标抬起，之后鼠标一动就在选文字。需要"点这里不触发外层"时，把两者做成兄弟元素。自己写的可点击元素，在按下时调用 `GlobalState::suppress_text_selection(cx)`（`Button` 已自带）。`Workspace` 里的 `mouse_guard` 只是兜底，不能依赖。
2. 嵌套更新同一个实体会 panic，见 [§4.2](#42-gpui-实体读写违反会直接-panic)。
3. `InputState::set_value` 不会触发 `InputEvent::Change`。订阅里维护的数据，在程序改值时要自己同步。
4. 弹窗打开时会抢走焦点：先 `open_dialog`，再调用 `input.focus(window, cx)`。
5. `img("…")`：能解析成 URL 的字符串走网络（经过 `image_http.rs`），其余从嵌入资源加载（`brand::AppAssets`）。SVG 按固有尺寸的 2 倍栅格化；LobeHub 图标的 `width="1em"` 由 `build.rs` 改成 64。
6. `svg().path(…)` 只用透明度，按文字颜色着色，适合单色图标；彩色 SVG 用 `img()`。
7. `Button` 高度是固定的，往里面放大块内容会被裁掉。
8. Markdown 插件的 `render` 每次渲染都会执行，状态变了调用 `window.refresh()` 就会重画。
9. 在写了 `use gpui_kit::*;` 的文件里，测试要写 `#[std::prelude::v1::test]`：gpui_kit 导出了同名的 `test` 宏。
10. Debug 构建主线程栈溢出：不要删 `build.rs` 里的 8MB 栈设置。
11. **GPUI 先按快捷键分发动作，最后才把按键交给 `on_key_down` / `capture_key_down`。** 输入框已经绑定的键（Ctrl+V、Ctrl+C 等）用按键监听拦截不到。要改输入框的粘贴，用 `Input` / `Textarea` 的 `on_paste`；要加窗口级快捷键，在 `main.rs` 里给 `Perch` 上下文绑定动作（输入框自己的绑定上下文更深，会优先生效）。
12. `ClipboardItem::text()` 在剪贴板里没有文字时，会返回复制的文件的路径。判断剪贴板里是什么要看 `entries()`，统一用 `clipboard::classify`。

## 11. 安全与隐私

- API Key 只存在系统凭据管理器里。提示、错误信息、备份文件里都不能出现明文密钥。
- 本地工具（`/bash`、`/read` 等）默认关闭。每次执行都要用户在授权卡片上确认，不能自动执行模型输出的命令。
- 模型回复是**不可信内容**，可能被提示词注入操纵。回复里的链接和图片地址，不能在用户不知情时访问；本机和局域网地址一律拦截（`image_http.rs`）。
- 程序只访问用户配置的渠道地址。要新增访问其他服务的功能（包括后台同步），需要用户同意，并且在设置里可以关闭。
- 附件只保存在数据目录里，文件名先清理（`file_store::sanitize_file_name`），单个文件不超过 `file_store::MAX_ATTACHMENT_BYTES`。
- 粘贴的文字就算恰好是一个本地文件路径，也按文字处理，不能自动把那个文件变成附件：否则复制一个路径，就可能把私密文件发给模型。

### 已确认的产品决策

以下不能推翻，要改先问用户：

- 先把普通对话做好，Agent 和 MCP 放在最后。
- 本地工具默认关闭，每次执行都要授权。
- API Key 不写进任何文件。
- 在较早的回答上"重新生成"会删掉后面的消息，必须先确认。
- 模板变量（`{{clipboard}}` 等）只在插入模板时展开，发送消息时不展开，避免剪贴板内容被悄悄发出去。
- OpenAI 渠道上的推理模型不发送默认温度（会报 400）。

## 12. 测试与交付

### 12.1 测试

- 纯逻辑必须有单元测试：请求体构建、响应解析、数据迁移、模型识别、格式化、路径处理等。测试写在同一文件的 `#[cfg(test)] mod tests` 里。
- 测试不能有副作用：
  - 不碰真实数据目录，用 `tempfile`；
  - 不写凭据管理器，确实需要就加 `#[ignore]`；
  - 不读写系统剪贴板，不访问外网。
- 测试要有断言。只打印、不断言的测试不要提交。
- 改了界面要实际运行看一遍：
  - 用临时数据目录启动：把环境变量 `APPDATA` 指向 `target/ui-test/appdata`，提前放好 `Perch/perch-config.json`；
  - 需要接口时，起一个本地模拟服务，渠道地址填 `http://127.0.0.1:<端口>/v1`；
  - Debug 构建设置 `PC_DEV_ALLOW_LOCAL_IMAGES=1`，可以加载本地模拟服务的图片；
  - 浅色和深色都要看；
  - 测完删掉临时目录。

### 12.2 交付检查清单

1. `cargo fmt`
2. `cargo build`：零警告
3. `cargo clippy`：改动的代码不新增警告
4. `cargo test`：全部通过
5. 界面改动在浅色、深色下都实际看过
6. 改了数据格式：有迁移、有测试，旧数据能正常读取
7. 最后的总结写清楚：改了什么、为什么改、怎么验证的、还有什么没做或有什么风险

### 12.3 Git

- 用户没有要求时，不提交、不推送。
- 一个提交只做一件事。格式化、移动文件、修改逻辑要分开提交。
- 提交信息格式：`类型: 简述`，类型用 `feat`、`fix`、`refactor`、`style`、`docs`、`test`、`chore`。

## 13. 技术债清单

下面是现有代码里已知的问题。**不要照着这些写法写新代码。**标"改到时修"的，碰到相关代码时顺手修掉；标 ⚠ 的需要用户先拍板。

| # | 问题 | 位置 | 处理 |
| --- | --- | --- | --- |
| 1 | 约 600 处写死中文，只有 24 处 `tr()`；`tr` 遇到未知 key 返回空字符串 | `ui/*`、`i18n.rs` | ⚠ 国际化方案待定 |
| 2 | 远程图片自动加载并写入磁盘缓存（没有容量上限，也不清理）；下载层去掉了"用户同意"的检查 | `ui/markdown_image.rs`、`image_http.rs` | ⚠ 和之前"默认不加载、不落盘"的决定冲突，待确认 |
| 3 | 启动时自动访问 models.dev；自建线程和 tokio 运行时；不走代理；错误全部静默 | `models_dev.rs` | ⚠ 是否保留自动同步待确认；保留的话改用 `runtime()`、走代理、在设置里加开关 |
| 4 | 启动或初始化失败直接 panic（7 处） | `main.rs`（`main`）、`app.rs`（`runtime`、`new`）、`config.rs`（`load`）、`model.rs`（`load_or_init`）、`paths.rs`（`data_file`）、`llm.rs`（`claude_body`） | 改成错误提示界面。`model_info.rs` 的 5 处 `LazyLock<Regex>` 属 [§6](#6-错误处理) 合法例外 |
| 5 | 阻塞界面线程：本地工具同步执行 | `app.rs` | 放到后台 |
| 6 | 重复的小组件：`filter_chip` 和 `chip`、`labeled` 和 `row_title`、`section` 和 `form_card` | `ui/*` | 合并到 `ui/widgets.rs` |
| 7 | 拉取模型、测试连接不走渠道代理 | `provider_api.rs` | 改到时修 |
| 8 | OpenAI Responses 渠道仍按 Chat Completions 格式发请求 | `llm.rs` | 修好之前不要推荐用户使用 |
| 9 | 全部会话和消息常驻内存，保存时全量比对 | `model.rs`、`storage.rs` | 见 ROADMAP |
| 10 | 2 处 `#[allow(clippy::too_many_arguments)]` 压着 clippy（`render_assistant_message` 9 个参数、`token_row` 8 个参数） | `ui/message_assistant.rs`、`ui/model_editor_dialog.rs` | 参考 `ui/params.rs` 的 `ChoiceRow`，用结构体收参数 |

修掉一项，就从这张表里删掉；新发现的问题也记进来。

---

本文件随代码一起维护：约定变了，先改这里，再改代码。
