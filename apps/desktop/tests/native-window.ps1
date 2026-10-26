# Inspect only the diagnostic application's window; restore it after a minimize test.
param(
  [Parameter(Mandatory = $true)][int]$ProcessId,
  [long]$WindowHandle = 0,
  [ValidateSet('inspect', 'restore')][string]$Action = 'inspect'
)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class TerminderWindowProbe {
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct Point { public int X, Y; }
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out Rect rect);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window, ref Point point);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr window);
  [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr window);
  [DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr window, int command);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
  public static long HitTest(IntPtr window, int x, int y) {
    return SendMessage(window, 0x84, IntPtr.Zero, new IntPtr((y << 16) | (x & 0xffff))).ToInt64();
  }
}
'@
# MainWindowHandle can select an auxiliary window after the application is minimized.
# Pin the original HWND across checks and reject handles owned by any other process.
$window = if ($WindowHandle -eq 0) { (Get-Process -Id $ProcessId).MainWindowHandle } else { [IntPtr]$WindowHandle }
if ($window -eq [IntPtr]::Zero) { throw 'Diagnostic window is unavailable' }
[uint32]$owner = 0
[void][TerminderWindowProbe]::GetWindowThreadProcessId($window, [ref]$owner)
if ($owner -ne $ProcessId) { throw 'Window does not belong to the diagnostic application' }
if ($Action -eq 'restore') { [void][TerminderWindowProbe]::ShowWindowAsync($window, 9) }
$outer = New-Object TerminderWindowProbe+Rect
$client = New-Object TerminderWindowProbe+Rect
$origin = New-Object TerminderWindowProbe+Point
if (-not [TerminderWindowProbe]::GetWindowRect($window, [ref]$outer)) { throw 'GetWindowRect failed' }
if (-not [TerminderWindowProbe]::GetClientRect($window, [ref]$client)) { throw 'GetClientRect failed' }
if (-not [TerminderWindowProbe]::ClientToScreen($window, [ref]$origin)) { throw 'ClientToScreen failed' }
@{
  handle = $window.ToInt64().ToString()
  minimized = [TerminderWindowProbe]::IsIconic($window)
  maximized = [TerminderWindowProbe]::IsZoomed($window)
  outer = $outer
  client = $client
  origin = $origin
  leftHit = [TerminderWindowProbe]::HitTest($window, $outer.Left + 1, [int](($outer.Top + $outer.Bottom) / 2))
  rightHit = [TerminderWindowProbe]::HitTest($window, $outer.Right - 1, [int](($outer.Top + $outer.Bottom) / 2))
  topHit = [TerminderWindowProbe]::HitTest($window, [int](($outer.Left + $outer.Right) / 2), $outer.Top + 1)
  bottomHit = [TerminderWindowProbe]::HitTest($window, [int](($outer.Left + $outer.Right) / 2), $outer.Bottom - 1)
} | ConvertTo-Json -Depth 3 -Compress
