// 仅供临时 Windows runner 模拟 NSIS 的退出码、异步清理结果及同名常驻进程。
using System;
using Microsoft.Win32;

class LegacyUninstaller
{
    static int Main(string[] args)
    {
        if (Array.IndexOf(args, "/wait") >= 0)
        {
            System.Threading.Thread.Sleep(System.Threading.Timeout.Infinite);
            return 0;
        }
        if (Array.IndexOf(args, "/S") < 0) return 24;
        if (Array.IndexOf(args, "/fail") >= 0) return 23;
        if (Array.IndexOf(args, "/keep") >= 0) return 0;
        if (Array.IndexOf(args, "/kept") < 0) return 25;
        Registry.CurrentUser.DeleteSubKeyTree(
            @"Software\Microsoft\Windows\CurrentVersion\Uninstall\FloatPaste-Installer-Legacy-Test", false);
        return 0;
    }
}
