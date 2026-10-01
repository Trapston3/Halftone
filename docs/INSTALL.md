# Installing Halftone

## One-liners (copy-paste)

README-ready snippets:

**Windows** (PowerShell, no admin):

```powershell
irm https://github.com/Trapston3/Halftone/raw/main/tools/install.ps1 | iex
```

**Linux** (x86_64; `bash` or `sh`):

```sh
curl -fsSL https://github.com/Trapston3/Halftone/raw/main/tools/install.sh | sh
```

Linux one-liner with the AppImage forced (skip the `.deb` on apt systems):

```sh
curl -fsSL https://github.com/Trapston3/Halftone/raw/main/tools/install.sh | sh -s -- --appimage
```

Both scripts download the latest release from
[github.com/Trapston3/Halftone/releases](https://github.com/Trapston3/Halftone/releases),
verify sha256 checksums when the release publishes them, and install for the
current user only — no admin/root required.

## What the Windows installer does

1. Downloads the latest release exe (checksum-verified against the release's
   `latest.json` when available) to `%LOCALAPPDATA%\Halftone\Halftone.exe`.
   The exe is portable and self-contained.
2. Kills a running Halftone first, so upgrades never hit a file lock.
3. Adds the install folder to your **user PATH** (`halftone` works in new
   shells).
4. Registers **App Paths**, so `Win+R` → `halftone` launches it.
5. Creates **Desktop + Start Menu shortcuts**.

Re-running the one-liner upgrades in place. Works on Windows PowerShell 5.1
and PowerShell 7+.

## What the Linux installer does

1. Checks your architecture (x86_64 only) and whether the latest release
   ships Linux builds at all.
2. **Debian/Ubuntu** (apt + sudo present): downloads `Halftone_amd64.deb`,
   checksum-verifies it, and installs with `sudo apt-get install -y ./…`,
   which pulls the dependencies (webkit2gtk 4.1 and friends) automatically.
3. **Everything else** (or `--appimage` on Debian/Ubuntu, or no sudo):
   downloads `Halftone_amd64.AppImage`, checksum-verifies it, installs it to
   `~/.local/share/halftone/` with a launcher at `~/.local/bin/halftone`,
   plus a menu entry (`~/.local/share/applications/halftone.desktop`) and an
   icon. No root needed.
4. Warns if `~/.local/bin` isn't on your PATH.

Re-running the one-liner upgrades in place and switches install styles
safely (AppImage files are removed if you later take the `.deb`).

## Linux dependencies and the FUSE note

- **`.deb` path**: dependency handling is automatic (apt installs
  `libwebkit2gtk-4.1-0` etc. as declared dependencies).
- **AppImage path**: the bundles target a normal desktop install; on very
  minimal systems webkit2gtk 4.1 must be present, e.g.
  - Debian/Ubuntu: `sudo apt install libwebkit2gtk-4.1-0`
  - Fedora: `sudo dnf install webkit2gtk4.1`
  - Arch: `pacman -S webkit2gtk-4.1`
- **FUSE**: AppImages normally need `libfuse2`, which newer distros ship
  without. The installer detects this and, if it's missing, installs a
  launcher that runs the AppImage with `--appimage-extract-and-run` instead —
  you don't need to install FUSE. (Installing `libfuse2` yourself gives
  faster, extraction-free starts: `sudo apt install libfuse2`.)

## Manual install

**Windows**: from
[Releases](https://github.com/Trapston3/Halftone/releases), download
`Halftone.exe` and run it from anywhere. Same app as the one-liner installs —
you just skip the PATH/App Paths/shortcut setup.

**Linux**: download `Halftone_amd64.AppImage` (or the `.deb`) from
[Releases](https://github.com/Trapston3/Halftone/releases). For the AppImage:
`chmod +x Halftone_amd64.AppImage` and run it; if it complains about FUSE,
run it with `--appimage-extract-and-run`.

## Uninstall

**Windows**:

```powershell
rm "$env:LOCALAPPDATA\Halftone" -Recurse -Force
rm "$env:USERPROFILE\Desktop\Halftone.lnk", "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Halftone.lnk"
# clear the Win+R "halftone" alias (the (Default) value of the App Paths key)
$k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\App Paths\Halftone.exe', $true); $k.DeleteValue('')
```

(Or just delete the folder + the two shortcuts; App Paths is cosmetic and
harmless left behind.)

**Linux**:

```sh
curl -fsSL https://github.com/Trapston3/Halftone/raw/main/tools/install.sh | sh -s -- --uninstall
```

Removes the launcher, menu entry, icon and AppImage. If you installed the
`.deb`: `sudo apt-get remove halftone`.

## Troubleshooting

- **Windows: "download failed (last HTTP status: 504…)" / "download failed
  after retries"** — GitHub's release CDN (or your network) hiccuped. The
  installer already retries every URL 4× with backoff and falls back between
  `curl.exe`, `Invoke-WebRequest`, `WebClient` and BITS; transient **504
  Gateway Timeout / 502 / timeouts** usually clear on their own — **rerun the
  same one-liner after a minute** before doing anything else.
- **Corporate proxy / firewall** — the installers use the system's `curl`
  first (Windows 10+ ships `curl.exe`), which honors `HTTP_PROXY` /
  `HTTPS_PROXY` environment variables. If GitHub is blocked at the proxy, ask
  IT to allow `github.com` and `*.githubusercontent.com`, or download
  [`Halftone.exe`](https://github.com/Trapston3/Halftone/releases/latest/download/Halftone.exe)
  manually in a browser and run it; the one-liner's only extras are
  PATH/shortcuts.
- **Manual download (any platform)** — everything is on
  [Releases](https://github.com/Trapston3/Halftone/releases): grab the asset
  for your platform, optionally verify it against the sibling
  `<asset>.sha256` (`sha256sum -c` / `Get-FileHash`), and see the Manual
  install section above.
- **Testing an installer without installing** — run the download + checksum
  pipeline only: `install.ps1 -DryRun` (or `HALFTONE_DRYRUN=1`) on Windows,
  `HALFTONE_DRYRUN=1 sh install.sh` on Linux. It fetches and verifies the
  release assets, then stops before touching PATH, shortcuts or binaries.
- **Windows: `halftone` not found in a shell** — PATH changes apply to *new*
  shells only; reopen the terminal, or use `Win+R` → `halftone`.
- **Windows: "checksum mismatch"** — the downloaded exe didn't match the
  release manifest. The bad copy is deleted automatically; retry in a few
  minutes (a release may be mid-upload) or grab it manually from Releases.
- **Linux: "the latest Halftone release has no Linux builds yet"** — the
  current release is Windows-only. Linux assets ship from v0.2.0 on.
- **Linux: AppImage dies instantly on a minimal system** — check
  `libwebkit2gtk-4.1-0` presence (see dependencies above); the AppImage
  bundles the app, not the system's webkit.
- **Linux: menu entry missing** — run the installer once more; it rewrites
  `~/.local/share/applications/halftone.desktop`. Some desktops need a cache
  refresh (`update-desktop-database ~/.local/share/applications`).
- **Checksums**: every release asset has a sibling `<asset>.sha256`
  (`sha256sum` format). Verify manually with `sha256sum -c Halftone.exe.sha256`
  (Windows: `Get-FileHash`).
