## Variant: Phosphor — CRT deck

### Design stance
An instrument panel, not a music card: horizontal deck where an LED segment meter IS the seek bar — dev-tool register, Pip-Boy cyan on cream.

### Key choices
- Layout: 400px horizontal deck — 96px art, meta column, controls row; reads like a hardware front panel
- Seek/visualizer: 48-column LED meter of 4px cells; played segments glow full cyan, unplayed stay dark; cream caret cursor + arrowhead
- Typography: 10px mono caps for ALL metadata (TRK 01/04, FLAC 44.1 kHz), 14px/600 title — instrument-label hierarchy
- Depth: NO soft shadows on components — border tiers (#262B30 → #3A4147 → cyan) carry elevation; one widget-level shadow only
- Volume: 5-LED stepped slider instead of a range input (panel metaphor); scanline overlay on the whole deck

### Trade-offs
- Strong at: compact height (~230px open), hardware personality, art never fights the controls
- Weak at: mono caps everywhere costs warmth; album art is small

### Best for
Power users and terminal-heavy desktops; the widget that matches a dev-tool setup.
