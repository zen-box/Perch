# 不抢焦点、不动真实鼠标键盘地操作测试窗口：
#   -Action size  -W -H        调整窗口大小（物理像素），不激活窗口
#   -Action shot  -Out a.png   用 PrintWindow 截取窗口本身（被别的窗口挡住也能截）
#   -Action click -X -Y        向窗口发送鼠标点击消息（客户区物理像素）
#   -Action move  -X -Y        向窗口发送鼠标移动消息（悬停效果）
#   -Action info               输出窗口位置、大小和 DPI
param([string]$Action = "info", [int]$ProcId = 0, [int]$W = 0, [int]$H = 0, [int]$X = 0, [int]$Y = 0, [string]$Out = "shot.png")
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class U {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int attr, out RECT r, int size);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  public struct POINT { public int X, Y; }
  public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
[U]::SetProcessDPIAware() | Out-Null
$p = if ($ProcId -gt 0) { Get-Process -Id $ProcId } else { Get-Process -Name perch-p3 | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1 }
if (-not $p -or $p.MainWindowHandle -eq 0) { Write-Output "NO_WINDOW"; exit 1 }
$hwnd = $p.MainWindowHandle
$r = New-Object U+RECT
[U]::GetWindowRect($hwnd, [ref]$r) | Out-Null
$ww = $r.Right - $r.Left; $wh = $r.Bottom - $r.Top
function LParam([int]$x, [int]$y) { [IntPtr](($y -shl 16) -bor ($x -band 0xFFFF)) }
# 传进来的坐标是截图里的像素（截图从 DWM 边框开始），换算成客户区坐标
$frame = New-Object U+RECT
[U]::DwmGetWindowAttribute($hwnd, 9, [ref]$frame, [System.Runtime.InteropServices.Marshal]::SizeOf($frame)) | Out-Null
$origin = New-Object U+POINT
[U]::ClientToScreen($hwnd, [ref]$origin) | Out-Null
$X = $X + $frame.Left - $origin.X
$Y = $Y + $frame.Top - $origin.Y
switch ($Action) {
  "info" {
    $c = New-Object U+RECT; [U]::GetClientRect($hwnd, [ref]$c) | Out-Null
    Write-Output "window $($r.Left),$($r.Top) ${ww}x${wh} client $($c.Right)x$($c.Bottom) dpi $([U]::GetDpiForWindow($hwnd))"
  }
  "size" {
    # SWP_NOZORDER | SWP_NOACTIVATE
    [U]::SetWindowPos($hwnd, [IntPtr]::Zero, $r.Left, $r.Top, $W, $H, 0x0014) | Out-Null
    Start-Sleep -Milliseconds 800
    Write-Output "resized to ${W}x${H}"
  }
  "shot" {
    $bmp = New-Object System.Drawing.Bitmap $ww, $wh
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    # PW_RENDERFULLCONTENT：DirectX 绘制的窗口也能截到
    $ok = [U]::PrintWindow($hwnd, $hdc, 2)
    $g.ReleaseHdc($hdc); $g.Dispose()
    # 去掉窗口四周看不见的缩放边框，只留 DWM 实际画出来的部分
    $f = New-Object U+RECT
    [U]::DwmGetWindowAttribute($hwnd, 9, [ref]$f, [System.Runtime.InteropServices.Marshal]::SizeOf($f)) | Out-Null
    $crop = New-Object System.Drawing.Rectangle ($f.Left - $r.Left), ($f.Top - $r.Top), ($f.Right - $f.Left), ($f.Bottom - $f.Top)
    $bmp.Clone($crop, $bmp.PixelFormat).Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
    Write-Output "shot ok=$ok $($crop.Width)x$($crop.Height) -> $Out"
  }
  "wheel" {
    # WM_MOUSEWHEEL：wParam 高位是滚动量（负数向下），lParam 用屏幕坐标
    $pt = New-Object U+POINT
    [U]::ClientToScreen($hwnd, [ref]$pt) | Out-Null
    [U]::PostMessage($hwnd, 0x0200, [IntPtr]::Zero, (LParam $X $Y)) | Out-Null
    $wp = [IntPtr]([int]((($W -band 0xFFFF) -shl 16)))
    [U]::PostMessage($hwnd, 0x020A, $wp, (LParam ($pt.X + $X) ($pt.Y + $Y))) | Out-Null
    Start-Sleep -Milliseconds 600
    Write-Output "wheel $W at $X,$Y"
  }
  "type" {
    foreach ($ch in $Out.ToCharArray()) {
      [U]::PostMessage($hwnd, 0x0102, [IntPtr][int]$ch, [IntPtr]1) | Out-Null
      Start-Sleep -Milliseconds 15
    }
    Start-Sleep -Milliseconds 300
    Write-Output "typed $($Out.Length) chars"
  }
  "enter" {
    [U]::PostMessage($hwnd, 0x0100, [IntPtr]0x0D, [IntPtr](1 -bor (0x1C -shl 16))) | Out-Null
    Start-Sleep -Milliseconds 40
    [U]::PostMessage($hwnd, 0x0101, [IntPtr]0x0D, [IntPtr]([int](1 -bor (0x1C -shl 16) -bor (1 -shl 30) -bor (1 -shl 31)))) | Out-Null
    Start-Sleep -Milliseconds 300
    Write-Output "enter"
  }
  "move" {
    [U]::PostMessage($hwnd, 0x0200, [IntPtr]::Zero, (LParam $X $Y)) | Out-Null
    Start-Sleep -Milliseconds 300
    Write-Output "moved $X,$Y"
  }
  "click" {
    [U]::PostMessage($hwnd, 0x0200, [IntPtr]::Zero, (LParam $X $Y)) | Out-Null
    Start-Sleep -Milliseconds 60
    [U]::PostMessage($hwnd, 0x0201, [IntPtr]1, (LParam $X $Y)) | Out-Null
    Start-Sleep -Milliseconds 60
    [U]::PostMessage($hwnd, 0x0202, [IntPtr]::Zero, (LParam $X $Y)) | Out-Null
    Start-Sleep -Milliseconds 500
    Write-Output "clicked $X,$Y"
  }
}
