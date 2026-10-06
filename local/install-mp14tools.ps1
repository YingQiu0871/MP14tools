#Requires -Version 5.1
<#
    MP14Tools 0.4.1 · 安装 + 管理员省电设置 + 写配置

    普通权限窗口里跑即可：需要管理员的那一步会自己弹 UAC。

    它会做四件事：
      1) 把 mp14tools.exe 复制到固定位置 %LOCALAPPDATA%\Programs\MP14Tools\（避免自启路径失效）；
      2) 启动一次，等它生成 %LOCALAPPDATA%\MP14Tools\config.json；
      3) 用 UAC 跑一次 tools\mp14-eco-setup.ps1 -Scope SaverOnly（建「MP14 省电」方案 + 两个计划任务）；
      4) 写 config.json：填好三个档位的 command、关闭原版电池策略（display.enabled=false）。
         写之前会先结束正在运行的程序、写完后读回校验，避免"写了又被程序覆盖"。

    用法：
      .\install-mp14tools.ps1
      .\install-mp14tools.ps1 -ExePath 'C:\...\mp14tools.exe'
      .\install-mp14tools.ps1 -SkipElevatedSetup     # 不做管理员那一步（只装 + 填配置）
      .\install-mp14tools.ps1 -DryRun                # 只打印计划，不动系统
#>
[CmdletBinding()]
param(
    [string]$ExePath,
    [string]$PatchDir,
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\MP14Tools'),
    [switch]$SkipElevatedSetup,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'

# 本脚本所在目录：$PSScriptRoot 在 param() 默认值求值时是空的（-File 方式），所以在这里算。
if ($PSScriptRoot) { $Here = $PSScriptRoot }
elseif ($MyInvocation.MyCommand.Path) { $Here = Split-Path $MyInvocation.MyCommand.Path -Parent }
else { $Here = (Get-Location).Path }
if (-not $PatchDir) { $PatchDir = Split-Path $Here -Parent }

$ConfigPath = Join-Path $env:LOCALAPPDATA 'MP14Tools\config.json'
$LogPath    = Join-Path $env:LOCALAPPDATA 'MP14Tools\mp14tools.log'
$Target     = Join-Path $InstallDir 'mp14tools.exe'

function Say([string]$Text, [string]$Color = 'Gray') { Write-Host $Text -ForegroundColor $Color }

# ---------------------------------------------------------------- 找 exe
$candidates = @()
if ($ExePath) { $candidates += $ExePath }
$candidates += (Join-Path $PatchDir 'mp14tools.exe')
$candidates += (Join-Path $env:USERPROFILE 'Downloads\mp14tools.exe')
$found = $candidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $found) {
    Say '找不到 mp14tools.exe。请把下载到的 exe 放到下面任一路径，或用 -ExePath 指定：' 'Red'
    $candidates | ForEach-Object { Say "  $_" }
    exit 1
}
$info = (Get-Item $found).VersionInfo
Say "安装包：$found"
Say ("FileVersion = {0}" -f $info.FileVersion)
if ("$($info.FileVersion)".Trim() -notmatch '^0\.4\.') {
    Say '注意：这不是 0.4.x 版本。如果不是你自己构建的版本，档位功能可能不存在。' 'Yellow'
}

Say ''
Say '计划：' 'Cyan'
Say "  1) 复制到  $Target"
Say "  2) 启动一次并等配置文件生成（$ConfigPath）"
if ($SkipElevatedSetup) { Say '  3) 跳过管理员设置（-SkipElevatedSetup）' }
else { Say "  3) 弹 UAC 跑 $PatchDir\tools\mp14-eco-setup.ps1 -Scope SaverOnly" }
Say "  4) 写 config.json：档位命令 + 关闭旧电池策略（先停程序、写后校验）"
if ($DryRun) { Say ''; Say '（-DryRun：以上是计划，未做任何改动）' 'Yellow'; return }

# ---------------------------------------------------------------- 1) 复制
if (-not (Test-Path $InstallDir)) { New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null }
Say ''
Say '1) 复制到固定位置' 'Cyan'

$needCopy = $true
if ($found -eq $Target) {
    $needCopy = $false
    Say '   源文件就是目标文件，跳过'
} elseif (Test-Path $Target) {
    # 同一个版本就不用动：还能避开"文件正由另一进程使用"（程序正在托盘里跑）
    $sameHash = (Get-FileHash $found -Algorithm SHA256).Hash -eq (Get-FileHash $Target -Algorithm SHA256).Hash
    if ($sameHash) {
        $needCopy = $false
        Say '   目标位置已经是同一个文件（哈希一致），跳过复制'
    }
}

if ($needCopy) {
    $procs = @(Get-Process -Name 'mp14tools' -ErrorAction SilentlyContinue)
    if ($procs.Count -gt 0) {
        Say ("   目标文件被占用：先结束正在运行的程序（{0} 个进程），复制完会重新启动" -f $procs.Count)
        $procs | Stop-Process -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 2
    }
    Copy-Item -Path $found -Destination $Target -Force
    Say "   ✓ $Target"
}

# ---------------------------------------------------------------- 2) 启动
Say ''
# 辅助脚本复制到配置目录：程序「省电」页的「应用 / 撤销」按钮调用的就是它
$stateDir = Join-Path $env:LOCALAPPDATA 'MP14Tools'
if (-not (Test-Path $stateDir)) { New-Item -ItemType Directory -Path $stateDir -Force | Out-Null }
$helperSrc = Join-Path $PatchDir 'tools\mp14-eco-setup.ps1'
$helperDst = Join-Path $stateDir 'mp14-eco-setup.ps1'
if (Test-Path $helperSrc) {
    Copy-Item $helperSrc $helperDst -Force
    Say "   ✓ 辅助脚本 → $helperDst"
} else {
    Say '   ⚠️ 找不到 tools\mp14-eco-setup.ps1，「省电」页的「应用」按钮会不可用' 'Yellow'
}

Say '2) 启动一次' 'Cyan'
$running = @(Get-Process -Name 'mp14tools' -ErrorAction SilentlyContinue)
if ($running.Count -gt 0) {
    Say ("   已经在运行了（{0} 个进程）" -f $running.Count)
} else {
    Start-Process -FilePath $Target
    Say '   已启动，等 10 秒让它写出配置……'
    Start-Sleep -Seconds 10
}
if (Test-Path $ConfigPath) {
    Say "   ✓ 配置文件存在：$ConfigPath"
} else {
    Say "   ⚠️ 还没看到配置文件。它可能在别处，或者程序没起来；先看日志：$LogPath" 'Yellow'
}

# ---------------------------------------------------------------- 3) 管理员设置
Say ''
Say '3) 省电方案（管理员）' 'Cyan'
$setup = Join-Path $PatchDir 'tools\mp14-eco-setup.ps1'
if ($SkipElevatedSetup) {
    Say '   按参数要求跳过'
} elseif (-not (Test-Path $setup)) {
    Say "   找不到 $setup，跳过" 'Yellow'
} else {
    Say '   现在会弹 UAC，请点"是"允许以管理员身份运行。'
    $ecoLog = Join-Path $PatchDir 'eco-setup-output.txt'
    if (Test-Path $ecoLog) { Remove-Item $ecoLog -Force -ErrorAction SilentlyContinue }
    try {
        # 管理员脚本跑在自己的窗口里，窗口一关输出就没了；所以把它全部写进文件
        Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -ArgumentList @(
            '-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command',
            "& '$setup' *> '$ecoLog'"
        )
        Say '   管理员脚本已结束'
    } catch {
        Say "   没能完成（可能是取消了 UAC）：$($_.Exception.Message)" 'Yellow'
    }
    if (Test-Path $ecoLog) {
        Say '   管理员脚本输出（末尾 25 行）：'
        Get-Content $ecoLog -Tail 25 -Encoding UTF8 | ForEach-Object { Say "   | $_" }
    }
    # 注意：不能写成 `& schtasks ... 2>&1`——在 $ErrorActionPreference='Stop' 下，原生命令的
    # stderr 一旦合流就变成终止性错误，"任务不存在"会把本脚本直接打断。这里临时放宽 EAP。
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $query = (& schtasks /query /tn 'MP14Tools-EcoOn' 2>&1 | Out-String)
        $queryCode = $LASTEXITCODE
        $scheme = (& powercfg /getactivescheme 2>&1 | Out-String)
    } finally {
        $ErrorActionPreference = $previous
    }
    if ($queryCode -eq 0) {
        Say '   ✓ 计划任务 MP14Tools-EcoOn 已存在'
        Say ("   当前电源方案：{0}" -f ($scheme -replace "`r?`n", ' ').Trim())
    } else {
        Say '   ⚠️ 没查到 MP14Tools-EcoOn 任务——原因就在上面那段输出里。' 'Yellow'
        Say "      把 '$ecoLog' 发我即可（或让程序先按现在的样子跑，只少了省电方案那部分）。" 'Yellow'
    }
}

# ---------------------------------------------------------------- 4) 写配置
Say ''
Say '4) 写配置' 'Cyan'
if (-not (Test-Path $ConfigPath)) {
    Say "   没有 $ConfigPath，跳过。程序跑起来之后再手工填，或重跑本脚本。" 'Yellow'
} else {
    # 关键：先结束正在运行的程序，写完再启动。
    # 否则会有已知竞态：程序界面保存时把内存里的旧配置写回磁盘，
    # 脚本刚写进去的 display.enabled=false 会被覆盖（0.4.0 实机观察到过）。
    $wasRunning = @(Get-Process -Name 'mp14tools' -ErrorAction SilentlyContinue)
    if ($wasRunning.Count -gt 0) {
        Say ("   先结束正在运行的程序（{0} 个进程），写完配置后再启动，避免写回覆盖" -f $wasRunning.Count)
        $wasRunning | Stop-Process -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 2
        $still = @(Get-Process -Name 'mp14tools' -ErrorAction SilentlyContinue)
        if ($still.Count -gt 0) {
            Say '   ⚠️ 程序没有完全退出；这次写配置可能被它覆盖。请先在托盘退出 MP14Tools，再重跑本脚本。' 'Red'
        }
    }

    $json = Get-Content $ConfigPath -Raw | ConvertFrom-Json
    if (-not $json.PSObject.Properties['profiles']) {
        Say '   配置里没有 profiles 段——确认你运行的是 0.4.x 版本的 exe。' 'Yellow'
    } else {
        $json.profiles | Add-Member -NotePropertyName enabled -NotePropertyValue $true -Force
        $mapping = @(
            @{ Name = 'eco';    Task = 'MP14Tools-EcoOn'  },
            @{ Name = 'high';   Task = 'MP14Tools-EcoOff' },
            @{ Name = 'medium'; Task = 'MP14Tools-EcoOff' }
        )
        foreach ($item in $mapping) {
            $prop = $json.profiles.PSObject.Properties[$item.Name]
            if (-not $prop) { Say ("   配置里没有 profiles.{0}，跳过" -f $item.Name) 'Yellow'; continue }
            $prop.Value | Add-Member -NotePropertyName command -NotePropertyValue ("schtasks /run /tn " + $item.Task) -Force
        }
        # 顺手关掉原版的电池策略：它和 profiles 会抢同一个刷新率
        # （旧的会在拔电时强制 60Hz、插电时恢复旧档，profiles 同时在写 120/60）
        $json.display | Add-Member -NotePropertyName enabled -NotePropertyValue $false -Force

        # 关键：必须写成 UTF-8 无 BOM，否则程序解析 JSON 会失败
        $text = $json | ConvertTo-Json -Depth 12
        [IO.File]::WriteAllText($ConfigPath, $text, (New-Object Text.UTF8Encoding($false)))

        # 读回校验：确认每一项都真的落盘了（杜绝"看起来写了、实际被覆盖"）
        $check = Get-Content $ConfigPath -Raw | ConvertFrom-Json
        $ok = $true
        if ($check.display.enabled -eq $false) {
            Say '   ✓ display.enabled = false（旧电池策略让位给 profiles）'
        } else {
            Say ("   ✗ display.enabled 仍是 {0}" -f $check.display.enabled) 'Red'
            $ok = $false
        }
        foreach ($item in $mapping) {
            $expected = "schtasks /run /tn " + $item.Task
            $actual = "$($check.profiles.PSObject.Properties[$item.Name].Value.command)"
            if ($actual -eq $expected) {
                Say ("   ✓ profiles.{0}.command = {1}" -f $item.Name, $expected)
            } else {
                Say ("   ✗ profiles.{0}.command 是 '{1}'，期望 '{2}'" -f $item.Name, $actual, $expected) 'Red'
                $ok = $false
            }
        }
        if ($ok) {
            Say '   已写入并通过校验（程序启动后会按新配置运行）'
        } else {
            Say '   写配置没有通过校验；请把这段输出发出来。' 'Red'
        }
    }

    if ($wasRunning.Count -gt 0) {
        Start-Process -FilePath $Target
        Say '   已重新启动'
    }
}

# ---------------------------------------------------------------- 5) 验证
Say ''
Say '5) 验证' 'Cyan'
Start-Sleep -Seconds 3
if (Test-Path $LogPath) {
    Say '   日志最后 12 行（按 UTF-8 读，否则中文会显示成乱码）：'
    Get-Content $LogPath -Tail 12 -Encoding UTF8 | ForEach-Object { Say "   | $_" }
} else {
    Say "   还没有日志文件：$LogPath" 'Yellow'
}
Say ''
Say '接着可以：' 'Cyan'
Say '  · 开机自启：在设置窗口「通用」页或托盘菜单里勾（手改 config.json 不会写注册表）'
Say "  · 看当前状态：powershell -ExecutionPolicy Bypass -File `"$PatchDir\tools\mp14-display-test.ps1`""
Say '  · 想看省电档：拔电源并把电量用到 40% 以下（或手动开节电模式），日志里会出现 profiles: eco ...'
Say '  · 回滚管理员设置：以管理员身份跑 tools\mp14-eco-setup.ps1 -Undo'
