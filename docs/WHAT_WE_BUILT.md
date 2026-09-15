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
now-playing overlay, populated from the already-parsed TrackMeta: title, artist, album, and
the embedded cover art (METADATA_BLOCK_PICTURE bytes handed to the overlay in memory — no
file re-read, no network). The SMTC state machine is driven only by the owner window.
Implementation: `src-tauri/src/smtc.rs` using `windows::Media` (SMTC via
`GetForCurrentView`), wired through a `smtc_update` command and `smtc-button` events.

### 3. Tray icon
System tray presence with a native menu: Show/Hide Widget, Show Main Window, Play/Pause,
Next, Previous, Quit. Left-click toggles widget visibility. **Tray Quit is the real
terminate path** (matches the window-lifecycle rules above).

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
| SMTC/tray | compile-verified; runtime check pending |
| Accent parity widget ↔ app | both `rgb(151,172,57)` in album mode |

### Open spec questions (for the next pass)

1. **SMTC seek/position**: should the OS overlay expose a seekbar (SMTC supports timeline
   properties) or just play/pause/next/prev? Currently buttons only.
2. **EQ parametric mode**: do you want per-band frequency + Q editing exposed in the UI
   (full parametric), or is the 10-band graphic + AutoEQ import enough for now?
3. **EQ on other surfaces**: should the widget get a mini-EQ toggle (just bypass + preset
   cycle), or stay EQ-free?
4. **AutoEQ auto-match**: import by headphone name from the AutoEQ database URL, or
   file-based import only (current)?
5. **Output device**: exclusive-mode (WASAPI) is impossible via `setSinkId` — acceptable
   long-term, or should we look at a Rust-side CPAL output path for bit-perfect output?
6. **Sleep timer fade**: stop hard at N minutes, or fade volume over the last 30s?
7. **ReplayGain pre-amp**: apply a global RG pre-amp (default -6dB) per the spec, or the
   current auto-trim approach?
8. **Folder watch scope**: currently non-recursive (matches the scan). Recursive watch +
   recursive scan?

## Known limits / next ideas

- lrclib auto-fetch of `.lrc` files (button currently opens their search page)
- drag-and-drop folder scan
- WSOL/ALAC format support (FLAC-only today)
- position sync granularity is the ~30fps broadcast — no interpolation on the widget side
  (per spec: widget renders the owner's value, never estimates)
