# DESIGN.md — Halftone Design System & Token Contract (v1.1)

## 1. Aesthetic Direction & Dual-Theme Philosophy
Halftone embodies two contrasting industrial/digital archetypes unified by a single semantic CSS custom property contract (`ui/themes/base.css`, `ui/themes/analogue.css`, `ui/themes/digital.css`, `ui/themes/palettes.css`):

| Dimension | `data-theme="analogue"` (Tactile Print & Hardware) | `data-theme="digital"` (Liquid Glass & Shaders) |
|---|---|---|
| **Surface Material** | Warm paper / anodized aluminum (`--bg-elev-1`), zero Gaussian blur, optional SVG print grain (`--grain-opacity`) | Translucent multi-layer glass (`backdrop-filter: blur(28px) saturate(180%)`), specular `1px` inner border |
| **Borders & Depth** | Crisp `2px` solid `--ink` borders + hard offset shadow (`3px 3px 0 var(--ink)`) | `1px` specular hairline (`var(--border)`) + ambient diffused elevation shadow |
| **Cover Art Treatment** | Bayer 4×4 ordered dithering or Atkinson dithering (`image-rendering: pixelated`) | Rounded `12px` card with cover-derived OKLCH ambient bloom (`--cover-accent`) |
| **Meters & Visualizers** | Segmented LED bar VU meters with peak-hold ticks, dot-matrix canvas | 60fps WebGL2 fragment shaders (Fluid Flow, Aurora Ribbons, Glass Orbs, Spectrum Tunnel) |
| **Iconography (`ui/icons.svg`)** | `--icon-stroke: 2px`, `--icon-cap: square`, `--icon-join: miter` | `--icon-stroke: 1.75px`, `--icon-cap: round`, `--icon-join: round` |
| **Motion Physics (`ui/motion.js`)** | Snappy mechanical detents (`120ms–160ms`), dither-inversion flash on click | Spring-physics (`--ease-spring-snappy`, `--ease-spring-fluid`), radial pointer luminance ripple |

## 2. Root Attribute Contract
Every surface (`ui/main.html` and `ui/index.html`) binds four root attributes on `<html>`:
- `data-theme="analogue" | "digital"`
- `data-mode="light" | "dark"`
- `data-palette="classic" | "catppuccin" | "tokyo-night" | "gruvbox" | "nord" | "rose-pine" | "dracula"`
- `data-density="compact" | "comfortable" | "spacious"`

## 3. Core Semantic Tokens (`ui/themes/base.css`)

### 3.1 Color & Surface Tokens
- `--bg`: Primary window canvas background
- `--bg-elev-1`: Primary card / sidebar / transport surface
- `--bg-elev-2`: Hover / secondary elevated module surface
- `--bg-glass`: Translucent surface (`backdrop-filter` target in Digital; opaque in Analogue)
- `--ink`: Primary foreground text and active iconography (`>= 4.5:1` against `--bg` and `--bg-elev-1`)
- `--ink-muted`: Secondary metadata text (`>= 4.5:1` WCAG AA)
- `--ink-faint`: Disabled / subtle ruler markers (`>= 3.0:1` for interactive controls)
- `--accent`: Primary interactive accent color (palette default or cover-derived OKLCH)
- `--accent-ink`: High-contrast foreground text on `--accent` surfaces (`>= 4.5:1`)
- `--cover-accent`: Dynamically extracted OKLCH dominant color from active album art (`art_thumb`), clamped in OKLab for WCAG AA contrast
- `--cover-secondary`: Secondary harmonic OKLCH color from album art
- `--border`: Primary structural border (`2px solid var(--ink)` in Analogue; `1px solid rgba(...)` in Digital)
- `--danger`, `--warning`, `--success`: Semantic status tokens

### 3.2 Typography & Spacing Scale
- `--font-sans`: `'Inter', 'Plus Jakarta Sans', system-ui, -apple-system, sans-serif`
- `--font-display`: `'Plus Jakarta Sans', 'Inter', system-ui, sans-serif`
- `--font-mono`: `'JetBrains Mono', 'Space Mono', 'Cascadia Mono', monospace`
- Numeric discipline: `font-variant-numeric: tabular-nums` enforced on all timecodes, frequencies, dB readouts, track numbers, and counters.
- Spacing scale (`4px` base grid): `--space-1` (`4px`), `--space-2` (`8px`), `--space-3` (`12px`), `--space-4` (`16px`), `--space-6` (`24px`), `--space-8` (`32px`).

### 3.3 Motion & Choreography Tokens (`ui/motion.js`)
- `--ease-spring-snappy`: `linear(0, 0.45 12%, 0.88 28%, 1.04 48%, 0.99 72%, 1)` (`220ms`)
- `--ease-spring-fluid`: `linear(0, 0.28 10%, 0.76 26%, 0.98 44%, 1.02 64%, 1)` (`380ms`)
- `--ease-out-quart`: `cubic-bezier(0.25, 1, 0.5, 1)`
- `--dur-micro`: `110ms` | `--dur-fast`: `180ms` | `--dur-normal`: `280ms` | `--dur-stage`: `460ms`

## 4. Curated Palette Families (`ui/themes/palettes.css`)
All 7 palettes provide dedicated Light and Dark token sets that work across **both** Analogue and Digital themes while guaranteeing WCAG 2.1 AA contrast (`node ui/test/test-contrast.js` passes all 28 theme × mode × palette combinations):
1. **Classic**: Warm cream paper & vermilion (`#d84a2a`) in Analogue; obsidian & electric indigo (`#7c6cff`) in Digital.
2. **Catppuccin**: Latte (Light) / Mocha (Dark) with Mauve & Peach accents.
3. **Tokyo Night**: Day (Light) / Storm & Night (Dark) with Cyan & Magenta accents.
4. **Gruvbox**: Light Hard/Medium / Dark Medium with Warm Orange & Aqua accents.
5. **Nord**: Snow Storm (Light) / Polar Night (Dark) with Frost Cyan (`#88c0d0`) accents.
6. **Rosé Pine**: Dawn (Light) / Main & Moon (Dark) with Rose & Iris accents.
7. **Dracula**: Alucard (Light) / Dracula Classic (Dark) with Purple (`#bd93f9`) & Pink (`#ff79c6`) accents.
