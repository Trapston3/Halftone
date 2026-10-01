//! formats.rs — bounded, panic-free parsers for the non-FLAC audio formats
//! Halftone Phase 1 supports: WAV (RIFF + LIST/INFO + ID3 chunks), MP3
//! (ID3v2.3/v2.4 text tags + APIC, CBR/VBR via Xing/Info/VBRI or frame scan),
//! MP4/M4A (moov/udta/meta/ilst tags incl. covr; AAC `mp4a` vs ALAC `alac`
//! via stsd fourcc), and raw AAC ADTS.
//!
//! Design rules (mirrors the FLAC pipeline contract):
//! - One buffer in (`buf` = the file bytes; for MP3 the FULL file should be
//!   supplied — see `parse`), metadata out. No second read inside the module.
//! - Magic-based detection, never file extensions.
//! - Every length field is bounds-checked against the buffer; malformed or
//!   truncated input yields `Err(String)`, never a panic.
//! - FLAC stays on the pre-existing `walk_flac` in lib.rs — untouched here.

use std::collections::HashMap;
use std::path::Path;

use crate::{Picture, StreamInfo, TrackMeta};
use crate::m4a;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Detect the audio format from magic bytes only (never the extension).
///
/// Returns one of `"wav" | "mp3" | "m4a" | "adts"` — or `Err` when the magic
/// does not match a supported container. FLAC is handled by the caller
/// (`walk_flac`) and is intentionally NOT detected here.
pub fn detect(buf: &[u8]) -> Result<&'static str, String> {
    // --- RIFF/WAVE ----------------------------------------------------------
    if buf.len() >= 12 && &buf[0..4] == b"RIFF" && &buf[8..12] == b"WAVE" {
        return Ok("wav");
    }
    // --- ID3v2-tagged MP3 ---------------------------------------------------
    if buf.len() >= 10 && &buf[0..3] == b"ID3" && buf[3] != 0xFF && buf[4] != 0xFF {
        // A trailing music data or 0 padding follows the tag. Accept versions
        // 2/3/4; anything else is not an ID3v2 tag we can walk.
        if (2..=4).contains(&buf[3]) {
            return Ok("mp3");
        }
    }
    // --- MPEG audio frame sync (bare MP3 or ADTS) ----------------------------
    if buf.len() >= 4 && buf[0] == 0xFF && (buf[1] & 0xE0) == 0xE0 {
        // ADTS: 0xFFF sync, MPEG version bits != reserved (01), layer == 00.
        // Layout of byte 1: [MPEG2 flag][MPEG4 flag = layer bits][protect].
        // sync(11111111) then byte1 = 111SS00P where SS in {11,10} (version),
        // layer 00, P = protection bit → (b1 & 0xF6) == 0xF0.
        if (buf[1] & 0xF6) == 0xF0 {
            return Ok("adts");
        }
        // Otherwise an MPEG audio frame (Layer I/II/III). Only Layer III is
        // MP3, but Layer I/II files are rare and still "mp3 container" for
        // our purposes — parse() validates the frame headers properly.
        return Ok("mp3");
    }
    // --- MP4 family (M4A) ----------------------------------------------------
    // ftyp is required to be the first box; brand check happens in parse().
    if buf.len() >= 12 && &buf[4..8] == b"ftyp" {
        return Ok("m4a");
    }
    let n = buf.len().min(8);
    Err(format!(
        "unsupported or unrecognized audio format (first {n} bytes {:02x?})",
        &buf[..n]
    ))
}

/// Parse one track's metadata from `buf` (the file's bytes) in a single pass.
///
/// `path` is used only for the `path` field and the filename fallback for a
/// missing TITLE tag — exactly like the FLAC path in lib.rs.
///
/// NOTE for callers: for MP3 (VBR without Xing/Info/VBRI) and for correct
/// average bitrate, `buf` should be the FULL file, not a header window.
pub fn parse(buf: &[u8], path: &Path) -> Result<TrackMeta, String> {
    let kind = detect(buf)?;
    match kind {
        "wav" => parse_wav(buf, path),
        "mp3" => parse_mp3(buf, path),
        "m4a" => m4a::parse_m4a(buf, path).map(|(m, _)| m),
        "adts" => m4a::parse_adts(buf, path),
        _ => Err(format!("unsupported format {kind:?}")),
    }
}

// ---------------------------------------------------------------------------
// Small shared helpers (no panics on short buffers)
// ---------------------------------------------------------------------------

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn le16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

/// ID3v2 unsynchronized 32-bit ("syncsafe") integer.
fn syncsafe(b: &[u8]) -> u32 {
    ((b[0] as u32 & 0x7F) << 21)
        | ((b[1] as u32 & 0x7F) << 14)
        | ((b[2] as u32 & 0x7F) << 7)
        | (b[3] as u32 & 0x7F)
}

fn id3_tag_size(buf: &[u8]) -> Option<usize> {
    // "ID3" ver(1) rev(1) flags(1) size(4, syncsafe)
    if buf.len() < 10 || &buf[0..3] != b"ID3" {
        return None;
    }
    Some(10 + syncsafe(&buf[6..10]) as usize)
}

// ---------------------------------------------------------------------------
// ID3v2 shared tag walker (used by MP3 files and the 'id3 ' chunk in WAV)
// ---------------------------------------------------------------------------

pub(crate) struct Id3Tags {
    pub text: HashMap<String, String>,
    /// first APIC (or v2 PIC): (mime, raw image bytes)
    pub apic: Option<(String, Vec<u8>)>,
}

/// Decode an ID3 text field per its encoding byte (0 latin1, 1 UTF-16 w/ BOM,
/// 2 UTF-16BE, 3 UTF-8). Tolerant of trailing NULs and bad sequences.
fn decode_id3_string(enc: u8, raw: &[u8]) -> String {
    // strip trailing NULs (multiple terminators are common in the wild)
    let raw = match enc {
        1 | 2 => {
            let end = raw.chunks(2).position(|c| c == [0, 0]).map(|i| i * 2).unwrap_or(raw.len());
            &raw[..end]
        }
        _ => {
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            &raw[..end]
        }
    };
    let s = match enc {
        0 => raw.iter().map(|&b| b as char).collect::<String>(), // latin1
        1 => {
            if raw.starts_with(&[0xFE, 0xFF]) {
                decode_utf16(&raw[2..], true)
            } else if raw.starts_with(&[0xFF, 0xFE]) {
                decode_utf16(&raw[2..], false)
            } else {
                decode_utf16(raw, true) // assume big-endian without BOM
            }
        }
        2 => decode_utf16(raw, true),
        _ => String::from_utf8_lossy(raw).to_string(), // 3 = UTF-8, others treated as UTF-8
    };
    s
}

fn decode_utf16(bytes: &[u8], big_endian: bool) -> String {
    let mut units = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i + 1 < bytes.len() {
        let u = if big_endian {
            u16::from_be_bytes([bytes[i], bytes[i + 1]])
        } else {
            u16::from_le_bytes([bytes[i], bytes[i + 1]])
        };
        units.push(u);
        i += 2;
    }
    String::from_utf16_lossy(&units)
}

/// Walk an ID3v2 tag collecting text frames + the first APIC. Malformed
/// frames abort the walk (tags collected so far are kept) — never panics.
fn walk_id3(buf: &[u8]) -> Id3Tags {
    let mut out = Id3Tags { text: HashMap::new(), apic: None };
    let Some(tag_len) = id3_tag_size(buf) else { return out };
    let version = buf[3];
    let tag_end = tag_len.min(buf.len());
    let mut p = 10usize;

    // Extended header present? (v3: 4-byte size, itself excluded; v4: syncsafe
    // size, itself included)
    if tag_end - p > 6 && buf[5] & 0x40 != 0 {
        if version == 4 {
            let ext = syncsafe(&buf[p..p + 4]) as usize;
            p += ext.clamp(4, tag_end - p);
        } else {
            let ext = be32(&buf[p..p + 4]) as usize + 4;
            p += ext.clamp(6, tag_end - p);
        }
    }

    while p < tag_end {
        // Frame header lengths: v2 = 6 bytes (3-char id, 3-byte size),
        // v3/v4 = 10 bytes (4-char id, 4-byte size, 2 flags).
        let (id, mut fsize, mut body_off) = if version == 2 {
            if p + 6 > tag_end {
                break;
            }
            let id = [buf[p], buf[p + 1], buf[p + 2]];
            if id == [0, 0, 0] {
                break; // padding
            }
            let sz = ((buf[p + 3] as usize) << 16) | ((buf[p + 4] as usize) << 8) | buf[p + 5] as usize;
            (id.to_vec(), sz, p + 6)
        } else {
            if p + 10 > tag_end {
                break;
            }
            let id = buf[p..p + 4].to_vec();
            if id[0] == 0 {
                break; // padding
            }
            let sz = if version == 4 {
                syncsafe(&buf[p + 4..p + 8]) as usize
            } else {
                be32(&buf[p + 4..p + 8]) as usize
            };
            (id, sz, p + 10)
        };

        let flags = if version >= 3 && p + 10 <= tag_end { buf[p + 9] } else { 0 };
        // Compressed / encrypted / grouped frames we can't decode: skip body.
        if flags & 0b1100_0000 != 0 && version == 3 {
            // v3 flags: %abc00000 — c=compression, a=tag alter... frame-level
            // compression is bit 7 of the second flags byte which we don't
            // track; be conservative and still try (most encoders don't set).
        }
        if fsize > tag_end - body_off {
            // truncated or corrupt frame — take what's there, then stop
            fsize = tag_end - body_off;
        }
        let body = &buf[body_off..body_off + fsize];

        // v4 data-length indicator (flag 0x01): first 4 bytes are the size.
        let mut data = body;
        if version == 4 && flags & 0x01 != 0 && body.len() >= 4 {
            data = &body[4..];
        }

        match id.as_slice() {
            b"APIC" | b"PIC" => {
                if out.apic.is_none() {
                    out.apic = parse_apic(version, data);
                }
            }
            _ => {
                if id.as_slice() == b"TXXX" && data.len() > 1 {
                    // user text: enc, description\0, value -> key "TXXX:DESC"
                    let enc = data[0];
                    let rest = &data[1..];
                    let split = if enc == 1 || enc == 2 {
                        rest.chunks(2).position(|c| c == [0, 0]).map(|i| (i * 2, i * 2 + 2))
                    } else {
                        rest.iter().position(|&b| b == 0).map(|i| (i, i + 1))
                    };
                    if let Some((a, b)) = split.filter(|&(_, b)| b <= rest.len()) {
                        let desc = decode_id3_string(enc, &rest[..a]).to_uppercase();
                        let val = decode_id3_string(enc, &rest[b..]);
                        if !val.is_empty() {
                            out.text.entry(format!("TXXX:{desc}")).or_insert(val);
                        }
                    }
                } else if id[0] == b'T' {
                    // Text frame: first byte = encoding, rest = value.
                    let enc = data.first().copied().unwrap_or(0);
                    let val = data.get(1..).unwrap_or(&[]);
                    let s = decode_id3_string(enc, val);
                    let key = String::from_utf8_lossy(&id).to_uppercase();
                    if !s.is_empty() {
                        out.text.entry(key).or_insert(s);
                    }
                }
                if id.as_slice() == b"USLT" && data.len() > 4 && !out.text.contains_key("USLT") {
                    // enc(1) lang(3) descriptor(terminated) text
                    let enc = data[0];
                    let rest = &data[4..];
                    let skip = if enc == 1 || enc == 2 {
                        rest.chunks(2).position(|c| c == [0, 0]).map(|i| i * 2 + 2)
                    } else {
                        rest.iter().position(|&b| b == 0).map(|i| i + 1)
                    };
                    if let Some(k) = skip.filter(|&k| k <= rest.len()) {
                        let mut lyr = &rest[k..];
                        // UTF-16 text after the descriptor carries its own BOM
                        if enc == 1 && lyr.len() < 2 { lyr = &[]; }
                        let t = decode_id3_string(enc, lyr);
                        if !t.trim().is_empty() {
                            out.text.insert("USLT".into(), t);
                        }
                    }
                }
                // Everything else (COMM, TSSE, ...) ignored.
            }
        }

        // advance: body_off + fsize is the next frame
        p = body_off + fsize;
    }
    out
}

/// Extract (mime, bytes) from an APIC (v3/v4) or PIC (v2) frame body.
fn parse_apic(version: u8, data: &[u8]) -> Option<(String, Vec<u8>)> {
    if data.is_empty() {
        return None;
    }
    let enc = data[0];
    let mut q = 1usize;
    let mime;
    if version == 2 {
        // v2 PIC: 3-char image format ("JPG"/"PNG")
        if q + 3 > data.len() {
            return None;
        }
        mime = match String::from_utf8_lossy(&data[q..q + 3]).to_lowercase().as_str() {
            "jpg" => "image/jpeg".to_string(),
            "png" => "image/png".to_string(),
            other => format!("image/{other}"),
        };
        q += 3;
    } else {
        // v3/v4 APIC: NUL-terminated latin1 mime
        let end = data[q..].iter().position(|&b| b == 0)? + q;
        mime = String::from_utf8_lossy(&data[q..end]).to_string();
        q = end + 1;
    }
    // picture type byte
    if q >= data.len() {
        return None;
    }
    q += 1;
    // description: encoded, NUL-terminated (UTF-16 = 0x00 0x00 terminator)
    if enc == 1 || enc == 2 {
        while q + 1 < data.len() {
            if data[q] == 0 && data[q + 1] == 0 {
                q += 2;
                break;
            }
            q += 2;
        }
    } else {
        while q < data.len() && data[q] != 0 {
            q += 1;
        }
        q += 1;
    }
    if q > data.len() {
        return None;
    }
    Some((mime, data[q..].to_vec()))
}

// ---------------------------------------------------------------------------
// WAV (RIFF)
// ---------------------------------------------------------------------------

fn parse_wav(buf: &[u8], path: &Path) -> Result<TrackMeta, String> {
    // RIFF header: "RIFF" <u32 size> "WAVE"
    if buf.len() < 12 || &buf[0..4] != b"RIFF" || &buf[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".into());
    }

    let mut fmt_tag: u16 = 0;
    let mut channels: u8 = 0;
    let mut sample_rate: u32 = 0;
    let mut bits: u8 = 0;
    let mut data_len: Option<u64> = None;
    let mut tags: HashMap<String, String> = HashMap::new();

    let mut p = 12usize;
    while p + 8 <= buf.len() {
        let id = &buf[p..p + 4];
        let sz = le32(&buf[p + 4..p + 8]) as usize;
        let body = p + 8;

        // Chunk extends past the buffer?
        if body.checked_add(sz).map(|e| e > buf.len()).unwrap_or(true) {
            if id == b"data" {
                // Header-only data chunk (probe.rs synthesizes these so the
                // audio payload is never read): trust the declared size.
                // Otherwise a streaming writer left a placeholder; clamp.
                data_len = Some(if body == buf.len() { sz as u64 } else { (buf.len() - body) as u64 });
            }
            break; // truncated/corrupt chunk list — parse what we have
        }

        match id {
            b"fmt " => {
                if sz < 16 {
                    return Err("WAVE fmt chunk too small".into());
                }
                let f = &buf[body..body + sz];
                fmt_tag = le16(&f[0..2]);
                channels = le16(&f[2..4]) as u8;
                sample_rate = le32(&f[4..8]);
                // byte_rate (f[8..12]) unused: duration comes from the data chunk
                bits = le16(&f[14..16]) as u8;
            }
            b"data" => {
                data_len = Some(sz as u64);
            }
            b"LIST" => {
                // LIST....INFO + sub-chunks (INAM/ARTI/IPRD/ISFT/IGNR/ICRD...)
                if sz >= 4 && &buf[body..body + 4] == b"INFO" {
                    let end = body + sz;
                    let mut q = body + 4;
                    while q + 8 <= end {
                        let sid = &buf[q..q + 4];
                        let ssz = le32(&buf[q + 4..q + 8]) as usize;
                        let sbody = q + 8;
                        if sbody.checked_add(ssz).map(|e| e > end).unwrap_or(true) {
                            break;
                        }
                        let raw = &buf[sbody..sbody + ssz];
                        let val = String::from_utf8_lossy(&raw.iter().copied().take_while(|&b| b != 0).collect::<Vec<u8>>()).to_string();
                        match sid {
                            b"INAM" => { tags.entry("TITLE".into()).or_insert(val); }
                            b"IART" => { tags.entry("ARTIST".into()).or_insert(val); }
                            b"IPRD" => { tags.entry("ALBUM".into()).or_insert(val); }
                            b"IGNR" => { tags.entry("GENRE".into()).or_insert(val); }
                            b"ICRD" => { tags.entry("DATE".into()).or_insert(val); }
                            _ => {}
                        }
                        q = sbody + ssz + (ssz & 1); // chunks are word-aligned
                    }
                }
            }
            b"id3 " | b"ID3 " => {
                let t = walk_id3(&buf[body..body + sz]);
                for (k, v) in t.text {
                    tags.entry(k).or_insert(v);
                }
                // NOTE: WAV cover art is intentionally NOT extracted
                // (Phase-1 spec: LIST/INFO + ID3 chunks, no cover).
            }
            _ => {}
        }

        p = body + sz + (sz & 1); // chunks are word-aligned
    }

    if fmt_tag == 0 {
        return Err("WAVE file has no fmt chunk".into());
    }
    // 1 = PCM. WAVE_FORMAT_EXTENSIBLE (0xFFFE) carries PCM too; accept it when
    // bits/channels look sane. Compressed WAV (e.g. a-law = 6) is out of scope.
    if !(fmt_tag == 1 || fmt_tag == 0xFFFE || fmt_tag == 3) {
        return Err(format!("unsupported WAVE format tag {fmt_tag:#06x} (PCM only)"));
    }
    if channels == 0 || sample_rate == 0 || bits == 0 {
        return Err("WAVE fmt chunk has zero channels/rate/depth".into());
    }
    let bytes_per_frame = (channels as u32) * (bits as u32 / 8);
    if bytes_per_frame == 0 {
        return Err("WAVE frame size computes to zero".into());
    }
    let data_len = data_len.ok_or("WAVE file has no data chunk")?;
    let total_samples = data_len / bytes_per_frame as u64;
    let duration = total_samples as f64 / sample_rate as f64;

    // Average bitrate over the audio payload (what ffprobe reports).
    let bitrate_kbps = if duration > 0.0 {
        ((data_len as f64) * 8.0 / duration / 1000.0).round() as u32
    } else {
        0
    };

    let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
    Ok(TrackMeta {
        path: path.to_string_lossy().to_string(),
        title: tags.get("TITLE").cloned().unwrap_or_else(|| file_stem.to_string()),
        artist: tags.get("ARTIST").cloned().unwrap_or_else(|| "Unknown artist".into()),
        album: tags.get("ALBUM").cloned().unwrap_or_else(|| "Unknown album".into()),
        streaminfo: StreamInfo {
            sample_rate,
            bits,
            channels,
            total_samples,
        },
        duration,
        cover: None, // Phase 1 spec: WAV tags only, no cover extraction
        replaygain: None,
        format_tag: "WAV".into(),
        lossless: true,
        bitrate_kbps,
        block_types: Vec::new(),
        has_cover: false,
        embedded_lyrics: None,
    })
}

// ---------------------------------------------------------------------------
// MP3
// ---------------------------------------------------------------------------

/// MPEG-1/2 Layer III bitrate table (kbps), index by [version][rate_idx].
/// Version: 0 = MPEG-1, 1 = MPEG-2/2.5.
const MP3_BITRATE: [[u16; 16]; 2] = [
    // MPEG-1 Layer III: 32..320
    [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0],
    // MPEG-2/2.5 Layer III: 8..160
    [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0],
];

/// Sample rates index by [version][rate_idx]. Version: 0 = MPEG-1, 1 = MPEG-2,
/// 2 = MPEG-2.5.
const MP3_SAMPLE_RATE: [[u32; 3]; 3] = [
    [44100, 48000, 32000],           // MPEG-1
    [22050, 24000, 16000],           // MPEG-2
    [11025, 12000, 8000],            // MPEG-2.5
];

const MP3_SAMPLES_PER_FRAME_MPEG1: u32 = 1152; // Layer III, MPEG-1
const MP3_SAMPLES_PER_FRAME_MPEG2: u32 = 576; // Layer III, MPEG-2/2.5

/// Parse one 4-byte MP3 (Layer III) frame header. Returns
/// (samples_per_frame, sample_rate, bitrate_bps, channels, frame_len_bytes).
/// `frame_len_bytes == 0` marks a free-format/invalid header.
fn mp3_frame_header(h: &[u8; 4]) -> (u32, u32, u32, u8, usize) {
    let sync = ((h[0] as u16) << 3) | ((h[1] as u16) >> 5); // 11 bits
    if sync != 0x7FF {
        return (0, 0, 0, 0, 0);
    }
    let version_bits = (h[1] >> 3) & 0x03; // 0=2.5, 1=reserved, 2=2, 3=1
    let layer_bits = (h[1] >> 1) & 0x03; // 0=reserved, 1=III, 2=II, 3=I
    let rate_idx = (h[2] >> 4) & 0x0F;
    let sr_idx = (h[2] >> 2) & 0x03;
    let mode = (h[3] >> 6) & 0x03; // 3 = mono

    if version_bits == 1 || layer_bits != 1 || sr_idx == 3 {
        // reserved version, non-Layer-III, or reserved rate index:
        // not an MP3 frame we can size.
        return (0, 0, 0, 0, 0);
    }
    let is_mpeg1 = version_bits == 3;
    let version = match version_bits {
        3 => 0usize, // MPEG-1
        2 => 1usize, // MPEG-2
        _ => 2usize, // MPEG-2.5
    };
    let sample_rate = MP3_SAMPLE_RATE[version][sr_idx as usize];
    let bitrate = (MP3_BITRATE[version.min(1)][rate_idx as usize] as u32) * 1000;
    if bitrate == 0 || sample_rate == 0 {
        return (0, 0, 0, 0, 0);
    }
    let samples_per_frame = if is_mpeg1 {
        MP3_SAMPLES_PER_FRAME_MPEG1
    } else {
        MP3_SAMPLES_PER_FRAME_MPEG2
    };
    let padding = ((h[2] >> 1) & 0x01) as u32;
    // Layer III: MPEG-1 frame = 144·br/sr bytes; MPEG-2/2.5 = 72·br/sr bytes.
    let coef: u32 = if is_mpeg1 { 144 } else { 72 };
    let frame_len = (coef * bitrate / sample_rate + padding) as usize;
    (
        samples_per_frame,
        sample_rate,
        bitrate,
        if mode == 3 { 1 } else { 2 },
        frame_len,
    )
}

/// Find the first MP3 frame header starting at/after `from` (skips ID3).
/// A candidate only counts when its successor frame (candidate + flen) is
/// also a valid header — avoids locking onto a stray sync inside tags/binary.
fn mp3_valid_header_at(buf: &[u8], p: usize) -> bool {
    if p + 4 > buf.len() {
        return false;
    }
    if buf[p] != 0xFF || (buf[p + 1] & 0xE0) != 0xE0 {
        return false;
    }
    let mut h = [0u8; 4];
    h.copy_from_slice(&buf[p..p + 4]);
    mp3_frame_header(&h).4 > 0
}

fn mp3_first_frame(buf: &[u8], from: usize) -> Option<usize> {
    let mut p = from;
    while p + 4 <= buf.len() {
        if buf[p] == 0xFF && (buf[p + 1] & 0xE0) == 0xE0 {
            let mut h = [0u8; 4];
            h.copy_from_slice(&buf[p..p + 4]);
            let (_spf, _sr, _br, _ch, flen) = mp3_frame_header(&h);
            if flen > 0
                && (p + flen >= buf.len() // last frame: no successor to check
                    || mp3_valid_header_at(buf, p + flen))
            {
                return Some(p);
            }
        }
        p += 1;
    }
    None
}

/// Look for Xing/Info/VBRI in the first frame's payload. Returns
/// (frames, bytes, present) — bytes == 0 when only frames are stored.
fn mp3_xing(buf: &[u8], frame_off: usize, frame_len: usize) -> Option<(u32, u32)> {
    let end = (frame_off + frame_len).min(buf.len());
    if frame_off >= end {
        return None;
    }
    let body = &buf[frame_off..end];
    // Xing/Info sit AFTER the side-info record, whose size is fixed by the
    // MPEG version and channel mode (see mp3_side_info). Searching the whole
    // frame instead can lock onto an "Xing" string inside audio data or a
    // tag copied into the first frame (files have been seen to do both).
    let body_si = body.get(mp3_side_info(body)?..).unwrap_or(body);
    for tag in [&b"Xing"[..], &b"Info"[..]] {
        // Only the canonical slot: right after side info (allow the tiny
        // offsets some encoders add for the ancillary bits).
        for off in mp3_xing_offsets(body_si.len()) {
            if off + 16 > body_si.len() || &body_si[off..off + 4] != tag {
                continue;
            }
            let q = off + 4;
            let flags = u32::from_be_bytes([body_si[q], body_si[q + 1], body_si[q + 2], body_si[q + 3]]);
            let mut r = q + 4;
            let mut frames = 0u32;
            let mut bytes = 0u32;
            if flags & 0x01 != 0 && r + 4 <= body_si.len() {
                frames = u32::from_be_bytes([body_si[r], body_si[r + 1], body_si[r + 2], body_si[r + 3]]);
                r += 4;
            }
            if flags & 0x02 != 0 && r + 4 <= body_si.len() {
                bytes = u32::from_be_bytes([body_si[r], body_si[r + 1], body_si[r + 2], body_si[r + 3]]);
            }
            if frames > 0 {
                return Some((frames, bytes));
            }
            // "Info" tag with zero frames = plain CBR (LAME): no VBR data.
            return None;
        }
    }
    // VBRI (Fraunhofer) sits at a fixed offset +32 in the frame
    if body.len() >= 40 && &body[32..36] == b"VBRI" {
        // header: version(2) delay(2) quality(2) bytes(4) frames(4)
        if body.len() >= 50 {
            let bytes = u32::from_be_bytes([body[40], body[41], body[42], body[43]]);
            let frames = u32::from_be_bytes([body[44], body[45], body[46], body[47]]);
            if frames > 0 {
                return Some((frames, bytes));
            }
        }
        return None;
    }
    None
}

/// MPEG frame side-info size: MPEG1 mono 17 / stereo 32; MPEG2/2.5 mono 9 /
/// stereo 17. The first byte of `body` is the version/channel-mode byte we
/// already parsed — re-derive from the frame header layout directly.
fn mp3_side_info(body: &[u8]) -> Option<usize> {
    // body[0..4] is the frame header (sync + version + layer + mode).
    if body.len() < 4 {
        return None;
    }
    let version_bits = (body[1] >> 3) & 0x03;
    let mode = (body[3] >> 6) & 0x03;
    let mono = mode == 3;
    Some(match version_bits {
        3 => if mono { 17 } else { 32 },   // MPEG-1
        0 | 2 => if mono { 9 } else { 17 }, // MPEG-2 / MPEG-2.5
        _ => return None,                   // reserved
    })
}

/// Candidate offsets of the Xing/Info tag relative to the side info end.
/// The spec puts it exactly there; a couple of encoders emit it a few bytes
/// into the ancillary area — try those, then stop (never scan the frame).
fn mp3_xing_offsets(after_si_len: usize) -> impl Iterator<Item = usize> {
    [0usize, 1, 2, 4].into_iter().take_while(move |&o| o + 8 <= after_si_len)
}

/// Count frames by walking every frame header. A frame is counted only when
/// its own header and its successor's header are both valid (the classic
/// sync+verify heuristic) — stray syncs inside audio data are skipped.
fn mp3_count_frames(buf: &[u8], start: usize) -> u64 {
    let mut p = start;
    let mut frames = 0u64;
    while p + 4 <= buf.len() {
        if buf[p] == 0xFF && (buf[p + 1] & 0xE0) == 0xE0 {
            let mut h = [0u8; 4];
            h.copy_from_slice(&buf[p..p + 4]);
            let (_spf, _sr, _br, _ch, flen) = mp3_frame_header(&h);
            if flen > 0 && p + flen <= buf.len() {
                let last = p + flen >= buf.len();
                if last || mp3_valid_header_at(buf, p + flen) {
                    frames += 1;
                    p += flen;
                    continue;
                }
            }
        }
        p += 1;
    }
    frames
}

/// Sample the first few frame headers; if ≥4 consecutive frames all report
/// the same bitrate the file is CBR (returns Some(bitrate_bps)).
fn mp3_detect_cbr(buf: &[u8], mut p: usize) -> Option<u32> {
    let mut first_br = 0u32;
    let mut seen = 0u32;
    while p + 4 <= buf.len() && seen < 16 {
        if buf[p] == 0xFF && (buf[p + 1] & 0xE0) == 0xE0 {
            let mut h = [0u8; 4];
            h.copy_from_slice(&buf[p..p + 4]);
            let (_spf, _sr, br, _ch, flen) = mp3_frame_header(&h);
            if flen == 0 || p + flen > buf.len() {
                return None;
            }
            if seen == 0 {
                first_br = br;
            } else if br != first_br {
                return None;
            }
            seen += 1;
            p += flen;
            continue;
        }
        return None; // gap right after the start: not clean CBR
    }
    if seen >= 4 {
        Some(first_br)
    } else {
        None
    }
}

fn parse_mp3(buf: &[u8], path: &Path) -> Result<TrackMeta, String> {
    parse_mp3_sized(buf, path, buf.len() as u64)
}

/// MP3 from a HEAD window of a file that is `file_len` bytes long. Without
/// Xing/Info/VBRI, duration is estimated from `file_len` instead of
/// frame-scanning the whole file.
pub fn parse_mp3_sized(buf: &[u8], path: &Path, file_len: u64) -> Result<TrackMeta, String> {
    let partial = (buf.len() as u64) < file_len;
    // Tags first (ID3v2.3/v2.4 — also handles the 'id3 ' chunk case).
    let tags = walk_id3(buf);
    let audio_start = id3_tag_size(buf).unwrap_or(0).min(buf.len());

    // First valid frame gives us the stream parameters.
    let frame_off = mp3_first_frame(buf, audio_start)
        .ok_or("mp3: no valid MPEG audio frame found after tags")?;
    let mut h = [0u8; 4];
    h.copy_from_slice(&buf[frame_off..frame_off + 4]);
    let (spf, sample_rate, first_bitrate, channels, frame_len) = mp3_frame_header(&h);
    if sample_rate == 0 || spf == 0 {
        return Err("mp3: invalid frame header".into());
    }

    // Duration + bitrate: Xing/Info/VBRI when present, else frame scan, else
    // CBR estimate from the first frame's bitrate.
    let mut duration;
    let mut bitrate_kbps;
    if let Some((frames, bytes)) = mp3_xing(buf, frame_off, frame_len) {
        duration = frames as f64 * spf as f64 / sample_rate as f64;
        bitrate_kbps = if bytes > 0 && duration > 0.0 {
            (bytes as f64 * 8.0 / duration / 1000.0).round() as u32
        } else {
            (bytes as f64 * 8.0 / (frames as f64 * spf as f64 / sample_rate as f64) / 1000.0).round() as u32
        };
        if bytes == 0 && duration > 0.0 {
            // Xing without the byte-count field: average over the real file
            bitrate_kbps = ((file_len.saturating_sub(audio_start as u64)) as f64 * 8.0 / duration / 1000.0).round() as u32;
        }
        if bitrate_kbps == 0 {
            bitrate_kbps = first_bitrate / 1000;
        }
    } else {
        // Full frame scan (buf is the whole file — see parse() docs).
        let frames = mp3_count_frames(buf, frame_off);
        if frames > 0 && partial {
            // Head window only: CBR estimate over the real audio length,
            // VBR-without-header estimate from the average frame size seen.
            let audio = file_len.saturating_sub(frame_off as u64) as f64;
            let seen = (buf.len() - frame_off) as f64;
            bitrate_kbps = match mp3_detect_cbr(buf, frame_off) {
                Some(br) => br / 1000,
                None => ((seen * 8.0) / (frames as f64 * spf as f64 / sample_rate as f64) / 1000.0).round() as u32,
            };
            duration = if bitrate_kbps > 0 { audio * 8.0 / (bitrate_kbps as f64 * 1000.0) } else { 0.0 };
        } else if frames > 0 {
            duration = frames as f64 * spf as f64 / sample_rate as f64;
            // CBR: constant frame bitrates -> report the exact bitrate.
            // VBR without Xing -> whole-file average.
            bitrate_kbps = match mp3_detect_cbr(buf, frame_off) {
                Some(br) => br / 1000,
                None => ((buf.len() as f64) * 8.0 / duration / 1000.0).round() as u32,
            };
        } else {
            return Err("mp3: no decodable frames".into());
        }
    }
    if duration <= 0.0 {
        return Err("mp3: computed zero duration".into());
    }

    let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
    Ok(TrackMeta {
        path: path.to_string_lossy().to_string(),
        title: tags.text.get("TIT2").cloned().unwrap_or_else(|| file_stem.to_string()),
        artist: tags.text.get("TPE1").cloned().unwrap_or_else(|| "Unknown artist".into()),
        album: tags.text.get("TALB").cloned().unwrap_or_else(|| "Unknown album".into()),
        streaminfo: StreamInfo {
            sample_rate,
            bits: 16, // convention for lossy formats
            channels,
            total_samples: (duration * sample_rate as f64).round() as u64,
        },
        duration,
        cover: tags.apic.map(|(mime, data)| Picture {
            mime,
            data_b64: crate::base64_encode(&data),
            width: 0, // APIC carries no dimensions; the UI falls back like FLAC
            height: 0,
            data_len: data.len() as u32,
        }),
        replaygain: None,
        format_tag: "MP3".into(),
        lossless: false,
        bitrate_kbps,
        block_types: Vec::new(),
        has_cover: false,
        embedded_lyrics: ["USLT", "TXXX:LYRICS", "TXXX:UNSYNCEDLYRICS", "TXXX:USLT"]
            .iter()
            .find_map(|k| tags.text.get(*k).cloned()),
    })
}

fn parse_m4a(_buf: &[u8], _path: &Path) -> Result<TrackMeta, String> {
    Err("m4a parsing not yet implemented".into())
}

fn parse_adts(_buf: &[u8], _path: &Path) -> Result<TrackMeta, String> {
    Err("adts parsing not yet implemented".into())
}

#[cfg(test)]
mod lyrics_tag_tests {
    use super::*;

    fn id3(frames: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (id, data) in frames {
            body.extend_from_slice(*id);
            body.extend((data.len() as u32).to_be_bytes()); // v2.3: plain BE size
            body.extend([0, 0]);
            body.extend(data);
        }
        let n = body.len() as u32;
        let mut out = b"ID3\x03\x00\x00".to_vec();
        out.extend([(n >> 21 & 0x7f) as u8, (n >> 14 & 0x7f) as u8, (n >> 7 & 0x7f) as u8, (n & 0x7f) as u8]);
        out.extend(body);
        out
    }

    #[test]
    fn uslt_and_txxx() {
        let mut uslt = vec![3u8]; // utf-8
        uslt.extend(b"eng");
        uslt.extend(b"desc\0");
        uslt.extend(b"[00:01.00]real uslt");
        let t = walk_id3(&id3(&[(b"USLT", uslt)]));
        assert_eq!(t.text.get("USLT").map(String::as_str), Some("[00:01.00]real uslt"));

        let mut txxx = vec![0u8];
        txxx.extend(b"LYRICS\0hello");
        let t = walk_id3(&id3(&[(b"TXXX", txxx)]));
        assert_eq!(t.text.get("TXXX:LYRICS").map(String::as_str), Some("hello"));
    }
}
