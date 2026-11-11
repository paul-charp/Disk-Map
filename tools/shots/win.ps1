# Drives and captures one window by handle: resize, list child windows, post a click, double click, right click,
# mouse move or key press, and save a PNG of that window.
# Parameters: -Handle HWND (required); -Shot path.png; -Action click|double|right|move|key; -X/-Y client offset from the
# window's top-left corner (device pixels); -Key virtual-key code; -Width/-Height outer size to set first; -Children.
# Needs Windows PowerShell 5.1 (System.Drawing). Only touches the given window: input is posted to it, nothing is
# sent to the desktop or other windows.
param(
    [long]$Handle,
    [string]$Shot,
    [string]$Action,
    [int]$X = 0,
    [int]$Y = 0,
    [int]$Key = 0,
    [int]$Width = 0,
    [int]$Height = 0,
    [switch]$Children
)
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Text;
using System.Drawing;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class W {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h, EnumProc p, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ScreenToClient(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] public static extern IntPtr ChildWindowFromPointEx(IntPtr h, POINT p, uint f);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint f);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    public static List<string> Kids(IntPtr h) {
        var o = new List<string>();
        EnumChildWindows(h, (c, l) => {
            var cn = new StringBuilder(256); GetClassName(c, cn, 256);
            var t = new StringBuilder(256); GetWindowText(c, t, 256);
            RECT r; GetWindowRect(c, out r);
            o.Add(String.Format("{0} {1} vis={2} [{3},{4} {5}x{6}] '{7}'", c, cn, IsWindowVisible(c), r.L, r.T, r.R - r.L, r.B - r.T, t));
            return true;
        }, IntPtr.Zero);
        return o;
    }
    public static void Capture(IntPtr h, string path) {
        RECT r; GetWindowRect(h, out r);
        using (var bmp = new Bitmap(r.R - r.L, r.B - r.T)) {
            using (var g = Graphics.FromImage(bmp)) {
                IntPtr dc = g.GetHdc();
                PrintWindow(h, dc, 2);
                g.ReleaseHdc(dc);
            }
            bmp.Save(path, System.Drawing.Imaging.ImageFormat.Png);
        }
    }
    // x,y are relative to the top-level window's top-left corner.
    public static string Mouse(IntPtr top, int x, int y, string kind) {
        RECT r; GetWindowRect(top, out r);
        var sp = new POINT { X = r.L + x, Y = r.T + y };
        IntPtr target = top;
        while (true) {
            var cp = sp; ScreenToClient(target, ref cp);
            IntPtr c = ChildWindowFromPointEx(target, cp, 1);
            if (c == IntPtr.Zero || c == target) break;
            target = c;
        }
        var p = sp; ScreenToClient(target, ref p);
        IntPtr lp = (IntPtr)((p.Y << 16) | (p.X & 0xFFFF));
        PostMessage(target, 0x0200, IntPtr.Zero, lp);
        if (kind == "move") return target.ToString();
        uint down = kind == "right" ? 0x0204u : 0x0201u, up = kind == "right" ? 0x0205u : 0x0202u;
        IntPtr wd = (IntPtr)(kind == "right" ? 2 : 1);
        PostMessage(target, down, wd, lp);
        PostMessage(target, up, IntPtr.Zero, lp);
        if (kind == "double") {
            PostMessage(target, 0x0203, wd, lp);
            PostMessage(target, up, IntPtr.Zero, lp);
        }
        return target.ToString();
    }
    public static void KeyPress(IntPtr h, int vk) {
        PostMessage(h, 0x0100, (IntPtr)vk, (IntPtr)1);
        PostMessage(h, 0x0101, (IntPtr)vk, (IntPtr)(-1073741823));
    }
}
'@
[void][W]::SetProcessDPIAware()

$hwnd = [IntPtr]$Handle
if ($Width -gt 0) { [void][W]::SetWindowPos($hwnd, [IntPtr]::Zero, 0, 0, $Width, $Height, 0x0016) ; Start-Sleep -Milliseconds 800 }
if ($Children) { [W]::Kids($hwnd) }
if ($Action -in 'click', 'double', 'right', 'move') { "target " + [W]::Mouse($hwnd, $X, $Y, $Action); Start-Sleep -Milliseconds 1200 }
if ($Action -eq 'key') { [W]::KeyPress($hwnd, $Key); Start-Sleep -Milliseconds 1200 }
if ($Shot) { [W]::Capture($hwnd, $Shot); "saved $Shot" }


