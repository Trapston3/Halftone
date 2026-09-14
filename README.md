# Halftone

*(working title — naming is open)*

A floating desktop mini-player widget for a local music player.
Free, open, and built to be contributed to. Cross-platform, primary target Windows.

**Status: Pass 1 — interaction model + design directions (interactive sketches).**
Open the three variants in a browser and click around; every control is live.

| Variant | Stance | Open it |
|---|---|---|
| Ink | Monochrome editorial — art as hero, quiet chrome | `sketches/001-ink/index.html` |
| Phosphor | CRT instrument deck — LED meter seek bar | `sketches/002-phosphor/index.html` |
| Pulp | Comic print — ink outlines, offset shadows, neon | `sketches/003-pulp/index.html` |

Screenshots of each state live in `sketches/shots/`.

## What the widget does

- Floating, frameless, always-on-top, draggable window (art area = drag handle), pinnable
- One integrated seek/visualizer: the spectrum bars ARE the progress bar — hover to expand, drag to seek with a time tooltip
- Play/pause, next/previous, precise drag-to-seek
- Synced lyrics: line-by-line highlight timed to playback, smooth center-anchored auto-scroll
- Queue, add-to-playlist, volume

Design rules the sketches are built under (carried into production):
one 4px spacing scale, one elevation logic per variant, one typographic hierarchy,
one signature animated moment (the seek/visualizer interaction) — everything else stays still.

## Architecture plan

```
halftone/
  desktop shell (Tauri or Electron — pass 2 decision)
  ├─ widget window  (this UI, frameless, always-on-top)
  ├─ player core    (playback, queue, playlists, volume — pure TS, shell-agnostic)
  │   └─ source providers (plugin seam)
  │       ├─ local-library  (ships first: real files, tags, .lrc/embedded lyrics)
  │       └─ ...            (community plugins; each provider owns its own licensing)
```

The core plays a **local music library** — your own files, real metadata,
embedded and `.lrc` synced lyrics. Streaming/source plugins are a deliberate seam:
third parties can contribute providers, each responsible for complying with the
licenses of whatever it touches. The core itself never ships a bundled
streaming source.

## Roadmap

- [x] Pass 1 — interaction model + 3 design directions (this repo)
- [ ] Pass 2 — desktop shell: real frameless always-on-top window, real audio + live analyser
- [ ] Pass 3 — real library: scan folders, read tags (music-metadata), embedded/LRC synced lyrics
- [ ] Pass 4 — queue + playlists persisted, volume/memory
- [ ] Pass 5 — polish pass, Windows packaging/installer, contributor docs

## Contributing

Early-stage: the design language and architecture skeleton are being set now,
which is the highest-leverage time to weigh in. Open an issue with `design:`,
`core:`, or `plugin:` prefix. Pass-2+ PRs welcome once the shell lands.

## License

MIT (to be finalized with first tagged release).
