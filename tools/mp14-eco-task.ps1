#Requires -Version 5.1
<#
    MP14Tools 省电档 · 任务脚本

    MP14Tools-EcoOn / MP14Tools-EcoOff 两个计划任务调用的就是这个脚本：

      1) 切换电源方案（省电副本 / 原方案）；
      2) 只有在"电池供电"时，才切换电源模式的电池档：
         On  → 最佳能效；Off → 还原安装时记录的旧值。
         之所以要限定在电池上：PowerSetActiveOverlayScheme 只会改
         "当前电源来源"的那一档，接电时调用改的是插电档。

    参数与路径都从 %LOCALAPPDATA%\MP14Tools\eco-setup.json 读取，
    这个文件由 mp14-eco-setup.ps1 在应用设置时写好。
#>
param(
    [Parameter(Mandatory)]
    [ValidateSet('On', 'Off')]
    [string]$Mode
)

$ErrorActionPreference = 'Continue'

$stateDir  = Join-Path $env:LOCALAPPDATA 'MP14Tools'
$stateFile = Join-Path $stateDir 'eco-setup.json'
$logFile   = Join-Path $stateDir 'mp14tools.log'

$BestEfficiency = '961cc777-2547-4f9d-8174-7d86181b8a7a'
$Balanced       = '00000000-0000-0000-0000-000000000000'

function Write-Log([string]$Text) {
    try { Add-Content -Path $logFile -Value ("task: " + $Text) -ErrorAction SilentlyContinue } catch { }
}

if (-not (Test-Path $stateFile)) {
    Write-Log "eco-setup.json 不存在，跳过（$Mode）"
    exit 1
}

$state = Get-Content $stateFile -Raw | ConvertFrom-Json

# ---------------------------------------------------------------- 1) 电源方案
$scheme = if ($Mode -eq 'On') { $state.ecoGuid } else { $state.originalGuid }
if ($scheme) {
    & powercfg.exe /setactive $scheme | Out-Null
    Write-Log "scheme -> $scheme ($Mode)"
}

# ---------------------------------------------------------------- 2) 电源模式（仅电池）
if ($state.powerModeEco) {
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class Mp14Powr {
    [DllImport("powrprof.dll")] public static extern uint PowerSetActiveOverlayScheme(Guid overlaySchemeGuid);
    [DllImport("kernel32.dll")] public static extern bool GetSystemPowerStatus(out MP14_POWER_STATUS status);
}
[StructLayout(LayoutKind.Sequential)]
public struct MP14_POWER_STATUS {
    public byte ACLineStatus;
    public byte BatteryFlag;
    public byte BatteryLifePercent;
    public byte SystemStatusFlag;
    public int BatteryLifeTime;
    public int BatteryFullLifeTime;
}
"@ -ErrorAction SilentlyContinue

    $status = New-Object MP14_POWER_STATUS
    $ok = [Mp14Powr]::GetSystemPowerStatus([ref]$status)

    if ($ok -and $status.ACLineStatus -eq 0) {
        $target = $Balanced
        if ($Mode -eq 'On') {
            $target = $BestEfficiency
        } elseif ($state.overlayDc) {
            $target = [string]$state.overlayDc
        }
        $result = [Mp14Powr]::PowerSetActiveOverlayScheme([Guid]$target)
        Write-Log "power mode -> $target (result $result)"
    } else {
        Write-Log "接电中，电源模式不动（$Mode）"
    }
}

exit 0
