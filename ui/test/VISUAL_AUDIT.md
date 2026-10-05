# VISUAL AUDIT — v0.1.2 (`944cb51`) vs v0.2.0 → v0.2.1 plan

Every visual feature the old UI had, whether it survives in v0.2.0, and the
fix plan. This table is the work checklist; rows get DONE markers as they
land. Old reference: `git show 944cb51:ui/...` extracted to /tmp/old;
old shots: `tools/shot_*.png` (upstream tree) + `/tmp/old/screen_*.png`.

| # | Feature (v0.1.2) | In v0.2.0? | Plan | Status |
|---|---|---|---|---|
| 1 | Rounded window corners (14px, `decorations:false,transparent:true`) | PARTIAL — `.shell` has `--R-w` radius but themes paint opaque `body` backgrounds (dot texture / sky gradient) which square the corners in the real WebView | themes must decorate `.shell`/`.shell-body`, never `body` (transparent body stays); harness ?max=1 simulates maximized (radius→0) | DONE — analogue.css/digital.css backdrop moved to `.shell`, `data-max="1"` zeroes radius |
| 2 | Rounded UI elements (buttons 6–8px, panels, orbs) | PARTIAL — digital yes; analogue tokens 3–8px OK but `.nav` mis-styled (see #9) | keep per-theme radii; verify everywhere incl. widget | DONE |
| 3 | Seekbar with spectrum visualiser (NBARS=32 LED-dot matrix, bars ride the analyser, played/unplayed alpha, sweep on drag, playhead marker) | LOST — `paintSeek` draws a flat unlit strip (drawMeter ignores bars for unplayed, no dot-matrix look); harness has no bars so shots are dead | port old dot-matrix renderer into `paintSeek` (common.js) + widget `drawSeekLed`; add mock bars to harness (sin/noise field when analyser absent) | DONE — `drawSeekLed` in common.js, mock bars in harness + shots |
| 4 | Glow: accent bloom on active buttons, LED glow, soft phosphor text glow (analogue dark) | LOST — analogue has no glow anywhere; digital dark partial (orb shadows) | per-theme glow tokens: `--glow-accent`, `--glow-text` applied to active/hover controls, lit LEDs, nav active, lyric active | DONE — glow tokens + applied in both themes |
| 5 | Hover states: border+color lift on every button, row hover fill | YES (base.css) | keep, add transition polish | DONE (kept) |
| 6 | Transitions: view-enter fade+slide, menu/menuin, sheet spring, art-swap | YES (base.css) | keep; add nav-collapse animation, play/pause icon morph, toast out | DONE — added nav anim, icon morph, menu scale |
| 7 | Ambient dither field behind now-playing | YES (`fx-ambient`) | keep | DONE (kept) |
| 8 | Dot-grid caps + edge dither band (`drawEdge`) | PARTIAL — dotgrid logo kept; edge band kept (`#edgeCv`) | keep | DONE (kept) |
| 9 | Nav: 148px rail, icon+label items, compact | BROKEN — both theme files style `.nav` as the ITEM (v0.1 architecture); v0.2 `.nav` is the container → items unthemed in analogue, container gets item paint = weird wide buttons; items stretch full width with window | rewrite nav skin for `.nav` container + `.nav-item`; width clamps; 3 states expanded/collapsed/hidden + hover reveal + Ctrl+B; persist `navState` | DONE |
| 10 | Large windows: content bounded, buttons fixed size | PARTIAL — no max-width on library/now-playing; buttons OK | add `.vscroll`/`.np-grid` max-width centering; verify 1920×1080 + 2560×1440 shots | DONE — max-width containers added |
| 11 | Widget buttons consistent (34px orbs, 22px action icons) | PARTIAL — hit areas <32px in strip (`w-actions` 22px), mixed shapes across themes | uniform hit areas ≥32 logical px, theme-consistent shapes, hover/press states | DONE — widget button audit pass |
| 12 | Theme: analogue dark = dithered print + LED/meter | YES | keep + glow (#4), rounded (#1) | DONE |
| 13 | Theme: analogue light = ??? (v0.2.0 has newsprint-y paper) | REPLACE per owner: warm risograph newsprint — off-white paper grain, riso blue #0078BF + fluoro pink #FF48B0 spot inks, misregistration on headings, ink buttons, halftone-dot fills; reference images show no analogue-light so spec stands | new token set + skin | DONE — risograph light skin |
| 14 | Theme: digital light = Frutiger Aero (sky, glossy orbs) | YES | keep + polish | DONE (kept) |
| 15 | Theme: digital dark = liquid glass (REPLACE Frutiger-aero-in-dark per owner) | NO — v0.2.0 dark is a navy aero clone | Apple liquid glass: near-black + art-accent ambient blobs, blur(24–40px) saturate(160–180%) panels, top-lit 1px gradient border, specular sheen, big radii, glass pill buttons, accent bloom glow | DONE — liquid glass dark skin |
| 16 | Motion setting respected (`htSetMotion`) | YES | keep, all new anims token-driven | DONE |
| 17 | Theme switch circular reveal | YES | keep | DONE (kept) |
| 18 | Toast/menu/sheet animations | PARTIAL — menuin kept; toast no out-anim | add toast-out, sheet scale .96→1 | DONE |

## Verification evidence (filled at the end)

- shots: `ui/test/shots/` — full matrix regenerated (main 4 combos @1280+1920,
  nav expanded/collapsed/hidden, widget card+strip in all 4 combos, ctx menu +
  sheet in liquid glass + risograph, seek close-up, compare_old_new.png)
- contrast: `node test-contrast.js` — **ALL PAIRS PASS** (4 combos, incl. new
  risograph light; sampler extended for riso tokens + glass-alpha orbs)
- console: `python3 console_check.py` — main scenarios CLEAN; widget recursion
  bug (drawSeekLed self-delegation) caught here and fixed, suite re-run clean
- `node --check` on every touched JS file — OK

## Known risks (real Windows WebView2 differs from headless chromium)

- window transparency/corners: `html,body` transparent + `.shell` radius —
  correct in chromium; WebView2 backdrop compositing must be eyeballed once
- `backdrop-filter: blur(32px) saturate(170%)` over many panels can cost
  frames on weak GPUs — `--glass-blur` token is the single knob to dial down
- masked `::before` rim needs `-webkit-mask-composite: xor` (WebView2/Chromium
  ships it; if a future engine drops it the rim falls back to a flat border)
