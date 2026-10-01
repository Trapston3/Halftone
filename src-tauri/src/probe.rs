//! probe.rs — bounded metadata reads. Never reads a whole audio file.
//!
//! Scanning used to `fs::read` every file in the library (all audio bytes,
//! plus every .jpg/.txt in the tree). Here each format reads only what its
//! parser needs and hands the existing parsers a synthesized buffer:
//!
//! - FLAC: the metadata blocks (seek over PICTURE when no cover is wanted).
//! - MP3:  the ID3v2 tag + 256 KiB of audio (Xing/Info/VBRI live there);
//!         duration falls back to a CBR estimate from the real file length.
//! - WAV:  every RIFF chunk except the `data` payload (header kept, last).
//! - M4A:  every top-level atom except `mdat`/`free`/`skip` (moov is often
//!         at the end of the file — we seek to it).
//! - ADTS: no container, frame scan needs the whole file (rare format).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::{formats, m4a, TrackMeta};

/// Head window used for magic sniffing and MP3 Xing lookup.
const SNIFF: usize = 64 * 1024;
const MP3_AUDIO_WINDOW: u64 = 256 * 1024;
/// Hard ceiling for any single metadata piece (FLAC block, moov, chunk).
const META_CAP: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { Flac, Wav, Mp3, M4a, Adts, Ogg, Opus }

impl Kind {
    /// Content-Type for the ORIGINAL bytes (ALAC is decided separately).
    pub fn mime(self) -> &'static str {
        match self {
            Kind::Flac => "audio/flac",
            Kind::Wav => "audio/wav",
            Kind::Mp3 => "audio/mpeg",
            Kind::M4a => "audio/mp4",
            Kind::Adts => "audio/aac",
            Kind::Ogg | Kind::Opus => "audio/ogg",
        }
    }
}

pub fn kind_of(head: &[u8]) -> Option<Kind> {
    if head.len() >= 4 && &head[0..4] == b"fLaC" {
        return Some(Kind::Flac);
    }
    // Ogg container: "OggS" + codec sniff from the first packet (Vorbis
    // identification header vs OpusHead). Ogg FLAC/Theora etc. fall through
    // to None (codec_of rejects non-vorbis/opus codecs). Page 0's first
    // packet starts at 27 + nsegs (lacing table is 1 byte here).
    if head.len() >= 4 && &head[0..4] == b"OggS" {
        let nsegs = head.get(26).copied().unwrap_or(0) as usize;
        return head
            .get(27 + nsegs..60 + nsegs)
            .and_then(|b| crate::ogg::codec_of(b, 0))
            .map(|c| match c {
                crate::ogg::Codec::Vorbis { .. } => Kind::Ogg,
                crate::ogg::Codec::Opus { .. } => Kind::Opus,
            });
    }
    match formats::detect(head) {
        Ok("wav") => Some(Kind::Wav),
        Ok("mp3") => Some(Kind::Mp3),
        Ok("m4a") => Some(Kind::M4a),
        Ok("adts") => Some(Kind::Adts),
        _ => None,
    }
}

fn open(path: &Path) -> Result<(File, u64), String> {
    let f = File::open(path).map_err(|e| format!("open {}: {}", path.display(), e))?;
    let len = f.metadata().map_err(|e| format!("stat {}: {}", path.display(), e))?.len();
    Ok((f, len))
}

fn read_at(f: &mut File, off: u64, n: u64) -> Result<Vec<u8>, String> {
    if n > META_CAP {
        return Err(format!("metadata piece too large ({n} bytes)"));
    }
    f.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
    let mut buf = Vec::with_capacity(n as usize);
    f.take(n).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

pub fn sniff_file(f: &mut File) -> Result<Vec<u8>, String> {
    read_at(f, 0, SNIFF as u64)
}

/// FLAC: "fLaC" + metadata blocks. PICTURE blocks are replaced by an empty
/// PADDING block (same last-flag) when `want_cover` is false.
fn flac_buf(f: &mut File, len: u64, want_cover: bool) -> Result<Vec<u8>, String> {
    let mut out = b"fLaC".to_vec();
    let mut off = 4u64;
    loop {
        if off + 4 > len {
            return Err("flac: metadata runs past end of file".into());
        }
        let h = read_at(f, off, 4)?;
        if h.len() < 4 {
            return Err("flac: truncated metadata header".into());
        }
        let last = h[0] & 0x80 != 0;
        let btype = h[0] & 0x7f;
        let blen = ((h[1] as u64) << 16) | ((h[2] as u64) << 8) | h[3] as u64;
        if btype == 6 && !want_cover {
            out.extend_from_slice(&[(h[0] & 0x80) | 1, 0, 0, 0]);
        } else if btype == 1 {
            out.extend_from_slice(&[(h[0] & 0x80) | 1, 0, 0, 0]); // padding: skip the bytes
        } else {
            out.extend_from_slice(&h);
            out.extend(read_at(f, off + 4, blen)?);
        }
        off += 4 + blen;
        if last {
            return Ok(out);
        }
    }
}

/// WAV: RIFF header + all chunks except the `data` payload. The data chunk
/// header goes LAST with zero payload bytes; `parse_wav` trusts its size.
fn wav_buf(f: &mut File, len: u64) -> Result<Vec<u8>, String> {
    let mut out = read_at(f, 0, 12)?;
    let mut data_hdr: Option<Vec<u8>> = None;
    let mut off = 12u64;
    while off + 8 <= len {
        let h = read_at(f, off, 8)?;
        if h.len() < 8 {
            break; // file shorter than its own chunk table says
        }
        let sz = u32::from_le_bytes([h[4], h[5], h[6], h[7]]) as u64;
        if &h[0..4] == b"data" {
            // clamp placeholder sizes (streaming writers) to the real file
            let real = sz.min(len.saturating_sub(off + 8));
            let mut dh = h.clone();
            dh[4..8].copy_from_slice(&(real as u32).to_le_bytes());
            data_hdr = Some(dh);
        } else {
            out.extend_from_slice(&h);
            let body = read_at(f, off + 8, sz.min(len.saturating_sub(off + 8)))?;
            let short = (body.len() as u64) < sz;
            out.extend(body);
            if sz & 1 == 1 && !short {
                out.push(0);
            }
            if short {
                break;
            }
        }
        off += 8 + sz + (sz & 1);
    }
    if let Some(dh) = data_hdr {
        out.extend(dh);
    }
    Ok(out)
}

/// M4A: every top-level atom except media payload atoms.
fn m4a_buf(f: &mut File, len: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut off = 0u64;
    while off + 8 <= len {
        let h = read_at(f, off, 16.min(len - off))?;
        if h.len() < 8 {
            break;
        }
        let sz32 = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as u64;
        let (size, hdr) = match sz32 {
            0 => (len - off, 8),
            1 if h.len() >= 16 => (u64::from_be_bytes(h[8..16].try_into().unwrap()), 16),
            n => (n, 8),
        };
        if size < hdr || off + size > len {
            break; // corrupt tail — keep what we have
        }
        let name = &h[4..8];
        if !matches!(name, b"mdat" | b"free" | b"skip" | b"wide") {
            out.extend(read_at(f, off, size.min(len - off))?);
        }
        off += size;
    }
    Ok(out)
}

/// Parse a track's metadata with bounded reads. `want_cover=false` for scans.
pub fn read_meta(path: &Path, want_cover: bool) -> Result<(TrackMeta, Kind, bool), String> {
    let (mut f, len) = open(path)?;
    if len == 0 {
        return Err("empty file".into());
    }
    let head = sniff_file(&mut f)?;
    let kind = kind_of(&head).ok_or_else(|| {
        formats::detect(&head).err().unwrap_or_else(|| "unrecognized audio format".into())
    })?;
    let mut alac = false;
    let mut m = match kind {
        Kind::Flac => {
            let buf = flac_buf(&mut f, len, want_cover)?;
            crate::track_from_flac_buf(&buf, path)?
        }
        Kind::Wav => formats::parse(&wav_buf(&mut f, len)?, path)?,
        Kind::Mp3 => {
            let tag = head
                .get(6..10)
                .filter(|_| &head[0..3] == b"ID3")
                .map(|b| 10 + (((b[0] as u64 & 0x7f) << 21) | ((b[1] as u64 & 0x7f) << 14)
                    | ((b[2] as u64 & 0x7f) << 7) | (b[3] as u64 & 0x7f)))
                .unwrap_or(0);
            let want = (tag + MP3_AUDIO_WINDOW).min(len);
            let buf = if want == len { read_at(&mut f, 0, len)? } else { read_at(&mut f, 0, want)? };
            formats::parse_mp3_sized(&buf, path, len)?
        }
        Kind::M4a => {
            let (m, is_alac) = m4a::parse_m4a(&m4a_buf(&mut f, len)?, path)?;
            alac = is_alac;
            m
        }
        Kind::Adts => {
            if len > 512 * 1024 * 1024 {
                return Err("adts: file too large to frame-scan".into());
            }
            m4a::parse_adts(&read_at(&mut f, 0, len)?, path)?
        }
        Kind::Ogg | Kind::Opus => {
            // Head pages (through the comment packet) + the last 64 KiB
            // tail read happen inside ogg::parse (bounded on both ends).
            crate::ogg::parse(&head, path, len)?
        }
    };
    // Bitrates from synthesized buffers would be wrong; use the real length.
    if matches!(kind, Kind::M4a | Kind::Flac) && m.duration > 0.0 {
        m.bitrate_kbps = ((len as f64) * 8.0 / m.duration / 1000.0).round() as u32;
    }
    m.format_tag = match kind {
        Kind::Flac => "FLAC",
        Kind::Wav => "WAV",
        Kind::Mp3 => "MP3",
        Kind::M4a if alac => "ALAC",
        Kind::M4a | Kind::Adts => "AAC",
        Kind::Ogg => "OGG",
        Kind::Opus => "OPUS",
    }
    .into();
    m.lossless = matches!(kind, Kind::Flac | Kind::Wav) || alac;
    if !want_cover {
        m.has_cover = m.has_cover || m.cover.is_some();
        m.cover = None;
        m.embedded_lyrics = None;
    } else {
        m.has_cover = m.cover.is_some();
    }
    Ok((m, kind, alac))
}

/// Extensions worth opening during a scan. Magic bytes still decide the
/// actual format — this only avoids opening every .jpg/.txt/.cue.
pub const AUDIO_EXTS: &[&str] =
    &["flac", "mp3", "wav", "wave", "m4a", "mp4", "aac", "alac", "ogg", "oga", "opus", "oggvorbis"];

pub fn is_candidate(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("halftone_probe_{}_{name}", std::process::id()));
        File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    fn wav(data_len: u32, list_after: bool) -> Vec<u8> {
        let mut fmt = b"fmt ".to_vec();
        fmt.extend(16u32.to_le_bytes());
        fmt.extend(1u16.to_le_bytes()); // PCM
        fmt.extend(2u16.to_le_bytes());
        fmt.extend(44100u32.to_le_bytes());
        fmt.extend((44100u32 * 4).to_le_bytes());
        fmt.extend(4u16.to_le_bytes());
        fmt.extend(16u16.to_le_bytes());
        let mut list = b"LIST".to_vec();
        let info = b"INFOINAM\x06\x00\x00\x00Hello\x00";
        list.extend((info.len() as u32).to_le_bytes());
        list.extend(info);
        let mut data = b"data".to_vec();
        data.extend(data_len.to_le_bytes());
        data.extend(vec![0u8; data_len as usize]);
        let mut body = b"WAVE".to_vec();
        body.extend(&fmt);
        if !list_after { body.extend(&list); }
        body.extend(&data);
        if list_after { body.extend(&list); }
        let mut out = b"RIFF".to_vec();
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }

    #[test]
    fn wav_duration_without_reading_payload() {
        // 1 s of 16-bit stereo 44.1k = 176400 bytes; LIST chunk AFTER data
        let p = tmp("a.wav", &wav(176_400, true));
        let (m, k, _) = read_meta(&p, false).unwrap();
        assert_eq!(k, Kind::Wav);
        assert!((m.duration - 1.0).abs() < 1e-6, "{}", m.duration);
        assert_eq!(m.title, "Hello");
        let real = std::fs::metadata(&p).unwrap().len();
        let synth = wav_buf(&mut File::open(&p).unwrap(), real).unwrap();
        assert!(synth.len() < 200, "payload must not be read ({} bytes)", synth.len());
    }

    #[test]
    fn truncated_wav_no_panic() {
        let mut w = wav(1000, false);
        w.truncate(60);
        let p = tmp("t.wav", &w);
        let _ = read_meta(&p, false); // Ok or Err, never a panic
        let _ = wav_buf(&mut File::open(&p).unwrap(), 10_000);
    }

    #[test]
    fn candidate_filter() {
        assert!(is_candidate(Path::new("a/B.FLAC")));
        assert!(is_candidate(Path::new("x.m4a")));
        assert!(!is_candidate(Path::new("cover.jpg")));
        assert!(!is_candidate(Path::new("noext")));
    }

    #[test]
    fn garbage_is_error_not_panic() {
        let p = tmp("g.flac", b"fLaC\x80\xff\xff\xff");
        assert!(read_meta(&p, true).is_err());
        let p = tmp("z.mp3", &[]);
        assert!(read_meta(&p, true).is_err());
    }
}

/// Real-file check against ffmpeg-made fixtures:
/// `HT_FIXTURES=C:\Users\traps\ht-fixtures cargo test --lib fixtures -- --ignored --nocapture`
#[cfg(test)]
mod fixture_tests {
    #[test]
    #[ignore]
    fn fixtures() {
        let Ok(dir) = std::env::var("HT_FIXTURES") else { return };
        let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
        names.sort();
        for p in names.into_iter().filter(|p| super::is_candidate(p)) {
            match super::read_meta(&p, true) {
                Ok((m, k, alac)) => println!(
                    "FIX {:<22} {:?} alac={} fmt={} dur={:.3} title={:?} artist={:?} album={:?} cover={} lyr={:?} kbps={}",
                    p.file_name().unwrap().to_string_lossy(), k, alac, m.format_tag, m.duration, m.title,
                    m.artist, m.album, m.cover.as_ref().map(|c| c.data_len).unwrap_or(0),
                    m.embedded_lyrics, m.bitrate_kbps
                ),
                Err(e) => println!("FIX {:<22} ERR {e}", p.file_name().unwrap().to_string_lossy()),
            }
        }
    }
}
