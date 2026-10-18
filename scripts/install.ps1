# ⚡ Blitz Windows install (run from repo root, or: make install from Git Bash/MSYS)
# Default: %LOCALAPPDATA%\Blitz\
# Optional Start Menu shortcut under Programs\Blitz.lnk

param(
    [switch]$Uninstall,
    [string]$Prefix = "",
    [string]$RepoRoot = ""
)

$ErrorActionPreference = "Stop"

if (-not $RepoRoot) {
    $RepoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
}

$AppName = "Blitz"
$BinName = "blitz.exe"
$DestRoot = if ($Prefix) { $Prefix } else { Join-Path $env:LOCALAPPDATA $AppName }
$StartMenuDir = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
$ShortcutPath = Join-Path $StartMenuDir "$AppName.lnk"

function Remove-Install {
    if (Test-Path $DestRoot) {
        Remove-Item -Recurse -Force $DestRoot
        Write-Host "Removed $DestRoot"
    } else {
        Write-Host "Nothing at $DestRoot"
    }
    if (Test-Path $ShortcutPath) {
        Remove-Item -Force $ShortcutPath
        Write-Host "Removed Start Menu shortcut"
    }
}

if ($Uninstall) {
    Remove-Install
    exit 0
}

$ReleaseBin = Join-Path $RepoRoot "target\release\$BinName"
if (-not (Test-Path $ReleaseBin)) {
    Write-Host "Building release..."
    Push-Location $RepoRoot
    try {
        cargo build --release
    } finally {
        Pop-Location
    }
}

if (-not (Test-Path $ReleaseBin)) {
    throw "Missing $ReleaseBin — run: cargo build --release"
}

New-Item -ItemType Directory -Force -Path $DestRoot | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $DestRoot "assets") | Out-Null

Copy-Item -Force $ReleaseBin (Join-Path $DestRoot $BinName)
$AssetsSrc = Join-Path $RepoRoot "assets"
if (Test-Path $AssetsSrc) {
    Copy-Item -Recurse -Force (Join-Path $AssetsSrc "*") (Join-Path $DestRoot "assets")
}

# Start Menu shortcut (best-effort)
try {
    $Wsh = New-Object -ComObject WScript.Shell
    $Sc = $Wsh.CreateShortcut($ShortcutPath)
    $Sc.TargetPath = Join-Path $DestRoot $BinName
    $Sc.WorkingDirectory = $DestRoot
    $Sc.Description = "⚡ Blitz — local-first API client"
    $IconPng = Join-Path $DestRoot "assets\logo.png"
    if (Test-Path $IconPng) {
        # .lnk prefers .ico; leave default if no ico
    }
    $Sc.Save()
    Write-Host "Start Menu: $ShortcutPath"
} catch {
    Write-Host "Note: Start Menu shortcut skipped ($($_.Exception.Message))"
}

Write-Host ""
Write-Host "Installed to: $DestRoot"
Write-Host "  Binary:     $(Join-Path $DestRoot $BinName)"
Write-Host "  Uninstall:  powershell -File scripts/install.ps1 -Uninstall"
Write-Host "           or make uninstall"
