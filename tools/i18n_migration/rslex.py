"""Rust 源码的词法扫描：找出所有字符串字面量，带精确的源码区间。

为什么不用正则：`grep`/正则会把注释里的中文、测试代码、`r#"..."#` 原始字符串
一起算进来，替换时还会把注释里的中文也改掉。这里用逐字符状态机：

- 跳过 `//` 行注释与 `/* */` 块注释（Rust 块注释可嵌套）
- 正确识别 `r"..."` / `r#"..."#` / `br#"..."#` 原始字符串
- 处理 `\\`、`\"` 转义
- 区分普通字符串与「格式串」（紧跟在 `format!(`, `write!(`, `println!(` 等之后的字面量）

产出每个字面量的：(行号, 解码后内容, 源码起始偏移, 源码结束偏移, 是否格式串)
"""

import re

CJK = re.compile(r"[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]")

# 这些宏的第一个参数必须是编译期字面量，不能换成 `tr(...)`
FORMAT_MACROS = (
    "format!",
    "format_args!",
    "print!",
    "println!",
    "eprint!",
    "eprintln!",
    "write!",
    "writeln!",
    "panic!",
    "unreachable!",
    "todo!",
    "unimplemented!",
    "assert!",
    "assert_eq!",
    "assert_ne!",
    "debug_assert!",
    "debug_assert_eq!",
    "debug_assert_ne!",
)


class Literal:
    __slots__ = ("line", "text", "start", "end", "is_format", "is_raw")

    def __init__(self, line, text, start, end, is_format, is_raw):
        self.line = line
        self.text = text
        self.start = start
        self.end = end
        self.is_format = is_format
        self.is_raw = is_raw

    def __repr__(self):
        return f"Literal(L{self.line}, {self.text!r})"


def strip_test_region(src: str) -> str:
    """按 `#[cfg(test)]` 首次出现处切掉后面的测试区（约定测试写在文件末尾）。"""
    idx = src.find("#[cfg(test)]")
    if idx == -1:
        return src
    return src[: src.rfind("\n", 0, idx) + 1]


def _is_format_macro(src: str, quote_start: int) -> bool:
    """判断这个字面量是不是某个格式化宏的第一个参数。"""
    i = quote_start - 1
    while i >= 0 and src[i] in " \t\r\n":
        i -= 1
    if i < 0 or src[i] != "(":
        return False
    j = i - 1
    while j >= 0 and src[j] in " \t\r\n":
        j -= 1
    end = j + 1
    while j >= 0 and (src[j].isalnum() or src[j] in "_!"):
        j -= 1
    name = src[j + 1 : end]
    return name in FORMAT_MACROS


def scan(src: str, limit: int | None = None):
    """扫描源码，返回 Literal 列表。limit 之后的内容不再扫描（用于切测试区）。"""
    out = []
    i = 0
    n = len(src) if limit is None else min(limit, len(src))
    line = 1
    while i < n:
        c = src[i]
        if c == "\n":
            line += 1
            i += 1
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            while i < n and src[i] != "\n":
                i += 1
            continue
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
        # 字符字面量，跳过，免得里面的引号打乱状态
        if c == "'":
            if i + 3 < n and src[i + 1] == "\\" and src[i + 3] == "'":
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
                if end == -1:
                    end = n
                start_line = line
                line += src.count("\n", i, end)
                out.append(
                    Literal(start_line, src[j:end], i, end + len(close), False, True)
                )
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
            raw = "".join(buf)
            # 把转义还原成实际字符，便于跟 i18n 表里的中文比对
            decoded = (
                raw.replace("\\n", "\n")
                .replace("\\t", "\t")
                .replace("\\r", "\r")
                .replace('\\"', '"')
                .replace("\\\\", "\\")
            )
            out.append(
                Literal(
                    line,
                    decoded,
                    i,
                    j + 1,
                    _is_format_macro(src, i),
                    False,
                )
            )
            i = j + 1
            continue
        i += 1
    return out
