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

    [switch]$Undo
)

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

function Invoke-PowerCfg {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)
    $out = & powercfg @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw ('powercfg ' + ($Arguments -join ' ') + " 失败（exit $LASTEXITCODE）：" + (($out | Out-String).Trim()))
    }
    $out
}

function Get-ActiveSchemeGuid {
    $text = (& powercfg /getactivescheme) -join ' '
    $m = [regex]::Match($text, '([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})')
    if (-not $m.Success) { throw "无法解析当前电源方案 GUID：$text" }
    $m.Groups[1].Value.ToLower()
}

# 读某一项当前的"直流（电池）"索引值；读不到返回 $null
function Get-DcValue {
    param([string]$Scheme, [string]$Sub, [string]$Setting)
    $out = (& powercfg /q $Scheme $Sub $Setting 2>&1) -join "`n"
    $m = [regex]::Match($out, '(?:当前直流电源设置索引|Current DC Power Setting Index)\s*:\s*(0x[0-9a-fA-F]+)')
    if ($m.Success) { return [Convert]::ToInt32($m.Groups[1].Value, 16) }
    return $null
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

if (-not (Test-Admin)) {
    Write-Host '这个脚本需要管理员权限（要改电源方案 + 建计划任务）。' -ForegroundColor Yellow
    Write-Host '请右键"以管理员身份运行 PowerShell"，然后：' -ForegroundColor Yellow
    Write-Host "  & '$PSCommandPath'" -ForegroundColor Cyan
    exit 1
}

if (-not (Test-Path $StateDir)) { New-Item -ItemType Directory -Path $StateDir -Force | Out-Null }

# ---------------------------------------------------------------- 要写入的项
function Get-EcoValues {
    param([int]$CpuMax, [int]$Brightness, [int]$ScreenOff)
    @(
        @{ Sub = 'SUB_PROCESSOR';  Set = 'PROCTHROTTLEMAX';  Val = $CpuMax;      What = "最大处理器状态 $CpuMax%" }
        @{ Sub = 'SUB_PROCESSOR';  Set = 'PERFBOOSTMODE';    Val = 0;             What = '关闭睿频加速' }
        @{ Sub = 'SUB_PCIEXPRESS'; Set = 'ASPM';             Val = 2;             What = 'PCIe 链接状态电源管理：最大省电' }
        @{ Sub = 'SUB_VIDEO';      Set = 'VIDEOIDLE';        Val = $ScreenOff;    What = "关屏时间 $ScreenOff 秒" }
        @{ Sub = 'SUB_VIDEO';      Set = 'VIDEONORMALLEVEL'; Val = $Brightness;   What = "显示器亮度档位 $Brightness%" }
        @{ Sub = 'SUB_DISK';       Set = 'DISKIDLE';         Val = 120;           What = '硬盘闲置 2 分钟后关闭' }
        @{ Sub = 'SUB_USB';        Set = '48e6b7a6-50f5-4782-a5d4-53bb8f07e226'; Val = 1; What = 'USB 选择性暂停：开启' }
        @{ Sub = '19cbb8fa-5279-450e-9fac-8a3d5fedd0c1'; Set = '12bbebe6-58d6-4636-95bb-3217ef867c1a'; Val = 3; What = '无线网卡省电：最高' }
    )
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

    foreach ($entry in $state.backup) {
        if ($null -eq $entry.value) { continue }
        try {
            Invoke-PowerCfg /setdcvalueindex $entry.scheme $entry.sub $entry.setting ([int]$entry.value) | Out-Null
            Write-Host ("  已还原 {0} / {1} = {2}" -f $entry.setting, $entry.scheme, $entry.value)
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

$values = Get-EcoValues -CpuMax $CpuMaxPercent -Brightness $BrightnessPercent -ScreenOff $ScreenOffSeconds

# 写入前逐项备份原值（回滚靠它，不依赖 /import）
$backupEntries = @()
$schemesToTouch = @($target)
if ($target -ne $original) { $schemesToTouch += $original }   # 节电阈值要写在平时生效的方案里
foreach ($scheme in $schemesToTouch) {
    foreach ($v in $values) {
        $old = Get-DcValue -Scheme $scheme -Sub $v.Sub -Setting $v.Set
        $backupEntries += [ordered]@{ scheme = $scheme; sub = $v.Sub; setting = $v.Set; value = $old }
    }
}

Write-Host "正在写入 $target ：" -ForegroundColor Cyan
foreach ($v in $values) {
    [void](Set-DcValue -Scheme $target -Sub $v.Sub -Setting $v.Set -Value $v.Val -What $v.What)
}

# 节电阈值必须写在"平时生效的那个方案"里，否则会死锁：
# 省电档只有在节电模式打开时才激活，而阈值决定节电模式何时自动打开。
$thresholdSub = 'SUB_ENERGYSAVER'
$thresholdSet = 'ESBATTTHRESHOLD'
$oldThreshold = Get-DcValue -Scheme $original -Sub $thresholdSub -Setting $thresholdSet
$backupEntries += [ordered]@{ scheme = $original; sub = $thresholdSub; setting = $thresholdSet; value = $oldThreshold }
foreach ($scheme in @($original, $target) | Select-Object -Unique) {
    [void](Set-DcValue -Scheme $scheme -Sub $thresholdSub -Setting $thresholdSet -Value $SaverThresholdPercent -What "节电模式自动开启阈值 $SaverThresholdPercent%（$scheme）")
}

if ($Scope -eq 'SaverOnly') {
    $principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType Interactive -RunLevel Highest
    $actOn  = New-ScheduledTaskAction -Execute 'powercfg.exe' -Argument "/setactive $target"
    $actOff = New-ScheduledTaskAction -Execute 'powercfg.exe' -Argument "/setactive $original"
    Register-ScheduledTask -TaskName $TaskOn  -Action $actOn  -Principal $principal -Force | Out-Null
    Register-ScheduledTask -TaskName $TaskOff -Action $actOff -Principal $principal -Force | Out-Null
    Write-Host "已创建计划任务：$TaskOn / $TaskOff（最高权限，之后无需 UAC）" -ForegroundColor Green
}

$state = [ordered]@{
    scope        = $Scope
    originalGuid = $original
    ecoGuid      = $ecoGuid
    backupFile   = $backup
    taskOn       = $TaskOn
    taskOff      = $TaskOff
    cpuMax       = $CpuMaxPercent
    brightness   = $BrightnessPercent
    saverThresh  = $SaverThresholdPercent
    screenOff    = $ScreenOffSeconds
    backup       = $backupEntries
    appliedAt    = (Get-Date).ToString('s')
}
$state | ConvertTo-Json -Depth 5 | Set-Content -Path $StateFile -Encoding UTF8

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
