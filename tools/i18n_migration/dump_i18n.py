"""按文件 dump 非测试区的中文字面量，附带所在行与左侧上下文，供分配 key 用。

词法扫描复用 `rslex.py`——和 `scan_cjk.py` / `apply_i18n.py` 同一份实现。
（这里早先 import 的是 `scan_cjk` 自己那份扫描器的 `CJK` / `scan_literals`，
`scan_cjk.py` 改成复用 `rslex` 之后就没这俩名字了，于是本脚本一直是坏的。）

用法（从仓库根目录跑）：
  python tools/i18n_migration/dump_i18n.py src/ui/sidebar.rs
  python tools/i18n_migration/dump_i18n.py src/ui            # 整个目录
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import rslex  # noqa: E402


def dump(path: Path):
    src = path.read_text(encoding="utf-8")
    # 测试区里的中文是测试数据，不算漏翻
    body = rslex.strip_test_region(src)
    lines = body.split("\n")
    hits = [
        (lit.line, lit.text)
        for lit in rslex.scan(src, len(body))
        if rslex.CJK.search(lit.text)
    ]
    if not hits:
        return 0
    print(f"===== {path.as_posix()}  ({len(hits)} 处) =====")
    for ln, s in hits:
        ctx = lines[ln - 1] if 0 < ln <= len(lines) else ""
        idx = ctx.find(s)
        left = ctx[:idx] if idx >= 0 else ctx
        left = left.strip()
        if len(left) > 70:
            left = "…" + left[-70:]
        print(f"  L{ln:<4} {left}")
        print(f"        {s!r}")
    print()
    return len(hits)


def main():
    target = Path(sys.argv[1])
    total = 0
    if target.is_dir():
        for f in sorted(target.rglob("*.rs")):
            total += dump(f)
    else:
        total = dump(target)
    print(f"合计 {total} 处")


if __name__ == "__main__":
    main()
