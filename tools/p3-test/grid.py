"""在截图上画坐标网格，用来把「目测的位置」翻译成「文件里的像素坐标」。

为什么需要：Read 工具把 1756x1163 的截图缩到 ~1092 宽再给模型看，
于是目测出来的坐标是**显示坐标**，直接拿去点会全部落空
（实测：目测按钮在 (955,635)，文件里那一点是纯白，真正的紫色块在 x 488..740）。
网格画上去之后直接读数字，不用再猜缩放比例。

用法：python grid.py shots/f12.png shots/f12-grid.png 200
"""
import sys

from PIL import Image, ImageDraw

path, out = sys.argv[1], sys.argv[2]
step = int(sys.argv[3]) if len(sys.argv) > 3 else 200
im = Image.open(path).convert("RGB")
d = ImageDraw.Draw(im)
w, h = im.size
for x in range(0, w, step):
    d.line([(x, 0), (x, h)], fill=(255, 0, 0), width=1)
    d.text((x + 3, 3), str(x), fill=(255, 0, 0))
for y in range(0, h, step):
    d.line([(0, y), (w, y)], fill=(0, 120, 255), width=1)
    d.text((3, y + 3), str(y), fill=(0, 120, 255))
im.save(out)
print(f"{w}x{h} step={step} -> {out}")
