# Halftone v0.2 UI contract

Binding interface between the **foundation** (HTML structure, JS engine,
layout, settings) and the **themes** (pure CSS). Foundation never hardcodes
a color; themes never depend on JS internals. If you need something not
listed here, add it to this file in the same commit.

## Files and ownership

| File | Owner | Role |
|---|---|---|
| `ui/main.html`, `ui/index.html` | foundation | markup + page wiring (main window / widget) |
| `ui/common.js` | foundation | shared engine (audio owner, sync, art, meters, lyrics) |
| `ui/settings.js` | foundation | settings schema, store, generated settings UI |
| `ui/theme.js` | foundation | theme engine, tokens -> canvas colors, transitions |
| `ui/themes/base.css` | foundation | layout + structure. Uses ONLY tokens below |
| `ui/themes/analogue.css` | themes | the current dither/LED look, dark + NEW light |
| `ui/themes/digital.css` | themes | Frutiger Aero x liquid glass, light + dark |
| `ui/test/theme_gallery.html` | themes | every component below, all 4 theme/mode combos |

Load order in both pages: `themes/base.css`, `themes/analogue.css`,
`themes/digital.css`, then `common.js`, `theme.js`, `settings.js`, page script.
No build step, no frameworks, no CDN (CSP is `'self'`).

## Root attributes (set by theme.js on `<html>`)

```
data-theme   = analogue | digital
data-mode    = light | dark            (resolved; "system" resolves via prefers-color-scheme)
data-motion  = full | reduced | off
data-density = compact | cozy | comfy
data-surface = main | widget
data-nav     = left | right | top | bottom | hidden     (main window only)
```
Theme selectors look like `:root[data-theme="digital"][data-mode="dark"] { ... }`.

## Tokens (themes MUST define all of these for each theme x mode)

Color: `--bg` (window backdrop), `--bg-2` (panel), `--bg-3` (raised/hover),
`--fg`, `--fg-2` (secondary), `--fg-3` (muted), `--line`, `--line-2`,
`--accent-fg` (text on accent), `--ok`, `--warn`, `--danger`.
`--accent`, `--accent-22`, `--accent-12` are written by JS (album-art accent);
themes may read them but never set them.

Surface: `--panel-bg` (may be translucent), `--panel-border`,
`--panel-highlight` (inner top gloss; `none` allowed), `--glass-blur`
(e.g. `0px` analogue, `24px` digital), `--glass-sat` (backdrop saturate %),
`--shadow-1`, `--shadow-2`.

Type: `--font-ui`, `--font-mono`, `--font-display` (system font stacks only).
Shape: `--r-s`, `--r-m`, `--r-l`, `--r-pill`. Final radii are multiplied by
the user setting `--radius-scale` (foundation sets it; use
`calc(var(--r-m) * var(--radius-scale, 1))` in base.css).

Motion: `--ease-out`, `--ease-spring`, `--dur-1` (~120ms), `--dur-2`
(~240ms), `--dur-3` (~420ms). `data-motion="off"` → foundation sets durations to 0.

Canvas (JS reads these with getComputedStyle on every `halftone:theme`
event; any CSS color syntax): `--canvas-bg` (dither art background),
`--canvas-off` (unlit meter cell), `--canvas-ink` (playhead / tick).

Theme-level switches (string tokens, read by JS):
`--seek-style: led | gel`, `--vol-style: leds | gel`,
`--art-default: dither | real`, `--ambient-default: dither | halo | aurora | off`.

## Components (foundation emits this DOM; themes style the look)

Structure: `.app-shell`, `.titlebar` (custom drag bar, `.win-btn`s),
`.nav` > `.nav-item(.active)` (icon `<svg>` + `.nav-label`), `.view`
(routed content), `.panel`, `.mini-bar` (now-playing strip).

Controls: `.btn`, `.btn-icon`, `.btn-orb` (round transport button;
`.btn-orb.lg` = play/pause), `.pill-tabs` > `.pill(.active)`, `.seg` >
`button(.on)` (segmented), `.switch` (checkbox role=switch),
`.slider` (`<input type=range>`; foundation keeps `--val: 0..100%` updated
for fills), `.field` (text input), `.badge` (`.badge.lossless`, `.badge.lossy`).

Seek: `.seek` container. LED style = `<canvas class="seek-led">`. Gel style =
`.seek-gel` > `.track` > `.fill` (width = `--val`) + `.knob` (left = `--val`).
Foundation renders both; CSS shows the one matching `--seek-style`
via `[data-seek="led"|"gel"]` on `.seek` (foundation sets it).
Volume: same pattern, `.vol` with `.vol-leds` or `.vol-gel`.

Lists: `.list` > `.row(.cur,.playing)` > `.row-art`, `.row-title`,
`.row-sub`, `.row-dur`, `.row-more`. Grid: `.grid` > `.card` > `.card-art`
(`<canvas>` or `<img>`), `.card-title`, `.card-sub`.

Now playing: `.np` > `.np-art` (`canvas.art-dither` or `img.art-real`),
`.np-title`, `.np-artist`, `.np-meta`, `.np-transport`, `.lyrics` >
`.lyric-line(.active,.past,.future)`; `.past` = already sung, `.future` =
upcoming (dimmed); `.lyrics.plain` for unsynced text;
`.lyrics-status` (searching / none found / source label).

Overlays: `.sheet` (bottom sheet) > `.sheet-handle`, `.sheet-head`,
`.sheet-row`; `.menu` > `.menu-item(.on)`, `.menu-sep`, nested `.menu.sub`;
`.toast(.ok,.warn,.error)`; `.tooltip`.

Decor layer (first child of `body`, pointer-events none):
```html
<div class="fx-layer"><div class="fx-aurora"></div><div class="fx-bokeh"></div><canvas class="fx-ambient"></canvas></div>
```
`fx-ambient` = JS canvas (dither field / halo). Aurora + bokeh = pure CSS
owned by the theme. Foundation sets `data-ambient` on `.fx-layer`.

## Motion hooks (foundation triggers, themes may restyle)

- Theme/mode switch: `document.startViewTransition` with a circular
  clip-path reveal from the pointer; fallback adds `html.theme-fading`
  for `--dur-3`. Themes may customise `::view-transition-new(root)`.
- View change: incoming `.view` gets `.view-enter` → `.view-enter-active`.
- Sheets/menus: `.open` class toggled; animate `transform`/`opacity` only.
- Track change: `.np-art` gets `.art-swap` for `--dur-3`.
- Buttons: `:active` press, `.btn-orb` gloss. Never animate layout props.
All motion must respect `data-motion` (`reduced` = fades only, `off` = none).

## Events (window `document`)

`halftone:theme` (tokens changed — re-read canvas colors),
`halftone:settings` (detail = changed keys), plus the existing
`halftone:track|state|tick|art|vol|collect|seeked`.
