"""扫一行像素，把非白/非背景的连续段列出来。

用来精确定位一行里的几个图标——目测（Read 会把截图缩到 ~1092 宽）出来的坐标
是**显示坐标**，直接拿去点击会落空。`pixel.py bands` 按 y 分组，同一行里并排的
几个图标会被合成一条，所以这里补一个按 x 分段的。
"""
import sys

from PIL import Image

path = sys.argv[1]
y = int(sys.argv[2])
x0 = int(sys.argv[3])
x1 = int(sys.argv[4])
# 背景阈值：比这个亮的算背景（窗口底色是白的，卡片底色也很浅）
limit = int(sys.argv[5]) if len(sys.argv) > 5 else 245

im = Image.open(path).convert("RGB")
px = im.load()
runs = []
start = None
for x in range(x0, x1):
    r, g, b = px[x, y]
    ink = min(r, g, b) < limit
    if ink and start is None:
        start = x
    elif not ink and start is not None:
        runs.append((start, x - 1))
        start = None
if start is not None:
    runs.append((start, x1 - 1))

print(f"{path} y={y} x {x0}..{x1} 阈值<{limit}：{len(runs)} 段")
for a, b in runs:
    mid = (a + b) // 2
    print(f"  x {a}..{b}  中心 x={mid}  (宽度 {b - a + 1})  中心点颜色 {px[mid, y]}")
