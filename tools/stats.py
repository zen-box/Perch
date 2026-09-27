"""数两个**会被文档引用、而且一直在变**的数字：i18n key 数、代码规模。

**为什么要有这个脚本**：这两个数字在 `AGENTS.md`、`ROADMAP.md`、`TODO.md`、
`I18N_PLAN.md` 里都被引用过，而它们每加一批功能就变一次。靠记忆写必错——已经错过两次：

- i18n key 数长期停在迁移结项时的 **427**，后来写 **496**，实际是 **544**；
- 代码规模写 **63 文件 / 26145 行**，实际是 **70 文件 / 29763 行**。

所以要用的时候**现数一遍**，别抄文档。

用法：
    python tools/stats.py

判据说明：
- key 数只认宏体里形如 `Name => { "zh-CN", "en-US", "ja-JP", "zh-TW" },` 的行，
  要求**四个**字符串字面量都在，免得把别的 `X => {` 结构算进来。
- 代码规模含 `#[cfg(test)]` 里的测试代码（它们也是这个仓库的代码），
  按 `src/**/*.rs` 的物理行数算。
"""
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parent.parent

KEY_PATTERN = re.compile(
    r'^\s*([A-Z][A-Za-z0-9_]*)\s*=>\s*\{\s*"'
    r'(?:[^"\\]|\\.)*"\s*,\s*"'
    r'(?:[^"\\]|\\.)*"\s*,\s*"'
    r'(?:[^"\\]|\\.)*"\s*,\s*"'
    r'(?:[^"\\]|\\.)*"\s*\}\s*,',
    re.M,
)


def i18n_keys():
    source = (ROOT / "src" / "i18n.rs").read_text(encoding="utf-8")
    return KEY_PATTERN.findall(source[source.index("i18n! {") :])


def code_size():
    files = sorted((ROOT / "src").rglob("*.rs"))
    total = sum(len(f.read_text(encoding="utf-8").splitlines()) for f in files)
    src = len([f for f in files if f.parent == ROOT / "src"])
    return len(files), total, src, len(files) - src


def main():
    keys = i18n_keys()
    print(f"i18n key 数 = {len(keys)}（`src/i18n.rs`）")
    files, total, src, ui = code_size()
    print(f"代码规模 = {files} 个 rs 文件 / {total} 行（src/ {src} 个 + src/ui/ {ui} 个）")


if __name__ == "__main__":
    main()
