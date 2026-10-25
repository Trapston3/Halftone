# ADR 0001: Dual-Window Single-Audio-Owner Architecture

- **Status**: Accepted
- **Date**: 2026-10-25

## Context
Desktop music players with both a full library window and a compact mini-player widget frequently suffer from audio glitches, duplicate decoding, desynchronized playback state, or playback stopping when switching between full and mini modes.

## Decision
Halftone enforces a strict **Single Audio Owner** architecture:
1. **`ui/main.html` (Main Window)** is the sole owner of the HTML5 `<audio>` element (plus the secondary gapless/crossfade preloader `<audio>`) and the WebAudio DSP graph (`AudioContext -> MediaElementSource -> Preamp -> 10-Band BiquadFilter EQ -> Crossfeed/Balance/Mono -> AnalyserNode -> Destination`).
2. **`ui/index.html` (Widget Window)** is a pure stateless/reactive viewer and controller. It never instantiates an `<audio>` element.
3. Closing the Main Window while playback is active or the Widget is visible intercepts the close event and hides the Main Window instead of destroying its WebView, keeping audio and WebAudio DSP uninterrupted.
4. Cross-window synchronization uses a dual-channel bridge (`BroadcastChannel('halftone-sync')` + `localStorage` storage events + Tauri `emit`/`listen` events) with throttled 60fps visualizer bin sharing only when the widget is visible.

## Consequences
- Zero audio dropouts when toggling between Main Window and Widget (`Ctrl+M`).
- Zero duplicate memory or CPU decoding overhead.
- Hidden windows automatically pause their `requestAnimationFrame` loops (`document.hidden`), yielding `< 1%` idle CPU.
