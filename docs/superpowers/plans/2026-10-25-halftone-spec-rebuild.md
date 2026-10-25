# Halftone v1.1 Spec Rebuild — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild and upgrade the Halftone repository on branch `spec-rebuild` to achieve 100% compliance with `Halftone — Build Specification.md` (Version 1.1), verified against a compiled release binary (`src-tauri/target/release/halftone.exe`) via CDP and OS-level probes.

**Architecture:** Dual-window Tauri 2 (Rust + WebView2) desktop application where `ui/main.html` is the sole `<audio>` + WebAudio DSP owner and `ui/index.html` is a pure modular viewer/controller. Two complete visual themes (`analogue` and `digital`) × Light/Dark modes × 7 curated palettes share semantic tokens in `ui/themes/*.css`. Rust handles single-pass magic-byte probing (`probe.rs`), 256px thumbnails + OKLCH contrast-clamped accents (`art_thumb.rs`), waveform/LUFS/spectral analysis (`analyze_track.rs`), persistent stable-ID database + smart playlists + stats (`db.rs`), word-synced lyrics (`lyrics.rs`), SMTC (`mpris.rs`), and `SetWindowRgn` rounded corners (`lib.rs`).

**Tech Stack:** Rust 2021 (Tauri 2, Symphonia, Lofty, Notify, Reqwest, Windows-rs), Vanilla HTML5 / CSS3 Custom Properties / ES6 JavaScript, WebAudio API, WebGL2 + 2D Canvas, Chrome DevTools Protocol (CDP) Python verification suite.

---

## Task 1: Rust Backend v1.1 Modules (`art_thumb.rs`, `analyze_track.rs`, `db.rs`, `lyrics.rs`, `lib.rs`)

**Files:**
- Create: `src-tauri/src/art_thumb.rs`
- Create: `src-tauri/src/analyze_track.rs`
- Create: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/lyrics.rs`
- Modify: `src-tauri/src/formats.rs`
- Modify: `src-tauri/src/probe.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Implement `src-tauri/src/art_thumb.rs` with unit tests**
  - Downscale cover art bytes or image file to a `256×256` PNG thumbnail in `<app_data_dir>/covers/thumbs/<hash>.png`.
  - Compute dominant and secondary colors using chroma-weighted median-cut / histogram clustering in sRGB -> OKLab -> OKLCH.
  - Clamp OKLCH lightness (`L`) for both Light and Dark modes so contrast against `--bg` and `--accent-ink` meets WCAG AA (`>= 4.5:1` text, `>= 3.0:1` UI controls).
  - Expose `pub struct ArtThumbResult { pub thumb_url: String, pub thumb_path: String, pub accent_hex: String, pub accent_dark_hex: String, pub accent_light_hex: String, pub secondary_hex: String, pub oklch: [f32; 3], pub is_dark_art: bool }`.

- [ ] **Step 2: Implement `src-tauri/src/analyze_track.rs` with unit tests**
  - Decode audio stream (or fast strided packet decode for long tracks) via Symphonia / native readers to compute:
    - `peaks`: `Vec<f32>` (128 normalized `0.0..1.0` waveform amplitude buckets).
    - `integrated_lufs`: `f32` (K-weighted BS.1770 approximate integrated loudness in LUFS).
    - `true_peak_dbtp`: `f32` (peak sample in dBTP).
    - `replaygain_track_db`: `f32` (`-18.0 - integrated_lufs` or tag value if present).
    - `replaygain_album_db`: `Option<f32>`.
    - `spectral_cutoff_khz`: `f32` (high-frequency energy roll-off estimation using zero-crossing / high-band energy ratio to flag fake-lossless transcodes where lossless container has `< 16.5 kHz` cutoff).
    - `is_suspect_transcode`: `bool`.
  - Cache analysis results in `<app_data_dir>/analysis_cache.json` keyed by stable track ID.

- [ ] **Step 3: Implement `src-tauri/src/db.rs` with unit tests**
  - Compute **Stable Track IDs**: `format!("{:x}-{:x}", file_size, head_tail_fnv64_hash)` using the first 16 KiB and last 16 KiB of the file so renamed or moved files retain play counts, likes, playlists, and custom tag overlays.
  - Manage persistent store (`<app_data_dir>/halftone_db.json` or portable `<exe_dir>/portable_data/halftone_db.json` when `halftone.portable` exists next to the executable):
    - `overlays`: `HashMap<String, MetadataOverlay>` (non-destructive user edits for title, artist, album, year, genre, track_number, disc_number, bpm, rating, lyrics_offset_ms, custom_cover_path).
    - `stats`: `HashMap<String, TrackPlayStats>` (`play_count`, `skip_count`, `last_played_ms`, `total_listen_sec`, `liked`, `date_added_ms`) + `play_events: Vec<PlayEvent>` for hourly/daily heatmaps and streaks.
    - `playlists`: `Vec<UserPlaylist>` + M3U/M3U8 import (`import_m3u8`) and export (`export_m3u8`).
    - `smart_playlists`: `Vec<SmartPlaylistDef>` with JSON rule tree evaluator (`field`, `op`, `value`, `match_all`) supporting `artist`, `album`, `genre`, `year`, `format`, `sample_rate`, `bit_depth`, `bpm`, `play_count`, `last_played`, `date_added`, `liked`, `duration`.
    - `relink_missing_tracks`: Automatic relink by matching `stable_id` when files move across folders.
    - `library_health`: Report missing art, missing tags, duplicates (by stable ID or normalized title+artist+duration), corrupt/unreadable files, and suspect fake-lossless transcodes.
    - `export_db_bundle` / `import_db_bundle`: Single-file JSON backup/restore of playlists, smart playlists, overlays, stats, and layout/theme profiles.

- [ ] **Step 4: Upgrade `src-tauri/src/lyrics.rs`, `formats.rs`, `probe.rs`, and `lib.rs`**
  - Add `[offset:±ms]` support, enhanced word-synced LRC (`<mm:ss.xx>word`), TTML `<span begin="..." end="...">` word timing, and syllable-weighted word timing synthesis (`synthesize_word_timings`) so every synced lyric line provides per-word `{ text, start_ms, end_ms }` spans for Apple Music–style word-lit lyrics.
  - Add AIFF (`.aiff`/`.aif`, `FORM....AIFF`/`AIFC`) and WavPack (`.wv`, `wvpk`) detection in `formats.rs` and `probe.rs`, plus MP3 LAME header encoder delay/padding (`encoder_delay_samples`, `encoder_padding_samples`).
  - Harden `serve_media` in `lib.rs`: canonicalize requested path and verify it belongs to a registered library root or explicitly opened track allowlist (`ALLOWED_MEDIA_PATHS` / `ALLOWED_MEDIA_ROOTS`), and cap lossless fallback decode memory at `512 MiB`.
  - Register Tauri commands in `lib.rs`: `art_thumb`, `analyze_track`, `lyrics_get`, `lyrics_save_lrc`, `db_get_state`, `db_set_overlay`, `db_record_play`, `db_toggle_like`, `db_save_playlist`, `db_delete_playlist`, `db_import_m3u`, `db_export_m3u`, `db_save_smart_playlist`, `db_eval_smart_playlist`, `db_get_library_health`, `db_relink_track`, `db_get_listening_stats`, `db_export_bundle`, `db_import_bundle`.

## Task 2: Custom SVG Icon Sprite (`ui/icons.svg`) & Zero-Emoji Sweep

**Files:**
- Create: `ui/icons.svg`
- Modify: `ui/index.html`, `ui/main.html`, `ui/common.js`, `ui/widget.js`, `ui/settings.js`

- [ ] **Step 1: Create `ui/icons.svg` with 80+ icons on a 24×24 grid**
  - Use `stroke="currentColor"`, `stroke-width="var(--icon-stroke, 1.75)"`, `stroke-linecap="var(--icon-cap, round)"`, `stroke-linejoin="var(--icon-join, round)"`.
  - Include transport, playback modes, volume stages, navigation, view switchers, audio/DSP icons, window controls, layout editor handles (move, resize, rotate, lock, snap, preset), status icons, and point-matched morph path pairs.
- [ ] **Step 2: Inject/inline sprite loader and replace all Unicode/emoji glyphs in HTML/JS**

## Task 3: Motion Engine (`ui/motion.js`) & Direct-Manipulation Layout Studio (`ui/layout.js`)

**Files:**
- Create: `ui/motion.js`
- Create: `ui/layout.js`
- Modify: `ui/theme.js`, `ui/settings.js`

- [ ] **Step 1: Implement `ui/motion.js`**
  - Export `HalftoneMotion` (`window.HalftoneMotion`): spring `linear()` tokens, pointer ripple (Digital) & dither-inversion flash (Analogue), traveling focus ring, FLIP list/queue reorder helper, shared-element cover transition (`morphCover`), rolling tabular digit counter, frame-time performance governor (`> 20ms` auto-downgrade), and `__qa.motionAudit()`.
- [ ] **Step 2: Implement `ui/layout.js`**
  - Export `HalftoneLayout` (`window.HalftoneLayout`): direct-manipulation layout studio (`Ctrl+L` / Edit Layout button) for Widget (`12` modules) and Main Window panels.
  - Support free move (`8px` grid snap + alignment guides), 8-handle resize, and **rotation handle** (`32px` stem above top-center, free angle `-180°..180°`, `15°` `Shift` snap, `0°/90°/180°/270°` `±3°` magnetic detents, angle pill readout) rendered via `transform: translate3d(x, y, 0) rotate(deg)`.
  - Provide local inverse-rotation coordinate helper for sliders/scrubbers inside rotated modules, OBB boundary clamping, safety rails (Play/Pause + exit cannot be hidden/off-screen), 7 presets (`Classic`, `Compact Strip`, `Square Art`, `Vertical Rack`, `Lyrics Focus`, `Meter Bar`, `Tilted Desk`), Undo/Redo (`Ctrl+Z`/`Ctrl+Y`), and JSON import/export.

## Task 4: Audio Pipeline, Main Window & Widget v1.1 Complete Integration

**Files:**
- Modify: `ui/common.js`
- Modify: `ui/main.html`
- Modify: `ui/index.html`
- Modify: `ui/widget.js`

- [ ] **Step 1: Upgrade `ui/common.js` audio & state engine**
  - Add Crossfeed network (700Hz low-pass cross-mix for headphones), Stereo Balance (`StereoPannerNode`), Mono fold-down toggle, Loudness Compensation (Fletcher-Munson low-volume bass/treble shelf), Gapless dual-`<audio>` preloader, configurable Crossfade (`0–12s` equal-power curve), Spectral Flux onset beat detector, A–B Loop markers, Sleep Timer with fade-out, and Signal Path popover model.
- [ ] **Step 2: Upgrade `ui/main.html` views & visualizers**
  - Wire all navigation views: `All Tracks`, `Albums`, `Artists`, `Genres`, `Folders`, `Cover Flow` (3D Digital perspective carousel + Analogue 9-cover pre-dithered strip), `Recently Added`, `Most Played`, `Liked Songs`, `Smart Playlists`, `Library Health`, `Listening Stats`, `Visualizer & EQ` (interactive SVG frequency response curve + live Spectrogram), `Now Playing Stage` (4 arrangements + 6 WebGL2 shaders + 4 Canvas 2D modes + Apple Music word-lit spring lyrics + tap-to-sync `.lrc` editor), and `Theme Gallery`.
- [ ] **Step 3: Upgrade `ui/index.html` and `ui/widget.js`**
  - Integrate `ui/layout.js` (12 rotatable/resizable modules + 7 presets + layout toolbar), `ui/motion.js`, and `ui/icons.svg`.

## Task 5: Release Build & Exhaustive CDP + OS Verification

**Files:**
- Modify: `tools/cdp_verify.py`
- Run: `cargo build --release`, `node ui/test/test-contrast.js`, `python tools/cdp_verify.py`, `python tools/smtc_probe.py`, `python ui/test/widget_matrix.py`

- [ ] **Step 1: Compile release binary (`cargo build --release`)**
- [ ] **Step 2: Run automated verification suites against `src-tauri/target/release/halftone.exe`**
- [ ] **Step 3: Author `docs/architecture.html`, `docs/WHAT_WE_BUILT.md`, and `.design/halftone-spec-rebuild/DESIGN_REVIEW.md`, commit, and push `spec-rebuild`**
