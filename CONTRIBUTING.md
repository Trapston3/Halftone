# Contributing to Halftone

Thanks for helping! A few ground rules to keep the codebase sane:

## The binding constraints

1. **Original-bytes contract** (`docs/audio-pipeline.md`): the file's own bytes to the audio
   element — FLAC, ALAC, WAV, MP3, AAC/M4A, Ogg Vorbis, Opus (ALAC: the one documented
   lossless decode step), one metadata read per track, format detection by magic bytes.
   No transcoding paths.
2. **Single audio owner**: the main window owns the only `<audio>` element, AudioContext and
   analyser. The widget (`ui/index.html`) must remain a pure view + controller — it sends
   `halftone:cmd` messages and renders broadcasts. Never construct audio objects there.
3. **Network surfaces are the ones documented in the README** (LRCLIB lyrics, cover-art
   lookups, OTA checks) — each user-disableable. New UI must not add another.
4. **Phosphor design system** (`ui/themes/`): segmented/quantized visual units, border-tier
   depth (no soft drop shadows), the existing type scale. New UI must use the same vocabulary
   (LED segments, dot caps, dither textures) — not a new one.

## Dev loop

```
cd src-tauri
cargo build --release          # ui/ is embedded at compile time — rebuild after UI edits
cargo run --release            # or launch the exe directly with CDP env (see tools/cdp_verify2.py)
```

- Verify changes against the **release exe** via CDP (`tools/verify_fixes.py` pattern: state
  probes + screenshots), not just the dev server.
- Never mark a feature verified that you didn't exercise at runtime.

## Code style

- Vanilla JS, no frameworks, no build step for the frontend
- Comments explain *why*, not what
- Keep the crate dependency-free where practical (metadata walking, base64, etc. are
  hand-rolled on purpose)

## Version notes

- **v0.2.1** — visual overhaul: Analogue light is a new risograph style, Digital dark is
  liquid glass, the nav rail collapses/hides (Ctrl+B), and the seek bar gains a spectrum
  visualiser; startup-freeze fix and hardened installers included.
- **v0.2.0** — multi-format original-bytes playback, two-theme engine with light/dark,
  one-line installers, network-surface disclosure.

## Reporting issues

Include: OS version (Windows / Linux + desktop environment), GPU/driver if rendering-related,
the track (or a synthetic file) that reproduces, console output if any, and what you expected
vs saw.
