"""裁一块截图放大保存，用来在实测里看清某个局部（标题栏图标、徽标、一行文字）。

`win.ps1` 截出来的是整窗物理像素（窗口 1774x1172 时约 1756x1163），整张图缩到
聊天里之后细节全糊了，没法确认「那个徽标到底是灰的还是绿的」。所以定位到区域之后
裁出来放大再看。

用法：
    python zoom.py in.png out.png x y w h [scale]
"""
import sys

from PIL import Image


def main():
    path, out, x, y, w, h = sys.argv[1:7]
    scale = float(sys.argv[7]) if len(sys.argv) > 7 else 2.0
    image = Image.open(path)
    box = (int(x), int(y), int(x) + int(w), int(y) + int(h))
    crop = image.crop(box)
    if scale != 1:
        crop = crop.resize((int(crop.width * scale), int(crop.height * scale)), Image.LANCZOS)
    crop.save(out)
    print(f"{path} {image.size} -> {out} {crop.size} (crop {box})")


if __name__ == "__main__":
    main()
