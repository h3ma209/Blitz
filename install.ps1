# Blitz instant installer for Windows
$ErrorActionPreference = 'Stop'
$repo = "h3ma209/Blitz"
$version = if ($args[0]) { $args[0] } else { "latest" }

Write-Host "⚡ Fetching Blitz ($version)..." -ForegroundColor Cyan

if ($version -eq "latest") {
    $zipUrl = "https://github.com/$repo/releases/latest/download/blitz-windows-x86_64.zip"
} else {
    $zipUrl = "https://github.com/$repo/releases/download/$version/blitz-windows-x86_64.zip"
}

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
Write-Host "Bypassing Windows SmartScreen Mark-of-the-Web quarantine..." -ForegroundColor Gray
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
Write-Host "  Windows SmartScreen quarantine unblocked: Unblock-File" -ForegroundColor Gray
Write-Host "  Launch from Start Menu or type: blitz" -ForegroundColor Gray
