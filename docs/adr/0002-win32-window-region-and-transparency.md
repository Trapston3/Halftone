# ADR 0002: Win32 `SetWindowRgn` & Native Transparency Discipline

- **Status**: Accepted
- **Date**: 2026-10-25

## Context
On Windows 10/11, frameless Tauri/WebView2 windows (`decorations: false`, `transparent: true`) often exhibit dark rectangular corner bleed or white background flash if `DwmExtendFrameIntoClientArea` / `SetWindowCompositionAttribute` is used alongside CSS `border-radius`, or if `html, body` defines an opaque background color.

## Decision
1. Both windows (`main` and `widget`) configure `"decorations": false`, `"transparent": true`, `"shadow": false` in `src-tauri/tauri.conf.json`.
2. In `src-tauri/src/lib.rs` (`win_corners`), we apply a true pixel-level rounded window region via Win32 `CreateRoundRectRgn` + `SetWindowRgn` (`16px` logical radius scaled by DPI) on initial creation and on `WindowEvent::Resized` / `ScaleFactorChanged`, and expose `ht_get_corner` so automated QA can assert that corner screen pixels outside the radius are clipped by the OS (`outside_in_rgn == false`).
3. `html` and `body` in both windows remain strictly `background: transparent !important`; the rounded theme background and border are painted exclusively on the inner `#app-frame` / `#widget-frame` container.

## Consequences
- Crisp, zero-bleed rounded corners on both Windows 10 and Windows 11 across all DPI scale factors.
- Directly verifiable in release builds via the `ht_get_corner` Tauri command.
