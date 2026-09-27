"""截图像素工具：找按钮/开关的真实坐标，或者问"这一点的颜色是什么"。

**为什么非有它不可**：Read 工具会把 1756x1163 的截图缩到 ~1092 宽再给模型看，
于是目测出来的坐标是**显示坐标**，拿去 `win.ps1 -Action click` 会全部落空——
实测踩过两次：

- 目测「允许执行一次」在 (955,635)，文件里那一点是纯白，真正的紫色块在 x 488~740；
- 目测设置齿轮在 (1570,26)，结果打在最小化上，窗口缩成 219x39。

所以**点击之前一律先用这个脚本问一遍真实坐标**，别凭印象。

用法：
    python pixel.py find  shots/f12.png purple [y0] [y1]    # 找某颜色的连通块（包围盒 + 中心）
    python pixel.py probe shots/f12.png 955,635 1470,788    # 逐点报颜色，并扫出紫色系的 y 带
"""
import sys
from collections import deque

from PIL import Image

# 每种颜色给一组宽松阈值（截图有抗锯齿和半透明，取严了会漏边）
RULES = {
    # 下界放到 60：GPUI 的 primary 色一档是 `#4F46E5`(79,70,229)，
    # 取 80 会差 1 个值把整个开关漏掉（踩过）。
    "purple": lambda r, g, b: 60 < r < 150 and 60 < g < 130 and b > 180 and b - r > 60,
    "green": lambda r, g, b: r < 120 and g > 120 and b < 140,
    "red": lambda r, g, b: r > 160 and g < 110 and b < 110,
    # 发送按钮空闲时是紫的、执行中是蓝的（`#3B82F6` 一档）。g 的下界用来和紫色分开。
    "blue": lambda r, g, b: b > 180 and b - r > 60 and 100 < g < 200,
    # 深色（灰/黑）。标题栏那排图标是灰的，没有这档就定位不到。
    "dark": lambda r, g, b: r < 160 and g < 160 and b < 160,
}

HELP = __doc__


def find(path, name, y0, y1):
    rule = RULES[name]
    im = Image.open(path).convert("RGB")
    w, h = im.size
    px = im.load()
    hits = {(x, y) for y in range(max(0, y0), min(h, y1)) for x in range(w) if rule(*px[x, y])}
    if not hits:
        print(f"{path}: 没找到 {name}")
        return

    # 按连通块分（8 邻域），把不同按钮分开
    blocks = []
    seen = set()
    for p in hits:
        if p in seen:
            continue
        q = deque([p])
        seen.add(p)
        xs, ys = [], []
        while q:
            x, y = q.popleft()
            xs.append(x)
            ys.append(y)
            for dx in (-1, 0, 1):
                for dy in (-1, 0, 1):
                    n = (x + dx, y + dy)
                    if n in hits and n not in seen:
                        seen.add(n)
                        q.append(n)
        blocks.append((min(xs), min(ys), max(xs), max(ys), len(xs)))

    blocks.sort(key=lambda b: -b[4])
    print(f"{path} ({w}x{h}) 找到 {len(blocks)} 块 {name}:")
    for x0, yy0, x1, yy1, n in blocks[:8]:
        print(f"  x {x0}..{x1}  y {yy0}..{yy1}  中心 ({(x0 + x1) // 2},{(yy0 + yy1) // 2})  {n}px")


def probe(path, points):
    im = Image.open(path).convert("RGB")
    w, h = im.size
    print(f"{path}: {w}x{h}")
    px = im.load()
    for arg in points:
        x, y = (int(v) for v in arg.split(","))
        if 0 <= x < w and 0 <= y < h:
            print(f"  ({x},{y}) = {px[x, y]}")
        else:
            print(f"  ({x},{y}) 越界")

    # 宽松找紫色系（含半透明、抗锯齿），按行统计再合并成 y 带
    rows = {}
    for y in range(h):
        xs = [x for x in range(w) if px[x, y][2] > 140 and px[x, y][2] - px[x, y][0] > 40]
        if xs:
            rows[y] = (min(xs), max(xs), len(xs))
    if not rows:
        print("  没有紫色系像素")
        return
    ys = sorted(rows)
    bands = []
    start = prev = ys[0]
    for y in ys[1:]:
        if y - prev > 3:
            bands.append((start, prev))
            start = y
        prev = y
    bands.append((start, prev))
    print(f"  紫色系分布在 {len(bands)} 个 y 带:")
    for a, b in bands:
        x0 = min(rows[y][0] for y in range(a, b + 1) if y in rows)
        x1 = max(rows[y][1] for y in range(a, b + 1) if y in rows)
        n = sum(rows[y][2] for y in range(a, b + 1) if y in rows)
        print(f"    y {a}..{b}  x {x0}..{x1}  {n}px")


def main():
    if len(sys.argv) < 3:
        sys.exit(HELP)
    action, path = sys.argv[1], sys.argv[2]
    if action == "find":
        name = sys.argv[3] if len(sys.argv) > 3 else "purple"
        y0 = int(sys.argv[4]) if len(sys.argv) > 5 else 0
        y1 = int(sys.argv[5]) if len(sys.argv) > 5 else 10**9
        find(path, name, y0, y1)
    elif action == "probe":
        probe(path, sys.argv[3:])
    else:
        sys.exit(HELP)


if __name__ == "__main__":
    main()
