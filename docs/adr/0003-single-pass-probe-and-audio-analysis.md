# ADR 0003: Single-Pass Audio Probing, `art_thumb` OKLCH Accent & `analyze_track` Loudness/Spectral Engine

- **Status**: Accepted
- **Date**: 2026-10-25

## Context
Scanning large local music libraries (10,000–50,000+ tracks across MP3, FLAC, OGG, Opus, M4A/AAC/ALAC, WAV, AIFF, WavPack) must be fast, accurate even when file extensions are wrong, and provide rich audiophile metadata (waveform peaks, integrated LUFS, ReplayGain, fake-lossless spectral cutoff detection, and contrast-safe album art accents).

## Decision
1. **Single-Pass Probing (`src-tauri/src/probe.rs` + `formats.rs`)**: Detect true container/codec by magic bytes first (`fLaC`, `OggS`, `RIFF....WAVE`, `FORM....AIFF`, `wvpk`, `ftyp`, `ID3` / MPEG sync), extract tags, cover bytes, exact sample-accurate duration, and LAME gapless delay/padding in one open handle.
2. **Thumbnail & OKLCH Accent Pipeline (`src-tauri/src/art_thumb.rs`)**: Downscale cover art once to a cached `256×256` PNG/WebP thumbnail on disk (`covers/thumbs/<hash>.png`), extract dominant palette colors via chroma-weighted median-cut in OKLab/OKLCH, and clamp lightness (`L`) so `--cover-accent` always satisfies WCAG AA (`>= 4.5:1` against text, `>= 3.0:1` for UI controls).
3. **Track Audio Analysis (`src-tauri/src/analyze_track.rs`)**: Compute and cache 128-bucket normalized waveform peaks, EBU R128 / BS.1770 integrated loudness (`LUFS`), true peak (`dBTP`), ReplayGain track/album gain, and high-frequency spectral roll-off (`kHz`) to flag transcode/fake-lossless files.
4. **Canonicalized `media://` Security**: `serve_media` canonicalizes requested file paths and verifies membership in allowlisted library roots or explicitly opened tracks before serving HTTP 206 byte ranges, and caps decoded lossless fallback buffers at `512 MiB`.

## Consequences
- Fast library grid scrolling (`256px` cached thumbnails instead of multi-megabyte full covers).
- Guaranteed readable dynamic cover accents in both Light and Dark modes.
- Instant waveform scrubbers, automatic loudness normalization, and fake-lossless badges in Library Health.
