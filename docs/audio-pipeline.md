# Audio Pipeline Contract — Multi-Format (v0.2)

Status: **VERIFIED** against real fixture files of every type (2026-09-17,
release build + CDP; ALAC exactness proven byte-identical vs ffmpeg; Ogg
Vorbis/Opus durations cross-checked against ffprobe on ffmpeg-made fixtures
2026-10-01).
This is the binding contract for the production audio backend.

## The contract

1. **Original bytes in, decode by the browser.** The player reads audio files
   from disk and hands the *original bytes* to the audio element via the
   format-neutral `media://` protocol (Windows WebView2 resolves custom
   schemes as `http://media.localhost/`). One documented exception: ALAC —
   see below.
2. **Single read per track.** Metadata, cover art, and the playback URL come
   out of ONE read of the file (open_track / scan_library). No second pass.
3. **Format detection is by MAGIC BYTES, never file extension.** A `.mp3`
   named file with FLAC content is FLAC.
4. **Single-audio-owner architecture unchanged.** All formats play through
   the owner window's one `<audio>` element; the widget renders broadcasts.
5. **LOSSLESS identity stays visible.** Every track carries
   `format` + `lossless`; the Now Playing tech line shows a
   `LOSSLESS` / `LOSSY` badge so lossy sources never blend in silently.

## Per-format contract table

| Format | Container/Parser (Rust) | Tags source | Cover art | Duration/bitrate | Browser decode | Byte path |
|--------|------------------------|-------------|-----------|------------------|----------------|-----------|
| **FLAC** | existing `walk_flac` (lib.rs) — untouched, verified | VORBIS_COMMENT | METADATA_BLOCK_PICTURE | STREAMINFO (exact) | native `<audio>` | original FLAC bytes, bit-exact-to-file |
| **WAV** | `formats.rs` RIFF walker | LIST/INFO chunk **or** bolted-on ID3 chunk (both handled) | none (spec: no WAV art convention; fallback cover) | `data` chunk size (exact) | native `<audio>` (PCM) | original WAV bytes, bit-exact-to-file |
| **MP3** | `formats.rs` ID3v2.3+v2.4 walker | ID3v2 text frames | APIC frame | Xing/Info/VBRI when present (read at the canonical post-side-info slot, never frame-scanned), else full frame-scan, else CBR estimate | native `<audio>` | original MP3 bytes (lossy source, served as-is) |
| **M4A/AAC** | `m4a.rs` MP4 atom tree | moov/udta/meta/ilst | `covr` atom | moov/trak mdhd (exact) | native `<audio>` | original M4A bytes (lossy source, served as-is) |
| **M4A/ALAC** | `m4a.rs` (same atoms; `stsd` fourcc `alac`) | moov/udta/meta/ilst | `covr` atom | moov/trak mdhd (exact) | **none — Symphonia decode in Rust** | decoded-PCM WAV wrapper (see below) |
| **ADTS (.aac)** | `m4a.rs::parse_adts` frame scan | none (no container tags) | none | frame count × 1024 / rate | native `<audio>` | original AAC bytes (lossy source, served as-is) |
| **Ogg Vorbis** | `ogg.rs` Ogg page walker (new) | `\x03vorbis` comment header | `METADATA_BLOCK_PICTURE` (base64 FLAC PICTURE) | last-page granule / rate (exact; verified vs ffprobe) | native `<audio>` (Chromium/WebView2 decode) | original Ogg bytes (lossy source, served as-is) |
| **Ogg Opus** | `ogg.rs` Ogg page walker (new) | `OpusTags` comment header | `METADATA_BLOCK_PICTURE` (base64 FLAC PICTURE) | (last granule − pre-skip) / 48000 — playable duration | native `<audio>` (Chromium/WebView2 decode) | original Ogg bytes (lossy source, served as-is) |

### Bounded reads (all formats)

Every metadata parse reads **two small windows**, never the whole file:

- the **head** — headers/comment tags/cover art live there; parsing stops as
  soon as the comment packet completes (Ogg: pages walked until the comment
  packet ends, capped at 64 MiB for pathological art), and
- for duration formats without a header-stated length (MP3, Ogg), a **tail**
  read of the last ~64 KiB from the end (last Xing frame / last Ogg page
  granule).

FLAC/WAV/M4A state duration in their headers, so no tail read is needed.
A scan of a 10,000-track library reads megabytes per track at most once, and
the scan index makes unchanged files free on rescan.

### The ALAC exception (decompression, not transcode)

Chromium has no ALAC decoder. Loading an ALAC track triggers a one-time
Rust-side decode of the whole file to interleaved integer PCM (Symphonia
0.5.5, features `alac`+`isomp4` only — no hand-rolled decoder), wrapped in a
minimal 44-byte RIFF/WAVE header and served through the same `media://`
byte-range protocol. Seek is a byte offset into already-decoded PCM.

**This is lossless decompression, not a lossy transcode** — the PCM is
proven byte-identical to ffmpeg's independent ALAC decoder output
(`alac_exactness` harness, EXACT MATCH on 529,200 bytes). No audio data is
lost at any stage; `LOSSLESS` badge is truthful.

Memory guard: decode refuses output above 512 MiB with an explicit error
(a ~5-minute 24/96 song decodes to ~160 MB — within budget; pathological
inputs are rejected, not OOM-killed).

Decode cache: ONE decoded file is held at a time (`(path, mtime) → WAV
bytes`, single-entry mutex cache). Re-seeking within the same track and
replayed ranges slice the cached WAV in memory — no re-decode per range.

### Range serving cap

Open-ended ranges (`bytes=N-`, what the media element asks for) are served
but capped at **2 MiB per response** (`RANGE_CAP` in lib.rs). The element
re-requests as it plays; no request ever pulls a whole file. Valid ranges
beyond the cap return the first 2 MiB of the requested window (a legal 206;
the browser just comes back for more).

## Scan behavior

`scan_library` detects every file by content signature:

- Supported (FLAC/WAV/MP3/M4A-AAC/M4A-ALAC/ADTS/Ogg Vorbis/Ogg Opus):
  parsed, counted per-format, included in the library with `format`/
  `lossless` fields.
- Genuinely unsupported (WMA/AIFF/APE/WV — by extension — or a corrupt
  file): reported by name through the skipped-file mechanism and per-format
  `unsupported` count.
- Non-audio junk: ignored silently.

The final `halftone:scan-progress` event carries
`counts: {flac, wav, mp3, aac, alac, ogg, opus, unsupported}`.

## What is bit-exact vs decoded

- **Bit-exact to the source file** (served bytes = file bytes): FLAC, WAV,
  MP3, AAC, ADTS, Ogg Vorbis, Ogg Opus. Whatever the file contains reaches
  the decoder unmodified.
- **Documented lossless decode step** (served bytes = decoded PCM): ALAC only.
  Proven exact vs an independent decoder; still labeled LOSSLESS because the
  transformation is lossless by construction and by measurement.

## Legacy notes

- The `flac://` protocol still resolves (aliased to the same handler) so
  stale cached URLs from v0.1.2 don't break; all new code emits `media://`.
- FLAC `bitrate_kbps` is 0 (unknown/VBR) — FLAC frame bitrates vary and the
  UI hides zero-bitrate readouts.
