# Halftone v1.1 — UI Overhaul, Digital Theme Redesign & Bugfix Implementation Plan

**Date:** 2026-10-25  
**Branch:** `spec-rebuild`  
**Goal:** Resolve all 11 UX, visual, layout, audio/artwork, and interaction issues reported during hands-on review, transforming Halftone's Digital mode, navigation, sidebar, settings, and scroll/transition behavior.

---

## Summary of Issues & Architectural Fixes

| # | Reported Issue | Root Cause | Concrete Fix |
|---|---|---|---|
| **1** | **Left sidebar is cluttered with 15 icons** | All v1.1 views were dumped flat into `<nav id="sidebar">` | Show only essentials by default (**Now Playing**, **Songs**, **Albums**, **Artists**, **Playlists**, and **Settings**). Add a **Customize Sidebar** popover & Settings section to toggle optional views (`Cover Flow`, `Genres`, `Folders`, `Liked`, `Queue`, `Smart Mix`, `Stats`, `Health`) with a **Reset Sidebar** button. |
| **2** | **Equalizer doesn't need its own sidebar section** | `tabEq` was a top-level sidebar tab opening a floating sheet | Remove `tabEq` from the sidebar. Embed the full 10-band Equalizer, Pre-amp, Presets, AutoEQ import, and DSP controls directly inside **Settings $\rightarrow$ Equalizer & Audio DSP**. |
| **3** | **No browser-style Back / Forward (`←` / `→`) navigation** | `setView()` mutated state directly with no history stack | Add a `navHistory` / `navIndex` router stack in `ui/main.html` with **Back (`←`)** and **Forward (`→`)** buttons in the top bar, plus `Alt+Left` / `Alt+Right` and mouse Button 3 / 4 support. |
| **4** | **Digital mode UI is a mess** | `ui/main.html` and `ui/index.html` hardcode Analogue TUI rules (`0px` corners, `1px` grid borders, uppercase monospace, LED blocks) that override `ui/themes/digital.css` | Overhaul `ui/themes/digital.css`, `ui/main.html`, and `ui/index.html` under `html[data-theme="digital"]`: floating liquid-glass sidebar dock, rounded `16px–20px` surfaces, `Inter` sentence-case typography, smooth pill sliders for seek & volume, diffused cover-art shadows, and glassmorphic cards. |
| **5** | **Album cover / photos broken** | `#artReal` / `#wArtReal` displayed broken `<img>` icon when `hasCover` was false or when `media://localhost/cover/` path decoding failed; no folder `cover.jpg` fallback | 1) In `src-tauri/src/lib.rs` & `covers.rs`, add automatic folder artwork fallback (`cover.jpg`, `cover.png`, `folder.jpg`, `front.jpg`) and fix Windows path decoding on `media://localhost/cover/` & `/thumb/`. 2) In `ui/main.html` & `ui/index.html`, hide `<img>` on error/empty and show a clean procedural cover placeholder instead of a broken image icon. |
| **6** | **No scrollbars in Main Settings or anywhere in the app** | `::-webkit-scrollbar` was hidden or containers had `overflow: hidden` | Add styled, interactive scrollbars (`8px` track/thumb, `overflow-y: auto`) across Settings, Track Lists, Album/Artist grids, Queue, and Lyrics panes. |
| **7** | **Widget settings clip inside the 400×260 mini-player** | Settings popover was rendered inside the tiny widget DOM | Add a dedicated Tauri **Settings Window** (`open_settings_window` command in `src-tauri/src/lib.rs`, `580×680` resizable window loading `ui/main.html?window=settings` or `ui/settings-window.html`) so clicking the gear in the widget opens a proper standalone scrollable Settings window. |
| **8** | **Volume changes on scroll everywhere (conflicts with Cover Flow & lists)** | Global wheel listener in `ui/main.html` and `ui/widget.js` adjusted volume on any wheel event | Restrict volume scroll-wheel adjustment **strictly** to hovering over the volume bar/cluster (`#volWrap`, `#volLed`, `#wVol`). Scrolling over Cover Flow, lists, or settings never alters volume. |
| **9** | **Missing "Reset to Default" setting** | Settings lacked a one-click global factory reset | Add a prominent **"Reset All to Defaults"** button in Settings (with confirmation toast) that restores theme, palette, sidebar items, EQ, DSP, layout rotations, and preferences across all windows. |
| **10** | **Smart Mix presets do nothing when clicked** | Clicking `.smart-card` in `ui/main.html` didn't filter or render the library tracks | Wire every Smart Mix card (*Lossless Reference*, *Heavy Rotation*, *Recently Added*, *Top Rated*, *Chill / Late Night*) to filter `S.library` / `db_eval_smart_playlist`, highlight the active mix, render a playable track table immediately below the cards, and provide a **"Play Mix"** button. |
| **11** | **No animated transitions between views, tracks, or interactions** | Views toggled via instant `display: none` / `display: flex` | Add spring-driven view transitions (`htAnimateViewIn`), staggered card/row entrance animations, sliding active sidebar pill indicator, and smooth cover-art/metadata crossfades on track change. |

---

## Work Breakdown Structure (Subagent Parallel Tracks)

### Track A — Rust Backend Fixes (`src-tauri/src/lib.rs`, `src-tauri/src/covers.rs`)
1. **Artwork Loading & Folder Fallback**:
   - When `open_track` or `serve_media` (`/cover/` or `/thumb/`) is called for a track without embedded picture bytes, automatically look in the track's parent directory for `cover.jpg`, `cover.jpeg`, `cover.png`, `cover.webp`, `folder.jpg`, ` folder.png`, `front.jpg`, or `album.jpg`.
   - Fix URL percent-decoding and Windows `\\?\` prefix normalization in `serve_media` so `http://media.localhost/cover/<encoded-path>` and `http://media.localhost/thumb/<encoded-path>` never 403/404 on valid library tracks.
2. **Standalone Settings Window Command (`open_settings_window`)**:
   - Register `open_settings_window(app: tauri::AppHandle, section: Option<String>)` in `src-tauri/src/lib.rs` that creates or focuses a dedicated `580×680` resizable Tauri `WebviewWindow` (`label = "settings"`) so widget settings never clip inside the 400×260 widget.

### Track B — Digital Theme Overhaul & Scrollbars (`ui/themes/digital.css`, `ui/style.css`, `ui/motion.js`)
1. **Complete Digital Mode Visual Redesign (`ui/themes/digital.css`)**:
   - Override all Analogue TUI artifacts when `html[data-theme="digital"]` is active:
     - Typography: `Inter, -apple-system, BlinkMacSystemFont, "Segoe UI Variable", sans-serif`, `text-transform: none`, comfortable tracking.
     - Surfaces: Translucent frosted glass (`backdrop-filter: blur(36px) saturate(190%)`), `border-radius: 16px` panels, `12px` buttons/inputs, subtle `1px solid rgba(255,255,255,0.10)` specular borders, soft ambient drop shadows.
     - Controls: Smooth continuous pill seek bar & volume slider in Digital mode (hiding the boxy TUI LED segments when in Digital theme), glowing circular Play button with spring hover/active scale.
     - Cards & Tables: Rounded album cards (`14px` art radius) with hover lift (`translateY(-4px)`), borderless Apple Music-style song rows with alternating subtle glass hover.
2. **Universal Custom Scrollbars (`ui/style.css`)**:
   - Style `::-webkit-scrollbar` (`width: 8px; height: 8px`), `::-webkit-scrollbar-track`, and `::-webkit-scrollbar-thumb` (rounded pill, `--accent` on hover) and ensure `.settings-body`, `.main-list`, `#viewExtraPane`, `.lyrics-pane`, and `#settingsModal` have `overflow-y: auto` and smooth scrolling.
3. **Animated Transitions (`ui/motion.js` & `ui/style.css`)**:
   - Add `.ht-view-enter` keyframe animation (`opacity: 0; transform: translateY(10px) scale(0.992)` $\rightarrow$ `opacity: 1; transform: none` over `260ms var(--ease-spring-fluid)`), staggered `.ht-stagger-item` animations, and smooth album cover crossfade transitions.

### Track C — Main Window, Sidebar, Navigation, Smart Mix, Equalizer-in-Settings & Volume Scroll (`ui/main.html`, `ui/settings.js`, `ui/widget.js`, `ui/index.html`)
1. **Customizable Essentials-First Sidebar**:
   - Default visible items: **Now Playing**, **Songs**, **Albums**, **Artists**, **Playlists**, and **Settings** (bottom).
   - Add a **Customize Sidebar (`+`)** button at the bottom of the sidebar (and in Settings) allowing users to toggle optional views (`Cover Flow`, `Genres`, `Folders`, `Liked`, `Queue`, `Smart Mix`, `Stats`, `Health`).
   - Remove standalone `EQ` icon from the sidebar.
2. **Browser-Style Back / Forward Navigation (`←` / `→`)**:
   - Add `#btnNavBack` and `#btnNavForward` buttons at the left of the top bar.
   - Maintain a `navStack` + `navIdx` recording `{ view, filter, search, artist, album, playlist }` states; support `Alt+ArrowLeft`, `Alt+ArrowRight`, and mouse back/forward buttons (`e.button === 3` / `e.button === 4`).
3. **Volume Scroll Restricted to Hovering Volume Control**:
   - Remove global wheel-to-volume handlers in `ui/main.html` and `ui/widget.js`.
   - Bind wheel volume changes **exclusively** to `#volLed` / `#volWrap` (main) and `#wVol` (widget).
4. **Equalizer Embedded in Settings + Global "Reset to Defaults"**:
   - Move the 10-band Equalizer, Pre-amp, AutoEQ import, and DSP controls into a dedicated **Equalizer & DSP** tab inside Settings (accessible from both Main Settings and the standalone Widget Settings window).
   - Add a **"Reset All Settings to Default"** button that clears custom overrides and restores factory defaults immediately.
5. **Working Smart Mix Presets**:
   - Clicking any Smart Mix card (*Lossless Reference*, *Heavy Rotation*, *Recently Added*, *Top Rated*, *Late Night / Chill*) evaluates the rule against the library, renders the playable track table with a **"Play Mix"** header button, and plays on click.
6. **Album Cover Fallback & Standalone Widget Settings**:
   - Ensure `#artReal` and `#wArtReal` never display a broken image icon; wire `#wCfgBtn` in the widget to open the standalone `settings` window via `open_settings_window`.


---

### Track D — Discord Rich Presence (RPC) & Stunning-Web Pro Design Upgrade
1. **Discord Rich Presence (`src-tauri/src/discord_rpc.rs`, `src-tauri/src/lib.rs`, `ui/main.html`, `ui/settings.js`)**:
   - Native Windows Named Pipe (`\\.\pipe\discord-ipc-0..9`) IPC client in `src-tauri/src/discord_rpc.rs`.
   - Broadcasts `type: 2` (*Listening*) activity with:
     - `details`: Track Title
     - `state`: `"by <Artist> • on Halftone"`
     - `timestamps`: Live `start` and `end` Unix ms timestamps when playing (shows live progress bar inside Discord profile)
     - `assets`: `"Listening on Halftone — FOSS Local Music Player (<Album>)"`
     - `buttons`: `[{ "label": "Get Halftone on GitHub", "url": "https://github.com/Trapston3/Halftone" }]`
   - Exposed via `discord_update_presence` and `discord_clear_presence` Tauri commands, with a toggle in Settings (enabled by default).
2. **`pro` Model Digital Theme & Choreographed Motion (`stunning-web`, `claude-design`, `frontend-design-system`, `popular-web-designs`, `animejs-motion`, `motion-design`)**:
   - Delegated to a `pro` subagent owning `ui/themes/digital.css`, `ui/style.css`, `ui/motion.js`, and `ui/ambient-webgl.js`.
   - Implements multi-layered frosted glass depth, specular edge highlights, fluid `Inter` display/body typography, magnetic transport buttons, WAAPI spring view transitions with automatic `MutationObserver` staggered card/row reveals, and an enhanced WebGL2 liquid-aurora ambient shader.
