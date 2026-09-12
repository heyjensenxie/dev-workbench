#Requires -Version 5.1
<#
.SYNOPSIS
    Builds the portable, single-file Dev Workbench executable for Windows.

.DESCRIPTION
    Runs the frontend build plus a release Tauri build without a bundler, then
    copies the resulting executable into `release/` under a stable file name.

    The output is self-contained: it embeds the frontend bundle and the icons,
    so it needs no installer, no admin rights, and no side-by-side files. It can
    live in any directory, on a USB stick, or be pinned to the taskbar. The only
    external prerequisite is the Microsoft Edge WebView2 runtime, which ships
    with Windows 10/11.

    Written against Windows PowerShell 5.1, which every supported Windows host
    ships, so the script does not depend on PowerShell 7 being installed.

.PARAMETER OutDir
    Directory that receives the executable. Defaults to `<repo>/release`.

.PARAMETER Configuration
    Cargo profile to build. Defaults to `release`.

.EXAMPLE
    pnpm build:portable

.EXAMPLE
    powershell -File scripts/build-portable.ps1 -OutDir D:\apps
#>
[CmdletBinding()]
param(
    [string] $OutDir,

    [ValidateSet('release', 'debug')]
    [string] $Configuration = 'release'
)

$ErrorActionPreference = 'Stop'

# `$PSScriptRoot` is not reliably populated inside a param() default, so the
# repository layout is resolved here instead.
$scriptDir = $PSScriptRoot
if (-not $scriptDir) { $scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path }
$repoRoot = Split-Path -Parent $scriptDir
if (-not $OutDir) { $OutDir = Join-Path $repoRoot 'release' }

$binaryName = 'dev-workbench.exe'
$friendlyName = 'Dev Workbench.exe'

$tauriArgs = @('--no-bundle')
if ($Configuration -eq 'debug') { $tauriArgs += '--debug' }

Write-Host '==> Building frontend and Rust shell (this takes several minutes)'
& pnpm --filter '@dev-workbench/desktop' tauri build @tauriArgs
if ($LASTEXITCODE -ne 0) { throw "tauri build failed with exit code $LASTEXITCODE" }

$built = Join-Path $repoRoot "target\$Configuration\$binaryName"
if (-not (Test-Path $built)) { throw "Expected binary was not produced: $built" }

# The friendly name stays stable across versions so a pinned taskbar shortcut
# keeps working after a rebuild; the version is reported instead of encoded.
$target = Join-Path $OutDir $friendlyName
New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
Copy-Item -Path $built -Destination $target -Force

$info = (Get-Item $target).VersionInfo
$sizeMb = [math]::Round((Get-Item $target).Length / 1MB, 2)

Write-Host ''
Write-Host '==> Portable build ready'
Write-Host "    Path    : $target"
Write-Host "    Version : $($info.ProductVersion)"
Write-Host "    Size    : $sizeMb MB"
Write-Host '    Double-click to run. No installation required.'
