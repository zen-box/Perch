# 第四批 i18n 方案（技术债 #1）

> 状态：**待拍板**。方案未确认前不改代码。
> 结论：不引入第三方 i18n 库，用宏把「漏 key / 漏语言」从运行期静默失败变成**编译期报错**。

---

## 一、现状量化

用自研词法扫描器实测（排除注释、原始字符串、`#[cfg(test)]` 之后的代码），不用 `grep`——注释里的中文不该算工作量。

| 指标 | 数值 |
| --- | --- |
| 非测试中文字面量 | **543** 处，分布在 45 个文件 |
| 其中 `i18n.rs` 自身译文 | 80 处（不算待迁） |
| 去重后唯一串 | **457** 个 |
| 含占位符的 | **89** 个 |
| 现有 `tr()` key | 33 个，调用点 23 处 |
| 实际覆盖率 | 约 **4%** |

按上下文分类：

| 类别 | 数量 | 典型形态 |
| --- | --- | --- |
| 界面组件参数 | 157 | `.child()` / `.label()` / `.tooltip()` |
| toast 提示 | 61 | `.toast(ToastLevel::...)` |
| 错误信息 | 32 | `Err(...)` / `.map_err()` |
| 日志 | 3 | `println!` / `log::` |
| 其他 | 290 | 需逐条判断，混着界面文案与不该迁的内容 |

占位符形态（89 个含参串）：

| 占位符 | 次数 | 处理方式 |
| --- | --- | --- |
| `{}` | 40 | 顺序替换 |
| `{error}` | 32 | 具名替换 |
| `{name}`/`{status}`/`{uri}`/`{text}` 等具名 | 约 15 | 具名替换 |
| `{:.1}`/`{:.2}`/`{:.4}` 格式化 | 7 | 需专用函数 |

另有 2 处是**提示词模板的字面花括号**（`app.rs:283` 的 `{{date}}`、`prompts.rs:80` 的 `{{selection}}`），不是 format 占位符，**不能当参数处理**。

---

## 三、四个坑（按危险度排序）

### 坑 1：`_ => ""` 静默失败（最危险）

`i18n.rs` 的 `tr` 末条是 `_ => ""`。key 写错、新增 key 忘了加分支，都会**静默返回空字符串**——界面直接空白，编译期无感，测试也测不到。这是第四批唯一真正需要先定方案的原因。

### 坑 2：数据层的「界面文案」拿不到语言

`Capability::label()`（`model_info.rs`）、`ReasoningLevel::label()`（`model.rs`）、`ChannelType::label()` 都是界面文案，却定义在非 UI 模块，签名里没有 `lang`。调用点 8 处，横跨 `ui/` 与 `provider_ops.rs`。

注意 `provider_ops.rs:100` 用 `ct.label()` 当**默认渠道名写进数据**——i18n 后要决定默认名是否跟随界面语言。

### 坑 3：89 个带参文案，`tr` 装不下

`tr` 返回 `&'static str`，带占位符的文案没法直接返回。而且**`format!(tr(lang, key), n)` 编译不过**——`format!` 要求格式串是编译期字面量。所以必须走「替换」或「专用函数」，不能靠 `format!` 套 `tr`。

### 坑 4：457 个 key × 4 语言手写 match

约 1800 行 match，手工维护必然出错（漏一个语言、漏一个 key）。项目现在只有 33 个 key 就够呛，457 个必须靠工具生成。

---

## 四、方案：宏驱动 key 枚举

**核心思路**：一处定义，自动生成枚举与查表函数，让所有遗漏都变成编译错误。

```rust
macro_rules! i18n {
    ($($variant:ident => { $zh_cn:expr, $en_us:expr, $ja_jp:expr, $zh_tw:expr }),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Key { $($variant),* }

        impl Key {
            /// 全部 key，供测试遍历
            pub const ALL: &'static [Key] = &[$(Key::$variant),*];
        }

        pub fn tr(lang: AppLanguage, key: Key) -> &'static str {
            match key {
                $(Key::$variant => match lang {
                    AppLanguage::ZhCn => $zh_cn,
                    AppLanguage::EnUs => $en_us,
                    AppLanguage::JaJp => $ja_jp,
                    AppLanguage::ZhTw => $zh_tw,
                }),*
            }
        }
    };
}

i18n! {
    AppTitle => { "Perch", "Perch", "Perch", "Perch" },
    Chat     => { "对话", "Chat", "チャット", "對話" },
    // ... 约 400 条
}
```

带来的保证（已用最小样例验证）：

| 出错场景 | 结果 |
| --- | --- |
| 某条译文漏写一个语言 | **编译报错**，指向缺失位置 |
| `match` 漏 key 分支 | **编译报错**（无 `_` 兜底，非穷尽匹配） |
| 调用时 key 名写错 | **编译报错**（枚举变体不存在） |
| `_ => ""` 静默返回空串 | **彻底消失** |
| `Key::ALL` 漏写 | 不可能，宏自动生成 |

`tr` 仍返回 `&'static str`，所以现有 23 处调用点的**类型用法不用改**，只把 `tr(lang, "chat")` 改成 `tr(lang, Key::Chat)`。

### 带参文案：两层处理

```rust
/// 「配置保存失败: xxx」这类，key 表存带 {error} 的模板。覆盖 32 处。
pub fn tr_error(lang: AppLanguage, key: Key, error: &str) -> String {
    tr(lang, key).replace("{error}", error)
}

/// 「已添加 3 个附件」这类，按顺序替换 {}。覆盖 40 处。
/// 语序差异靠译文里 {} 的位置解决，不靠代码。
pub fn tr_args(lang: AppLanguage, key: Key, args: &[&str]) -> String {
    let mut text = tr(lang, key).to_string();
    for arg in args {
        text = text.replacen("{}", arg, 1);
    }
    text
}

/// 英文有单复数、或语序差太大的（约 15 处），单独写函数。
pub fn added_attachments(lang: AppLanguage, n: usize) -> String {
    match lang {
        AppLanguage::ZhCn => format!("已添加 {n} 个附件"),
        AppLanguage::EnUs => match n {
            1 => "Added 1 attachment".to_string(),
            _ => format!("Added {n} attachments"),
        },
        AppLanguage::JaJp => format!("添付ファイルを {n} 件追加しました"),
        AppLanguage::ZhTw => format!("已新增 {n} 個附件"),
    }
}
```

### 数据层 label：加 `lang` 参数

```rust
impl Capability {
    pub fn label(self, lang: AppLanguage) -> &'static str {
        tr(lang, match self {
            Capability::Vision => Key::CapabilityVision,
            Capability::Files => Key::CapabilityFiles,
            // ...
        })
    }
}
```

调用点从 `capability.label()` 变成 `capability.label(lang)`——**漏传编译报错**，符合「最稳」原则。

### UI 层：不用改函数签名

所有渲染函数都是统一签名 `(state: &mut AppState, p: &Palette, cx: &mut Context<AppState>)`，函数内一行 `let lang = state.language();` 即可。深层子函数（如 `render_assistant_message`）若不接收 `state`，把 `lang` 作为参数传下去即可。

---

## 五、明确不 i18n 的白名单

写进 AGENTS.md，防止后来人（或我）机械替换：

| 内容 | 位置 | 理由 |
| --- | --- | --- |
| 厂商品牌名（31 处） | `brand.rs` 的 `title` 字段 | 品牌名，**待拍板**是否给英文用户显示英文名 |
| 模型匹配表 | `model_info.rs:187` 的 `["推理","reason","think"]` | 判断旧数据标签的关键词 |
| 发给 API 的取值 | `model.rs` 的 `openai_effort()`（`"none"`/`"high"`） | 接口协议值，翻译就废 |
| 预设提示词正文 | `prompts.rs`、`config.rs` 的 system prompt | 用户可编辑的内容，且要发给模型 |
| 提示词模板花括号 | `app.rs:283`、`prompts.rs:80` 的 `{{date}}` | 模板语法 |
| Markdown 附件模板 | `llm.rs` 的 `**附件文件: {}**` | 发给模型的上下文格式 |
| 迁移/日志输出 | `paths.rs` | 开发者可见即可 |
| 测试代码 | 全部 `#[cfg(test)]` 区 | 与界面无关 |

---

## 六、防回归

```rust
#[test]
fn all_keys_have_four_languages() {
    for &key in Key::ALL {
        for lang in [AppLanguage::ZhCn, AppLanguage::EnUs, AppLanguage::JaJp, AppLanguage::ZhTw] {
            assert!(!tr(lang, key).is_empty(), "{key:?} 在 {lang:?} 下为空");
        }
    }
}
```

宏已保证 key 齐全，这个测试兜住「译文写成空串」这类漏网（已跑通）。

可选加强：再写一个扫描型测试，用 `include_str!` 读 `ui/` 下源文件，断言没有裸中文字面量，拦住以后新写的硬编码。实现要排除注释，可复用本次的扫描逻辑。

---

## 七、迁移分批（每批一提交 + 完整检查清单）

节奏与第三批一致：`fmt --check` → `clippy --all-targets` → `cargo test` → 涉及启动路径的做冒烟。

| 批次 | 范围 | 规模 | 说明 |
| --- | --- | --- | --- |
| 4.1 | `i18n.rs` 地基：宏 + `Key` 枚举 + 现有 33 key 迁移 + 防回归测试 | 23 处调用点 | 先打地基，验证机制 |
| 4.2 | 数据层 3 个 label（`Capability`/`ReasoningLevel`/`ChannelType`）+ 8 处调用点 | 约 21 处 | 定下「数据层传 lang」范式 |
| 4.3 | `ui/` 目录 21 个文件 | 约 200 处 | 主体，可按文件再分几个提交 |
| 4.4 | `app.rs` + `*_ops.rs` 的 toast 与错误信息 | 约 100 处 | |
| 4.5 | 收尾：白名单写进 AGENTS.md、扫描测试、§13 债务条目结项 | — | |

---

## 八、待拍板

1. **key 机制**：宏生成枚举（推荐）还是保持 `&str`？
2. **品牌名**：`brand.rs` 31 个厂商名（通义千问 / 智谱 / 月之暗面…）——英文界面显示中文原名，还是给英文名（Qwen / Zhipu / Moonshot）？
3. **默认 system prompt**：`config.rs` 里那句「你是强大的个人 AI 工作台 Perch…」要不要按界面语言给不同默认值？（新装用户的初始体验）
4. **译文来源**：英文/日语/繁体由我生成，还是你提供术语表？日语与繁体是否需要人工校对？
5. **默认渠道名**：`provider_ops.rs:100` 用 `ChannelType::label()` 当新建渠道的默认名——跟随界面语言（推荐）还是固定中文？
