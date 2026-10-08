#Requires -Version 5.1
<#
    Builds mp14tools-<version>-x64.msi with the WiX Toolset (v5) from an
    already-built mp14tools.exe.

    Needs the `wix` .NET tool and its UI / Util extensions:
        dotnet tool install --global wix --version 5.0.2
        wix extension add --global WixToolset.UI.wixext/5.0.2
        wix extension add --global WixToolset.Util.wixext/5.0.2

    Usage:
        .\installer\build-msi.ps1                                 # release exe from build.ps1
        .\installer\build-msi.ps1 -ExePath build\obj\debug\mp14tools.exe
#>
[CmdletBinding()]
param(
    [string]$ExePath = 'build\obj\release\mp14tools.exe',
    [string]$OutDir  = 'build\msi',
    [string]$Version
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent

if (-not [IO.Path]::IsPathRooted($ExePath)) { $ExePath = Join-Path $repo $ExePath }
if (-not [IO.Path]::IsPathRooted($OutDir))  { $OutDir  = Join-Path $repo $OutDir }
if (-not (Test-Path $ExePath)) { throw "no executable at $ExePath - build it first (build.ps1 -Release)" }

# Same source of truth release.yml checks the tag against.
if (-not $Version) {
    $Version = (Select-String -Path (Join-Path $repo 'Cargo.toml') -Pattern '^version = "(.+)"$').Matches[0].Groups[1].Value.Trim()
}
# MSI ProductVersion is major.minor.build; drop any pre-release suffix.
$Version = ($Version -split '[-+]')[0]
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "version '$Version' is not major.minor.patch" }

New-Item -ItemType Directory -Force $OutDir | Out-Null

# The license dialog wants RTF; the repository ships plain text.
$rtf = Join-Path $OutDir 'license.rtf'
$text = (Get-Content (Join-Path $repo 'LICENSE') -Raw -Encoding UTF8) -replace "`r", ''
$text = $text.Replace('\', '\\').Replace('{', '\{').Replace('}', '\}')
$body = ($text -split "`n" | ForEach-Object { $_ + '\par' }) -join "`r`n"
$body = [regex]::Replace($body, '[^\u0000-\u007F]', { param($m) '\u' + [int][char]$m.Value + '?' })
Set-Content -Path $rtf -Encoding ascii -Value ('{\rtf1\ansi\deff0{\fonttbl{\f0 Consolas;}}\f0\fs18' + "`r`n" + $body + '}')

$msi = Join-Path $OutDir "mp14tools-$Version-x64.msi"
& wix build -arch x64 -culture zh-CN `
    -ext WixToolset.UI.wixext -ext WixToolset.Util.wixext `
    -d "Version=$Version" -d "ExePath=$ExePath" -d "RepoRoot=$repo" -d "LicenseRtf=$rtf" `
    -o $msi (Join-Path $PSScriptRoot 'mp14tools.wxs')
if ($LASTEXITCODE -ne 0) { throw "wix build exited with $LASTEXITCODE" }

$hash = (Get-FileHash $msi -Algorithm SHA256).Hash
Write-Host "$msi : $((Get-Item $msi).Length) bytes, SHA256 $hash"
if ($env:GITHUB_OUTPUT) {
    "msi=$msi" >> $env:GITHUB_OUTPUT
    "msiVersion=$Version" >> $env:GITHUB_OUTPUT
    "msiHash=$hash" >> $env:GITHUB_OUTPUT
}
