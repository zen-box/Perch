"""扫描 Rust 源码里的硬编码中文字符串字面量（i18n 迁移工作量统计）。

词法扫描**复用 `rslex.py`**：`apply_i18n.py` 也用它，两边必须是同一份实现。
早先这里另抄了一份扫描器，抄漏了「把 `\\n` 之类的转义还原成实际字符」这一步，
于是含转义的文案既匹配不上白名单、也匹配不上 i18n 表——数字虚高，还会误导判断。

产出：非测试区中文字面量总数、去重后唯一串数（≈ key 数量）、含占位符的串数。
白名单（`i18n_skip.txt`，见 `i18n_entries.SKIP`）单独计数，不计入待迁工作量。
`src/i18n.rs` 本身就是译文表，排除。

用法（从仓库根目录跑）：
    python tools/i18n_migration/scan_cjk.py            # 统计 src/
    python tools/i18n_migration/scan_cjk.py src --dump # 连唯一串一起列出来

迁移已完成，这个脚本现在主要用于**复核**：确认没有新增的漏翻中文。
日常的防回归由 `cargo test` 里的 `i18n::tests::no_hardcoded_chinese_outside_whitelist` 负责，
它读的是同一份 `i18n_skip.txt`。
"""

import sys
from pathlib import Path

# 仓库根：本文件在 tools/i18n_migration/ 下
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).parent))
import i18n_entries  # noqa: E402
import rslex  # noqa: E402

I18N_RS = (ROOT / "src" / "i18n.rs").resolve()


def main():
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "src"
    files = [f for f in sorted(root.rglob("*.rs")) if f.resolve() != I18N_RS]

    total = 0
    uniq = {}
    placeholder = 0
    skipped = 0
    by_file = {}

    for f in files:
        text = f.read_text(encoding="utf-8")
        # 测试区里的中文是测试数据，不算漏翻
        lits = rslex.scan(text, len(rslex.strip_test_region(text)))
        kept = []
        for lit in lits:
            if not rslex.CJK.search(lit.text):
                continue
            if lit.text in i18n_entries.SKIP:
                skipped += 1
            else:
                kept.append(lit)
        if not kept:
            continue
        by_file[str(f).replace("\\", "/")] = len(kept)
        total += len(kept)
        for lit in kept:
            uniq[lit.text] = uniq.get(lit.text, 0) + 1
            if "{}" in lit.text or "{error}" in lit.text:
                placeholder += 1

    print(f"文件数：{len(files)}")
    print(f"非测试区中文字面量总数：{total}")
    print(f"去重后唯一串数（≈ key 数量）：{len(uniq)}")
    print(f"含占位符的串数：{placeholder}")
    print(f"白名单跳过（品牌名 / 语言名 / 数据层 / 模型提示词）：{skipped}")
    print()
    print("按文件（多→少）：")
    for path, cnt in sorted(by_file.items(), key=lambda kv: -kv[1]):
        print(f"  {cnt:4d}  {path}")
    print()
    if len(sys.argv) > 2 and sys.argv[2] == "--dump":
        print("唯一串：")
        for s, cnt in sorted(uniq.items(), key=lambda kv: -kv[1]):
            print(f"  {cnt:3d}x  {s!r}")


if __name__ == "__main__":
    main()
