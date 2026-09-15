#!/usr/bin/env powershell
# Halftone one-line install: run from ANY shell (no admin needed):
#   irm https://trapston3.github.io/halftone/install.ps1 | iex
# Fallback direct (if the repo is up): 
#  irm https://raw.githubusercontent.com/Trapston3/Halftone/main/tools/install.ps1 | iex
$ErrorActionPreference = "Stop"

$repo   = "Trapston3/Halftone"
$dest   = "$env:LOCALAPPDATA\Halftone"
$exe    = "$dest\Halftone.exe"

Write-Host ""
Write-Host "  HALFTONE - dithered local music player" -ForegroundColor Cyan
Write-Host "  --------------------------------------"

# 1. get the latest release asset (Halftone.exe standalone)
$url = "https://github.com/$repo/releases/latest/download/Halftone.exe"
Write-Host "  downloading $url"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
Invoke-WebRequest -Uri $url -OutFile $exe -UseBasicParsing

# 2. PATH (user scope)
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$dest*") {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$dest", "User")
    Write-Host "  PATH updated (new shells only)"
}

# 3. App Paths so Win+R "halftone" works
$appPaths = "HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\Halftone.exe"
New-Item -Path $appPaths -Force | Out-Null
Set-ItemProperty -Path $appPaths -Name "(Default)" -Value $exe

# 4. shortcuts
$ws = New-Object -ComObject WScript.Shell
$ws.CreateShortcut("$env:USERPROFILE\Desktop\Halftone.lnk").TargetPath = $exe
$ws.CreateShortcut("$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Halftone.lnk").TargetPath = $exe

Write-Host ""
Write-Host "  installed. launch with: halftone  (Win+R) or the Start-menu shortcut" -ForegroundColor Green
Write-Host "  first run: point Halftone at your music folder - FLAC only." 
Write-Host ""
