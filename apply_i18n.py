"""把硬编码中文替换成 `tr(lang, Key::Xxx)`，并把新 key 追加进 `src/i18n.rs`。

流程：
1. 解析 `src/i18n.rs` 里已有的 key（含 zh-CN 值），建立「中文 -> Key 名」反查表，
   这样已存在的文案会自动复用，不会造重复 key。
2. 读 `i18n_entries.ENTRIES`（本批次新加的 key 与四种语言译文）与 `SKIP`（白名单）。
3. 词法扫描目标文件，把中文字面量替换成查表调用：
   - 普通字面量 -> `tr(lang, Key::Xxx)`
   - 含 `{}` 的字面量 -> 仍按普通字面量替换，调用点由人改成 `tr_args(...)`
   - `format!` 之类的格式串 -> 跳过并报告（必须手工改成 `tr_args`）
   - 在 SKIP 里 -> 跳过
   - 没分配 key -> 报告并失败，避免漏翻
4. 给需要的文件补 `use crate::i18n::{...}`。

用法：
  python apply_i18n.py --report src/ui/foo.rs ...   # 只看报告，不改文件
  python apply_i18n.py src/ui/foo.rs ...            # 实际替换
"""

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import i18n_entries  # noqa: E402
import rslex  # noqa: E402

I18N = Path("src/i18n.rs")

KEY_LINE = re.compile(
    r'^\s*([A-Z][A-Za-z0-9]*)\s*=>\s*\{\s*"((?:[^"\\]|\\.)*)"\s*,\s*"((?:[^"\\]|\\.)*)"\s*,'
    r'\s*"((?:[^"\\]|\\.)*)"\s*,\s*"((?:[^"\\]|\\.)*)"\s*\}\s*,\s*$'
)


def unescape(s: str) -> str:
    return (
        s.replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\r", "\r")
        .replace('\\"', '"')
        .replace("\\\\", "\\")
    )


def parse_existing(src: str):
    """返回 (keys: dict[name]=zh, zh_to_key: dict[zh]=name, block_start, block_end)。"""
    keys = {}
    zh_to_key = {}
    start = src.index("i18n! {")
    # 从 i18n! 块开始，找第一个独占一行的 `}`
    lines = src[start:].split("\n")
    offset = start
    end = None
    for idx, ln in enumerate(lines):
        if idx > 0 and ln.rstrip() == "}":
            end = offset + sum(len(x) + 1 for x in lines[:idx])
            break
    if end is None:
        raise SystemExit("找不到 i18n! 块的结尾")
    for ln in src[start:end].split("\n"):
        m = KEY_LINE.match(ln)
        if not m:
            continue
        name = m.group(1)
        zh = unescape(m.group(2))
        keys[name] = zh
        zh_to_key.setdefault(zh, name)
    return keys, zh_to_key, start, end


def esc(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")


def main():
    report_only = "--report" in sys.argv
    targets = [a for a in sys.argv[1:] if not a.startswith("--")]
    if not targets:
        raise SystemExit("用法：python apply_i18n.py [--report] <文件>...")

    src = I18N.read_text(encoding="utf-8")
    keys, zh_to_key, _start, end = parse_existing(src)

    # 1) 校验并登记新 key
    new_lines = []
    for name, zh, en, ja, tw in i18n_entries.ENTRIES:
        if name in keys:
            raise SystemExit(f"key 名重复：{name}")
        if zh in zh_to_key and zh_to_key[zh] != name:
            raise SystemExit(f"中文「{zh}」已由 {zh_to_key[zh]} 覆盖，不要再造 {name}")
        keys[name] = zh
        zh_to_key.setdefault(zh, name)
        new_lines.append(
            f"    {name} => {{ \"{esc(zh)}\", \"{esc(en)}\", \"{esc(ja)}\", \"{esc(tw)}\" }},"
        )

    # 2) 逐文件替换
    need_key = {}
    need_args = {}
    unmapped = {}
    format_lits = {}
    skipped = {}
    changed_files = {}

    for t in targets:
        path = Path(t)
        text = path.read_text(encoding="utf-8")
        limit = rslex.strip_test_region(text)
        lits = rslex.scan(text, len(limit))
        repls = []
        for lit in lits:
            if not rslex.CJK.search(lit.text):
                continue
            if lit.text in i18n_entries.SKIP:
                skipped.setdefault(path.as_posix(), []).append(lit)
                continue
            key = zh_to_key.get(lit.text)
            if key is None:
                unmapped.setdefault(path.as_posix(), []).append(lit)
                continue
            if lit.is_format:
                format_lits.setdefault(path.as_posix(), []).append(lit)
                continue
            expr = f"tr(lang, Key::{key})"
            if "{}" in lit.text or "{error}" in lit.text:
                need_args.setdefault(path.as_posix(), []).append(lit)
            need_key.setdefault(path.as_posix(), set()).add(key)
            repls.append((lit.start, lit.end, expr))
        if repls:
            for start, stop, expr in sorted(repls, reverse=True):
                text = text[:start] + expr + text[stop:]
            changed_files[path.as_posix()] = text

    # 3) 报告
    print(f"新 key：{len(new_lines)} 个；已存在复用：{len(need_key)} 个文件受影响")
    if unmapped:
        print("\n!! 没有分配 key 的中文字面量（必须先决定翻还是进 SKIP）：")
        for f, items in unmapped.items():
            print(f"  {f}")
            for lit in items:
                print(f"    L{lit.line}: {lit.text!r}")
    if format_lits:
        print("\n!! 格式串（必须手工改成 tr_args / tr_error）：")
        for f, items in format_lits.items():
            print(f"  {f}")
            for lit in items:
                print(f"    L{lit.line}: {lit.text!r}")
    if need_args:
        print("\n!! 带占位符、替换后需要手工把调用点改成 tr_args：")
        for f, items in need_args.items():
            print(f"  {f}")
            for lit in items:
                print(f"    L{lit.line}: {lit.text!r}")
    if skipped:
        print("\n（白名单跳过）")
        for f, items in skipped.items():
            print(f"  {f}: {len(items)} 处")

    if report_only:
        return
    if unmapped:
        raise SystemExit("有未分配 key 的文案，已中止（文件未改动）")

    # 4) 写回 i18n.rs
    if new_lines:
        title = getattr(i18n_entries, "BATCH_TITLE", "本批迁移新增")
        header = f"\n    // ---- {title} ----\n"
        src = src[:end] + header + "\n".join(new_lines) + "\n" + src[end:]
        I18N.write_text(src, encoding="utf-8", newline="\n")

    # 5) 写回目标文件 + 补 import
    for f, text in changed_files.items():
        text = ensure_import(text)
        Path(f).write_text(text, encoding="utf-8", newline="\n")
    print(f"\n已改写 {len(changed_files)} 个文件")


def ensure_import(text: str) -> str:
    m = re.search(r"^use crate::i18n::\{([^}]*)\};$", text, re.M)
    if m:
        items = {x.strip() for x in m.group(1).split(",") if x.strip()}
        items |= {"Key", "tr"}
        ordered = sorted(items)
        return text[: m.start()] + "use crate::i18n::{" + ", ".join(ordered) + "};" + text[m.end() :]
    uses = list(re.finditer(r"^use .*;$", text, re.M))
    if not uses:
        return text
    last = uses[-1]
    return text[: last.end()] + "\nuse crate::i18n::{Key, tr};" + text[last.end() :]


if __name__ == "__main__":
    main()
