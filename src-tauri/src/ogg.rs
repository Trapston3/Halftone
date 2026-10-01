//! ogg.rs — Ogg Vorbis + Ogg Opus from bounded reads.
//!
//! The Ogg page format (RFC 3533): "OggS" + version(1) + header_type(1) +
//! granule(8 LE) + serial(4) + sequence(4) + CRC(4) + nsegs(1) + segment
//! table (nsegs bytes) + body. A packet is a chain of segment "lacing
//! values" (255 = continues, <255 = last segment of the packet); packets
//! continue across pages (header_type 0x01 = continued page).
//!
//! Reads, per file:
//! 1. pages from byte 0 until the comment header packet COMPLETES (the
//!    comment can carry base64 cover art — cap the head walk at 64 MiB).
//! 2. the last ~64 KiB (seek from END) to find the final page's granule
//!    position for the duration. Never the whole file.
//!
//! Codec identification lives in the first packet: "\x01vorbis" (Vorbis
//! identification header: channels + sample rate) or "OpusHead" (channels,
//! pre-skip; codec runs at 48000 Hz by spec). Comments: "\x03vorbis" or
//! "OpusTags" — the same "KEY=value" comment list for both. Cover art is
//! METADATA_BLOCK_PICTURE: base64 of a FLAC PICTURE block.

use crate::{base64_decode_pub, Picture, ReplayGain, TrackMeta};
use std::path::Path;

/// Hard cap on the head walk (comment packets with embedded art can be
/// large; 64 MiB matches probe.rs's META_CAP).
const HEAD_CAP: u64 = 64 * 1024 * 1024;
/// Tail window scanned for the final page (granule position).
const TAIL: u64 = 64 * 1024;
/// Per-comment length cap (a corrupt length must not allocate gigabytes).
const COMMENT_CAP: usize = 8 * 1024 * 1024;

pub enum Codec {
    Vorbis { sample_rate: u32, channels: u8 },
    Opus { channels: u8, pre_skip: u32 },
}

impl Copy for Codec {}

impl Clone for Codec {
    fn clone(&self) -> Self {
        *self
    }
}

impl Codec {
    fn rate(self) -> u32 {
        match self {
            Codec::Vorbis { sample_rate, .. } => sample_rate,
            // Opus always decodes at 48 kHz (RFC 7845 §4.2).
            Codec::Opus { .. } => 48000,
        }
    }
    fn channels(self) -> u8 {
        match self {
            Codec::Vorbis { channels, .. } | Codec::Opus { channels, .. } => channels,
        }
    }
    fn pre_skip(self) -> u32 {
        match self {
            Codec::Vorbis { .. } => 0,
            Codec::Opus { pre_skip, .. } => pre_skip,
        }
    }
}

/// The first packet's packet-type byte + magic (codec identification).
pub fn codec_of(head: &[u8], off: usize) -> Option<Codec> {
    let b = &head[off..];
    if b.len() >= 7 && b[0] == 1 && &b[1..7] == b"vorbis" {
        // ID header: version(4) channels(1) rate(4 LE) ...
        if b.len() >= 16 {
            let channels = b[11];
            let sample_rate = u32::from_le_bytes([b[12], b[13], b[14], b[15]]);
            if sample_rate > 0 && (1..=255).contains(&channels) {
                return Some(Codec::Vorbis { sample_rate, channels });
            }
        }
        return None;
    }
    if b.len() >= 8 && &b[0..8] == b"OpusHead" {
        // version(1) channels(1) pre-skip(2 LE) input-rate(4 LE) ...
        if b.len() >= 12 {
            let channels = b[9];
            let pre_skip = u32::from_le_bytes([b[10], b[11], 0, 0]);
            if (1..=255).contains(&channels) {
                return Some(Codec::Opus { channels, pre_skip });
            }
        }
    }
    None
}

fn is_vorbis_comment(head: &[u8], off: usize) -> bool {
    head.len() >= off + 7 && head[off] == 3 && &head[off + 1..off + 7] == b"vorbis"
}

fn is_opus_tags(head: &[u8], off: usize) -> bool {
    head.len() >= off + 8 && &head[off..off + 8] == b"OpusTags"
}

// ---------------------------------------------------------------------------
// Ogg page reader over a cursor
// ---------------------------------------------------------------------------

/// One page: granule position, whether it continues the previous packet,
/// and the packet boundaries it finishes (offsets into `buf`).
struct Page<'a> {
    #[allow(dead_code)] // granule of intermediate pages; only the last page's is read
    granule: u64,
    continued: bool,
    body: &'a [u8],
    /// Length in bytes of each COMPLETE packet that ends on this page
    /// (measured across `body`); a packet spanning into the next page does
    /// not appear here.
    packet_ends: Vec<usize>,
}

/// Parse the page at `buf[p]`. Returns the page and the byte offset of the
/// next page header. `None` on any structural violation.
fn page_at<'a>(buf: &'a [u8], p: usize) -> Option<(Page<'a>, usize)> {
    if buf.len() < p + 27 || &buf[p..p + 4] != b"OggS" {
        return None;
    }
    let granule = u64::from_le_bytes(buf[p + 6..p + 14].try_into().ok()?);
    let ht = buf[p + 5];
    let nsegs = buf[p + 26] as usize;
    let table = buf.get(p + 27..p + 27 + nsegs)?;
    let body_len: usize = table.iter().map(|&s| s as usize).sum();
    let body_end = p + 27 + nsegs + body_len;
    let body = buf.get(p + 27 + nsegs..body_end)?;
    // Packet ends: a lacing value < 255 terminates the current packet.
    let mut packet_ends = Vec::new();
    let mut acc = 0usize;
    for &s in table {
        acc += s as usize;
        if s != 255 {
            packet_ends.push(acc);
        }
    }
    // Final lacing 255 = the packet continues onto the next page (also the
    // only shape a "no complete packet" page can have).
    Some((
        Page { granule, continued: ht & 0x01 != 0, body, packet_ends },
        body_end,
    ))
}

/// Walk pages from 0, collecting the FIRST packet that satisfies `done`
/// (checked on the completed packet bytes). Packets that complete before
/// the wanted one are skipped, not concatenated — the caller's predicate
/// always sees one packet's bytes. Returns the packet bytes, or Err on
/// truncation or when the cap is hit first.
fn collect_packet<F>(buf: &[u8], mut done: F) -> Result<Vec<u8>, String>
where
    F: FnMut(&[u8]) -> bool,
{
    let mut out = Vec::new();
    let mut p = 0usize;
    let mut mid_packet = false;
    loop {
        let (page, next) = page_at(buf, p)
            .ok_or_else(|| "ogg: truncated or corrupt page (comment header incomplete)".to_string())?;
        if mid_packet && !page.continued {
            return Err("ogg: packet continuation missing".into());
        }
        let mut prev = 0usize;
        for &end in &page.packet_ends {
            let seg = &page.body[prev..end];
            prev = end;
            if mid_packet {
                out.extend_from_slice(seg);
                mid_packet = false;
                if done(&out) {
                    return Ok(out);
                }
            } else if done(seg) {
                // The wanted packet completed within this page.
                return Ok(seg.to_vec());
            }
            if out.len() > HEAD_CAP as usize {
                return Err(format!("ogg: comment header exceeds the {HEAD_CAP} byte cap"));
            }
        }
        if prev < page.body.len() {
            // Packet continues onto the next page: start or continue the
            // open chain in `out` (multi-page packets accumulate).
            out.extend_from_slice(&page.body[prev..]);
            mid_packet = true;
        }
        if out.len() > HEAD_CAP as usize {
            return Err(format!("ogg: comment header exceeds the {HEAD_CAP} byte cap"));
        }
        if next >= buf.len() {
            return Err("ogg: stream ends before the comment header completes".into());
        }
        p = next;
    }
}

// ---------------------------------------------------------------------------
// Comment list
// ---------------------------------------------------------------------------

/// Parse the "KEY=value" comment list shared by \x03vorbis and OpusTags
/// (after their 7/8-byte magic). `skip_len` = vendor length field offset.
fn parse_comments(packet: &[u8], magic_len: usize) -> Result<Vec<(String, String)>, String> {
    let mut q = magic_len;
    let rd32 = |b: &[u8], o: usize| -> Result<u32, String> {
        b.get(o..o + 4)
            .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
            .ok_or_else(|| "ogg: truncated comment header".to_string())
    };
    let vendor_len = rd32(packet, q)? as usize;
    q += 4 + vendor_len;
    if q > packet.len() || vendor_len > COMMENT_CAP {
        return Err("ogg: corrupt vendor string".into());
    }
    let count = rd32(packet, q)? as usize;
    q += 4;
    if count > 100_000 {
        return Err("ogg: absurd comment count".into());
    }
    let mut out = Vec::with_capacity(count.min(64));
    for _ in 0..count {
        let l = rd32(packet, q)? as usize;
        q += 4;
        if l > COMMENT_CAP || q + l > packet.len() {
            return Err("ogg: corrupt comment list".into());
        }
        let s = String::from_utf8_lossy(&packet[q..q + l]);
        q += l;
        if let Some(eq) = s.find('=') {
            out.push((s[..eq].to_uppercase(), s[eq + 1..].to_string()));
        }
    }
    Ok(out)
}

/// METADATA_BLOCK_PICTURE value (base64 of a FLAC PICTURE block) -> Picture.
fn decode_picture(value: &str) -> Option<Picture> {
    let b = base64_decode_pub(value.trim())?;
    let rd32 = |o: usize| -> Option<u32> {
        b.get(o..o + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    };
    let mut q = 8; // picture type + reserved
    let mime_len = rd32(q)? as usize;
    q += 4;
    let mime = String::from_utf8_lossy(b.get(q..q + mime_len)?).to_string();
    q += mime_len;
    let desc_len = rd32(q)? as usize;
    q += 4 + desc_len;
    let (w, h, _depth, _colors) = (rd32(q)?, rd32(q + 4)?, rd32(q + 8)?, rd32(q + 12)?);
    q += 16;
    let data_len = rd32(q)? as usize;
    q += 4;
    let data = b.get(q..q + data_len)?.to_vec();
    Some(Picture {
        mime,
        data_b64: crate::base64_encode(&data),
        width: w,
        height: h,
        data_len: data_len as u32,
    })
}

/// FLAC-style ReplayGain from the comment list (same keys as VORBIS_COMMENT).
fn replaygain(comments: &[(String, String)]) -> Option<ReplayGain> {
    let tg = comments.iter().find(|(k, _)| k == "REPLAYGAIN_TRACK_GAIN");
    let ag = comments.iter().find(|(k, _)| k == "REPLAYGAIN_ALBUM_GAIN");
    if tg.is_none() && ag.is_none() {
        return None;
    }
    let parse = |v: &str| -> Option<f64> {
        let num: String = v
            .trim()
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '+' || *c == '.')
            .collect();
        num.parse::<f64>().ok()
    };
    Some(ReplayGain {
        track_gain: tg.and_then(|(_, v)| parse(v)),
        album_gain: ag.and_then(|(_, v)| parse(v)),
    })
}

// ---------------------------------------------------------------------------
// Final granule: scan the last TAIL bytes for the LAST valid page
// ---------------------------------------------------------------------------

/// The final page's granule position. Ogg streams can carry arbitrary bytes
/// between pages (rare); scanning every 27-byte-aligned candidate and taking
/// the LAST one whose segment table stays inside the buffer is robust
/// without trusting any single offset.
fn last_granule(buf: &[u8]) -> Option<u64> {
    let mut best = None;
    for p in 0..buf.len().saturating_sub(27) {
        if &buf[p..p + 4] != b"OggS" || buf[p + 4] != 0 {
            continue;
        }
        let nsegs = buf[p + 26] as usize;
        let table = buf.get(p + 27..p + 27 + nsegs)?;
        let body: usize = table.iter().map(|&s| s as usize).sum();
        if p + 27 + nsegs + body <= buf.len() {
            best = Some(u64::from_le_bytes(buf[p + 6..p + 14].try_into().ok()?));
        }
    }
    best
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Parse an Ogg Vorbis / Ogg Opus file. Reads: pages from the start until
/// the comment packet completes + the final TAIL bytes. Never the whole
/// file. `file_len` allows reading the tail by seeking.
pub fn parse(buf: &[u8], path: &Path, file_len: u64) -> Result<TrackMeta, String> {
    if buf.len() < 35 || &buf[0..4] != b"OggS" {
        return Err("ogg: not an Ogg file".into());
    }
    // Packet 1: identification header (page 0, after the 1-byte segment
    // table — page 0 is BOS with one segment).
    let codec0 = codec_of(buf, 28).ok_or_else(|| {
        "ogg: unknown codec (not Vorbis/Opus — HALFTONE plays Vorbis and Opus only)".to_string()
    })?;

    // Packet 2: comment header ("\x03vorbis" or "OpusTags"). It starts on
    // page 1 (possibly continued across pages) — collect until complete.
    let packet = collect_packet(buf, |whole| {
        is_vorbis_comment(whole, 0) || is_opus_tags(whole, 0)
    })?;
    let magic_len = if is_opus_tags(&packet, 0) { 8 } else { 7 };
    let comments = parse_comments(&packet, magic_len)?;

    let get = |k: &str| comments.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
    let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?").to_string();

    // Duration: granule position of the LAST page / rate. Opus granules are
    // 48 kHz and include the pre-skip padding (RFC 7845 §4.5). When the
    // head buffer is the whole file (small files), reuse it for the tail
    // scan; otherwise read the last TAIL bytes from disk.
    let tail_len = TAIL.min(file_len);
    let granule = if buf.len() as u64 == file_len {
        last_granule(&buf[buf.len() - tail_len as usize..])
    } else {
        let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
        use std::io::{Read, Seek, SeekFrom};
        f.seek(SeekFrom::End(-(tail_len as i64))).map_err(|e| e.to_string())?;
        let mut t = Vec::with_capacity(tail_len as usize);
        f.read_to_end(&mut t).map_err(|e| e.to_string())?;
        last_granule(&t)
    };
    let rate = codec0.rate();
    let pre_skip = codec0.pre_skip();
    let channels = codec0.channels();
    let duration = match granule {
        Some(g) if g > pre_skip as u64 && rate > 0 => {
            (g - pre_skip as u64) as f64 / rate as f64
        }
        _ => 0.0,
    };
    if duration <= 0.0 {
        return Err("ogg: no final page found (truncated stream)".into());
    }

    let pic = get("METADATA_BLOCK_PICTURE").and_then(|v| decode_picture(&v));
    Ok(TrackMeta {
        path: path.to_string_lossy().to_string(),
        title: get("TITLE").unwrap_or(file_stem),
        artist: get("ARTIST").unwrap_or_else(|| "Unknown artist".into()),
        album: get("ALBUM").unwrap_or_else(|| "Unknown album".into()),
        streaminfo: crate::StreamInfo {
            sample_rate: rate,
            bits: 16, // convention for lossy formats
            channels,
            total_samples: (duration * rate as f64).round() as u64,
        },
        duration,
        cover: pic,
        block_types: Vec::new(),
        replaygain: replaygain(&comments),
        format_tag: match codec0 {
            Codec::Vorbis { .. } => "OGG",
            Codec::Opus { .. } => "OPUS",
        }
        .into(),
        lossless: false,
        // Whole-file average bitrate from the real file length.
        bitrate_kbps: (file_len as f64 * 8.0 / duration / 1000.0).round() as u32,
        has_cover: false,
        embedded_lyrics: get("LYRICS")
            .or_else(|| get("UNSYNCEDLYRICS"))
            .or_else(|| get("SYNCEDLYRICS")),
    })
}

// ---------------------------------------------------------------------------
// Tests: synthetic page builders + real ffmpeg fixtures
// ---------------------------------------------------------------------------

#[cfg(test)]
mod ogg_tests {
    use super::*;

    /// Build one Ogg page. `segments` = lacing table; body = seg sums.
    fn page(granule: u64, continued: bool, segments: &[u8], body: &[u8]) -> Vec<u8> {
        let mut out = b"OggS".to_vec();
        out.push(0); // version
        out.push(if continued { 0x01 } else { 0x00 });
        out.extend(granule.to_le_bytes());
        out.extend(1u32.to_le_bytes()); // serial
        out.extend(0u32.to_le_bytes()); // sequence
        out.extend(0u32.to_le_bytes()); // CRC (not validated by our parser)
        out.push(segments.len() as u8);
        out.extend(segments);
        assert_eq!(segments.iter().map(|&s| s as usize).sum::<usize>(), body.len());
        out.extend(body);
        out
    }

    /// A comment packet split into lacing segments (255s + terminator).
    fn laced(data: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let mut segs = Vec::new();
        let mut rest = data;
        while rest.len() >= 255 {
            segs.push(255u8);
            rest = &rest[255..];
        }
        segs.push(rest.len() as u8);
        (segs, data.to_vec())
    }

    fn vorbis_id(rate: u32, channels: u8) -> Vec<u8> {
        let mut p = vec![1u8];
        p.extend(b"vorbis");
        p.extend(0u32.to_le_bytes()); // version
        p.push(channels);
        p.extend(rate.to_le_bytes());
        p.extend(0i32.to_le_bytes()); // bitrate max (sig)
        p.extend(0i32.to_le_bytes()); // nominal
        p.extend(0i32.to_le_bytes()); // min
        p
    }

    fn vorbis_comment(comments: &[(&str, &str)]) -> Vec<u8> {
        let mut p = vec![3u8];
        p.extend(b"vorbis");
        p.extend(4u32.to_le_bytes());
        p.extend(b"test");
        p.extend((comments.len() as u32).to_le_bytes());
        for (k, v) in comments {
            let s = format!("{k}={v}");
            p.extend((s.len() as u32).to_le_bytes());
            p.extend(s.bytes());
        }
        p
    }

    fn opus_head(channels: u8, pre_skip: u16) -> Vec<u8> {
        let mut p = b"OpusHead".to_vec();
        p.push(1); // version
        p.push(channels);
        p.extend(pre_skip.to_le_bytes());
        p.extend(48000u32.to_le_bytes()); // input sample rate
        p.extend(0i16.to_le_bytes()); // gain
        p.push(0); // mapping family
        p
    }

    fn opus_tags(comments: &[(&str, &str)]) -> Vec<u8> {
        let mut p = b"OpusTags".to_vec();
        p.extend(4u32.to_le_bytes());
        p.extend(b"test");
        p.extend((comments.len() as u32).to_le_bytes());
        for (k, v) in comments {
            let s = format!("{k}={v}");
            p.extend((s.len() as u32).to_le_bytes());
            p.extend(s.bytes());
        }
        p
    }

    /// Vorbis file: page 0 (id, BOS) + page 1 (comment, one packet laced).
    fn vorbis_file(rate: u32, comments: &[(&str, &str)], last_granule: u64) -> Vec<u8> {
        let id = vorbis_id(rate, 2);
        let cm = vorbis_comment(comments);
        let mut out = page(0, false, &[id.len() as u8], &id);
        let (segs, body) = laced(&cm);
        out.extend(&page(0, false, &segs, &body));
        // final audio page
        out.extend(&page(last_granule, false, &[255, 10], &vec![0u8; 265]));
        out
    }

    #[test]
    fn vorbis_synthetic_basic() {
        let p = std::env::temp_dir().join(format!("halftone_ogg_v_{}.ogg", std::process::id()));
        std::fs::write(&p, vorbis_file(44100, &[("TITLE", "T"), ("ARTIST", "A")], 441000)).unwrap();
        let m = super::parse(
            &std::fs::read(&p).unwrap(),
            &p,
            std::fs::metadata(&p).unwrap().len(),
        )
        .unwrap();
        assert_eq!(m.format_tag, "OGG");
        assert!((m.duration - 10.0).abs() < 1e-6);
        assert_eq!(m.title, "T");
        assert_eq!(m.streaminfo.sample_rate, 44100);
        assert!(!m.lossless);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn opus_synthetic_pre_skip() {
        // 480000 granule with pre-skip 312 -> (480000-312)/48000 = 9.9935s
        let head = opus_head(2, 312);
        let tags = opus_tags(&[("TITLE", "Op")]);
        let mut out = page(0, false, &[head.len() as u8], &head);
        let (segs, body) = laced(&tags);
        out.extend(&page(0, false, &segs, &body));
        out.extend(&page(480000, false, &[255, 10], &vec![0u8; 265]));
        let p = std::env::temp_dir().join(format!("halftone_ogg_o_{}.opus", std::process::id()));
        std::fs::write(&p, &out).unwrap();
        let m = super::parse(&out, &p, out.len() as u64).unwrap();
        assert_eq!(m.format_tag, "OPUS");
        assert!((m.duration - (480000 - 312) as f64 / 48000.0).abs() < 1e-6, "{}", m.duration);
        assert_eq!(m.streaminfo.sample_rate, 48000);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn comment_packet_spanning_pages() {
        // The comment packet must survive page boundaries: page 1 carries
        // 117 full 255-byte segments (29850 bytes, no terminator — the
        // packet is NOT complete), page 2 continues it, page 3 finishes it.
        let big = "x".repeat(60_000);
        let head = opus_head(1, 0);
        let tags = opus_tags(&[("TITLE", "Big"), ("PADDING", &big)]);
        let mut out = page(0, false, &[head.len() as u8], &head);
        // Page 1: 117 laced segments, all 255 → packet continues.
        let (seg1, rest) = tags.split_at(117 * 255);
        out.extend(&page(0, false, &vec![255u8; 117], seg1));
        // Page 2 (continued): 118 laced segments, all 255 → still continues.
        let (seg2, seg3) = rest.split_at(118 * 255);
        out.extend(&page(0, true, &vec![255u8; 118], seg2));
        // Page 3 (continued): the remainder, terminated lacing.
        let (segs3, body3) = laced(seg3);
        out.extend(&page(0, true, &segs3, &body3));
        // Final audio page.
        out.extend(&page(480000, false, &[255, 10], &vec![0u8; 265]));
        let p = std::env::temp_dir().join(format!("halftone_ogg_big_{}.opus", std::process::id()));
        std::fs::write(&p, &out).unwrap();
        let m = super::parse(&out, &p, out.len() as u64).unwrap();
        assert_eq!(m.title, "Big");
        assert_eq!(m.format_tag, "OPUS");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn garbage_never_panics() {
        let p = Path::new("x.ogg");
        assert!(super::parse(b"", p, 0).is_err());
        assert!(super::parse(b"OggS\x00", p, 5).is_err());
        // Real header, unknown codec (theora: packet type \x01, not vorbis magic)
        let mut f = page(0, false, &[10], b"\x01theora\0\0\0");
        f.extend(page(100, false, &[3], b"\x7fFC"));
        assert!(super::parse(&f, p, f.len() as u64).is_err());
        // Truncated comment (no second page)
        let id = vorbis_id(44100, 2);
        let f2 = page(0, false, &[id.len() as u8], &id);
        assert!(super::parse(&f2, p, f2.len() as u64).is_err());
        // ID header with invalid sample rate must be rejected, not div-zero
        let bad_rate = {
            let mut v = vec![1u8];
            v.extend(b"vorbis");
            v.extend(0u32.to_le_bytes());
            v.push(2u8);
            v.extend(0u32.to_le_bytes()); // rate 0
            v.extend(0i32.to_le_bytes());
            v.extend(0i32.to_le_bytes());
            v.extend(0i32.to_le_bytes());
            v
        };
        let f3 = page(0, false, &[bad_rate.len() as u8], &bad_rate);
        assert!(super::parse(&f3, p, f3.len() as u64).is_err());
    }

    #[test]
    fn picture_block_decodes() {
        let mut pic = Vec::new();
        pic.extend(3u32.to_be_bytes()); // type: front cover
        pic.extend(0u32.to_be_bytes()); // reserved
        pic.extend(9u32.to_be_bytes());
        pic.extend(b"image/png");
        pic.extend(0u32.to_be_bytes()); // description len
        pic.extend(512u32.to_be_bytes());
        pic.extend(512u32.to_be_bytes());
        pic.extend(24u32.to_be_bytes());
        pic.extend(0u32.to_be_bytes()); // colors
        pic.extend(3u32.to_be_bytes());
        pic.extend(b"PNG");
        let b64 = crate::base64_encode(&pic);
        let pic2 = decode_picture(&b64).unwrap();
        assert_eq!(pic2.mime, "image/png");
        assert_eq!(pic2.width, 512);
        assert_eq!(pic2.data_len, 3);
        assert!(decode_picture("!!!not base64!!!").is_none());
    }

    #[test]
    fn replaygain_and_lyrics_comments() {
        let p = std::env::temp_dir().join(format!("halftone_ogg_rg_{}.ogg", std::process::id()));
        std::fs::write(
            &p,
            vorbis_file(
                44100,
                &[
                    ("REPLAYGAIN_TRACK_GAIN", "-7.20 dB"),
                    ("REPLAYGAIN_ALBUM_GAIN", "-6.10 dB"),
                    ("LYRICS", "[00:01.00]line"),
                ],
                44100,
            ),
        )
        .unwrap();
        let m = super::parse(
            &std::fs::read(&p).unwrap(),
            &p,
            std::fs::metadata(&p).unwrap().len(),
        )
        .unwrap();
        let rg = m.replaygain.unwrap();
        assert!((rg.track_gain.unwrap() + 7.2).abs() < 1e-6);
        assert!((rg.album_gain.unwrap() + 6.1).abs() < 1e-6);
        assert_eq!(m.embedded_lyrics.as_deref(), Some("[00:01.00]line"));
        let _ = std::fs::remove_file(&p);
    }

    /// Real ffmpeg fixtures (HT_FIXTURES). Verified against ffprobe values.
    #[test]
    #[ignore]
    fn ogg_fixtures() {
        let Ok(dir) = std::env::var("HT_FIXTURES") else { return };
        for name in [
            "t.ogg", "cover_art.ogg", "big_art.ogg", "long_v.ogg", "t_opus.opus",
            "opus_art.opus", "long_opus.opus",
        ] {
            let p = std::path::Path::new(&dir).join(name);
            let buf = match std::fs::read(&p) {
                Ok(b) => b,
                Err(_) => {
                    println!("FIX-OGG {name:<16} SKIP (missing)");
                    continue;
                }
            };
            match super::parse(&buf, &p, buf.len() as u64) {
                Ok(m) => println!(
                    "FIX-OGG {:<16} dur={:.3} title={:?} artist={:?} album={:?} cover={} lyr={:?} kbps={} rg={:?}",
                    name, m.duration, m.title, m.artist, m.album,
                    m.cover.as_ref().map(|c| c.data_len).unwrap_or(0),
                    m.embedded_lyrics, m.bitrate_kbps,
                    m.replaygain.as_ref().and_then(|r| r.track_gain),
                ),
                Err(e) => println!("FIX-OGG {name:<16} ERR {e}"),
            }
        }
    }
}
