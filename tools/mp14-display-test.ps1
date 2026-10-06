#Requires -Version 5.1
<#
    MP14Tools 显示器能力诊断（只读，不需要管理员）

    作用：在你自己的终端里确认三件事，用来判断改写后的 MP14Tools 能不能干活：
      1. 内置屏是否被 Windows 报成"内置"（INTERNAL，0x80000000）——插件靠这个区分内/外屏；
      2. 当前刷新率、这块屏在 3120x2080 下有哪些档位（应有 60 / 120）；
      3. HDR 是否被支持、当前是开还是关（DisplayConfigGetDeviceInfo）。

    用法：
      powershell -ExecutionPolicy Bypass -File .\mp14-display-test.ps1
          # 只读，看看现状

      powershell -ExecutionPolicy Bypass -File .\mp14-display-test.ps1 -Toggle
          # 额外做一次"切到另一个刷新率再切回来"和"HDR 关掉再打开"的往返测试，
          # 屏幕会闪几下，最后恢复原状。用来验证写接口在你机器上是否真的可用。

    注意：请在普通 PowerShell 窗口里跑（不要在受限沙箱/远程非交互会话里跑）。
    如果这里 HDR 那行显示 err=87，先确认不是沙箱导致，再把结果发我。
#>
[CmdletBinding()]
param([switch]$Toggle)

$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public class MP14Diag
{
    const int PATHSZ = 72;   // sizeof(DISPLAYCONFIG_PATH_INFO)：source 20 + target 48 + flags 4
    const int MODESZ = 64;   // sizeof(DISPLAYCONFIG_MODE_INFO)
    const uint QDC_ONLY_ACTIVE_PATHS = 2;
    const uint CDS_UPDATEREGISTRY = 0x00000001;

    [DllImport("user32.dll")] static extern int GetDisplayConfigBufferSizes(uint flags, out uint np, out uint nm);
    [DllImport("user32.dll")] static extern int QueryDisplayConfig(uint flags, ref uint np, IntPtr paths, ref uint nm, IntPtr modes, IntPtr topology);
    [DllImport("user32.dll")] static extern int DisplayConfigGetDeviceInfo(IntPtr info);
    [DllImport("user32.dll")] static extern int DisplayConfigSetDeviceInfo(IntPtr info);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool EnumDisplaySettingsW(string device, int modeNum, IntPtr devmode);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int ChangeDisplaySettingsExW(string device, IntPtr devmode, IntPtr hwnd, uint flags, IntPtr param);

    [StructLayout(LayoutKind.Sequential)]
    public struct SPS { public byte Ac; public byte Flag; public byte Pct; public byte Saver; public uint Life; public uint Full; }
    [DllImport("kernel32.dll")] static extern int GetSystemPowerStatus(out SPS s);

    static IntPtr paths = IntPtr.Zero;
    static IntPtr modes = IntPtr.Zero;
    static uint np = 0, nm = 0;

    static string DoQuery()
    {
        uint wantPaths, wantModes;
        int e = GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, out wantPaths, out wantModes);
        if (e != 0) return "GetDisplayConfigBufferSizes 失败，错误码 " + e;
        if (paths != IntPtr.Zero) Marshal.FreeHGlobal(paths);
        if (modes != IntPtr.Zero) Marshal.FreeHGlobal(modes);
        paths = Marshal.AllocHGlobal((int)wantPaths * PATHSZ + 128);
        modes = Marshal.AllocHGlobal((int)wantModes * MODESZ + 128);
        for (int i = 0; i < (int)wantPaths * PATHSZ + 128; i++) Marshal.WriteByte(paths, i, 0);
        for (int i = 0; i < (int)wantModes * MODESZ + 128; i++) Marshal.WriteByte(modes, i, 0);
        np = wantPaths; nm = wantModes;
        e = QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, ref np, paths, ref nm, modes, IntPtr.Zero);
        if (e != 0) return "QueryDisplayConfig 失败，错误码 " + e;
        return null;
    }

    static uint P(int pathIndex, int offset) { return (uint)Marshal.ReadInt32(paths, pathIndex * PATHSZ + offset); }
    static uint M(uint modeIndex, int offset) { return (uint)Marshal.ReadInt32(modes, (int)modeIndex * MODESZ + offset); }

    // 返回第一条"内置屏"路径的下标；没有则返回 -1
    public static int InternalPathIndex()
    {
        for (uint i = 0; i < np; i++) if (P((int)i, 36) == 0x80000000u) return (int)i;
        return -1;
    }

    public static string Report()
    {
        StringBuilder sb = new StringBuilder();
        SPS s;
        GetSystemPowerStatus(out s);
        string ac = s.Ac == 1 ? "已插电" : (s.Ac == 0 ? "电池供电" : "未知");
        sb.AppendFormat("电源状态：{0}    节电模式：{1}    电量：{2}%\r\n", ac, s.Saver == 1 ? "开" : "关", s.Pct);

        string err = DoQuery();
        if (err != null) return sb.ToString() + err + "\r\n";

        sb.AppendFormat("活动显示路径：{0} 条\r\n", np);
        for (uint i = 0; i < np; i++)
        {
            int p = (int)i;
            uint tech = P(p, 36);
            uint rateNum = P(p, 48), rateDen = P(p, 52);
            double hz = rateDen == 0 ? 0 : (double)rateNum / rateDen;
            string techName = tech == 0x80000000u ? "内置屏 (INTERNAL)" : ("0x" + tech.ToString("X8"));
            sb.AppendFormat("  [{0}] 输出技术 = {1}，刷新率 = {2:0.###} Hz\r\n", i, techName, hz);

            uint smi = P(p, 12) & 0xFFFF;
            uint tmi = P(p, 32);
            if (smi != 0xFFFF) sb.AppendFormat("      分辨率 = {0}x{1}\r\n", M(smi, 16), M(smi, 20));
            if (tmi != 0xFFFF) sb.AppendFormat("      目标模式 vSync = {0}/{1}\r\n", M(tmi, 32), M(tmi, 36));

            // 显示器名字（GET_TARGET_NAME = 2；负载是 420 字节，不是文档里写的 164）
            IntPtr tn = Marshal.AllocHGlobal(460);
            for (int k = 0; k < 460; k++) Marshal.WriteByte(tn, k, 0);
            Marshal.WriteInt32(tn, 0, 2);
            Marshal.WriteInt32(tn, 4, 420);
            Marshal.WriteInt32(tn, 8, (int)P(p, 20));
            Marshal.WriteInt32(tn, 12, (int)P(p, 24));
            Marshal.WriteInt32(tn, 16, (int)P(p, 28));
            int r = DisplayConfigGetDeviceInfo(tn);
            if (r == 0)
            {
                string friendly = Marshal.PtrToStringUni(tn + 36, 64).TrimEnd('\0');
                string devpath = Marshal.PtrToStringUni(tn + 164, 128).TrimEnd('\0');
                sb.AppendFormat("      显示器 = {0}\r\n", friendly.Length > 0 ? friendly : "(驱动未给友好名)");
                if (devpath.Length > 0) sb.AppendFormat("      设备路径 = {0}\r\n", devpath);
            }
            else sb.AppendFormat("      显示器名读取失败 err={0}\r\n", r);
            Marshal.FreeHGlobal(tn);

            // HDR（GET_ADVANCED_COLOR_INFO = 9）
            // Win11 24H2 起要求 32 字节负载；只给 24 会被拒（err=87）。两个都试。
            string hdrText = null;
            foreach (int size in new int[] { 32, 24 })
            {
                IntPtr aci = Marshal.AllocHGlobal(64);
                for (int k = 0; k < 64; k++) Marshal.WriteByte(aci, k, 0);
                Marshal.WriteInt32(aci, 0, 9);
                Marshal.WriteInt32(aci, 4, size);
                Marshal.WriteInt32(aci, 8, (int)P(p, 20));
                Marshal.WriteInt32(aci, 12, (int)P(p, 24));
                Marshal.WriteInt32(aci, 16, (int)P(p, 28));
                r = DisplayConfigGetDeviceInfo(aci);
                if (r == 0)
                {
                    uint v = (uint)Marshal.ReadInt32(aci, 20);
                    hdrText = string.Format("      HDR：支持={0} 当前={1}  (负载 {2} 字节, raw=0x{3:X8})\r\n",
                        (v & 1) != 0 ? "是" : "否", (v & 2) != 0 ? "开" : "关", size, v);
                }
                Marshal.FreeHGlobal(aci);
                if (hdrText != null) break;
            }
            sb.Append(hdrText != null ? hdrText : "      HDR 状态读取失败（32 / 24 都被拒）\r\n");
        }

        sb.AppendFormat("  3120x2080 可用刷新率：{0}\r\n", Rates(3120, 2080));
        return sb.ToString();
    }

    public static string Rates(uint w, uint h)
    {
        IntPtr dm = Marshal.AllocHGlobal(256);
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < 256; i++) Marshal.WriteByte(dm, i, 0);
        Marshal.WriteInt16(dm, 68, (short)220);   // dmSize
        int last = -1;
        for (int i = 0; EnumDisplaySettingsW(null, i, dm); i++)
        {
            uint mw = (uint)Marshal.ReadInt32(dm, 172);
            uint mh = (uint)Marshal.ReadInt32(dm, 176);
            uint fq = (uint)Marshal.ReadInt32(dm, 184);
            if (mw == w && mh == h && (int)fq != last)
            {
                if (sb.Length > 0) sb.Append(", ");
                sb.Append(fq);
                last = (int)fq;
            }
            if (i > 2000) break;
        }
        Marshal.FreeHGlobal(dm);
        return sb.Length == 0 ? "（枚举不到）" : sb.ToString();
    }

    public static uint CurrentRate()
    {
        if (DoQuery() != null) return 0;
        int idx = InternalPathIndex();
        if (idx < 0) idx = 0;
        uint num = P(idx, 48), den = P(idx, 52);
        return den == 0 ? 0 : (uint)Math.Round((double)num / den);
    }

    // 0 = 失败，1 = 成功
    public static int SetRate(int w, int h, int hz)
    {
        IntPtr dm = Marshal.AllocHGlobal(256);
        for (int i = 0; i < 256; i++) Marshal.WriteByte(dm, i, 0);
        Marshal.WriteInt16(dm, 68, (short)220);              // dmSize
        Marshal.WriteInt32(dm, 72, 0x00580000);              // DM_PELSWIDTH|DM_PELSHEIGHT|DM_DISPLAYFREQUENCY
        Marshal.WriteInt32(dm, 172, w);
        Marshal.WriteInt32(dm, 176, h);
        Marshal.WriteInt32(dm, 184, hz);
        int r = ChangeDisplaySettingsExW(null, dm, IntPtr.Zero, CDS_UPDATEREGISTRY, IntPtr.Zero);
        Marshal.FreeHGlobal(dm);
        return r == 0 ? 1 : 0;
    }

    public static int HdrState()   // -1 = 不支持/读不到, 0 = 关, 1 = 开
    {
        if (DoQuery() != null) return -1;
        int idx = InternalPathIndex();
        if (idx < 0) return -1;
        foreach (int size in new int[] { 32, 24 })
        {
            IntPtr aci = Marshal.AllocHGlobal(64);
            for (int k = 0; k < 64; k++) Marshal.WriteByte(aci, k, 0);
            Marshal.WriteInt32(aci, 0, 9);
            Marshal.WriteInt32(aci, 4, size);
            Marshal.WriteInt32(aci, 8, (int)P(idx, 20));
            Marshal.WriteInt32(aci, 12, (int)P(idx, 24));
            Marshal.WriteInt32(aci, 16, (int)P(idx, 28));
            int r = DisplayConfigGetDeviceInfo(aci);
            if (r == 0)
            {
                uint v = (uint)Marshal.ReadInt32(aci, 20);
                Marshal.FreeHGlobal(aci);
                return (v & 2) != 0 ? 1 : 0;
            }
            Marshal.FreeHGlobal(aci);
        }
        return -1;
    }

    public static int SetHdr(bool on)   // 0 = 失败，1 = 成功
    {
        if (DoQuery() != null) return 0;
        int idx = InternalPathIndex();
        if (idx < 0) return 0;
        foreach (int size in new int[] { 32, 24 })
        {
            IntPtr aci = Marshal.AllocHGlobal(64);
            for (int k = 0; k < 64; k++) Marshal.WriteByte(aci, k, 0);
            Marshal.WriteInt32(aci, 0, 10);                    // SET_ADVANCED_COLOR_STATE
            Marshal.WriteInt32(aci, 4, size);
            Marshal.WriteInt32(aci, 8, (int)P(idx, 20));
            Marshal.WriteInt32(aci, 12, (int)P(idx, 24));
            Marshal.WriteInt32(aci, 16, (int)P(idx, 28));
            Marshal.WriteInt32(aci, 20, on ? 1 : 0);
            int r = DisplayConfigSetDeviceInfo(aci);
            Marshal.FreeHGlobal(aci);
            if (r == 0) return 1;
            if (r != 87) return 0;                             // 87 = 负载大小不符，才值得换 24 再试
        }
        return 0;
    }
}
'@

Write-Output '================ 只读诊断 ================'
Write-Output ([MP14Diag]::Report())

if (-not $Toggle) {
    Write-Output '（想验证写接口是否可用，加 -Toggle 再跑一次；会闪屏并自动恢复）'
    return
}

Write-Output '================ 往返测试 ================'
$rate0 = [MP14Diag]::CurrentRate()
Write-Output "当前刷新率：$rate0 Hz"
$target = if ($rate0 -ge 120) { 60 } else { 120 }
$ok = [MP14Diag]::SetRate(3120, 2080, $target)
Start-Sleep -Seconds 3
$rate1 = [MP14Diag]::CurrentRate()
Write-Output "切到 ${target}Hz：调用返回=$ok，实测现在 = $rate1 Hz"
$ok = [MP14Diag]::SetRate(3120, 2080, [int]$rate0)
Start-Sleep -Seconds 3
Write-Output "切回 ${rate0}Hz：调用返回=$ok，实测现在 = $([MP14Diag]::CurrentRate()) Hz"

$hdr0 = [MP14Diag]::HdrState()
Write-Output "HDR 当前状态：$hdr0  (-1=读不到, 0=关, 1=开)"
if ($hdr0 -ge 0) {
    $ok = [MP14Diag]::SetHdr($hdr0 -eq 0)
    Start-Sleep -Seconds 3
    Write-Output "取反 HDR：调用返回=$ok，实测现在 = $([MP14Diag]::HdrState())"
    $ok = [MP14Diag]::SetHdr($hdr0 -eq 1)
    Start-Sleep -Seconds 2
    Write-Output "恢复 HDR：调用返回=$ok，实测现在 = $([MP14Diag]::HdrState())"
} else {
    Write-Output "HDR 状态读不到，跳过 HDR 往返测试（把上面的 err 码发我）"
}
