#!/usr/bin/env pwsh
# Halftone installer - Windows, no admin.
#   irm https://github.com/Trapston3/Halftone/raw/main/tools/install.ps1 | iex
# Works on Windows PowerShell 5.1 and PowerShell 7+.
#
# Downloads the latest release exe, verifies sha256 against the release's
# latest.json when available, kills a running Halftone, then installs to
# %LOCALAPPDATA%\Halftone (PATH + App Paths + Desktop/Start Menu shortcuts).
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
try {
  [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
  [void][Net.SecurityProtocolType]::Tls13
} catch { }

$repo = "Trapston3/Halftone"
$dest = "$env:LOCALAPPDATA\Halftone"
$exe  = "$dest\Halftone.exe"

Write-Host ""
Write-Host "  HALFTONE - dithered local music player" -ForegroundColor Cyan
Write-Host "  --------------------------------------"

# --- fetch latest.json (best effort: checksum + pinned URL if present) ---
$checksum = $null      # hex sha256 of the exe, if the release publishes one
$pinnedUrl = $null     # direct download URL from latest.json, if present
$relUrl = "https://github.com/$repo/releases/latest/download/latest.json"
try {
  Write-Host "  checking latest.json ..."
  $r = Invoke-WebRequest -Uri $relUrl -UseBasicParsing -TimeoutSec 30
  # PS 5.1 hands back byte[] when the server sends application/octet-stream
  # (github does for release assets) - normalize to text before parsing.
  $content = $r.Content
  if ($content -is [byte[]]) { $content = [System.Text.Encoding]::UTF8.GetString($content) }
  $meta = $content | ConvertFrom-Json
  if ($meta.url)     { $pinnedUrl = [string]$meta.url }
  if ($meta.sha256)  { $checksum = [string]$meta.sha256 }
} catch {
  Write-Host "  no latest.json (older release) - falling back to asset listing" -ForegroundColor DarkGray
}

# --- resolve download URL: pinned url -> canonical latest/ asset name -> api listing ---
# 1. latest.json url (v0.2.0+ contract, points at the exact tagged asset)
# 2. releases/latest/download/Halftone.exe (stable name, v0.2.0+ naming contract)
# 3. GitHub API asset listing: first asset matching ^Halftone.*\.exe$ (works for
#    old releases named Halftone_v0.1.2.exe too)
$candidates = @()
if ($pinnedUrl) { $candidates += $pinnedUrl }
$candidates += "https://github.com/$repo/releases/latest/download/Halftone.exe"

function Get-ApiAssetUrl {
  # First asset on the latest release whose name matches ^Halftone.*\.exe$.
  try {
    $assets = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/latest" -TimeoutSec 30
    foreach ($a in $assets.assets) {
      if ($a.name -match "^Halftone.*\.exe$") {
        return [string]$a.browser_download_url
      }
    }
  } catch {
    Write-Host "  api listing failed: $($_.Exception.Message)" -ForegroundColor DarkGray
  }
  return $null
}

function Test-Mz {
  param([string]$Path)
  try {
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -lt 2) { return $false }
    return ($bytes[0] -eq 0x4D -and $bytes[1] -eq 0x5A)
  } catch { return $false }
}

$ok = $false
$tried = @()
foreach ($u in $candidates) {
  if (-not $u -or ($tried -contains $u)) { continue }
  $tried += $u
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

# --- API fallback: pick ^Halftone.*\.exe$ from the latest release (old naming) ---
if (-not $ok) {
  $apiUrl = Get-ApiAssetUrl
  if ($apiUrl -and (-not ($tried -contains $apiUrl))) {
    $tried += $apiUrl
    for ($try = 1; $try -le 3; $try++) {
      try {
        Write-Host "  downloading ($try/3): $apiUrl"
        Invoke-WebRequest -Uri $apiUrl -OutFile $exe -UseBasicParsing -TimeoutSec 120
        if ((Get-Item $exe).Length -gt 1MB) { $ok = $true; break }
        throw "file too small"
      } catch {
        Write-Host "  attempt failed: $($_.Exception.Message)" -ForegroundColor DarkGray
        Start-Sleep -Seconds (2 * $try)
      }
    }
  }
}

if (-not $ok) {
  Write-Host ""
  Write-Host "  download failed after retries. Manual install:" -ForegroundColor Yellow
  Write-Host "    1. open https://github.com/$repo/releases" -ForegroundColor Yellow
  Write-Host "    2. download Halftone.exe" -ForegroundColor Yellow
  Write-Host "    3. put it anywhere, run it. done." -ForegroundColor Yellow
  exit 1
}

# --- checksum verify (only when latest.json publishes a sha256) ---
if ($checksum) {
  $actual = (Get-FileHash -Path $exe -Algorithm SHA256).Hash.ToLower()
  if ($actual -ne $checksum.ToLower()) {
    Write-Host ""
    Write-Host "  checksum mismatch - the downloaded exe does not match the" -ForegroundColor Red
    Write-Host "  release manifest. Deleting it; try again later or install" -ForegroundColor Red
    Write-Host "  manually from https://github.com/$repo/releases" -ForegroundColor Red
    Remove-Item -Path $exe -Force -ErrorAction SilentlyContinue
    exit 1
  }
  Write-Host "  sha256 verified: $actual"
}

# --- kill a running Halftone so the file is not locked ---
Get-Process -Name "Halftone" -ErrorAction SilentlyContinue | ForEach-Object {
  Write-Host "  stopping running Halftone (pid $($_.Id)) ..."
  try { $_.Kill(); $_.WaitForExit(5000) | Out-Null } catch { }
}
Start-Sleep -Milliseconds 300

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
