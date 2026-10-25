# Blitz instant installer for Windows
$ErrorActionPreference = 'Stop'
$repo = "h3ma209/Blitz"
Write-Host "⚡ Fetching latest release of Blitz..." -ForegroundColor Cyan

$release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/latest"
$tag = $release.tag_name

if (-not $tag) {
    Write-Error "Could not determine latest release tag."
    exit 1
}

$zipUrl = "https://github.com/$repo/releases/download/$tag/blitz-windows-x86_64.zip"
$installDir = Join-Path $env:LOCALAPPDATA "Blitz"
$zipPath = Join-Path $env:TEMP "blitz-windows-x86_64.zip"

Write-Host "Downloading $zipUrl..." -ForegroundColor Gray
Invoke-WebRequest -Uri $zipUrl -OutFile $zipPath

if (-not (Test-Path $installDir)) {
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
}

Write-Host "Extracting to $installDir..." -ForegroundColor Gray
Expand-Archive -Path $zipPath -DestinationPath $installDir -Force
Remove-Item -Path $zipPath -Force

# Automatically unblock from Windows SmartScreen quarantine
try {
    Unblock-File -Path (Join-Path $installDir "blitz.exe") -ErrorAction SilentlyContinue
} catch {
    # Non-fatal
}

# Add to user PATH if not present
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -split ';' -notcontains $installDir) {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$installDir", "User")
    $env:Path += ";$installDir"
    Write-Host "Added $installDir to User PATH." -ForegroundColor Gray
}

# Create Start Menu shortcut
try {
    $startMenuDir = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
    $shortcutPath = Join-Path $startMenuDir "Blitz.lnk"
    $wscript = New-Object -ComObject WScript.Shell
    $shortcut = $wscript.CreateShortcut($shortcutPath)
    $shortcut.TargetPath = Join-Path $installDir "blitz.exe"
    $shortcut.WorkingDirectory = $installDir
    $shortcut.Description = "Blitz API Client"
    $shortcut.Save()
} catch {
    # Non-fatal if shortcut creation fails
}

Write-Host ""
Write-Host "✓ Blitz successfully installed to $installDir\blitz.exe!" -ForegroundColor Green
Write-Host "  Launch from Start Menu or type: blitz" -ForegroundColor Gray
