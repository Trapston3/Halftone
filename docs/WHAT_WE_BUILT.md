# HALFTONE — What We Built

A FOSS floating music widget for your local library, grown into a two-surface desktop player.
Tauri 2 + vanilla JS, no frameworks, no transcode, no cloud. Everything below is verified on the
release exe via Chrome DevTools Protocol (screenshots + `__qa` probes), not just written.

## The short version

Halftone is a small always-on-top widget (400×254, resizable) plus a full player window
(1200×760) that behave as **two views of one player**. Point it at a folder of FLAC files and
it reads everything straight from disk — original audio bytes, embedded tags, embedded cover
art, and `.lrc` synced lyrics sidecars. Nothing is copied or re-encoded.

## Architecture

```
src-tauri/src/lib.rs     Rust backend, zero audio crates
  ├─ scan_library(dir)   header-only FLAC walk (8 MiB window, full-read fallback)
  ├─ open_track(path)    STREAMINFO + VORBIS_COMMENT + PICTURE in one pass
  ├─ read_lyrics(path)   .lrc sidecar parser (multi-timestamp lines, sorted, deduped)
  └─ flac:// protocol    serves ORIGINAL bytes with Accept-Ranges → seek without transcode

ui/common.js             shared engine: accent system, Bayer ditherers, LED meters,
                         transport, collections, cross-window sync protocol
ui/index.html            the widget (floating mini-player)
ui/main.html             the full app (library + now playing)
ui/style.css             one design system for both surfaces
tools/install.py         installs to %LOCALAPPDATA%\Halftone, PATH + App Paths + shortcuts
tools/cdp_verify*.py     live verification against the release exe
```

The frontend talks to Tauri through `window.__TAURI__` (withGlobalTauri). Audio is a plain
`<audio>` element fed by the custom `flac://` scheme; a WebAudio analyser taps it for the
spectrum-reactive visuals; a gain node handles volume.

## Feature list (all live)

### Player core
- Direct FLAC playback — original bytes via `flac://` range requests, gapless-free, no transcode
- Library scan of a folder (95-track test lib), skipped-file reporting
- Transport: play/pause, prev (restart-if->3s), next, shuffle, repeat off/all/one
- Seek bar with hover-expand, illumination sweep drag physics, wheel = ±5s scrub
- LED volume (click + arrow keys + **scroll wheel anywhere**)
- Liked tracks, playlists (create from any track), search across title/artist/album

### The two surfaces, synced
- Single-player protocol: play-takeover ensures only one window ever sounds
- State sync (`halftone:sync`): track index, play/pause, position (drift-corrected beyond 1.5s),
  volume, shuffle, repeat, and the accent theme — change anything in either window and the
  other follows within a beat
- Widget controls the app and vice versa; both show the same song at the same position

### Theming
- Accent color extracted from the album art (hue-bucketed, contrast-clamped to ≥3:1 on bg)
- Smooth tweened transitions between accents; dither art repaints DURING the tween so colors
  never lag behind
- ACCENT menu: follow the album, or lock to mint / sky / violet / rose / amber / red
- One shared theme across both windows (menu in either window changes both)

### Dither / art
- Bayer-dither album art in the accent color, three densities (standard/fine/ultra)
- Real-photo mode for the actual cover (fixed: the dither canvas no longer overlays it)
- Perimeter dither edge on both windows, album-sampled
- Perimeter dither edge + scanline overlay = the TUI signature look

### Lyrics
- `.lrc` sidecar parsing (multiple timestamps per line supported)
- Widget: collapsible 168px lyric pane, active line follows playback
- App: 28px lyric pane with NATIVE scrolling, smooth auto-follow centered on the active line,
  wheel-scroll pauses the follow, clicking a line seeks AND resumes the follow
- No-lrc state offers a lrclib.net search link (opens the default browser via the opener
  plugin — the earlier "blank blue page" was the WebView's own about:blank being opened)
- Lyrics can be disabled entirely (LYRICS > HIDE): album art grows to a centered hero,
  transport centered beneath it

### Ambient backgrounds (both eyes-safe, both selectable in the menu)
- DITHER BLOOM — Bayer cells drifting with the music's spectrum energy (the original)
- HALO GLOW — soft radial light blobs breathing with the music + sparse dither veil
  (replaces nothing; added for people who found the bloom busy)
- OFF — clean background

### QoL
- Collapsible sidebar in the app (hamburger in the top bar, remembered across restarts)
- Widget ↔ app toggle button in the app's top-right (shows/hides the widget)
- Widget's right-click configurator is a NATIVE OS menu — it renders outside the window,
  ending the old clipping problem; HTML fallback kept if the bridge is missing
- Widget UI scales with the window (zoom 0.7–2.4), drag zones, pin always-on-top
- Window controls: widget close ends the app; app close just hides the window
- Keyboard: space play/pause, arrows seek, volume LEDs respond to arrows when focused
- Right-click context menus with submenus in both windows

### Install & verify
- `tools/install.py` → `%LOCALAPPDATA%\Halftone\Halftone.exe` (+ PATH, App Paths for
  `Win+R halftone`, Desktop + Start Menu shortcuts)
- Every feature above was verified by driving the installed exe over CDP: state probes,
  synthetic wheel/drag events, and screenshots read back to confirm what actually rendered

## Verification highlights (this pass)

| Check | Result |
| --- | --- |
| Track change in app → widget follows | i=10 → widget i=10 |
| Track change in widget → app follows | i=20 → app i=20 |
| Play in app → widget playing | ✓ |
| Pause in widget → app paused | ✓ |
| Volume wheel in widget → app volume | 0.20 → 0.25 mirrored |
| Accent set in app → widget RGB | both `rgb(81,159,214)` (sky) |
| Progress drift correction | app 42.0s, widget 41.9s |
| Lyrics off → single-column centered hero | gridCols 1, lyrics pane hidden |
| Halo ambient lit ratio | 0.72 (visible, soft) |
| Dither ambient lit ratio | 0.63 |
| Real-art mode shows photo, hides dither canvas | ✓ |
| Widget native menu opens/closes cleanly | promise resolves on close |

## Pass: audio ownership + native integration + EQ + output (2026-09-15, commit c04b71c)

### 1. Single audio owner (structural fix for drift)
The **main window is the permanent, sole audio owner** — the only window that constructs an
`<audio>` element, AudioContext, gain node or AnalyserNode. The widget is a pure view +
controller: it sends `halftone:cmd` messages (play/pause/next/prev/seek/nudge/volume/load/scan/
like/playlist) and renders what the owner broadcasts. It never decodes, never taps an analyser,
never estimates position.

Owner broadcasts:
- `halftone:sync` — authoritative full state (track, meta, lyrics, library, likes, playlists,
  volume, shuffle, repeat, accent, EQ)
- `halftone:time` — current position, ~30fps
- `halftone:bars` — the 48 analyser bars quantized to one byte each, ~30fps (via `setInterval`,
  which stays live while the window is hidden because audible audio exempts the page from
  timer throttling)

**Deleted:** the drift-correction logic and the play-takeover protocol — with exactly one
decoder they are structurally meaningless, and leaving them would mask real bugs.

Window lifecycle is load-bearing: hiding a window does not destroy its WebView, so the owner
keeps playing while invisible (widget-only workflow verified: position advanced, bars animated,
with main hidden). Main's close button only hides; only the widget's close or tray Quit ends
the process. This dependency is documented in code comments in both `lib.rs` and `main.html`.

Verified: owner/viewer identity probes (`__qa.owner()`), widget→owner commands, position
readouts identical at broadcast resolution, zero AudioContext construction in the widget's JS
context (`__qa.owner().hasAC === false`, no `<audio>` in widget DOM).

### 2. Windows SMTC
Global media keys (play/pause/next/prev from keyboards, headsets, Bluetooth) + the OS
now-playing overlay, showing title, artist, album and the embedded cover art
(METADATA_BLOCK_PICTURE bytes — no file re-read, no network).

**Architecture note (Phase 0 correction):** the first implementation used a Rust-side SMTC
session via `ISystemMediaTransportControlsInterop::GetForWindow`. It compiled and ran but was
a zombie — WebView2 already owns the OS media session for the process (it auto-exposes the
owner's `<audio>` element) and the two sessions fought; the overlay kept showing the window
title with empty tags. The Rust module was removed. The real mechanism is the
**`navigator.mediaSession` API in the owner page** (`ui/main.html`): metadata + artwork +
playbackState + `setPositionState` (OS seekbar) + action handlers (play/pause/prev/next/
seekbackward/seekforward/seekto). Tray menu and OS media keys land in the same owner window
via the `smtc-button` Tauri event.

**Runtime verification — machine-checked, not claimed** (`tools/smtc_probe.py`, WinRT
GlobalSystemMediaTransportControlsSessionManager via `py -3.10` + winsdk):
- Session exists, source `msedgewebview2.exe`, title/artist correct ("Harvey" / "Her's")
- Status follows the owner: PLAYING after play, PAUSED after pause
- Track change reflected ("Chest Pain (I Love)")
- Timeline `end` matches track duration (211.2s); position tracks via setPositionState
Not machine-verifiable from script: the overlay's *visual* rendering (OS-drawn) — confirmed by
the probe reading the same data the overlay draws, but a human glance at Win+G / volume HUD is
the final word.

### 3. Tray icon
System tray presence with a native menu: Show/Hide Widget, Show Main Window, Play/Pause,
Next, Previous, Quit. Left-click toggles widget visibility. **Tray Quit is the real
terminate path** (matches the window-lifecycle rules above).
Verified: menu-item actions and left-click route through `on_menu_event` /
`on_tray_icon_event`, whose `smtc-button` emit path is machine-verified end-to-end (toggling
flips playback both ways). The icon's on-screen presence and the Quit terminate path are wired
but not scriptable from CDP — visually confirm on first run; not marked machine-verified.

### 4. Parametric / graphic EQ
10-band chain (31 Hz lowshelf → 16 kHz highshelf, peaking in between) inserted into the
single owner graph between source and analyser. UI is a sheet over the now-playing ambient
in the established dither/LED language:

- Band sliders are **vertical LED ladders** — 12 segments of 2 dB, same segment rhythm as
  the volume LEDs and spectrum bars; drag, click or scroll-wheel (0.5 dB fine trim)
- Separate pre-amp as a **horizontal LED strip** (center = 0 dB)
- Clipping protection: positive sum of pre-amp + ReplayGain + boosted bands is auto-trimmed
  and shown as `TRIM -x.xdB` next to the pre-amp
- True bypass: toggling off physically disconnects the filter chain (`eqIn`/`eqOut` null),
  verified via graph probes — filters are not merely zeroed
- Presets: 7 built-ins (FLAT/ROCK/POP/JAZZ/VOCAL/ELECTRONIC/ACOUSTIC) + user
  save/load/delete (localStorage), named and marked with `*` in the dropdown
- **AutoEQ import**: parse the AutoEQ database's `ParametricEQ.txt` (Preamp line + per-filter
  `Filter N: ON <LSC|PK|HSC> Fc f Hz Gain g dB Q q` lines), map each filter onto the nearest
  band, apply pre-amp, show the imported profile name in the sheet. This gives the entire
  AutoEQ headphone database for free.

### 5. Output device selection
`navigator.mediaDevices.enumerateDevices()` + `setSinkId()` on the owner's `<audio>` element.
Persisted in `cfg.sink`; hot-plug handled via `devicechange` (falls back to system default if
the saved device disappears). Honest limitation shown in the UI: **routes through the Windows
shared mixer — not exclusive-mode output**. Also only reachable on the owner window.

### 6. Smaller wins
- **ReplayGain**: REPLAYGAIN_TRACK_GAIN / REPLAYGAIN_ALBUM_GAIN parsed from VORBIS_COMMENT
  in the same single read (zero extra I/O). Mode toggle off/track/album, applied at the
  pre-amp, visible in the EQ sheet readout.
- **Folder watching**: the backend watches the library folder (notify crate); any change
  triggers a debounced auto-rescan and re-broadcast — no manual SCAN needed after edits.
- **Sleep timer**: 15/30/60 min, end-of-track, or off, with a live countdown readout.

### This pass's verification table

| Check | Result |
| --- | --- |
| Owner identity (main) | `owner:true`, has `<audio>` |
| Viewer identity (widget) | `owner:false`, no `<audio>`, no AC |
| Track change in owner → widget | meta + lyrics + library + cover arrive via sync |
| Widget command → owner | play/pause/seek/vol/load all applied |
| Position readouts | identical at broadcast resolution |
| Spectrum in widget while main hidden | bars animate (owner pump lives) |
| Playback continues with main hidden | yes (hide ≠ destroy) |
| EQ bypass | filters disconnected, not zeroed |
| EQ boost/cut | chain responds (graph probes) |
| AutoEQ import | profile name shown, bands mapped |
| Sink enumeration | works; persisted; hotplug fallback |
| SMTC metadata (title/artist) | machine-verified via winsdk probe ("Harvey"/"Her's") |
| SMTC status PLAYING/PAUSED | machine-verified, follows owner |
| SMTC track change | machine-verified ("Chest Pain (I Love)") |
| SMTC timeline end | machine-verified (211.2s = track duration) |
| Tray smtc-button channel | machine-verified end-to-end (toggles playback) |
| Tray icon visual presence / Quit | wired; visual-confirm only (not scriptable) |
| Accent parity widget ↔ app | both `rgb(151,172,57)` in album mode |

### Phase 0 hardening (commit 4b86404)

SMTC and tray were promoted from "compile-verified; runtime check pending" to runtime-verified
(see section 2 above). The Rust `smtc.rs` interop approach was **deleted** after runtime probing
showed it produced a zombie session that never reached the overlay — WebView2's built-in media
session bridge, driven by `navigator.mediaSession` in the owner page, is the correct and now
verified mechanism. A reusable probe (`tools/smtc_probe.py`) reads the real OS session state so
this stays verifiable.

### Open spec questions — status after Phase 6 decisions

1. **SMTC seek/position**: ✅ DECIDED + DONE — OS overlay now exposes a working seekbar
   (`setPositionState` + `seekto` handler), not just buttons.
2. **EQ parametric mode**: DECIDED NO for now — 10-band graphic + AutoEQ import is the scope.
3. **EQ on other surfaces**: DECIDED — widget gets bypass toggle + preset cycle ONLY, no full EQ.
4. **AutoEQ auto-match**: DECIDED NO — file-based import only.
5. **Output device**: DECIDED — WASAPI exclusive NOT this pass; README documents the
   shared-mixer limitation as a known limitation + roadmap item.
6. **Sleep timer fade**: DECIDED — fade volume over the final 30 seconds (not a hard stop).
7. **ReplayGain pre-amp**: DECIDED — apply the standard -6dB global RG pre-amp per spec,
   keeping the existing auto-trim clipping protection on top.
8. **Folder watch scope**: DECIDED — recursive watch + recursive scan (folded into Phase 2).

## Hardening pass — IN PROGRESS (phases 1–6 of 6 open)

Phase 0 (verification gap) is closed and committed `4b86404`; see sections 2–3 above. Phases
1–6 (robustness, scale, first-run, distribution, accessibility, and the settled spec decisions
listed above) are being implemented phase-by-phase, each with a hard CDP gate before the next
begins. Status will be recorded here as gates pass.

## Pass: folder picker + OTA self-update (2026-09-16, v0.1.1)

Two distribution/UX gaps closed in one pass:

1. **Native folder picker** — `pick_folder` (rfd) command + a folder icon button next to every
   library path input (main sidebar, main empty-state, widget settings). Picking a folder fills
   the input and scans immediately; pasting "Copy as path" quoted paths also works now
   (`sanitize_dir` strips Explorer's quotes and trailing slashes before scanning).

2. **OTA self-update channel** — Halftone checks `latest.json` (a release asset) and can update
   itself: `ota_check` (compare versions), `ota_download` (blocking, staged as
   `Halftone.update.next` next to the exe, MZ-magic sanity check), `ota_apply` (detached helper
   waits for exit, copies, relaunches). UI lives in the EQ/OUTPUT panel as an UPDATES row:
   CHECK → DOWNLOAD → INSTALL + RESTART, with staged-update state painted at boot. Override
   with `HALFTONE_OTA_URL`, disable with `HALFTONE_NO_OTA`. No admin, no installer — the exe
   replaces itself in place.

## Pass: LRCLIB auto-fetch + drag-and-drop scan (2026-09-16, v0.1.2)

The last two items from the backlog, closed in one pass:

1. **LRCLIB auto-fetch** — GET LYRICS no longer just opens a browser tab. A new `lrc_fetch`
   Tauri command queries `lrclib.net/api/search` (artist_name/track_name first, generic `q=`
   fallback), picks the first result with non-empty `syncedLyrics` (exact title match
   preferred), and writes `<track>.lrc` next to the FLAC. The lyric pane rebuilds and follows
   immediately; the widget gets the pane via the normal sync (viewer `GET LYRICS` forwards an
   `lrcfetch` command to the owner — single-audio/single-writer discipline holds: only the
   owner hits the network and writes files). The browser tab still opens so you can eyeball
   the source. Verified live: The Cure — "Burn" fetched 45 lines, pane live in BOTH windows,
   file on disk next to the FLAC.

2. **Drag-and-drop folder scan** — drop an audio file (or a selection) anywhere on the main
   window or the widget: the dropped file's parent folder fills the library path and a scan
   fires. `wireDropZone` in common.js, attached to `document.body` (main) and `#widget`.
   Verified via synthetic DragEvents (dragover prevented → drop → scan ran, 95-track lib
   re-pointed). Real Explorer drops work because both windows set `dragDropEnabled: false` —
   WebView2's native drag-DOM interception is off, so HTML5 drag events reach the page.

## Features completed in v0.1.1

- Native folder picker — `pick_folder` (rfd) command + folder icon button next to every library path input (main sidebar, main empty-state, widget settings). Picking a folder fills the input and scans immediately; pasting "Copy as path" quoted paths also works now (`sanitize_dir` strips Explorer's quotes and trailing slashes before scanning).

## Pass: LRCLIB auto-fetch + drag-and-drop scan (2026-09-16, v0.1.2)

The last two items from the backlog, closed in one pass:

1. **LRCLIB auto-fetch** — GET LYRICS no longer just opens a browser tab. A new `lrc_fetch`
   Tauri command queries `lrclib.net/api/search` (artist_name/track_name first, generic `q=`
   fallback), picks the first result with non-empty `syncedLyrics` (exact title match
   preferred), and writes `<track>.lrc` next to the FLAC. The lyric pane rebuilds and follows
   immediately; the widget gets the pane via the normal sync (viewer `GET LYRICS` forwards an
   `lrcfetch` command to the owner — single-audio/single-writer discipline holds: only the
   owner hits the network and writes files). The browser tab still opens so you can eyeball
   the source. Verified live: The Cure — "Burn" fetched 45 lines, pane live in BOTH windows,
   file on disk next to the FLAC.

2. **Drag-and-drop folder scan** — drop an audio file (or a selection) anywhere on the main
   window or the widget: the dropped file's parent folder fills the library path and a scan
   fires. `wireDropZone` in common.js, attached to `document.body` (main) and `#widget`.
   Verified via synthetic DragEvents (dragover prevented → drop → scan ran, 95-track lib
   re-pointed). Real Explorer drops work because both windows set `dragDropEnabled: false` —
   WebView2's native drag-DOM interception is off, so HTML5 drag events reach the page.

## Known limits / next ideas

- WSOL/ALAC format support (FLAC-only today)
- position sync granularity is the ~30fps broadcast — no interpolation on the widget side
  (per spec: widget renders the owner's value, never estimates)
- WASAPI exclusive / bit-perfect output — deliberately out of scope (shared mixer via setSinkId)


---

## Pass: Halftone v1.1 Spec Rebuild (2026-10-25, branch `spec-rebuild`)

Complete architectural and visual upgrade implementing the **Halftone v1.1 Build Specification** across the Rust backend (`src-tauri/src/`) and both frontend surfaces (`ui/main.html` + `ui/index.html`).

### 1. Rust Backend v1.1 (`src-tauri/src/`)
- **Multi-Format Audio Engine (`formats.rs`, `probe.rs`, `lib.rs`)**:
  - Magic-byte detection and metadata/artwork probing for **FLAC, MP3, M4A/AAC, ALAC, WAV, OGG Vorbis, Opus, AIFF (`FORM....AIFF`/`AIFC`), and WavPack (`wvpk`)**.
  - Canonicalized library-root allowlist enforcement in `serve_media` (`media://localhost/stream/<path>` and `media://localhost/thumb/<path>`) blocking path traversal (`..`).
- **Cached 256×256 Artwork Thumbnails & OKLab/OKLCH Palette Extraction (`art_thumb.rs`)**:
  - `art_thumb` Tauri command decodes embedded cover art, resizes to 256×256 via Lanczos3, caches PNGs under `%APPDATA%\Halftone\thumbs\<hash>.png`, and extracts chroma-weighted dominant + secondary colors in OKLab/OKLCH with automatic WCAG AA lightness clamping ($\ge 4.5:1$).
- **Single-Pass Audio Analysis & Integrity Verification (`analyze_track.rs`)**:
  - `analyze_track` Tauri command decodes audio via Symphonia to compute **128-bucket normalized waveform peaks**, **BS.1770-4 / EBU R128 integrated loudness (`LUFS`)**, **true peak (`dBTP`)**, **ReplayGain track/album gain**, **dynamic range (DR)**, **MP3 LAME encoder delay/padding** for gapless playback, and a **high-frequency spectral energy ratio detector** that flags transcode/fake-lossless files.
- **Persistent Library Database, Smart Playlists & Listening Stats (`db.rs`)**:
  - Stable Track IDs (`size` + head/tail 16 KiB FNV-1a fingerprint) that survive file renames/moves.
  - Non-destructive **Metadata Tag Overlay** (`db_set_overlay` / `db_get_overlays`) so users can edit Title, Artist, Album, Year, Genre, and Rating ($0\text{–}5$) without mutating read-only audio files.
  - **Smart Playlists** (`db_eval_smart_playlist`) with JSON rule evaluation (`artist`, `album`, `genre`, `format`, `year`, `plays`, `rating`, `lossless`) plus built-in presets (*Lossless Reference*, *Heavy Rotation*, *Recently Added*, *Top Rated*).
  - **M3U / M3U8 Import & Export** (`db_export_m3u`, `db_import_m3u`).
  - **Library Health Scanner & Auto-Relinker** (`db_get_library_health`, `db_relink_missing`) detecting missing paths, duplicates by track hash, tracks missing artwork, and suspect fake-lossless files.
  - **Listening Statistics** (`db_get_listening_stats`) tracking total plays, listening hours, 24-hour and 7-weekday heatmaps, daily streaks, and top tracks/artists.
  - **Portable Mode** (`halftone.portable` marker next to executable redirects all state to `./portable_data/`) and single-file JSON database backup/restore (`db_export_bundle`, `db_import_bundle`).
- **Word-Synced Karaoke Lyrics & Offset Persistence (`lib.rs`)**:
  - Enhanced LRC (`<mm:ss.xx>word`), TTML (`<p begin="..." end="...">`), and `[offset:±ms]` support, with syllable-weighted word timing synthesis (`LyricWord { w, t0, t1 }`) and `lyrics_save_lrc` offset persistence.

### 2. Dual-Theme Design System & Frontend v1.1 (`ui/`)
- **Analogue + Digital Themes × 7 Palettes × Light/Dark (`ui/themes/`, `ui/theme.js`)**:
  - **Analogue**: Bayer ordered dither (`standard`, `fine`, `ultra`, `halftone-dot`), segmented LED meters, 0px corners, hairline grids.
  - **Digital**: Liquid glass (`backdrop-filter: blur(28px) saturate(180%)`), WebGL2 liquid-mesh ambient shader (`ui/ambient-webgl.js`), rounded cards, and physical spring choreography (`ui/motion.js`).
  - **7 Curated Palettes**: `classic`, `catppuccin`, `tokyo-night`, `gruvbox`, `nord`, `rose-pine`, `dracula` + dynamic `album` OKLCH palette.
- **Custom Geometric Icon Sprite (`ui/icons.svg`)**:
  - 34 custom icons on a 24×24 grid (`stroke-width="1.75"`, zero external icon libraries, zero emojis).
- **Rotatable Module Layout Studio (`ui/layout.js`)**:
  - Interactive layout editor (`#wLayoutBtn` in widget, `#btnLayout` in main window) with per-module rotation (`0°`, `90°`, `180°`, `270°`), orientation-aware flex/grid adaptation (`data-orient="horizontal|vertical"`), visibility toggles, 4 built-in presets (`Classic Deck`, `Vertical Rack`, `Minimal Strip`, `Lyrics Focus`), and JSON import/export.
- **Audiophile Signal Path & Extended WebAudio DSP (`ui/main.html`)**:
  - **Signal Path Popover (`#btnSignal`)**: Live 5-stage visual chain (Source File $\rightarrow$ Symphonia/WebAudio Decode $\rightarrow$ 10-Band EQ & Crossfeed DSP $\rightarrow$ ReplayGain/Preamp Trim $\rightarrow$ Shared WASAPI Output Sink).
  - **Extended WebAudio DSP**: Stereo Balance (`StereoPannerNode`), Mono Fold-Down, Equal-Loudness Contour (Fletcher-Munson low/high shelf compensation), and Jan Meier Headphone Crossfeed (`700 Hz` low-pass + `280 µs` interaural delay).
- **Word-Lit Spring Karaoke Lyrics (`ui/common.js`, `ui/main.html`)**:
  - Per-word `.lyr-word` spans with `--w-prog` illumination sweep and `--ease-spring-snappy` vertical lift on the currently sung word.

### 3. v1.1 Release-Build Verification Summary (`tools/cdp_verify_v11.py`)

| Check | Result |
| --- | --- |
| Rust Unit Test Suite (`cargo test`) | **51 passed, 0 failed** |
| WCAG 2.1 AA Contrast Suite (`node ui/test/test-contrast.js`) | **ALL PAIRS PASS (0 failures)** |
| Check 1: Single Audio Owner Invariant | `main.owner=true`, `widget.owner=false`, `widget.hasAC=false` |
| Check 2: Theme × Mode × 7 Palettes Sync | All 7 palettes + Light/Dark modes sync across both windows |
| Check 3: Custom SVG Icon Sprite (`ui/icons.svg`) | 34 symbols mounted in both windows |
| Check 4: Spring Motion System (`ui/motion.js`) | `--ease-spring-snappy`, `--ease-spring-gentle`, `--ease-spring-bouncy` active |
| Check 5: Rotatable Layout Studio (`ui/layout.js`) | `0° -> 90° -> 180° -> 270° -> 0°` + presets verified in both windows |
| Check 6: Multi-Format Audio Scan | FLAC, WAV, AIFF, WavPack scanned and indexed cleanly |
| Check 7: `art_thumb` 256×256 Cache & OKLCH Palette | Thumbnail generated + WCAG-clamped hex colors returned |
| Check 8: `analyze_track` Waveform + BS.1770 LUFS + Integrity | 128 waveform peaks, `-9.0 LUFS`, `isSuspectLossless` verified |
| Check 9: Word-Synced Karaoke Lyrics & Offset Save | `.lyr-word` spans rendered with syllable timings + `lyrics_save_lrc` verified |
| Check 10: Signal Path & Extended WebAudio DSP | 5-node signal chain + Balance/Mono/Loudness/Crossfeed verified |
| Check 11: Main Window v1.1 Views | Cover Flow, Artists, Genres, Folders, Smart Mix, Stats, Health verified |
| Check 12: Non-Destructive Tag Overlay & M3U Export | `db_set_overlay` + `db_export_m3u` / `db_import_m3u` verified |
| Check 13: Path Traversal Security Guard | `media://localhost/stream/../../Windows/win.ini` blocked |
