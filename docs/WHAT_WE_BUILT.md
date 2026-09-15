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

## Known limits / next ideas

- lrclib auto-fetch of `.lrc` files (button currently opens their search page)
- tray icon / menubar presence for the widget
- drag-and-drop folder scan
- WSOL/ALAC format support (FLAC-only today)
- the sync tick is event-driven: position drift is corrected only when it exceeds ~1.5s
