# Lists the top-level windows of a process and their child windows (handle, class, visibility, rect, title).
# Parameters: -ProcId process id (required). Use the handle of the visible top-level window with win.ps1.
# Needs Windows PowerShell 5.1. Read-only.
param([int]$ProcId)
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class L {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h, EnumProc p, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    static string Desc(IntPtr c) {
        var cn = new StringBuilder(256); GetClassName(c, cn, 256);
        var t = new StringBuilder(256); GetWindowText(c, t, 256);
        RECT r; GetWindowRect(c, out r);
        return String.Format("{0} {1} vis={2} [{3},{4} {5}x{6}] '{7}'", c, cn, IsWindowVisible(c), r.L, r.T, r.R - r.L, r.B - r.T, t);
    }
    public static List<string> All(uint pid) {
        SetProcessDPIAware();
        var o = new List<string>();
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == pid) {
                o.Add(Desc(h));
                EnumChildWindows(h, (c, l2) => { o.Add("    " + Desc(c)); return true; }, IntPtr.Zero);
            }
            return true;
        }, IntPtr.Zero);
        return o;
    }
}
'@
[L]::All([uint32]$ProcId)
