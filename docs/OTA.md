# Halftone OTA self-update

How the Windows app updates itself in place: check → download → verify →
stage → swap on restart. Linux installs are excluded (no OTA; the commands
report `supported: false`).

## The update channel

The app fetches `latest.json` from the **latest** release
(`https://github.com/Trapston3/Halftone/releases/latest/download/latest.json`)
and compares `version` against its own (`src-tauri/tauri.conf.json` /
`CARGO_PKG_VERSION`, kept in lockstep). QA overrides: `HALFTONE_OTA_URL`
(custom channel URL) and `HALFTONE_NO_OTA=1` (disable the check entirely —
dev builds set nothing; the UI shows "OTA DISABLED (DEV BUILD)" only when
the env var is present).

### latest.json schema

```jsonc
{
  "version": "0.2.0",              // semver, leading "v" tolerated on compare
  "url": "https://github.com/Trapston3/Halftone/releases/download/v0.2.0/Halftone.exe",
                                   // TAGGED, immutable URL — never /latest/download/
  "sha256": "<hex of the exe>",    // optional; verified when present
  "size": 9438208,                 // optional bytes; verified when present
  "notes": "..."                   // optional, surfaced to release tooling only
}
```

Rules the updater enforces:

- **Upgrade only.** `remote > current` by semver (crate `semver`). Equal or
  older versions never trigger an offer — no downgrades, no re-update loops.
  Unparseable remote versions are treated as "no update".
- **Integrity before staging.** The download must start with the `MZ` PE
  magic, must match `size` when present, and must match `sha256` when
  present (hex case-insensitive). Old manifests without `sha256`/`size` are
  still accepted; the MZ check always applies. A failed check deletes the
  staged bytes and reports an error — a corrupt file never lands next to
  the exe.
- **Atomic stage.** Bytes are written to `Halftone.update.download.tmp`
  next to the running exe, then renamed to `Halftone.update.next`. A crash
  mid-download can never leave a half-written stage file that looks ready.

## End-to-end flow

1. **check** (`ota_check`) — fetches latest.json, semver-compares, returns
   the `OtaStatus` JSON (see docs/BACKEND_API.md). A staged file already
   next to the exe short-circuits to `ready: true`.
2. **download** (`ota_download`) — fetches `url`, verifies MZ + size +
   sha256, stages atomically as `Halftone.update.next` beside the running
   exe. Returns `ready: true` / `error` in the same `OtaStatus` shape.
3. **apply** (`ota_apply`) — re-opens the staged file (MZ check again), then
   writes the helper batch file `Halftone.update.cmd` beside the exe and
   spawns it detached (`CREATE_NO_WINDOW`); the app exits immediately.

The helper script (Windows-only, generated at apply time):

```bat
:: retry loop — ping is a delay that works without a console;
:: timeout /T aborts instantly under CREATE_NO_WINDOW ("Input redirection
:: is not supported"), which is exactly the bug this replaced.
:retry
ping -n 2 127.0.0.1 >NUL
copy /Y "<exe>\Halftone.update.next" "<exe>" >NUL 2>&1
if errorlevel 1 ( retry up to ~20 rounds ≈ 20-40 s, then :fail )
del /Q staged file
echo ... > "Halftone.update.log"
start "" "<exe>"          :: relaunch the NEW exe
del /Q "%~f0"             :: self-delete the helper
```

On final failure (`:fail`) the helper **keeps the staged file**, relaunches
the **old** exe, writes the reason to the log and exits 1 — the user keeps
a working player and the next `ota_apply` can retry the swap.

### Log location

`Halftone.update.log`, in the same directory as `Halftone.exe` (next to
`Halftone.update.next` and `Halftone.update.cmd`).

## Failure modes

| Symptom | Log / signal | Recovery |
|---|---|---|
| Channel 404 / bad JSON | `ota_check` error field | UI shows RETRY; check the release has `latest.json` as an asset |
| sha256 / size mismatch | "sha256 mismatch"/"size mismatch" in `ota_download` error | Stage deleted; re-check picks the manifest up again once the release is fixed |
| exe locked by AV / slow exit | copy fails in helper → logged, retries ~20× (~40 s) | old exe relaunches, staged file kept; next `ota_apply` retries |
| Download truncated | size check (when present) or too-small guard | error, nothing staged |
| Update staged but never applied | `ready: true` on every `ota_check` | press INSTALL + RESTART again (the UI's boot check also surfaces this) |

## Releasing (tools/release.py)

Prereq: bump the version in **both** `src-tauri/tauri.conf.json` and
`src-tauri/Cargo.toml` (release.py refuses a mismatch), build the Windows
exe (NSIS bundle target produces `Halftone.exe`; the portable exe is what
OTA ships).

```bash
python3 tools/release.py --version 0.2.0 \
    --exe <built>/Halftone.exe \
    [--appimage <built>/Halftone_amd64.AppImage] [--deb <built>/Halftone_amd64.deb] \
    --notes "..."
```

- Stages into `dist/` under the **asset naming contract** (stable names —
  installers and OTA URLs depend on them, do not deviate):
  - `Halftone.exe` (Windows; OTA's `url` points at this exact name forever)
  - `Halftone_amd64.AppImage`, `Halftone_amd64.deb` (Linux, optional)
  - `latest.json` (written by the tool: version, tagged URL, sha256, size, notes)
- Prints the exact `gh release create v<ver> dist/* --repo Trapston3/Halftone
  --title ... --notes-file ...` command — the CI workflow runs it; locally
  the script never uploads anything.
- Guard rails: refuses to run when `--version` ≠ `tauri.conf.json` version ≠
  `Cargo.toml` version, or when a named artifact is missing.

Upload **all** `dist/*` files to the release — `latest.json` must be an
asset of the same release as the exe, or clients resolve a version whose
manifest points nowhere (the original v0.1.2 failure).

## Data compatibility across an upgrade (verified by code reading)

The v0.1.2 → v0.2.0 OTA swap replaces only `Halftone.exe`; every
user-facing data location is derived from the **bundle identifier**
`com.trapston3.halftone`, which is unchanged:

- **WebView2 profile / localStorage** — Tauri 2 forces the WebView2
  `data_directory` to `%LOCALAPPDATA%\<identifier>` when the app doesn't
  set one (tauri-2.11.5 `src/manager/webview.rs`, "in `windows`, we need to
  force a data_directory"; wry-0.55.1 passes it to
  `CreateCoreWebView2EnvironmentWithOptions`). Same identifier → same
  `EBWebView` folder → `localStorage["halftone.store"]` (ui/common.js)
  survives the update. The v0.2 foundation migrates those settings into
  `settings.json` on first run (settings_load / settings_save commands).
- **App config** — `app_config_dir()` = `%APPDATA%\<identifier>`
  (`dirs::config_dir()` + identifier; tauri-2.11.5 `src/path/desktop.rs`) →
  `settings.json` path unchanged.
- **App data** — `app_data_dir()` = `%APPDATA%\<identifier>` →
  `scan_index.json` unchanged.

Because the OTA swap never deletes anything but its own `.next`/`.cmd`
files, an updated binary starts with the exact profile the old one had.
