"""扫描 Rust 源码里的硬编码中文字符串字面量（i18n 迁移工作量统计）。

为什么不用 grep：`grep '[一-龥]'` 会把注释里的中文和测试代码里的中文全算进来，
数字虚高好几倍。这里用逐字符状态机，只认真实字符串字面量：

- 跳过 `//` 行注释、`/* */` 块注释（Rust 块注释可嵌套）
- 正确识别 `r"..."` / `r#"..."#` 原始字符串
- 处理 `\\` 与 `\"` 转义
- 从 `#[cfg(test)]` 首次出现处切掉测试区（约定：测试都写在文件末尾）

产出三个数字：中文字面量总数、去重后唯一串数（≈ key 数量）、带占位符的串数。
"""

import re
import sys
from pathlib import Path

CJK = re.compile(r"[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]")


def strip_test_region(src: str) -> str:
    """按 `#[cfg(test)]` 首次出现处切掉后面的测试区。"""
    idx = src.find("#[cfg(test)]")
    if idx == -1:
        return src
    # 只保留到该行行首，避免把 `mod tests` 前的代码切掉
    line_start = src.rfind("\n", 0, idx) + 1
    return src[:line_start]


def scan_literals(src: str):
    """产出 (行号, 字符串字面量内容) 列表。"""
    out = []
    i = 0
    n = len(src)
    line = 1
    while i < n:
        c = src[i]
        if c == "\n":
            line += 1
            i += 1
            continue
        # 行注释
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            while i < n and src[i] != "\n":
                i += 1
            continue
        # 块注释（可嵌套）
        if c == "/" and i + 1 < n and src[i + 1] == "*":
            depth = 1
            i += 2
            while i < n and depth:
                if src[i] == "\n":
                    line += 1
                if src[i] == "/" and i + 1 < n and src[i + 1] == "*":
                    depth += 1
                    i += 2
                    continue
                if src[i] == "*" and i + 1 < n and src[i + 1] == "/":
                    depth -= 1
                    i += 2
                    continue
                i += 1
            continue
        # 字符字面量：'x' / '\n' / '\'' —— 跳过，免得里面的引号打乱状态
        if c == "'":
            # 形如 'a' 或 '\n'；生命周期 'a 没有闭合引号，靠下面判断
            if i + 2 < n and src[i + 1] == "\\" and src[i + 3 : i + 4] == "'":
                i += 4
                continue
            if i + 2 < n and src[i + 2] == "'":
                i += 3
                continue
            i += 1
            continue
        # 原始字符串 r"..." / r#"..."# / br#"..."#
        if c == "r" and i + 1 < n and src[i + 1] in '#"':
            j = i + 1
            hashes = 0
            while j < n and src[j] == "#":
                hashes += 1
                j += 1
            if j < n and src[j] == '"':
                j += 1
                close = '"' + "#" * hashes
                end = src.find(close, j)
                end = n if end == -1 else end
                line += src.count("\n", i, end)
                out.append((line, src[j:end]))
                i = end + len(close)
                continue
        # 普通字符串
        if c == '"':
            j = i + 1
            buf = []
            while j < n:
                if src[j] == "\\":
                    buf.append(src[j : j + 2])
                    j += 2
                    continue
                if src[j] == '"':
                    break
                if src[j] == "\n":
                    line += 1
                buf.append(src[j])
                j += 1
            out.append((line, "".join(buf)))
            i = j + 1
            continue
        i += 1
    return out


def main():
    root = Path(sys.argv[1] if len(sys.argv) > 1 else "src")
    files = sorted(root.rglob("*.rs"))
    total = 0
    uniq = {}
    placeholder = 0
    by_file = {}
    for f in files:
        src = strip_test_region(f.read_text(encoding="utf-8"))
        hits = [(ln, s) for ln, s in scan_literals(src) if CJK.search(s)]
        if not hits:
            continue
        by_file[str(f).replace("\\", "/")] = len(hits)
        total += len(hits)
        for _, s in hits:
            uniq[s] = uniq.get(s, 0) + 1
            if "{}" in s or "{error}" in s:
                placeholder += 1

    print(f"文件数：{len(files)}")
    print(f"非测试区中文字面量总数：{total}")
    print(f"去重后唯一串数（≈ key 数量）：{len(uniq)}")
    print(f"含占位符的串数：{placeholder}")
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
