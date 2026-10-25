# Halftone v1.1 Spec Rebuild — Design Review, Accessibility Audit & Verification Report

**Date:** 2026-10-25
**Branch:** `spec-rebuild`
**Skills Applied:** `design-review`, `design-critique`, `accessibility-review`, `click-path-audit`, `deep-qa`, `santa-method`, `ponytail-audit`, `verification-before-completion`

---

## 1. Executive Summary

Halftone v1.1 has been rebuilt against `Halftone — Build Specification.md` (Version 1.1) and verified end-to-end on the compiled release executable (`src-tauri/target/release/halftone.exe`) via Chrome DevTools Protocol (`tools/cdp_verify_v11.py`), Rust unit tests (`cargo test`), and WCAG 2.1 AA contrast validation (`node ui/test/test-contrast.js`).

Both independent Santa-Method verification passes (**Agent A: Spec Compliance & Architecture** and **Agent B: Live Runtime CDP & Visual Polish**) converged with **0 blocking defects**.

---

## 2. Design Critique & Visual Hierarchy (`design-critique` / `design-review`)

### 2.1 Dual-Theme Fidelity
- **Analogue Theme (`data-theme="analogue"`)**:
  - Preserves the hardware field-recorder / risograph identity: sharp 0px structural corners, 1px hairline grids (`--border`), monospace technical labels (`JetBrains Mono`), segmented LED bar ladders, and Bayer ordered-dither canvas treatments (`standard`, `fine`, `ultra`, `halftone-dot`).
- **Digital Theme (`data-theme="digital"`)**:
  - Implements Apple Music / Dynamic Island liquid-glass surfaces: `backdrop-filter: blur(28px) saturate(180%)`, rounded radii (`--r-lg: 18px`), specular top-edge inner highlight, WebGL2 liquid-mesh ambient shader with `@media (prefers-reduced-motion: reduce)` awareness, and spring-physics choreography (`ui/motion.js`).

### 2.2 All 7 Palette Families × Light/Dark Modes
- Verified across all 7 palette families (`classic`, `catppuccin`, `tokyo-night`, `gruvbox`, `nord`, `rose-pine`, `dracula`) in both `dark` and `light` modes, plus dynamic `album` OKLCH color extraction (`art_thumb` command in `src-tauri/src/art_thumb.rs`).

### 2.3 Post-Screenshot Polish Fixes Verified
During visual inspection of the initial release-build CDP screenshots (`ui/test/shots_v11/*.png`), three subtle visual regressions were caught and resolved:
1. **Layout Studio Gizmo Badge Visibility**: `.ht-mod-gizmo` previously specified `display: inline-flex` after the base `display: none` rule, causing `0°` rotation badges to appear in normal playback mode. Fixed in `ui/layout.js` with `html:not([data-layout-edit="true"]) .ht-mod-gizmo { display: none !important; }`.
2. **Layout Studio Floating Toolbar in Scaled Widget**: `#ht-layout-bar` used `translateY(140%)` when hidden, which remained partially visible inside the widget's `transform: scale(...)` root container. Fixed in `ui/layout.js` by toggling `display: none` when `data-layout-edit !== "true"`.
3. **Word-Synced Karaoke Spacing**: `.lyr-word` spans use `display: inline-block` for vertical spring lift (`translateY(-1.5px) scale(1.04)`), which caused trailing spaces inside spans to collapse. Fixed in `ui/common.js` by inserting explicit `document.createTextNode(" ")` nodes between `.lyr-word` spans.

---

## 3. WCAG 2.1 AA Accessibility Audit (`accessibility-review`)

- **Contrast Ratio Verification (`node ui/test/test-contrast.js`)**:
  - All `(theme, mode, palette)` combinations tested for `--fg` vs `--bg`, `--fg` vs `--bg-elev`, `--fg-dim` vs `--bg`, and `--accent` vs `--bg`.
  - **Result**: `ALL PAIRS PASS` (100% pass rate; body text $\ge 4.5:1$, large/UI text $\ge 3.0:1$).
- **Dynamic Album Accent Contrast Guard (`src-tauri/src/art_thumb.rs`)**:
  - Extracted OKLCH dominant/secondary colors are automatically lightness-clamped so contrast against dark (`#111113`) and light (`#f5f2eb`) backgrounds always meets WCAG AA ($\ge 4.5:1$).
- **Keyboard & Screen Reader Navigation**:
  - All interactive controls include `aria-label` or `title` attributes, `:focus-visible` outlines, and keyboard shortcuts (`Space`, `ArrowLeft`/`ArrowRight`, `ArrowUp`/`ArrowDown`, `M`, `L`, `E`, `Ctrl+L`, `Ctrl+F`, `?`).
- **Reduced Motion**:
  - `@media (prefers-reduced-motion: reduce)` collapses spring transitions and halts ambient shader/canvas loops.

---

## 4. Click-Path & State Consistency Audit (`click-path-audit`)

| User Touchpoint | Event Sequence | Verified Final State |
| --- | --- | --- |
| Widget Play/Pause (`#wPlay`) | Click $\rightarrow$ `emitCmd({cmd:"play"})` $\rightarrow$ Owner `<audio>` plays $\rightarrow$ `halftone:sync` broadcast | Both windows show playing state; `owner().hasAC === false` in widget |
| Theme / Mode / Palette Switch (`__htTheme.setTheme`) | Change in either window $\rightarrow$ `halftone:theme-sync` + `localStorage` | Both windows update `[data-theme]`, `[data-mode]`, `[data-palette]` within 1 frame |
| Layout Studio (`#wLayoutBtn` / `#btnLayout`) | Toggle edit mode $\rightarrow$ click module gizmo (`0 -> 90 -> 180 -> 270 -> 0`) $\rightarrow$ `localStorage` | Module updates `data-rot`, flex direction adapts via `[data-orient]`, gizmos hide on exit |
| Signal Path (`#btnSignal`) | Click `SIGNAL` button $\rightarrow$ `renderSignalChain()` inspects track + DSP + output | Popover displays 5-node signal chain: Source $\rightarrow$ Decode $\rightarrow$ DSP $\rightarrow$ ReplayGain $\rightarrow$ Output |
| Library Views (`#tabCoverFlow`, `#tabArtists`, `#tabGenres`, `#tabFolders`, `#tabSmart`, `#tabStats`, `#tabHealth`) | Click view tab $\rightarrow$ `setView(v)` $\rightarrow$ Tauri DB/health command $\rightarrow$ DOM render | Active tab highlighted, `#viewExtraPane` renders cards/stats/health actions cleanly |

---

## 5. Ponytail Over-Engineering Audit (`ponytail-audit`)

- **Zero External UI Dependencies**: No React, Vue, Tailwind, Lucide, GSAP, or Anime.js runtime bundles. All icons live in a single inline-loaded `ui/icons.svg` sprite, and all spring physics are generated deterministically by the 140-line `ui/motion.js` module.
- **Single-Pass Audio & Artwork Pipeline**: `art_thumb.rs` and `analyze_track.rs` reuse `symphonia` and `image` already present in `Cargo.toml`, storing cached artifacts in `%APPDATA%\Halftone\` to avoid redundant disk I/O.
- **No Unnecessary Unsafe Blocks**: Cleaned up nested `unsafe` blocks in `src-tauri/src/lib.rs`.
