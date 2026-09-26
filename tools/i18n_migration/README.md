# i18n 迁移工具（已完工，保留备查）

界面文案的国际化迁移（把硬编码中文换成 `i18n::tr` / `tr_args`）已经在 2026-09-26 完成：
`src/` 下（`i18n.rs` 与测试区除外）已没有白名单之外的硬编码中文。

**日常开发不需要碰这个目录。** 新增文案直接在 `src/i18n.rs` 的 `i18n!` 宏里加一行：

```rust
KeyName => { "简体中文", "English", "日本語", "繁體中文" },
```

漏写一种语言、key 名写错，都是编译错误；新写的界面文案没走 `tr`，会被
`cargo test` 里的 `i18n::tests::no_hardcoded_chinese_outside_whitelist` 拦下来。
规则细节见 `AGENTS.md` §9.10。

## 目录里的东西

| 文件 | 作用 | 现在还用得上吗 |
| --- | --- | --- |
| `rslex.py` | Rust 词法扫描器：跳过注释 / 原始字符串 / 字符字面量，挑出字符串字面量并把 `\n` 之类的转义**解码成实际字符**。其余脚本都基于它 | 是，其余脚本依赖它 |
| `scan_cjk.py` | 统计 `src/` 里还有多少待迁中文，白名单单独计数 | 是，用于**复核** |
| `dump_i18n.py` | 按文件列出中文字面量 + 所在行 + 左侧上下文，方便分配 key | 偶用 |
| `apply_i18n.py` | 批量替换 + 往 `i18n.rs` 追加新 key。**会改文件** | 备查，将来再要批量迁移时不必重写 |
| `i18n_entries.py` | 迁移时的文案总表（384 条 key 的四种语言译文）与白名单加载 | 备查 |

白名单 `i18n_skip.txt` 在**仓库根目录**（Rust 侧的测试也要读它，所以没跟着搬进来）。

## 用法

都在仓库根目录下跑：

```bash
python tools/i18n_migration/scan_cjk.py              # 复核：应该全是白名单跳过
python tools/i18n_migration/scan_cjk.py src --dump   # 连唯一串一起列出来
python tools/i18n_migration/dump_i18n.py src/ui      # 看某个目录里还剩哪些中文
python tools/i18n_migration/apply_i18n.py --report src/ui/foo.rs   # 只看报告，不改文件
```

## 两个坑

1. **扫描器只有一份。** `rslex.py` 是全目录唯一的词法扫描实现——`scan_cjk.py` 曾经自己抄了一份，
   抄漏了「把 `\n` 解码成实际字符」这一步，导致含转义的文案既匹配不上白名单、也匹配不上
   i18n 表，统计出来的数字虚高。要加扫描能力就改 `rslex.py`，别再抄。
2. **白名单只有一份。** `i18n_skip.txt` 被这里的 Python 工具和 `src/i18n.rs` 的防回归测试
   同时读取。转义约定：`\n` 表示换行、`\\` 表示反斜杠，其余原样。
