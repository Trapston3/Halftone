# Halftone v1.1 — Design Brief (`spec-rebuild`)

## 1. Product Vision & Dual-Surface Philosophy
Halftone is a free, open-source, standalone desktop music player built with **Tauri 2 (Rust + WebView2)** and **zero-bundler vanilla HTML/CSS/ES6**. It eliminates the compromise between a compact desktop widget and a full library manager by giving both surfaces first-class citizenship over a single shared audio owner:

- **Main Window ([`ui/main.html`](file:///c:/Users/traps/Documents/antigravity/hopeful-meitner/ui/main.html), `1200×760`, min `940×620`)**: The single `<audio>` + WebAudio graph owner, library index, Cover Flow stage, Now Playing full-bleed visualizer & Apple Music–style word-lit lyrics stage, 10-band parametric EQ + live spectrogram, Smart Playlists, Library Health diagnostics, Listening Statistics, and Theme Gallery.
- **Widget Window ([`ui/index.html`](file:///c:/Users/traps/Documents/antigravity/hopeful-meitner/ui/index.html), `400×260`, min `320×200`)**: A pure viewer/controller surface with 12 modular, freely movable, resizable, and rotatable tactile components that renders at 0% CPU when hidden.

## 2. Two Complete Visual Worlds × Light & Dark × 7 Palettes
Halftone ships two distinct industrial/digital design languages, each supporting **Light** and **Dark** modes and all **7 curated palette families** (`classic`, `catppuccin`, `tokyo-night`, `gruvbox`, `nord`, `rose-pine`, `dracula`), plus **Independent Per-Surface Theme/Mode** configuration:

1. **Analogue Theme ([`ui/themes/analogue.css`](file:///c:/Users/traps/Documents/antigravity/hopeful-meitner/ui/themes/analogue.css))**:
   - **Inspiration**: Teenage Engineering TP-7 / OP-1 Field, Braun Dieter Rams T1000, Risograph halftone print, dot-matrix LED displays.
   - **Tactile Rules**: Crisp `2px` ink borders, hard offset shadows (`3px 3px 0 var(--ink)`), zero Gaussian blur, Bayer 4×4 ordered dithering & Atkinson dithering on cover art, segmented LED bar VU meters with peak-hold ticks, stepped rotary knobs with tick marks, physical bevel press states (`translate(1px, 1px)`).
   - **Typography**: `Space Mono` / `JetBrains Mono` for transport, timecodes, metadata, and frequencies; `Inter` / `Plus Jakarta Sans` for dense tables.

2. **Digital Theme ([`ui/themes/digital.css`](file:///c:/Users/traps/Documents/antigravity/hopeful-meitner/ui/themes/digital.css))**:
   - **Inspiration**: Apple Music macOS/iOS 18 Liquid Glass, Linear Dark, Sony Walkman NW-WM1ZM2 UI, Teenage Engineering Choir.
   - **Tactile Rules**: Multi-layer translucent glass (`backdrop-filter: blur(28px) saturate(180%)`), `1px` specular inner hairline borders (`linear-gradient(135deg, rgba(255,255,255,0.22), rgba(255,255,255,0.04))`), dynamic cover-derived OKLCH ambient mesh gradients (`--cover-accent`, `--cover-secondary`), spring-physics hover lift and press scale (`scale(0.96)`), and 60fps WebGL2 shader visualizers.
   - **Typography**: `Plus Jakarta Sans` / `Inter` display headers (`font-weight: 650`, `-0.02em`), `Inter` body, `JetBrains Mono` (`tabular-nums`) for timecodes and audio specs.

## 3. Experience Pillars
1. **Zero-Jank 60fps Performance**: Compositor-only animations (`transform`, `opacity`), pre-computed `linear()` spring curves, `IntersectionObserver` + `document.hidden` loop gating, and a runtime frame-time governor in [`ui/motion.js`](file:///c:/Users/traps/Documents/antigravity/hopeful-meitner/ui/motion.js) that downgrades blur/shader resolution when frame times exceed `20ms`.
2. **Custom Vector Iconography ([`ui/icons.svg`](file:///c:/Users/traps/Documents/antigravity/hopeful-meitner/ui/icons.svg))**: 80+ bespoke `24×24` stroke icons with CSS variable hooks (`--icon-stroke`, `--icon-cap`, `--icon-join`) and point-matched SVG morph pairs. Zero emoji or raw Unicode symbols in the UI.
3. **Direct-Manipulation Layout Studio ([`ui/layout.js`](file:///c:/Users/traps/Documents/antigravity/hopeful-meitner/ui/layout.js))**: Move, resize, and **rotate at any angle** (`transform: translate3d(x,y,0) rotate(deg)`) with 15° `Shift` snap, 0°/90°/180°/270° magnetic detents, alignment guides, safety rails, 7 named presets, and full interactive hit-testing inside rotated modules.
4. **Audiophile Signal Integrity**: Single-pass magic-byte format detection (`probe.rs`), ReplayGain & LUFS analysis (`analyze_track.rs`), gapless dual-audio preloader, crossfade, 10-band parametric EQ with interactive frequency response curve, crossfeed, stereo balance, mono fold-down, and Signal Path verification popover.
