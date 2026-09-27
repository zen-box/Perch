"""用**真实光标**操作 Perch 窗口，坐标和 `win.ps1` 的截图坐标同一套。

为什么要单独有这么个脚本：实测里出现过「点设置齿轮生效、点授权卡片的『允许执行一次』
不生效」，而 `win.ps1` 的 click 走 `PostMessage`，没法区分是「坐标算错了」还是
「合成消息被吞了」。真实光标是绝对的，一试就知道。

代价是会挪动用户的鼠标、也会抢焦点，所以只用来做定性诊断，
确认之后还是优先用 `win.ps1`。

用法（在 `tools/p3-test/` 下跑）：
    python mouse.py screen            # 打印主屏物理分辨率（判断窗口有没有超出屏幕）
    python mouse.py hover 955 635     # 只挪光标，看有没有 hover 反馈
    python mouse.py click 955 635     # 挪过去并点一下
    python mouse.py where             # 打印窗口的可见边框矩形
"""
import ctypes
import sys
import time
from ctypes import wintypes

user32 = ctypes.windll.user32
dwmapi = ctypes.windll.dwmapi
# 不设的话拿到的是逻辑像素，和截图坐标（物理像素）对不上
user32.SetProcessDPIAware()


class RECT(ctypes.Structure):
    _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long),
                ("right", ctypes.c_long), ("bottom", ctypes.c_long)]


def find_hwnd():
    found = []
    cb = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    def visit(hwnd, _):
        if user32.IsWindowVisible(hwnd):
            buf = ctypes.create_unicode_buffer(256)
            user32.GetClassNameW(hwnd, buf, 256)
            if buf.value == "Zed::Window":
                found.append(hwnd)
        return True

    user32.EnumWindows(cb(visit), 0)
    if not found:
        sys.exit("没找到 Perch 窗口（class=Zed::Window），它是不是没在跑？")
    return found[0]


def frame_of(hwnd):
    """DWM 的可见边框矩形——`win.ps1` 截图就是从它的左上角开始裁的。"""
    r = RECT()
    dwmapi.DwmGetWindowAttribute(hwnd, 9, ctypes.byref(r), ctypes.sizeof(r))
    return r


def to_screen(hwnd, x, y):
    f = frame_of(hwnd)
    return f.left + x, f.top + y


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    action = sys.argv[1]
    if action == "screen":
        # 窗口右边缘超出屏幕时真实光标到不了那些位置，只能靠 `PostMessage`，
        # 这时候坐标必须算准——先问一句屏有多大，省得白试几轮。
        print(f"screen  {user32.GetSystemMetrics(0)}x{user32.GetSystemMetrics(1)}")
        print(f"virtual {user32.GetSystemMetrics(78)}x{user32.GetSystemMetrics(79)} at "
              f"{user32.GetSystemMetrics(76)},{user32.GetSystemMetrics(77)}")
        return

    hwnd = find_hwnd()
    f = frame_of(hwnd)
    print(f"hwnd={hwnd} frame=({f.left},{f.top}) {f.right - f.left}x{f.bottom - f.top}")

    if action == "where":
        return
    if len(sys.argv) < 4:
        sys.exit("要给 x y")
    x, y = int(sys.argv[2]), int(sys.argv[3])
    sx, sy = to_screen(hwnd, x, y)
    print(f"截图坐标 ({x},{y}) -> 屏幕坐标 ({sx},{sy})")
    user32.SetCursorPos(sx, sy)
    time.sleep(0.3)
    if action == "click":
        user32.mouse_event(0x0002, 0, 0, 0, 0)  # LEFTDOWN
        time.sleep(0.05)
        user32.mouse_event(0x0004, 0, 0, 0, 0)  # LEFTUP
        time.sleep(0.4)
        print("clicked")
    else:
        print("hovered")


if __name__ == "__main__":
    main()
