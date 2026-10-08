#Requires -Version 5.1
<#
    MP14Tools 省电方案 · 一次性管理员配置脚本

    为什么需要它：改写后的 MP14Tools 是免管理员的，它只能切刷新率和 HDR。而
    "处理器上限 / 睿频 / 亮度 / 关屏时间 / 节电阈值"这些属于 Windows 电源方案，
    实测普通权限下 powercfg /setdcvalueindex、/setactive、/change 全部返回
    "你没有执行此操作所需的权限"。所以这些设置由本脚本以管理员身份跑一次，
    之后由插件通过"计划任务"切换，不会再弹 UAC。

    两种模式：
      -Scope SaverOnly（默认）
          复制当前电源方案为「MP14 省电」，在副本里写入激进省电设置，并创建两个
          "最高权限"计划任务 MP14Tools-EcoOn / MP14Tools-EcoOff。
          插件的 profiles.eco.command 调用 EcoOn，high/medium 的 command 调用 EcoOff，
          于是"只有打开节电模式时"才切到省电方案。
      -Scope Always
          直接把激进省电设置写进当前方案（这些值只作用于"电池供电"），
          不建任务、不做切换：拔电即生效、插电即恢复。最简单，但无法只对节电模式生效。

    回滚：.\mp14-eco-setup.ps1 -Undo
          按状态文件里记录的原值逐项写回（不依赖导入备份），并删除本脚本创建的
          计划任务与副本方案。

    不需要重启；每一步都会读回校验，失败项会明确打印出来。
#>
[CmdletBinding()]
param(
    [ValidateSet('SaverOnly', 'Always')]
    [string]$Scope = 'SaverOnly',

    # 电池供电时的最大处理器状态（%）。50 = 明显省电，编译/多任务会变慢。
    [ValidateRange(5, 100)]
    [int]$CpuMaxPercent = 50,

    # 电池供电时的显示器亮度档位（电源方案里的"显示器亮度"，0-100）。
    # 注意：如果你平时手动调亮度，这个值不一定生效，以实际观感为准。
    [ValidateRange(0, 100)]
    [int]$BrightnessPercent = 40,

    # 节电模式自动开启的电量阈值（%）。40 = 到 40% 自动进入节电模式 → 触发省电档。
    [ValidateRange(0, 100)]
    [int]$SaverThresholdPercent = 40,

    # 电池供电时的关屏时间（秒）。
    [ValidateRange(30, 3600)]
    [int]$ScreenOffSeconds = 60,

    # 下面三项默认开（1/0）。用 int 而不是 switch：界面以 -File 调用，-File 下无法可靠地
    # 传 -DisableTurbo:$false；-DisableTurbo 0 / -DisableTurbo:$false 都能关掉。
    [int]$DisableTurbo = 1,
    [int]$MaxPcieAspm = 1,
    [int]$WifiMaxSaving = 1,

    # 处理器能效偏好（0 = 性能优先，100 = 能效优先）。
    [ValidateRange(0, 100)]
    [int]$EppPercent = 80,

    # 核心停放：电池下让更多核心保持停放。$CoreParkingPercent 是允许保持
    # 未停放的核心比例，越低越激进。
    [switch]$CoreParking,
    [ValidateRange(0, 100)]
    [int]$CoreParkingPercent = 25,

    # 系统散热方式：被动（先降频再提速风扇）。
    [switch]$PassiveCooling,

    # 自适应亮度（需要环境光传感器）。
    [switch]$AdaptiveBrightness,

    # 屏幕变暗 / 睡眠超时（秒）；0 = 不改这一项。
    [ValidateRange(0, 600)]
    [int]$DimSeconds = 30,
    [ValidateRange(0, 86400)]
    [int]$SleepSeconds = 900,

    # 电池下电源模式 = 最佳能效（写入电源模式的电池档）。
    [int]$PowerModeEco = 1,

    [switch]$Undo,

    # 给了日志路径就把全部输出（含错误）写到这个文件，供设置窗口的「省电」页显示。
    # 重定向放在脚本里，这样调用方不必再拼 powershell -Command 命令行。
    [string]$LogPath
)

if ($LogPath) {
    $forward = @{}
    foreach ($name in $PSBoundParameters.Keys) {
        if ($name -ne 'LogPath') { $forward[$name] = $PSBoundParameters[$name] }
    }
    $logDir = Split-Path -Parent $LogPath
    if ($logDir -and -not (Test-Path -LiteralPath $logDir)) {
        New-Item -ItemType Directory -Path $logDir -Force | Out-Null
    }
    & $PSCommandPath @forward *>&1 | Out-File -LiteralPath $LogPath -Encoding utf8
    exit $LASTEXITCODE
}

$ErrorActionPreference = 'Stop'
$StateDir  = Join-Path $env:LOCALAPPDATA 'MP14Tools'
$StateFile = Join-Path $StateDir 'eco-setup.json'
$EcoName   = 'MP14 省电'
$TaskOn    = 'MP14Tools-EcoOn'
$TaskOff   = 'MP14Tools-EcoOff'

function Test-Admin {
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    (New-Object Security.Principal.WindowsPrincipal $id).IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)
}

# 在 $ErrorActionPreference='Stop' 下，用 2>&1 把原生命令的 stderr 合流会变成"终止性错误"，
# 于是一句无害的提示就能把整个脚本打断（powercfg 的 "找不到该设置"、schtasks 的 "找不到任务"）。
# 所以原生命令统一走这里：临时放宽 EAP，把 stdout+stderr 一起取回来，只把退出码交给调用者判断。
function Invoke-Native {
    param([string]$Exe, [string[]]$Arguments)
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $text = (& $Exe @Arguments 2>&1 | Out-String)
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previous
    }
    return [pscustomobject]@{ code = $code; text = $text }
}

function Invoke-PowerCfg {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)
    $r = Invoke-Native 'powercfg' $Arguments
    if ($r.code -ne 0) {
        throw ('powercfg ' + ($Arguments -join ' ') + " 失败（exit $($r.code)）：" + $r.text.Trim())
    }
    $r.text
}

function Get-ActiveSchemeGuid {
    $text = (Invoke-Native 'powercfg' @('/getactivescheme')).text -replace "`r?`n", ' '
    $m = [regex]::Match($text, '([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})')
    if (-not $m.Success) { throw "无法解析当前电源方案 GUID：$text" }
    $m.Groups[1].Value.ToLower()
}

# 读某一项当前的"直流（电池）"索引值；读不到返回 $null。
#
# 直接读注册表而不是解析 powercfg 的输出：隐藏设置（EPP、核心停放、散热策略…）
# powercfg /query 根本不会显示，而注册表里一视同仁，且不受系统语言影响。
$PowerGuidMap = @{
    'SUB_PROCESSOR'    = '54533251-82be-4824-96c1-47b60b740d00'
    'SUB_VIDEO'        = '7516b95f-f776-4464-8c53-06167f40cc99'
    'SUB_SLEEP'        = '238c9fa8-0aad-41ed-83f4-97be242c8f20'
    'SUB_PCIEXPRESS'   = '501a4d13-42af-4429-9fd1-a8218c268e20'
    'SUB_DISK'         = '0012ee47-9041-4b5d-9b77-535fba8b1442'
    'SUB_ENERGYSAVER'  = 'de830923-a562-41af-a086-e3a2c6bad2da'
    'PROCTHROTTLEMAX'  = 'bc5038f7-23e0-4960-96da-33abaf5935ec'
    'PERFBOOSTMODE'    = 'be337238-0d82-4146-a960-4f3749d470c7'
    'PERFEPP'          = '36687f9e-e3a5-4dbf-b1dc-15eb381c6863'
    'CPMINCORES'       = '0cc5b647-c1df-4637-891a-dec35c318583'
    'SYSCOOLPOL'       = '94d3a615-a899-4ac5-ae2b-e4d8f634367f'
    'ASPM'             = 'ee12f906-d277-404b-b6da-e5fa1a576df5'
    'VIDEOIDLE'        = '3c0bc021-c8a8-4e07-a973-6b14cbcb2b7e'
    'VIDEONORMALLEVEL' = 'aded5e82-b909-4619-9949-f5d71dac0bcb'
    'VIDEODIMLEVEL'    = 'f1fbfde2-a960-4165-9f88-50667911ce96'
    'ADAPTBRIGHT'      = 'fbd9aa66-9553-4097-ba44-ed6e9d65eab8'
    'STANDBYIDLE'      = '29f6c1db-86da-48c5-9fdb-f2b67b1f44da'
    'ESBATTTHRESHOLD'  = 'e69653ca-cf7f-4f05-aa73-cb833fa90ad4'
    'DISKIDLE'         = '6738e2c4-e8a5-4a42-b16a-e040e769756e'
}

function Resolve-PowerGuid([string]$Value) {
    if ($PowerGuidMap.ContainsKey($Value)) { return $PowerGuidMap[$Value] }
    return $Value.ToLower()
}

function Get-DcValue {
    param([string]$Scheme, [string]$Sub, [string]$Setting)
    $subGuid = Resolve-PowerGuid $Sub
    $setGuid = Resolve-PowerGuid $Setting
    $path = "HKLM:\SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes\$Scheme\$subGuid\$setGuid"
    $item = Get-ItemProperty -Path $path -Name DCSettingIndex -ErrorAction SilentlyContinue
    if ($null -eq $item) { return $null }
    return [int]$item.DCSettingIndex
}

# 写入并读回校验
function Set-DcValue {
    param([string]$Scheme, [string]$Sub, [string]$Setting, [int]$Value, [string]$What)
    try {
        Invoke-PowerCfg /setdcvalueindex $Scheme $Sub $Setting $Value | Out-Null
    } catch {
        Write-Host ("  ✗ {0} —— {1}" -f $What, $_.Exception.Message) -ForegroundColor Yellow
        return $false
    }
    $now = Get-DcValue -Scheme $Scheme -Sub $Sub -Setting $Setting
    if ($null -ne $now -and $now -ne $Value) {
        Write-Host ("  ! {0}：写入 {1}，读回却是 {2}" -f $What, $Value, $now) -ForegroundColor Yellow
        return $false
    }
    Write-Host ("  ✓ {0}" -f $What)
    return $true
}

# 把某一项恢复成"方案自带的值"（开关关掉后重新应用时走这里）。
#
# 不能靠删注册表值：PowerSchemes 下的键只给管理员读权限，写权限在 SYSTEM 手里
# （powercfg 是通过电源服务写的），Remove-ItemProperty 会被静默拒绝。所以这里
# 改成写回参考值——副本方案参考"原方案现在是什么"，-Scope Always 参考首次应用前
# 记下的 pristine；参考值是空且当前本来就是空的话，什么都不用做。
function Clear-DcValue {
    param([string]$Scheme, [string]$Sub, [string]$Setting, [string]$What, [Nullable[int]]$Reference)
    $now = Get-DcValue -Scheme $Scheme -Sub $Sub -Setting $Setting
    if ($null -eq $Reference) {
        if ($null -eq $now) {
            Write-Host ("  ✓ {0}（方案默认）" -f $What)
            return $true
        }
        Write-Host ("  ! {0}：原本没有显式值，当前是 {1}，无法自动清除；需要时用 -Undo 回滚" -f $What, $now) -ForegroundColor Yellow
        return $false
    }
    if ($now -eq [int]$Reference) {
        Write-Host ("  ✓ {0}（跟随原方案：{1}）" -f $What, [int]$Reference)
        return $true
    }
    return (Set-DcValue -Scheme $Scheme -Sub $Sub -Setting $Setting -Value ([int]$Reference) -What $What)
}

if (-not (Test-Admin)) {
    Write-Host '这个脚本需要管理员权限（要改电源方案 + 建计划任务）。' -ForegroundColor Yellow
    Write-Host '请右键"以管理员身份运行 PowerShell"，然后：' -ForegroundColor Yellow
    Write-Host "  & '$PSCommandPath'" -ForegroundColor Cyan
    exit 1
}

if (-not (Test-Path $StateDir)) { New-Item -ItemType Directory -Path $StateDir -Force | Out-Null }

# ---------------------------------------------------------------- 要写入的项
function Get-EcoValues {
    param(
        [int]$CpuMax, [int]$Brightness, [int]$ScreenOff, [bool]$Turbo, [bool]$Aspm, [bool]$Wifi,
        [int]$Epp, [bool]$Parking, [int]$ParkingPct, [bool]$Passive, [bool]$Adaptive,
        [int]$Dim, [int]$SleepSeconds
    )

    # USB 选择性暂停不在这里：这台机器的电源方案不接受该项（powercfg 报"参数无效"）。
    # Val = $null 表示"这一项不写值"：副本方案会保持从原方案复制来的样子，
    # -Scope Always 则写回 pristine 里记的原值——开关关掉后重新应用不会残留。
    $list = @(
        @{ Sub = 'SUB_PROCESSOR';  Set = 'PROCTHROTTLEMAX';  Val = $CpuMax;      What = "最大处理器状态 $CpuMax%" },
        @{ Sub = 'SUB_PROCESSOR';  Set = 'PERFEPP';          Val = $Epp;         What = "处理器能效偏好 $Epp%（0=性能优先，100=能效优先）" }
    )
    if ($Turbo) { $list += @{ Sub = 'SUB_PROCESSOR'; Set = 'PERFBOOSTMODE'; Val = 0; What = '关闭睿频加速' } }
    else        { $list += @{ Sub = 'SUB_PROCESSOR'; Set = 'PERFBOOSTMODE'; Val = $null; What = '睿频加速：恢复方案默认' } }
    if ($Parking) { $list += @{ Sub = 'SUB_PROCESSOR'; Set = 'CPMINCORES'; Val = $ParkingPct; What = "核心停放：最小未停放核心 $ParkingPct%" } }
    else          { $list += @{ Sub = 'SUB_PROCESSOR'; Set = 'CPMINCORES'; Val = $null; What = '核心停放：恢复方案默认' } }
    if ($Passive) { $list += @{ Sub = 'SUB_PROCESSOR'; Set = 'SYSCOOLPOL'; Val = 1; What = '系统散热方式：被动（先降频再提速风扇）' } }
    else          { $list += @{ Sub = 'SUB_PROCESSOR'; Set = 'SYSCOOLPOL'; Val = $null; What = '系统散热方式：恢复方案默认' } }
    if ($Aspm)  { $list += @{ Sub = 'SUB_PCIEXPRESS'; Set = 'ASPM'; Val = 2; What = 'PCIe 链接状态电源管理：最大省电' } }
    else        { $list += @{ Sub = 'SUB_PCIEXPRESS'; Set = 'ASPM'; Val = $null; What = 'PCIe 链接电源管理：恢复方案默认' } }
    $list += @{ Sub = 'SUB_VIDEO'; Set = 'VIDEOIDLE';        Val = $ScreenOff;  What = "关屏时间 $ScreenOff 秒" }
    $list += @{ Sub = 'SUB_VIDEO'; Set = 'VIDEONORMALLEVEL'; Val = $Brightness; What = "显示器亮度档位 $Brightness%" }
    if ($Dim -gt 0) { $list += @{ Sub = 'SUB_VIDEO'; Set = 'f1fbfde2-a960-4165-9f88-50667911ce96'; Val = $Dim; What = "屏幕变暗超时 $Dim 秒" } }
    else            { $list += @{ Sub = 'SUB_VIDEO'; Set = 'f1fbfde2-a960-4165-9f88-50667911ce96'; Val = $null; What = '屏幕变暗超时：恢复方案默认' } }
    if ($Adaptive) { $list += @{ Sub = 'SUB_VIDEO'; Set = 'ADAPTBRIGHT'; Val = 1; What = '自适应亮度：开启' } }
    else           { $list += @{ Sub = 'SUB_VIDEO'; Set = 'ADAPTBRIGHT'; Val = $null; What = '自适应亮度：恢复方案默认' } }
    if ($SleepSeconds -gt 0) { $list += @{ Sub = 'SUB_SLEEP'; Set = 'STANDBYIDLE'; Val = $SleepSeconds; What = "睡眠超时 $SleepSeconds 秒" } }
    else                     { $list += @{ Sub = 'SUB_SLEEP'; Set = 'STANDBYIDLE'; Val = $null; What = '睡眠超时：恢复方案默认' } }
    $list += @{ Sub = 'SUB_DISK';  Set = 'DISKIDLE';         Val = 120;         What = '硬盘闲置 2 分钟后关闭' }
    if ($Wifi)  { $list += @{ Sub = '19cbb8fa-5279-450e-9fac-8a3d5fedd0c1'; Set = '12bbebe6-58d6-4636-95bb-3217ef867c1a'; Val = 3; What = '无线网卡省电：最高' } }
    else        { $list += @{ Sub = '19cbb8fa-5279-450e-9fac-8a3d5fedd0c1'; Set = '12bbebe6-58d6-4636-95bb-3217ef867c1a'; Val = $null; What = '无线网卡省电：恢复方案默认' } }
    return $list
}

# 是否在电池供电（GetSystemPowerStatus：ACLineStatus 0=电池，1=交流，255=未知）
function Test-OnBattery {
    if (-not ('Mp14SetupPowr' -as [type])) {
        Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class Mp14SetupPowr {
    [DllImport("powrprof.dll")] public static extern uint PowerSetActiveOverlayScheme(Guid overlaySchemeGuid);
    [DllImport("kernel32.dll")] public static extern bool GetSystemPowerStatus(out MP14_SETUP_POWER_STATUS status);
}
[StructLayout(LayoutKind.Sequential)]
public struct MP14_SETUP_POWER_STATUS {
    public byte ACLineStatus;
    public byte BatteryFlag;
    public byte BatteryLifePercent;
    public byte SystemStatusFlag;
    public int BatteryLifeTime;
    public int BatteryFullLifeTime;
}
"@ -ErrorAction SilentlyContinue
    }
    $status = New-Object MP14_SETUP_POWER_STATUS
    $ok = [Mp14SetupPowr]::GetSystemPowerStatus([ref]$status)
    if (-not $ok) { return $false }
    return ($status.ACLineStatus -eq 0)
}

# 设置"当前电源来源"的电源模式；成功返回 $true
function Set-OverlayScheme([string]$Guid) {
    try {
        $result = [Mp14SetupPowr]::PowerSetActiveOverlayScheme([Guid]$Guid)
        return ($result -eq 0)
    } catch {
        return $false
    }
}

# ---------------------------------------------------------------- 回滚
if ($Undo) {
    if (-not (Test-Path $StateFile)) {
        Write-Host "找不到 $StateFile，没有可回滚的记录。" -ForegroundColor Yellow
        exit 1
    }
    $state = Get-Content $StateFile -Raw | ConvertFrom-Json
    Write-Host "回滚：上次使用 -Scope $($state.scope)" -ForegroundColor Cyan

    foreach ($t in @($TaskOn, $TaskOff)) {
        if (Get-ScheduledTask -TaskName $t -ErrorAction SilentlyContinue) {
            Unregister-ScheduledTask -TaskName $t -Confirm:$false
            Write-Host "  已删除计划任务 $t"
        }
    }

    if ($state.ecoGuid) {
        try {
            Invoke-PowerCfg /setactive $state.originalGuid | Out-Null
            Write-Host "  已把活动方案切回 $($state.originalGuid)"
        } catch { Write-Host "  切回原方案失败：$($_.Exception.Message)" -ForegroundColor Yellow }
    }

    if ($state.powerModeEco) {
        if (Test-OnBattery) {
            $target = '00000000-0000-0000-0000-000000000000'
            if ($state.overlayExisted -and $state.overlayDc) { $target = [string]$state.overlayDc }
            if (Set-OverlayScheme $target) {
                Write-Host "  已还原电池下电源模式：$target"
            } else {
                Write-Host '  还原电源模式失败（可能需要在电池上再跑一次）' -ForegroundColor Yellow
            }
        } else {
            Write-Host '  接电中：电源模式的电池档要在拔电后再跑一次 -Undo 才能还原' -ForegroundColor Yellow
        }
    }

    # pristine 比最后一份备份可靠：多次应用后，备份里存的可能已经是我们
    # 自己写过的值。
    $pristine = @{}
    if ($null -ne $state.pristine) {
        foreach ($property in $state.pristine.PSObject.Properties) {
            $pristine[$property.Name] = $property.Value
        }
    }

    foreach ($entry in $state.backup) {
        # 副本方案紧接着整个删除，逐项还原没有意义。
        if ($entry.scheme -eq $state.ecoGuid) { continue }

        $key = "$($entry.scheme)|$($entry.sub)|$($entry.setting)"
        $value = $entry.value
        if ($pristine.ContainsKey($key)) { $value = $pristine[$key] }

        if ($null -eq $value) {
            # 原值就是方案默认。注册表删不了（写权限在 SYSTEM 手里），
            # 只能看当前是不是刚好也空着。
            $now = Get-DcValue -Scheme $entry.scheme -Sub $entry.sub -Setting $entry.setting
            if ($null -eq $now) {
                Write-Host ("  已恢复 {0}（方案默认）" -f $entry.setting)
            } else {
                Write-Host ("  ! {0}：原值为方案默认、当前却是 {1}，无法自动清除；可导入 {2} 手动恢复" -f $entry.setting, $now, $state.backupFile) -ForegroundColor Yellow
            }
            continue
        }
        try {
            Invoke-PowerCfg /setdcvalueindex $entry.scheme $entry.sub $entry.setting ([int]$value) | Out-Null
            Write-Host ("  已还原 {0} / {1} = {2}" -f $entry.setting, $entry.scheme, $value)
        } catch { Write-Host ("  还原 {0} 失败：{1}" -f $entry.setting, $_.Exception.Message) -ForegroundColor Yellow }
    }

    if ($state.ecoGuid) {
        try {
            Invoke-PowerCfg /delete $state.ecoGuid | Out-Null
            Write-Host "  已删除副本方案「$EcoName」"
        } catch { Write-Host "  删除副本方案失败（可能正在使用）：$($_.Exception.Message)" -ForegroundColor Yellow }
    }

    Remove-Item $StateFile -Force
    Write-Host '回滚完成。' -ForegroundColor Green
    exit 0
}

# ---------------------------------------------------------------- 应用
$original = Get-ActiveSchemeGuid
Write-Host "当前电源方案：$original" -ForegroundColor Cyan

# 上次应用留下的状态：pristine 记录每一项"我们第一次动手之前"的原值。
# 副本方案模式用实时读取的原方案做参考（更准）；-Scope Always 直接写当前
# 方案，只能靠这张表把"开关关掉"的这一项恢复原样。
$previous = $null
if (Test-Path $StateFile) {
    try { $previous = Get-Content $StateFile -Raw | ConvertFrom-Json } catch { }
}
$pristine = @{}
if ($null -ne $previous -and $null -ne $previous.pristine) {
    foreach ($property in $previous.pristine.PSObject.Properties) {
        $pristine[$property.Name] = $property.Value
    }
}

$stamp  = Get-Date -Format 'yyyyMMdd-HHmmss'
$backup = Join-Path $StateDir "scheme-backup-$stamp.pow"
try {
    Invoke-PowerCfg /export $backup $original | Out-Null
    Write-Host "（额外保险）已导出整个方案：$backup —— 需要时可用 powercfg /import 手动恢复"
} catch { Write-Host "导出方案备份失败（不影响后续）：$($_.Exception.Message)" -ForegroundColor Yellow }

$ecoGuid = $null
$target  = $original

if ($Scope -eq 'SaverOnly') {
    # 同名旧副本先清掉，避免越积越多
    $list = (& powercfg /list) -join "`n"
    foreach ($m in [regex]::Matches($list, '([0-9a-fA-F-]{36})\s+\((.*?)\)')) {
        if ($m.Groups[2].Value.Trim() -eq $EcoName) {
            Write-Host "  发现同名旧方案 $($m.Groups[1].Value)，删除" -ForegroundColor Yellow
            try { Invoke-PowerCfg /delete $m.Groups[1].Value | Out-Null } catch { Write-Host "    删除失败：$($_.Exception.Message)" -ForegroundColor Yellow }
        }
    }

    $dupOut = Invoke-PowerCfg /duplicatescheme $original
    $m = [regex]::Match(($dupOut -join ' '), '([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})')
    if (-not $m.Success) { throw "无法从 /duplicatescheme 的输出解析新方案 GUID：$($dupOut -join ' ')" }
    $ecoGuid = $m.Groups[1].Value.ToLower()
    Invoke-PowerCfg /changename $ecoGuid $EcoName 'MP14Tools：节电模式开启时使用' | Out-Null
    Write-Host "已创建副本方案「$EcoName」：$ecoGuid" -ForegroundColor Green
    $target = $ecoGuid
} else {
    Write-Host '将直接修改当前方案（只改电池/D C 值，插电侧完全不动）' -ForegroundColor Green
}

$values = Get-EcoValues -CpuMax $CpuMaxPercent -Brightness $BrightnessPercent -ScreenOff $ScreenOffSeconds `
    -Turbo ([bool]$DisableTurbo) -Aspm ([bool]$MaxPcieAspm) -Wifi ([bool]$WifiMaxSaving) `
    -Epp $EppPercent -Parking $CoreParking.IsPresent -ParkingPct $CoreParkingPercent `
    -Passive $PassiveCooling.IsPresent -Adaptive $AdaptiveBrightness.IsPresent `
    -Dim $DimSeconds -SleepSeconds $SleepSeconds

# 写入前逐项备份原值（回滚靠它，不依赖 /import）
$backupEntries = @()
$schemesToTouch = @($target)
if ($target -ne $original) { $schemesToTouch += $original }   # 节电阈值要写在平时生效的方案里
foreach ($scheme in $schemesToTouch) {
    foreach ($v in $values) {
        $old = Get-DcValue -Scheme $scheme -Sub $v.Sub -Setting $v.Set
        $backupEntries += [ordered]@{ scheme = $scheme; sub = $v.Sub; setting = $v.Set; value = $old }
    }
    # 首次写入某个方案之前记下它的原值。-Scope Always 下"关掉开关"要靠它恢复；
    # 副本方案的 GUID 每次都换，不入表。
    if ($scheme -eq $original) {
        foreach ($v in $values) {
            $key = "$scheme|$($v.Sub)|$($v.Set)"
            if (-not $pristine.ContainsKey($key)) {
                $pristine[$key] = Get-DcValue -Scheme $scheme -Sub $v.Sub -Setting $v.Set
            }
        }
    }
}

Write-Host "正在写入 $target ：" -ForegroundColor Cyan
foreach ($v in $values) {
    if ($null -eq $v.Val) {
        # 开关关掉：目标方案里应当是"原方案的值"。
        $reference = $null
        if ($target -ne $original) {
            $reference = Get-DcValue -Scheme $original -Sub $v.Sub -Setting $v.Set
        } else {
            $key = "$target|$($v.Sub)|$($v.Set)"
            if ($pristine.ContainsKey($key)) { $reference = $pristine[$key] }
        }
        [void](Clear-DcValue -Scheme $target -Sub $v.Sub -Setting $v.Set -What $v.What -Reference $reference)
    } else {
        [void](Set-DcValue -Scheme $target -Sub $v.Sub -Setting $v.Set -Value $v.Val -What $v.What)
    }
}

# 节电阈值必须写在"平时生效的那个方案"里，否则会死锁：
# 省电档只有在节电模式打开时才激活，而阈值决定节电模式何时自动打开。
$thresholdSub = 'SUB_ENERGYSAVER'
$thresholdSet = 'ESBATTTHRESHOLD'
$oldThreshold = Get-DcValue -Scheme $original -Sub $thresholdSub -Setting $thresholdSet
$backupEntries += [ordered]@{ scheme = $original; sub = $thresholdSub; setting = $thresholdSet; value = $oldThreshold }
$thresholdKey = "$original|$thresholdSub|$thresholdSet"
if (-not $pristine.ContainsKey($thresholdKey)) { $pristine[$thresholdKey] = $oldThreshold }
foreach ($scheme in @($original, $target) | Select-Object -Unique) {
    [void](Set-DcValue -Scheme $scheme -Sub $thresholdSub -Setting $thresholdSet -Value $SaverThresholdPercent -What "节电模式自动开启阈值 $SaverThresholdPercent%（$scheme）")
}

# 电池下电源模式 = 最佳能效。电源模式按 AC/DC 分开存储，而设置它的 API
# （PowerSetActiveOverlayScheme）只会改"当前电源来源"的那一档，注册表又只有
# SYSTEM 可写——所以：
#   · 现在在电池上 → 直接调用 API 生效；
#   · 现在接着电源 → 交给 EcoOn 任务在电池上设置（mp14-eco-task.ps1）。
$bestEfficiency = '961cc777-2547-4f9d-8174-7d86181b8a7a'
$overlayOld     = $null
$overlayExisted = $false
if ($PowerModeEco) {
    $overlayKey = 'HKLM:\SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes'
    $item = Get-ItemProperty -Path $overlayKey -Name 'ActiveOverlayDcPowerScheme' -ErrorAction SilentlyContinue
    if ($null -ne $item) { $overlayOld = [string]$item.ActiveOverlayDcPowerScheme; $overlayExisted = $true }

    $onBattery = Test-OnBattery
    if ($onBattery) {
        $ok = Set-OverlayScheme $bestEfficiency
        if ($ok) { Write-Host '  ✓ 电池下电源模式：最佳能效（已立即生效）' -ForegroundColor Green }
        else     { Write-Host '  ✗ 电池下电源模式设置失败' -ForegroundColor Yellow }
    } else {
        Write-Host '  ✓ 电池下电源模式：最佳能效（接电中，将在电池上由省电档任务设置）' -ForegroundColor Green
    }
}

# 状态文件先写：即使后面建计划任务失败，-Undo 也能正常工作
$state = [ordered]@{
    scope           = $Scope
    originalGuid    = $original
    ecoGuid         = $ecoGuid
    backupFile      = $backup
    taskOn          = $TaskOn
    taskOff         = $TaskOff
    cpuMax          = $CpuMaxPercent
    brightness      = $BrightnessPercent
    saverThresh     = $SaverThresholdPercent
    screenOff       = $ScreenOffSeconds
    eppPercent      = $EppPercent
    coreParking     = $CoreParking.IsPresent
    coreParkingPct  = $CoreParkingPercent
    passiveCooling  = $PassiveCooling.IsPresent
    adaptiveBright  = $AdaptiveBrightness.IsPresent
    dimSeconds      = $DimSeconds
    sleepSeconds    = $SleepSeconds
    powerModeEco    = [bool]$PowerModeEco
    overlayDc       = $overlayOld
    overlayExisted  = $overlayExisted
    pristine        = $pristine
    backup          = $backupEntries
    appliedAt       = (Get-Date).ToString('s')
}
$state | ConvertTo-Json -Depth 5 | Set-Content -Path $StateFile -Encoding UTF8

if ($Scope -eq 'SaverOnly') {
    # 任务脚本：切换电源方案 + （在电池上）设置电源模式。装到配置目录，
    # 任务引用的是这个固定路径，而不是仓库/补丁目录。
    $taskScriptSrc = Join-Path $PSScriptRoot 'mp14-eco-task.ps1'
    $taskScript = Join-Path $StateDir 'mp14-eco-task.ps1'
    if (Test-Path $taskScriptSrc) {
        Copy-Item $taskScriptSrc $taskScript -Force
        # 计划任务用 RemoteSigned 运行它；带网络标记的副本会被拒绝。
        Unblock-File -Path $taskScript -ErrorAction SilentlyContinue
        Write-Host "  ✓ 任务脚本 → $taskScript"
    } else {
        Write-Host "  ⚠️ 找不到 $taskScriptSrc（任务将退回只切电源方案）" -ForegroundColor Yellow
    }

    $principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType Interactive -RunLevel Highest
    # 关键：默认设置是"电池供电时不启动、切到电池就停止"，而 EcoOn 恰恰要在电池上跑，
    # 不覆盖这三项就会静默拒绝运行。
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable
    $made = 0

    foreach ($job in @(
        [ordered]@{ Name = $TaskOn;  Arg = "-NoProfile -ExecutionPolicy RemoteSigned -File `"$taskScript`" -Mode On" },
        [ordered]@{ Name = $TaskOff; Arg = "-NoProfile -ExecutionPolicy RemoteSigned -File `"$taskScript`" -Mode Off" }
    )) {
        try {
            $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument $job.Arg
            Register-ScheduledTask -TaskName $job.Name -Action $action -Principal $principal -Settings $settings -Force | Out-Null
            Write-Host "  ✓ 计划任务 $($job.Name)（最高权限，电池下也能跑，之后无需 UAC）" -ForegroundColor Green
            $made++
            continue
        } catch {
            Write-Host "  ✗ Register-ScheduledTask $($job.Name) 失败：$($_.Exception.Message)" -ForegroundColor Yellow
        }
        # 退回 schtasks.exe：/xml 里显式写上"允许在电池下启动/不因切电池而停止"
        $xmlPath = Join-Path $StateDir ("task-" + $job.Name + ".xml")
        $xml = @"
<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Principals><Principal id="Author"><UserId>$env:USERDOMAIN\$env:USERNAME</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <ExecutionTimeLimit>PT10M</ExecutionTimeLimit>
  </Settings>
  <Actions Context="Author"><Exec><Command>powershell.exe</Command><Arguments>$($job.Arg)</Arguments></Exec></Actions>
</Task>
"@
        Set-Content -Path $xmlPath -Value $xml -Encoding Unicode
        $r = Invoke-Native 'schtasks.exe' @('/create', '/tn', $job.Name, '/xml', $xmlPath, '/f')
        if ($r.code -eq 0) {
            Write-Host "    ✓ 改用 schtasks.exe（XML）建成 $($job.Name)" -ForegroundColor Green
            $made++
        } else {
            Write-Host "    ✗ schtasks.exe 也失败（exit $($r.code)）：$($r.text.Trim())" -ForegroundColor Yellow
        }
    }

    # 校验：确认建出来的任务真的允许在电池下运行（上一版就是栽在这里）
    foreach ($name in @($TaskOn, $TaskOff)) {
        $file = Join-Path $env:WINDIR "System32\Tasks\$name"
        if (Test-Path $file) {
            $text = Get-Content $file -Raw -Encoding Unicode
            $batteryOk = ($text -match '<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>') -and
                         ($text -match '<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>')
            if ($batteryOk) { Write-Host "  ✓ $name 允许在电池下启动" -ForegroundColor Green }
            else { Write-Host "  ⚠️ $name 仍写着'电池下不启动'，档位切换时会失败（把这一行发我）" -ForegroundColor Yellow }
        }
    }

    if ($made -lt 2) {
        Write-Host '  计划任务没建全。可以只补任务，或改用 -Scope Always（不需要任务：电池档值直接写进当前方案）。' -ForegroundColor Yellow
    }
}

Write-Host ''
Write-Host '完成。接下来：' -ForegroundColor Cyan
if ($Scope -eq 'SaverOnly') {
    Write-Host '  1) 在 MP14Tools 的 config.json 里填钩子命令：' -ForegroundColor Gray
    Write-Host '       profiles.eco.command    = schtasks /run /tn MP14Tools-EcoOn' -ForegroundColor Gray
    Write-Host '       profiles.high.command   = schtasks /run /tn MP14Tools-EcoOff' -ForegroundColor Gray
    Write-Host '       profiles.medium.command = schtasks /run /tn MP14Tools-EcoOff' -ForegroundColor Gray
    Write-Host '  2) 验证（普通权限即可，不该弹 UAC）：' -ForegroundColor Gray
    Write-Host '       schtasks /run /tn MP14Tools-EcoOn    # 然后 powercfg /getactivescheme 应显示副本方案' -ForegroundColor Gray
    Write-Host '       schtasks /run /tn MP14Tools-EcoOff   # 切回原方案' -ForegroundColor Gray
} else {
    Write-Host '  拔掉电源即生效（这些值只作用于电池供电），插电自动恢复原样。' -ForegroundColor Gray
}
Write-Host ''
Write-Host '回滚：' -ForegroundColor Cyan
Write-Host "  & '$PSCommandPath' -Undo" -ForegroundColor Gray
