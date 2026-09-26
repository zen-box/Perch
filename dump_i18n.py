"""按文件 dump 非测试区的中文字面量，附带所在行与左侧上下文，供分配 key 用。

用法：
  python dump_i18n.py src/ui/sidebar.rs
  python dump_i18n.py src/ui            # 整个目录
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from scan_cjk import CJK, scan_literals, strip_test_region  # noqa: E402


def dump(path: Path):
    src = path.read_text(encoding="utf-8")
    lines = strip_test_region(src).split("\n")
    hits = [(ln, s) for ln, s in scan_literals(strip_test_region(src)) if CJK.search(s)]
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
