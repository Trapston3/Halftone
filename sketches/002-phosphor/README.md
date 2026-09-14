## Variant: Phosphor — CRT deck (refined)

### Design stance
An instrument panel, not a music card: horizontal deck where an LED segment meter IS the seek bar — dev-tool register, accent tinted from the album art itself.

### Key choices
- Layout: 400px horizontal deck — 96px art, meta column, controls row; reads like a hardware front panel
- Seek/visualizer: 48 log-bucketed AnalyserNode bars drawn as 4px LED cells (same segmented language as the volume meter); played segments glow full accent, unplayed stay dim; cream caret cursor
- Album art: the real cover, downsampled and run through an 8x8 Bayer ordered dither onto a 32x32 LED grid — the art reads as a lit phosphor display, tinted by the extracted accent during transitions
- Accent system: dominant hue weighted out of the cover pixels, saturation clamped 0.5–0.9, lightness clamped 0.45–0.68 and raised until >=3:1 contrast against the deck background; 420ms ease crossfade on track change (art cells re-tint during the fade, nothing snaps)
- Typography: 10px mono caps for ALL metadata (TRK 01/04, FLAC 44.1 kHz 24-BIT), 14px/600 title — instrument-label hierarchy
- Depth: NO soft shadows on components — border tiers (#262B30 -> #3A4147 -> accent) carry elevation; one widget-level shadow only
- Tooltips: every tool button names its function explicitly (PIN · ALWAYS ON TOP / LYRICS · SYNCED / QUEUE · UPCOMING / ADD TO PLAYLIST)
- Volume: 5-LED stepped slider; scanline overlay on the whole deck

### Sketch modes
- `index.html` (committed): synth mode — procedural covers + WebAudio-generated audio. Zero third-party bytes, runs anywhere.
- `index.local.html` (gitignored, built by `tools/make_embed.py`): splices real covers, 45s AAC excerpts, and real .lrc lyrics from your library via `assets/embed.json`. Same code path, real data.

### Trade-offs
- Strong at: compact height (~230px open), hardware personality, art never fights the controls, accent keeps the deck alive on every track change
- Weak at: mono caps everywhere costs warmth; 96px dithered art is atmospheric, not photographic

### Best for
Power users and terminal-heavy desktops; the widget that matches a dev-tool setup.
