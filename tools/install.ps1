#!/usr/bin/env powershell
# Halftone installer — Windows, no admin.
#   irm https://github.com/Trapston3/Halftone/raw/main/tools/install.ps1 | iex
$ErrorActionPreference = "Stop"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13
$ProgressPreference = "SilentlyContinue"

$repo = "Trapston3/Halftone"
$dest = "$env:LOCALAPPDATA\Halftone"
$exe  = "$dest\Halftone.exe"

Write-Host ""
Write-Host "  HALFTONE - dithered local music player" -ForegroundColor Cyan
Write-Host "  --------------------------------------"

# --- download with retries (github occasionally resets idle conns) ---
$urls = @(
  "https://github.com/$repo/releases/latest/download/Halftone.exe",
  "https://raw.githubusercontent.com/$repo/main/src-tauri/target/release/halftone.exe"
)
$ok = $false
foreach ($u in $urls) {
  for ($try = 1; $try -le 3; $try++) {
    try {
      Write-Host "  downloading ($try/3): $u"
      Invoke-WebRequest -Uri $u -OutFile $exe -UseBasicParsing -TimeoutSec 120
      if ((Get-Item $exe).Length -gt 1MB) { $ok = $true; break }
      throw "file too small"
    } catch {
      Write-Host "  attempt failed: $($_.Exception.Message)" -ForegroundColor DarkGray
      Start-Sleep -Seconds (2 * $try)
    }
  }
  if ($ok) { break }
}
if (-not $ok) {
  Write-Host ""
  Write-Host "  download failed after retries. Manual install:" -ForegroundColor Yellow
  Write-Host "    1. open https://github.com/$repo/releases" -ForegroundColor Yellow
  Write-Host "    2. download Halftone.exe" -ForegroundColor Yellow
  Write-Host "    3. put it anywhere, run it. done." -ForegroundColor Yellow
  exit 1
}

# --- PATH (user scope) ---
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$dest*") {
  [Environment]::SetEnvironmentVariable("Path", "$userPath;$dest", "User")
  Write-Host "  PATH updated (new shells only)"
}

# --- App Paths: Win+R -> halftone ---
$appPaths = "HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\Halftone.exe"
New-Item -Path $appPaths -Force | Out-Null
Set-ItemProperty -Path $appPaths -Name "(Default)" -Value $exe

# --- shortcuts ---
$ws = New-Object -ComObject WScript.Shell
$ws.CreateShortcut("$env:USERPROFILE\Desktop\Halftone.lnk").TargetPath = $exe
$ws.CreateShortcut("$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Halftone.lnk").TargetPath = $exe

Write-Host ""
Write-Host "  installed. launch: halftone (Win+R) or the Start-menu shortcut" -ForegroundColor Green
Write-Host "  first run: point Halftone at your music folder - FLAC only."
Write-Host ""
