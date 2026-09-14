# Audio Pipeline Contract — Direct FLAC

Status: **VERIFIED** against real library files (2026-09-14, headless Chrome 153 via CDP).
This is the binding contract for the production audio backend.

## The contract

1. **FLAC in, FLAC out.** The player reads `.flac` files from disk and hands the
   *original bytes* to the audio element (object URL / custom scheme). There is
   **no transcoding** — no AAC, no PCM re-encode, no ffmpeg anywhere in the
   playback path. What decodes is the file's own FLAC stream, bit-exact.

2. **Single read per track.** One user-space read of the file per track load
   (browser: one `arrayBuffer()` on the File; desktop shell: one
   `fs::read`/`convertFileSrc` stream). Metadata, cover, and playback all
   derive from that one buffer. No second fetch for tags, none for artwork.

3. **Embedded cover art comes from METADATA_BLOCK_PICTURE** (FLAC block type 6)
   in the same read that yields STREAMINFO and VORBIS_COMMENT. There is no
   separate cover step, no folder.jpg convention, no network lookup. If a file
   has no PICTURE block, the widget shows the fallback cover — it never tries
   to "find" art elsewhere.

4. **Metadata from VORBIS_COMMENT** (block type 4): TITLE, ARTIST, ALBUM, and
   the rest, parsed from the same buffer. `STREAMINFO` (block type 0) is the
   source of truth for duration/bits/channels; it must agree with the audio
   element's reported duration (verified to < 1 ms in practice).

## Demo tooling is NOT the pipeline

`tools/make_embed.py` (base64 AAC excerpts + separate cover JPEGs +
`index.local.html` splice) exists only so the Phosphor sketch can demo against
real music inside a single double-clickable HTML file without shipping
copyrighted bytes. It **must not** influence the real backend: no AAC excerpts,
no pre-extracted covers, no embed JSON. The real path is
`tools/demo_playlist.html` — pick a FLAC, one read, direct playback — and that
is the reference implementation the production backend will mirror (Tauri: read
file once in Rust, serve bytes via custom protocol / `asset:` scope; parse
STREAMINFO/VORBIS_COMMENT/PICTURE in the same pass).

## Verification (2026-09-14)

Real files from the local library, driven through the actual file-picker path
via CDP `DOM.setFileInputFiles`, headless Chrome 153:

| file | STREAMINFO | duration SI vs element | PICTURE block | reads |
|---|---|---|---|---|
| Her's — Harvey.flac (66 MB) | 96 kHz · 24-bit · stereo | 211.17192 vs 211.17192 s | image/jpeg 230,234 B | 1 |
| Eagles — Hotel California (2013 Remaster).flac (239 MB) | 192 kHz · 24-bit · stereo | 391.36338 vs 391.36338 s | image/jpeg 361,258 B | 1 |
| Bruno Mars — 24K Magic.flac (27 MB) | 44.1 kHz · 16-bit · stereo | 226.92345 vs 226.92345 s | image/jpeg 186,846 B | 1 |

Additional verified properties:
- Block walk found `[STREAMINFO, VORBIS_COMMENT, PICTURE, PADDING+last]` — i.e.
  the picture really is a metadata block, not an ID3 bolt-on.
- Tags (TITLE/ARTIST/ALBUM) parsed from VORBIS_COMMENT in the same read.
- Audio element source is the original FLAC bytes (`blob:` URL, MIME
  `audio/flac`, `canPlayType('audio/flac')` = true in Chromium 153).
- AnalyserNode meter confirmed live (max bar 0.81 → 0.72 across samples) with
  the element playing the direct FLAC — the visualizer needs no decoded-PCM
  side channel; it works through the same MediaElementSource tap.
- Zero console/unhandled errors across all loads.

## Notes / known edges

- FLAC PICTURE `width`/`height` fields are 0x0 in these files (legal —
  dimensions optional); the demo reads them but falls back to the JPEG's own
  dimensions when painting. Production should do the same.
- Some encoders put PADDING as the last block (type 1, 0x81 header). The walk
  honors the last-block flag and never scans past it.
- WAV/MP3/M4A are out of scope for the first release; the seam is the source
  provider, same as streaming.
