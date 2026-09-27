"""临时工具：不依赖 Pillow，纯标准库解 PNG 找出截图里紫色按钮的中心坐标。

实测时 win.ps1 用 PostMessage 发点击，坐标必须是截图里的物理像素；
靠肉眼估位置误差太大（窗口 1774x1172 时差 5% 就是 80 像素），所以扫像素定位。
"""
import struct
import sys
import zlib


def load_png(path):
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit("不是 PNG")
    pos = 8
    idat = b""
    w = h = depth = color = None
    while pos < len(data):
        length = struct.unpack(">I", data[pos : pos + 4])[0]
        kind = data[pos + 4 : pos + 8]
        chunk = data[pos + 8 : pos + 8 + length]
        if kind == b"IHDR":
            w, h, depth, color = struct.unpack(">IIBB", chunk[:10])
        elif kind == b"IDAT":
            idat += chunk
        pos += 12 + length
    if depth != 8:
        raise SystemExit(f"只支持 8 位色深，实际 {depth}")
    channels = {0: 1, 2: 3, 4: 2, 6: 4}.get(color)
    if channels is None:
        raise SystemExit(f"不支持的色彩类型 {color}")
    raw = zlib.decompress(idat)
    stride = w * channels
    out = bytearray(h * stride)
    prev = bytearray(stride)
    p = 0
    for y in range(h):
        filt = raw[p]
        p += 1
        line = bytearray(raw[p : p + stride])
        p += stride
        if filt == 1:
            for i in range(channels, stride):
                line[i] = (line[i] + line[i - channels]) & 0xFF
        elif filt == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 0xFF
        elif filt == 3:
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                line[i] = (line[i] + ((left + prev[i]) >> 1)) & 0xFF
        elif filt == 4:
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                up = prev[i]
                upleft = prev[i - channels] if i >= channels else 0
                pa, pb, pc = abs(up - upleft), abs(left - upleft), abs(left + up - 2 * upleft)
                pred = left if (pa <= pb and pa <= pc) else (up if pb <= pc else upleft)
                line[i] = (line[i] + pred) & 0xFF
        out[y * stride : (y + 1) * stride] = line
        prev = line
    return w, h, channels, out


def main():
    path = sys.argv[1]
    w, h, channels, pix = load_png(path)
    print(f"图片 {w}x{h} {channels} 通道")

    def rgb(x, y):
        i = (y * w + x) * channels
        return pix[i], pix[i + 1], pix[i + 2]

    min_x, max_x, min_y, max_y, hits = w, 0, h, 0, 0
    bands = {}
    for y in range(h // 2, h, 2):
        for x in range(0, w, 2):
            r, g, b = rgb(x, y)
            if b > 150 and b - r > 40 and b - g > 40:
                hits += 1
                min_x, max_x = min(min_x, x), max(max_x, x)
                min_y, max_y = min(min_y, y), max(max_y, y)
                band = bands.setdefault(y // 20 * 20, [0, x, x])
                band[0] += 1
                band[1] = min(band[1], x)
                band[2] = max(band[2], x)
    if not hits:
        print("下半部分没找到紫色像素")
        return
    print(f"紫色块 {hits} 个采样点，范围 x {min_x}~{max_x}  y {min_y}~{max_y}")
    print("按 y 分桶（找出实心按钮所在的那一条）：")
    for y in sorted(bands):
        count, lo, hi = bands[y]
        print(f"  y {y}~{y + 19}: {count:5d} 点  x {lo}~{hi}  宽 {hi - lo}")

    # 标题栏图标：找顶部那条里连成一片的暗色像素，按 x 聚类
    print("标题栏暗色像素的连续区间（精确到像素）：")
    cols = [x for x in range(0, w) if any(sum(rgb(x, y)) < 480 for y in range(0, 60))]
    groups = []
    for x in cols:
        if groups and x - groups[-1][1] <= 6:
            groups[-1][1] = x
        else:
            groups.append([x, x])
    for lo, hi in groups:
        print(f"  x {lo}~{hi}  中心 {(lo + hi) // 2}  宽 {hi - lo}")

    print("下部暗色像素的连续区间（找档位文字所在列）：")
    cols = [x for x in range(0, w) if any(sum(rgb(x, y)) < 400 for y in range(int(h * 0.85), h))]
    groups = []
    for x in cols:
        if groups and x - groups[-1][1] <= 10:
            groups[-1][1] = x
        else:
            groups.append([x, x])
    for lo, hi in groups:
        print(f"  x {lo}~{hi}  中心 {(lo + hi) // 2}  宽 {hi - lo}")
    print("下部暗色像素的 y 分布：")
    for y in range(int(h * 0.85), h, 4):
        n = sum(1 for x in range(0, w, 2) if sum(rgb(x, y)) < 400)
        if n > 3:
            print(f"  y {y}: {n} 点")


if __name__ == "__main__":
    main()
