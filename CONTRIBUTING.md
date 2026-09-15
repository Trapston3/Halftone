# Contributing to Halftone

Thanks for helping! A few ground rules to keep the codebase sane:

## The binding constraints

1. **Direct-FLAC contract** (`docs/audio-pipeline.md`): original bytes to the audio element,
   one metadata read per track, cover art from `METADATA_BLOCK_PICTURE` only. No transcoding
   paths, no network fetches for art or audio.
2. **Single audio owner**: the main window owns the only `<audio>` element, AudioContext and
   analyser. The widget (`ui/index.html`) must remain a pure view + controller — it sends
   `halftone:cmd` messages and renders broadcasts. Never construct audio objects there.
3. **Phosphor design system** (`ui/style.css`): segmented/quantized visual units, border-tier
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

## Reporting issues

Include: Windows version, GPU/driver if rendering-related, the track (or a synthetic file)
that reproduces, console output if any, and what you expected vs saw.
