#!/usr/bin/env pwsh
# Halftone installer - Windows, no admin.
#   irm https://github.com/Trapston3/Halftone/raw/main/tools/install.ps1 | iex
# Works on Windows PowerShell 5.1 and PowerShell 7+.
#
# Downloads the latest release exe, verifies sha256 against the release's
# latest.json (or the Halftone.exe.sha256 sidecar) when available, kills a
# running Halftone, then installs to %LOCALAPPDATA%\Halftone
# (PATH + App Paths + Desktop/Start Menu shortcuts).
#
# Downloads try curl.exe (Windows 10+ ships it) first, then
# Invoke-WebRequest, then WebClient / Start-BitsTransfer as a last resort,
# with retries and 2s/5s/10s backoff between attempts - GitHub's release CDN
# occasionally answers 504, and one downloader alone is not enough.
#
# Switches and environment overrides (mainly for testing):
#   -DryRun / HALFTONE_DRYRUN=1  download + verify, but do not install
#   HALFTONE_REPO                use <owner>/<name> instead of Trapston3/Halftone
#   HALFTONE_BASE_URL            fetch latest.json + Halftone.exe from here
#                                instead of the GitHub release (flat directory)
#   HALFTONE_NO_API=1            skip the GitHub API asset listing (tests)
param([switch]$DryRun)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"   # the progress bar makes IWR 10-50x slower
try {
  [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
  [void][Net.SecurityProtocolType]::Tls13
} catch { }

if (-not $DryRun -and $env:HALFTONE_DRYRUN) { $DryRun = ($env:HALFTONE_DRYRUN -match "^(1|true)$") }

$repo = "Trapston3/Halftone"
if ($env:HALFTONE_REPO) { $repo = $env:HALFTONE_REPO }
$baseOverride = $env:HALFTONE_BASE_URL
if ($baseOverride) { $baseOverride = $baseOverride.TrimEnd("/") }
$dest = "$env:LOCALAPPDATA\Halftone"
$exe  = "$dest\Halftone.exe"
$ua   = "Halftone-installer"
$iwrHeaders = @{ "User-Agent" = $ua }

Write-Host ""
Write-Host "  HALFTONE - dithered local music player" -ForegroundColor Cyan
Write-Host "  --------------------------------------"
if ($DryRun) {
  Write-Host "  dry run: download + checksum verify only - nothing will be installed" -ForegroundColor Yellow
}

$backoff = @(2, 5, 10)                   # seconds between download attempts
$dl = @{ Status = $null; Msg = $null }   # last download error, for the final message

function Get-HttpStatus([object]$Err) {
  # Best-effort HTTP status code out of a WebException / HttpResponseException.
  try {
    $sc = $Err.Exception.Response.StatusCode
    if ($sc) { return [int]$sc }
  } catch { }
  if ($Err.Exception.Message -match "\((\d{3})\)") { return [int]$Matches[1] }
  return $null
}

# --- fetch latest.json (best effort: checksum + pinned URL if present) ------
$checksum  = $null   # hex sha256 of the exe, if the release publishes one
$pinnedUrl = $null   # direct download URL from latest.json, if present
if ($baseOverride) { $relUrl = "$baseOverride/latest.json" }
else               { $relUrl = "https://github.com/$repo/releases/latest/download/latest.json" }
$meta = $null
$metaErr = $null
$meta404 = $false
for ($try = 1; $try -le 4; $try++) {
  try {
    Write-Host "  checking latest.json ... (attempt $try/4)"
    $r = Invoke-WebRequest -Uri $relUrl -UseBasicParsing -TimeoutSec 30 -Headers $iwrHeaders
    # PS 5.1 hands back byte[] when the server sends application/octet-stream
    # (github does for release assets) - normalize to text before parsing.
    $content = $r.Content
    if ($content -is [byte[]]) { $content = [System.Text.Encoding]::UTF8.GetString($content) }
    $meta = $content | ConvertFrom-Json
    break
  } catch {
    $meta = $null
    $metaErr = $_
    if ((Get-HttpStatus $_) -eq 404) { $meta404 = $true; break }   # deterministic, no retry
    if ($try -lt 4) { Start-Sleep -Seconds $backoff[$try - 1] }
  }
}
if ($meta) {
  if ($meta.url)    { $pinnedUrl = [string]$meta.url }
  if ($meta.sha256) { $checksum  = [string]$meta.sha256 }
} elseif ($meta404) {
  Write-Host "  no latest.json (older release) - falling back to asset listing" -ForegroundColor DarkGray
} else {
  Write-Host ("  couldn't fetch latest.json: {0} - continuing without checksum" -f $metaErr.Exception.Message) -ForegroundColor DarkGray
}

# --- curl.exe available? (Windows 10+ ships it; PS5.1's `curl` alias is IWR) --
$curl = Get-Command "curl.exe" -ErrorAction SilentlyContinue
$curlRetryAll = ""
if ($curl) {
  $v = [string](& curl.exe --version | Select-Object -First 1)
  if ($v -match "curl (\d+)\.(\d+)") {
    if (([int]$Matches[1] -gt 7) -or ([int]$Matches[1] -eq 7 -and [int]$Matches[2] -ge 71)) {
      $curlRetryAll = "--retry-all-errors"   # needs curl 7.71+
    }
  }
}
$bitsAvailable = [bool](Get-Command Start-BitsTransfer -ErrorAction SilentlyContinue)

function Invoke-CurlDownload([string]$Url, [string]$OutFile, [string]$Accept) {
  $a = @("-f", "-L", "-sS", "--retry", "3", "--retry-delay", "2", "--connect-timeout", "15", "-A", $ua)
  if ($curlRetryAll) { $a += $curlRetryAll }
  if ($Accept)       { $a += @("-H", "Accept: $Accept") }
  $a += @("-o", $OutFile, "-w", "%{http_code}", $Url)
  $code = [string](& curl.exe @a)
  if ($LASTEXITCODE -ne 0) {
    if ($code -match "^\d{3}$" -and $code -ne "000") { $dl.Status = [int]$code }
    $dl.Msg = "curl.exe exit code $LASTEXITCODE"
    return $false
  }
  return $true
}

function Invoke-IwrDownload([string]$Url, [string]$OutFile, [string]$Accept) {
  $h = @{ "User-Agent" = $ua }
  if ($Accept) { $h["Accept"] = $Accept }
  Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing -TimeoutSec 300 -Headers $h
  return $true
}

function Invoke-WebClientDownload([string]$Url, [string]$OutFile, [string]$Accept) {
  $wc = New-Object System.Net.WebClient
  $wc.Headers.Add("User-Agent", $ua)
  if ($Accept) { $wc.Headers.Add("Accept", $Accept) }
  $wc.DownloadFile($Url, $OutFile)
  return $true
}

function Invoke-BitsDownload([string]$Url, [string]$OutFile) {
  # Last resort: no custom User-Agent, but a different network stack.
  Start-BitsTransfer -Source $Url -Destination $OutFile -DisplayName "Halftone installer"
  return $true
}

function Invoke-DownloadAttempt([string]$Url, [string]$OutFile, [string]$Accept) {
  # One attempt: curl.exe -> Invoke-WebRequest -> WebClient -> BITS.
  if ($curl) {
    if (Invoke-CurlDownload $Url $OutFile $Accept) { return $true }
  } else {
    Write-Host "  (curl.exe not found - using Invoke-WebRequest)" -ForegroundColor DarkGray
  }
  try { if (Invoke-IwrDownload $Url $OutFile $Accept) { return $true } } catch {
    $dl.Msg = "Invoke-WebRequest: $($_.Exception.Message)"
    $s = Get-HttpStatus $_
    if ($s) { $dl.Status = $s }
  }
  try { if (Invoke-WebClientDownload $Url $OutFile $Accept) { return $true } } catch {
    $dl.Msg = "WebClient: $($_.Exception.Message)"
    $s = Get-HttpStatus $_
    if ($s) { $dl.Status = $s }
  }
  if ($bitsAvailable) {
    try { if (Invoke-BitsDownload $Url $OutFile) { return $true } } catch {
      $dl.Msg = "Start-BitsTransfer: $($_.Exception.Message)"
    }
  }
  return $false
}

function Get-ApiAsset {
  # First asset on the latest release whose name matches ^Halftone.*\.exe$.
  if ($env:HALFTONE_NO_API -eq "1") { return $null }
  try {
    $assets = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/latest" -TimeoutSec 30 -Headers $iwrHeaders
    foreach ($a in $assets.assets) {
      if ($a.name -match "^Halftone.*\.exe$") {
        return @{ BrowserUrl = [string]$a.browser_download_url; ApiUrl = [string]$a.url }
      }
    }
  } catch {
    Write-Host "  api listing failed: $($_.Exception.Message)" -ForegroundColor DarkGray
  }
  return $null
}

# --- resolve download URL candidates ----------------------------------------
# 1. latest.json url (v0.2.0+ contract, points at the exact tagged asset)
# 2. releases/latest/download/Halftone.exe (stable name, v0.2.0+ naming contract)
# 3. GitHub API asset listing: browser_download_url (works for old releases
#    named Halftone_v0.1.2.exe too)
# 4. the API asset's api.github.com url with Accept: application/octet-stream -
#    hits a different endpoint that skips the CDN redirect, useful when it 504s
$candidates = @()
if ($pinnedUrl) { $candidates += @{ Url = $pinnedUrl; Accept = $null } }
if ($baseOverride) { $canonicalUrl = "$baseOverride/Halftone.exe" }
else               { $canonicalUrl = "https://github.com/$repo/releases/latest/download/Halftone.exe" }
$candidates += @{ Url = $canonicalUrl; Accept = $null }
$apiAsset = Get-ApiAsset
if ($apiAsset) {
  $candidates += @{ Url = $apiAsset.BrowserUrl; Accept = $null }
  $candidates += @{ Url = $apiAsset.ApiUrl; Accept = "application/octet-stream" }
}

# --- download to a temp file (never straight onto the install path) ---------
$tmpExe = Join-Path ([System.IO.Path]::GetTempPath()) ("Halftone-" + [Guid]::NewGuid().ToString("N") + ".exe")
$ok = $false
$tried = @()
$downloadedUrl = $null
foreach ($c in $candidates) {
  if (-not $c.Url -or ($tried -contains $c.Url)) { continue }
  $tried += $c.Url
  for ($try = 1; $try -le 4; $try++) {
    Remove-Item $tmpExe -Force -ErrorAction SilentlyContinue
    Write-Host "  downloading (attempt $try/4): $($c.Url)"
    $dl.Status = $null
    $dl.Msg = $null
    if (Invoke-DownloadAttempt $c.Url $tmpExe $c.Accept) {
      $len = (Get-Item $tmpExe).Length
      if ($len -gt 1MB) { $ok = $true; $downloadedUrl = $c.Url; break }
      $dl.Msg = "downloaded file too small ($len bytes)"
    }
    Write-Host "  attempt failed: $($dl.Msg)" -ForegroundColor DarkGray
    if ($try -lt 4) { Start-Sleep -Seconds $backoff[$try - 1] }
  }
  if ($ok) { break }
}

if (-not $ok) {
  Remove-Item $tmpExe -Force -ErrorAction SilentlyContinue
  $lastStatus = "unknown"
  if ($dl.Status) { $lastStatus = $dl.Status }
  Write-Host ""
  Write-Host "  download failed (last HTTP status: $lastStatus). GitHub may be having" -ForegroundColor Red
  Write-Host "  issues - retry in a minute. Manual install:" -ForegroundColor Red
  Write-Host "    1. open https://github.com/$repo/releases" -ForegroundColor Yellow
  Write-Host "    2. download Halftone.exe" -ForegroundColor Yellow
  Write-Host "    3. put it anywhere, run it. done." -ForegroundColor Yellow
  exit 1
}

# --- checksum verify: latest.json sha256, else the Halftone.exe.sha256 sidecar
$sidecarUrls = @()
if ($downloadedUrl -match "\.exe$") { $sidecarUrls += ($downloadedUrl + ".sha256") }
$canonicalSidecar = "https://github.com/$repo/releases/latest/download/Halftone.exe.sha256"
if (-not $baseOverride -and ($sidecarUrls -notcontains $canonicalSidecar)) {
  $sidecarUrls += $canonicalSidecar
}
$expected = $checksum
$how = "latest.json"
if (-not $expected) {
  foreach ($u in $sidecarUrls) {
    try {
      $r = Invoke-WebRequest -Uri $u -UseBasicParsing -TimeoutSec 30 -Headers $iwrHeaders
      $txt = $r.Content
      if ($txt -is [byte[]]) { $txt = [System.Text.Encoding]::UTF8.GetString($txt) }
      $h = ($txt -split "\s+")[0]
      if ($h -match "^[0-9a-fA-F]{64}$") { $expected = $h.ToLower(); $how = "sidecar"; break }
    } catch { }
  }
}
if ($expected) {
  $actual = (Get-FileHash -Path $tmpExe -Algorithm SHA256).Hash.ToLower()
  if ($actual -ne $expected.ToLower()) {
    Write-Host ""
    Write-Host "  checksum mismatch - the downloaded exe does not match the" -ForegroundColor Red
    Write-Host "  release manifest. Deleting it; try again later or install" -ForegroundColor Red
    Write-Host "  manually from https://github.com/$repo/releases" -ForegroundColor Red
    Remove-Item -Path $tmpExe -Force -ErrorAction SilentlyContinue
    exit 1
  }
  Write-Host "  sha256 verified ($how): $actual"
} else {
  Write-Host "  no sha256 published (latest.json / .sha256 sidecar) - skipping verification" -ForegroundColor DarkGray
}

# --- dry run stops here: download + verify happened, nothing was installed ---
if ($DryRun) {
  Remove-Item $tmpExe -Force -ErrorAction SilentlyContinue
  Write-Host ""
  Write-Host "  dry run OK: download + checksum verified - nothing was installed" -ForegroundColor Green
  Write-Host ""
  exit 0
}

# --- kill a running Halftone so the file is not locked ---
Get-Process -Name "Halftone" -ErrorAction SilentlyContinue | ForEach-Object {
  Write-Host "  stopping running Halftone (pid $($_.Id)) ..."
  try { $_.Kill(); $_.WaitForExit(5000) | Out-Null } catch { }
}
Start-Sleep -Milliseconds 300

# --- move the verified exe into place ---
New-Item -ItemType Directory -Path $dest -Force | Out-Null
Move-Item -Path $tmpExe -Destination $exe -Force

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
