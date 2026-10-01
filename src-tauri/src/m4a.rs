// ---------------------------------------------------------------------------
// M4A / MP4 parser (ftyp, moov/udta/meta/ilst, stsd fourcc, covr)
// Single-pass tree builder, then direct lookups.
// ---------------------------------------------------------------------------

use crate::{Picture, StreamInfo, TrackMeta};
use std::path::Path;

/// Parse M4A and return (TrackMeta, is_alac) for disambiguation in scan_library
pub fn parse_m4a(buf: &[u8], path: &Path) -> Result<(TrackMeta, bool), String> {
    let tree = build_tree(buf)?;
    
    // Verify ftyp
    let ftyp = tree.find_box(b"ftyp").ok_or("m4a: missing ftyp")?;
    if !ftyp.data.starts_with(b"M4A") && !ftyp.data.starts_with(b"isom") {
        return Err("m4a: unsupported brand".into());
    }

    // Find moov box
    let moov = tree.find_box(b"moov").ok_or("m4a: missing moov")?;

    // Find first audio trak
    let trak = moov.children.iter().find(|b| b.name == "trak").ok_or("m4a: no trak")?;

    // mdhd: sample rate and duration
    let mdhd = trak.find_box(b"mdhd").ok_or("m4a: missing mdhd")?;
    let (sample_rate, total_samples) = parse_mdhd(&mdhd.data)?;

    // stsd fourcc: mp4a = AAC, alac = ALAC
    let stsd = trak.find_box(b"stsd").ok_or("m4a: missing stsd")?;
    let fourcc = parse_stsd_fourcc(&stsd.data)?;

    let (format, lossless, is_alac) = match fourcc.as_str() {
        "mp4a" => ("AAC", false, false),
        "alac" => ("ALAC", true, true),
        _ => return Err(format!("m4a: unsupported codec fourcc {}", fourcc)),
    };

    // ilst tags via udta/meta/ilst
    let mut title = String::new();
    let mut artist = String::new();
    let mut album = String::new();
    let mut cover: Option<Picture> = None;
    let mut lyrics = String::new();

    if let Some(udta) = moov.children.iter().find(|b| b.name == "udta") {
        if let Some(meta) = udta.children.iter().find(|b| b.name == "meta") {
            if let Some(ilst) = meta.children.iter().find(|b| b.name == "ilst") {
                parse_ilst(&ilst.children, &mut title, &mut artist, &mut album, &mut cover, &mut lyrics)?;
            }
        }
    }

    let duration = total_samples as f64 / sample_rate as f64;
    let bitrate_kbps = if duration > 0.0 {
        ((buf.len() as f64) * 8.0 / duration / 1000.0).round() as u32
    } else {
        0
    };

    let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
    Ok((
        TrackMeta {
            path: path.to_string_lossy().to_string(),
            title: if title.is_empty() { file_stem.to_string() } else { title },
            artist: if artist.is_empty() { "Unknown artist".into() } else { artist },
            album: if album.is_empty() { "Unknown album".into() } else { album },
            streaminfo: StreamInfo {
                sample_rate,
                bits: 16,
                channels: 2,
                total_samples,
            },
            duration,
            cover,
            replaygain: None,
            format_tag: format.into(),
            lossless,
            bitrate_kbps,
            block_types: Vec::new(), // M4A doesn't have FLAC-style metadata blocks
            has_cover: false,
            embedded_lyrics: if lyrics.trim().is_empty() { None } else { Some(lyrics) },
        },
        is_alac,
    ))
}

#[derive(Debug)]
struct Mp4Box {
    name: String,
    data: Vec<u8>,
    children: Vec<Mp4Box>,
}

impl Mp4Box {
    fn find_box(&self, name: &[u8]) -> Option<&Mp4Box> {
        let target: String = name.iter().map(|&b| b as char).collect();
        // Search children recursively
        for child in &self.children {
            if child.name == target {
                return Some(child);
            }
            if let Some(found) = child.find_box(name) {
                return Some(found);
            }
        }
        None
    }
}

/// Build the entire box tree from the buffer.
fn build_tree(buf: &[u8]) -> Result<Mp4Box, String> {
    let mut off = 0;
    let mut root_children = Vec::new();
    while off + 8 <= buf.len() {
        if let Some(box_struct) = parse_one_box(buf, &mut off, buf.len())? {
            root_children.push(box_struct);
        } else {
            break;
        }
    }
    Ok(Mp4Box {
        name: "root".into(),
        data: Vec::new(),
        children: root_children,
    })
}

/// Parse a single box at `off`, advance `off` to after the box.
/// Returns None if no box could be parsed.
fn parse_one_box(buf: &[u8], off: &mut usize, end: usize) -> Result<Option<Mp4Box>, String> {
    if *off + 8 > end {
        return Ok(None);
    }
    let sz = u32::from_be_bytes([buf[*off], buf[*off + 1], buf[*off + 2], buf[*off + 3]]) as usize;
    let name_bytes = &buf[*off + 4..*off + 8];
    let name: String = name_bytes.iter().map(|&b| b as char).collect(); // latin1: keeps 0xA9 ("©nam")
    let hdr = 8;
    // MP4 box size INCLUDES the 8-byte header
    let size = if sz == 1 {
        if *off + 16 > end {
            return Err("m4a: 64-bit size too short".into());
        }
        u64::from_be_bytes(buf[*off + 8..*off + 16].try_into().map_err(|_| "m4a: 64-bit size bad")?) as usize
    } else if sz == 0 {
        end - *off
    } else {
        sz
    };
    let data_off = *off + hdr;
    let data_end = *off + size;
    if data_end > end {
        return Err("m4a: box extends past end".into());
    }
    let data = buf[data_off..data_end].to_vec();
    let is_container = is_container_box(&name);
    let children = if is_container {
        // Parse children using RELATIVE offsets within the data slice
        let mut child_off = 0;
        let child_end = data_end - data_off;
        if name == "meta" {
            child_off += 4; // skip version/flags
        }
        let mut children = Vec::new();
        while child_off + 8 <= child_end {
            if let Some(child) = parse_one_box(&data, &mut child_off, child_end)? {
                children.push(child);
            } else {
                break;
            }
        }
        children
    } else {
        Vec::new()
    };
    *off = data_end;
    Ok(Some(Mp4Box { name, data, children }))
}

fn is_container_box(name: &str) -> bool {
    matches!(
        name,
        "moov" | "trak" | "mdia" | "minf" | "stbl" | "udta" | "meta" | "ilst"
            | "edts" | "dinf" | "stsd" | "stts" | "stsc" | "stsz" | "stco"
    )
}

fn parse_mdhd(data: &[u8]) -> Result<(u32, u64), String> {
    if data.len() < 20 {
        return Err("m4a: mdhd too short".into());
    }
    let ver = data[0];
    if ver == 1 {
        if data.len() < 36 {
            return Err("m4a: mdhd v1 too short".into());
        }
        let ts = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let dur = u64::from_be_bytes([
            data[20], data[21], data[22], data[23], data[24], data[25], data[26], data[27],
        ]);
        Ok((ts, dur))
    } else {
        if data.len() < 24 {
            return Err("m4a: mdhd v0 too short".into());
        }
        let ts = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
        let dur = u32::from_be_bytes([data[16], data[17], data[18], data[19]]) as u64;
        Ok((ts, dur))
    }
}

fn parse_stsd_fourcc(data: &[u8]) -> Result<String, String> {
    if data.len() < 16 {
        return Err("m4a: stsd too short".into());
    }
    let entry_off = 8;
    if entry_off + 4 <= data.len() {
        Ok(std::str::from_utf8(&data[entry_off + 4..entry_off + 8])
            .unwrap_or("")
            .to_string())
    } else {
        Ok(String::new())
    }
}

fn parse_ilst(
    children: &[Mp4Box],
    title: &mut String,
    artist: &mut String,
    album: &mut String,
    cover: &mut Option<Picture>,
    lyrics: &mut String,
) -> Result<(), String> {
    for box_item in children {
        match box_item.name.as_str() {
            "\u{a9}nam" => *title = parse_ilst_string(&box_item.data)?,
            "\u{a9}ART" => *artist = parse_ilst_string(&box_item.data)?,
            "\u{a9}alb" => *album = parse_ilst_string(&box_item.data)?,
            "\u{a9}lyr" => *lyrics = parse_ilst_string(&box_item.data)?,
            "covr" => {
                // covr is not in the container list, so its `data` atom is
                // still raw bytes: size(4) "data"(4) type(4) locale(4) image
                let d = &box_item.data;
                if box_item.children.is_empty() && d.len() > 16 && &d[4..8] == b"data" {
                    let end = (u32::from_be_bytes([d[0], d[1], d[2], d[3]]) as usize).clamp(16, d.len());
                    let img = &d[16..end];
                    let (mime, w, h) = detect_image(img);
                    *cover = Some(Picture {
                        mime,
                        data_b64: crate::base64_encode(img),
                        width: w,
                        height: h,
                        data_len: img.len() as u32,
                    });
                }
                for child in &box_item.children {
                    if child.name == "data" && !child.data.is_empty() {
                        let (mime, w, h) = detect_image(&child.data);
                        *cover = Some(Picture {
                            mime,
                            data_b64: crate::base64_encode(&child.data),
                            width: w,
                            height: h,
                            data_len: child.data.len() as u32,
                        });
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_ilst_string(data: &[u8]) -> Result<String, String> {
    if data.len() < 16 {
        return Ok(String::new());
    }
    let s = std::str::from_utf8(&data[16..])
        .unwrap_or("")
        .trim_end_matches('\0');
    Ok(s.to_string())
}

fn detect_image(data: &[u8]) -> (String, u32, u32) {
    if data.len() >= 8 && &data[0..8] == b"\x89PNG\r\n\x1a\n" {
        if data.len() >= 24 {
            let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
            let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
            return ("image/png".into(), w, h);
        }
        return ("image/png".into(), 0, 0);
    }
    if data.len() >= 3 && &data[0..3] == b"\xFF\xD8\xFF" {
        return ("image/jpeg".into(), 0, 0);
    }
    ("application/octet-stream".into(), 0, 0)
}

// ---------------------------------------------------------------------------
// ADTS (raw AAC) — minimal frame scan for sr/ch/duration
// ---------------------------------------------------------------------------

pub fn parse_adts(buf: &[u8], path: &Path) -> Result<TrackMeta, String> {
    let mut p = 0;
    let mut sample_rate = 0u32;
    let mut channels = 0u8;
    let mut total_samples = 0u64;
    while p + 7 <= buf.len() {
        if buf[p] != 0xFF || (buf[p + 1] & 0xF6) != 0xF0 {
            p += 1;
            continue;
        }
        let sr_idx = (buf[p + 2] >> 2) & 0x0F;
        let ch = (buf[p + 2] & 0x01) << 2 | (buf[p + 3] >> 6);
        let frame_len = (((buf[p + 3] & 0x03) as usize) << 11)
            | ((buf[p + 4] as usize) << 3)
            | ((buf[p + 5] >> 5) as usize);
        if frame_len < 7 {
            p += 1; // corrupt header: zero/short length would never advance
            continue;
        }
        let frame_samples = 1024u64;
        let adts_rates = [
            96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025,
            8000, 7350, 0, 0, 0,
        ];
        let sr = adts_rates[sr_idx as usize];
        if sr > 0 {
            sample_rate = sr;
        }
        if ch > 0 {
            channels = ch;
        }
        total_samples += frame_samples;
        p += frame_len;
    }
    if sample_rate == 0 || channels == 0 {
        return Err("adts: no valid frames".into());
    }
    let duration = total_samples as f64 / sample_rate as f64;
    let bitrate_kbps = if duration > 0.0 {
        ((buf.len() as f64) * 8.0 / duration / 1000.0).round() as u32
    } else {
        0
    };
    let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
    Ok(TrackMeta {
        path: path.to_string_lossy().to_string(),
        title: file_stem.to_string(),
        artist: "Unknown artist".into(),
        album: "Unknown album".into(),
        streaminfo: StreamInfo {
            sample_rate,
            bits: 16,
            channels,
            total_samples,
        },
        duration,
        cover: None,
        replaygain: None,
        format_tag: "AAC".into(),
        lossless: false,
        bitrate_kbps,
        block_types: Vec::new(),
        has_cover: false,
        embedded_lyrics: None,
    })
}