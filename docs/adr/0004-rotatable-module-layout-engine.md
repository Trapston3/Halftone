# ADR 0004: Direct-Manipulation Layout Studio with Arbitrary-Angle Module Rotation

- **Status**: Accepted
- **Date**: 2026-10-25

## Context
Section 11 of the Halftone v1.1 Build Specification requires a direct-manipulation layout studio (`ui/layout.js`) where users can freely move, resize, and **rotate** any widget module or main-window panel at arbitrary angles (`rotate(deg)`), while preserving full interactive hit-testing (buttons, sliders, scrubbers, scrollbars) inside rotated modules and guaranteeing safety rails so essential controls are never lost.

## Decision
1. **Normalized Spatial Model**: Every module stores `{ x, y, w, h, rot, z, visible, scale, opacity, locked }` normalized to the surface frame, rendered via hardware-accelerated CSS `transform: translate3d(x, y, 0) rotate(rot deg)` with `transform-origin: center center`.
2. **Inverse-Rotated Pointer Coordinate Mapping**: Because CSS transforms automatically transform the browser's DOM hit-test tree for clicks, native `<button>` clicks work at any rotation angle. For custom pointer drag operations inside rotated modules (such as scrubbing the seek bar or dragging the volume slider at `37°`), pointer deltas `(dx, dy)` are projected onto the module's local rotated axis via `localDx = dx * cos(-θ) - dy * sin(-θ)`.
3. **Magnetic Snapping & Safety Rails**:
   - Dragging snaps to an `8px` grid and center/edge alignment guides (`1px` accent lines).
   - Rotation supports free angle (`-180°` to `+180°`), `15°` increments when holding `Shift`, and magnetic detents (`±3°`) at `0°`, `90°`, `180°`, and `270°`.
   - Oriented bounding box (OBB) clamping keeps rotated modules inside the window frame, and `transport-cluster` (Play/Pause) + `window-controls` cannot be hidden or dragged off-screen.
4. **7 Built-In Presets + Undo/Redo**: Ships `Classic`, `Compact Strip`, `Square Art`, `Vertical Rack`, `Lyrics Focus`, `Meter Bar`, and `Tilted Desk` (which showcases tasteful `2°–6°` analogue hardware tilt), plus a 50-step Undo/Redo stack (`Ctrl+Z` / `Ctrl+Y`) and JSON profile export/import.

## Consequences
- Users can craft playful Teenage Engineering / desk-hardware angled layouts or ultra-clean orthogonal glass grids.
- Every control remains 100% clickable and scrubbable at any rotation angle, verified by automated CDP hit-test audits.
