"""列出某个进程的所有顶层窗口（句柄 / 可见性 / 矩形 / 类名 / 标题）。

`win.ps1` 是按 `MainWindowHandle` 找窗口的，而那个属性在多窗口进程上会指到
"第一个可见的无主顶层窗口"——实测踩过：主窗口正常时它却指到了一个 219x39 的
小窗口（隐藏的浮层），于是所有点击和截图都作用在错的东西上，看起来像"功能坏了"。
所以出这种症状时先用这个脚本看一眼，别靠猜。

用法：python windows.py perch-p3
"""
import ctypes
import sys
from ctypes import wintypes

user32 = ctypes.windll.user32

EnumWindows = user32.EnumWindows
EnumWindowsProc = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

GetWindowThreadProcessId = user32.GetWindowThreadProcessId
IsWindowVisible = user32.IsWindowVisible
GetWindowTextW = user32.GetWindowTextW
GetClassNameW = user32.GetClassNameW
GetWindowRect = user32.GetWindowRect


class RECT(ctypes.Structure):
    _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long),
                ("right", ctypes.c_long), ("bottom", ctypes.c_long)]


def main():
    name = sys.argv[1] if len(sys.argv) > 1 else "perch-p3"
    import subprocess
    out = subprocess.run(["tasklist", "/FI", f"IMAGENAME eq {name}.exe", "/FO", "CSV", "/NH"],
                         capture_output=True, text=True).stdout
    pids = set()
    for line in out.splitlines():
        parts = [p.strip('"') for p in line.split('","')]
        if len(parts) >= 2 and parts[1].isdigit():
            pids.add(int(parts[1]))
    if not pids:
        sys.exit(f"没找到进程 {name}.exe")
    print(f"{name}.exe -> pids {sorted(pids)}")

    rows = []

    def visit(hwnd, _):
        pid = wintypes.DWORD()
        GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if pid.value in pids:
            rect = RECT()
            GetWindowRect(hwnd, ctypes.byref(rect))
            title = ctypes.create_unicode_buffer(256)
            GetWindowTextW(hwnd, title, 256)
            cls = ctypes.create_unicode_buffer(256)
            GetClassNameW(hwnd, cls, 256)
            rows.append((hwnd, bool(IsWindowVisible(hwnd)), rect, cls.value, title.value))
        return True

    EnumWindows(EnumWindowsProc(visit), 0)
    for hwnd, visible, rect, cls, title in rows:
        size = f"{rect.right - rect.left}x{rect.bottom - rect.top}"
        print(f"hwnd={hwnd} visible={visible} at=({rect.left},{rect.top}) {size} "
              f"class={cls!r} title={title!r}")


if __name__ == "__main__":
    main()
