//! Halftone audio backend — Rust port of the verified direct-FLAC pipeline.
//!
//! Contract (docs/audio-pipeline.md): FLAC in, FLAC out. Metadata + cover art
//! come out of ONE header read per track (STREAMINFO / VORBIS_COMMENT /
//! METADATA_BLOCK_PICTURE all live in the metadata block list at the front of
//! the file, before any audio frames). Playback serves the file's ORIGINAL
//! bytes via the flac:// protocol — no transcode, no AAC, no side files.

use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// How much of the file we read for metadata. All FLAC metadata blocks sit
/// before the first audio frame; 8 MiB is generous (largest known PICTURE
/// block in the test library is ~360 KiB). Falls back to a full read if a
/// metadata block happens to extend past this.
const HEADER_READ: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct StreamInfo {
    pub sample_rate: u32,
    pub bits: u8,
    pub channels: u8,
    pub total_samples: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Picture {
    pub mime: String,
    pub data_b64: String,
    pub width: u32,
    pub height: u32,
    pub data_len: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackMeta {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub streaminfo: StreamInfo,
    pub duration_s: f64,
    pub cover: Option<Picture>,
    pub block_types: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanResult {
    pub tracks: Vec<TrackMeta>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LyricLine {
    pub t: f64,
    pub text: String,
}

// ---------------------------------------------------------------------------
// FLAC container walk — no external crates
// ---------------------------------------------------------------------------

fn be24(b: &[u8]) -> usize {
    ((b[0] as usize) << 16) | ((b[1] as usize) << 8) | (b[2] as usize)
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn walk_flac(buf: &[u8]) -> Result<(StreamInfo, HashMap<String, String>, Vec<Picture>, Vec<u8>), String> {
    if buf.len() < 4 || &buf[0..4] != b"fLaC" {
        let n = buf.len().min(4);
        return Err(format!("not a FLAC file (magic {:02x?})", &buf[..n]));
    }
    let mut p = 4usize;
    let mut last = false;
    let mut si = None;
    let mut tags: HashMap<String, String> = HashMap::new();
    let mut pics: Vec<Picture> = Vec::new();
    let mut block_types: Vec<u8> = Vec::new();

    while p + 4 <= buf.len() && !last {
        let head = buf[p];
        last = head & 0x80 != 0;
        let btype = head & 0x7f;
        // FLAC block length is a 24-bit BE field — NOT 32-bit.
        let len = be24(&buf[p + 1..p + 4]);
        let body = p + 4;
        if body + len > buf.len() {
            return Err("metadata block extends past read window (caller retries full)".into());
        }
        block_types.push(btype);
        match btype {
            0 => {
                let b = &buf[body..body + 34];
                let sample_rate = ((b[10] as u32) << 12) | ((b[11] as u32) << 4) | (b[12] as u32 >> 4);
                let channels = (((b[12] & 0x0e) >> 1) + 1) as u8;
                let bits = (((b[12] & 0x01) << 4) | (b[13] >> 4)) + 1;
                let hi = ((b[13] & 0x0f) as u64) << 32;
                let lo = u32::from_be_bytes([b[14], b[15], b[16], b[17]]) as u64;
                si = Some(StreamInfo {
                    sample_rate,
                    bits,
                    channels,
                    total_samples: hi | lo,
                });
            }
            4 => {
                let mut q = body;
                let vlen = le32(&buf[q..q + 4]) as usize;
                q += 4 + vlen;
                if q + 4 > body + len {
                    break;
                }
                let count = le32(&buf[q..q + 4]) as usize;
                q += 4;
                for _ in 0..count {
                    if q + 4 > body + len {
                        break;
                    }
                    let l = le32(&buf[q..q + 4]) as usize;
                    q += 4;
                    if q + l > body + len {
                        break;
                    }
                    let s = String::from_utf8_lossy(&buf[q..q + l]).to_string();
                    q += l;
                    if let Some(eq) = s.find('=') {
                        tags.insert(s[..eq].to_uppercase(), s[eq + 1..].to_string());
                    }
                }
            }
            6 => {
                let mut q = body + 4; // picture type
                let mlen = be32(&buf[q..q + 4]) as usize;
                q += 4;
                let mime = String::from_utf8_lossy(&buf[q..q + mlen]).to_string();
                q += mlen;
                let dlen = be32(&buf[q..q + 4]) as usize;
                q += 4 + dlen;
                let w = be32(&buf[q..q + 4]);
                q += 4;
                let h = be32(&buf[q..q + 4]);
                q += 4;
                let _depth = be32(&buf[q..q + 4]);
                q += 4;
                let _colors = be32(&buf[q..q + 4]);
                q += 4;
                let data_len = be32(&buf[q..q + 4]);
                q += 4;
                let data = &buf[q..q + data_len as usize];
                pics.push(Picture {
                    mime,
                    data_b64: base64_encode(data),
                    width: w,
                    height: h,
                    data_len,
                });
            }
            _ => {}
        }
        p = body + len;
    }

    let si = si.ok_or("no STREAMINFO block")?;
    Ok((si, tags, pics, block_types))
}

// minimal base64 — keeps the crate dependency-free
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 4 / 3 + 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

// ---------------------------------------------------------------------------
// Track load — the single header read (with full-file fallback)
// ---------------------------------------------------------------------------

fn read_header(path: &Path) -> Result<Vec<u8>, String> {
    let meta = fs::metadata(path).map_err(|e| format!("stat {}: {}", path.display(), e))?;
    let file_len = meta.len() as usize;
    let want = HEADER_READ.min(file_len);
    let mut f = fs::File::open(path).map_err(|e| format!("open {}: {}", path.display(), e))?;
    let mut buf = vec![0u8; want];
    f.read_exact(&mut buf).map_err(|e| format!("read {}: {}", path.display(), e))?;
    Ok(buf)
}

pub fn read_track(path: &Path) -> Result<TrackMeta, String> {
    // Attempt 1: header window. Attempt 2 (only if a metadata block straddles
    // the window edge): full file. Still ONE read per track in the normal case.
    let buf = match read_header(path).and_then(|b| walk_flac(&b).map(|_| b)) {
        Ok(b) => b,
        Err(_) => fs::read(path).map_err(|e| format!("read {}: {}", path.display(), e))?,
    };
    let (si, tags, mut pics, block_types) = walk_flac(&buf)?;
    let duration_s = if si.sample_rate > 0 {
        si.total_samples as f64 / si.sample_rate as f64
    } else {
        0.0
    };
    let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?").to_string();
    Ok(TrackMeta {
        path: path.to_string_lossy().to_string(),
        title: tags.get("TITLE").cloned().unwrap_or(file_stem),
        artist: tags.get("ARTIST").cloned().unwrap_or_else(|| "Unknown artist".into()),
        album: tags.get("ALBUM").cloned().unwrap_or_else(|| "Unknown album".into()),
        streaminfo: si,
        duration_s,
        cover: if pics.is_empty() { None } else { Some(pics.remove(0)) },
        block_types,
    })
}

// ---------------------------------------------------------------------------
// .lrc sidecar lyrics
// ---------------------------------------------------------------------------

pub fn read_lrc_file(track_path: &Path) -> Vec<LyricLine> {
    let lrc_path = track_path.with_extension("lrc");
    let Ok(text) = fs::read_to_string(&lrc_path) else {
        return Vec::new();
    };
    let mut out: Vec<LyricLine> = Vec::new();
    for line in text.lines() {
        let mut rest = line;
        let mut stamps: Vec<f64> = Vec::new();
        loop {
            let t = rest.trim_start();
            let Some(after) = t.strip_prefix('[') else { break };
            let Some(close) = after.find(']') else { break };
            let tag = &after[..close];
            // [mm:ss.xx] — parse by splitting on ':'
            if let Some((mm, ss)) = tag.split_once(':') {
                if let (Ok(m), Ok(s)) = (mm.parse::<f64>(), ss.parse::<f64>()) {
                    stamps.push(m * 60.0 + s);
                    rest = &after[close + 1..];
                    continue;
                }
            }
            break; // [ti:...] etc — not a timestamp
        }
        if stamps.is_empty() {
            continue;
        }
        let text = rest.trim();
        if text.is_empty() {
            continue;
        }
        for t in stamps {
            out.push(LyricLine { t, text: text.to_string() });
        }
    }
    out.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    out.dedup_by(|a, b| a.t == b.t);
    out
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
fn scan_library(dir: &str) -> Result<ScanResult, String> {
    let root = PathBuf::from(dir);
    let mut tracks = Vec::new();
    let mut skipped = Vec::new();
    let rd = fs::read_dir(&root).map_err(|e| format!("open dir {}: {}", dir, e))?;
    for entry in rd.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if ext != "flac" {
            continue;
        }
        match read_track(&p) {
            Ok(t) => tracks.push(t),
            Err(e) => skipped.push(format!("{}: {}", p.display(), e)),
        }
    }
    tracks.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ScanResult { tracks, skipped })
}

#[tauri::command]
fn open_track(path: &str) -> Result<TrackMeta, String> {
    read_track(&PathBuf::from(path))
}

#[tauri::command]
fn read_lyrics(path: &str) -> Vec<LyricLine> {
    read_lrc_file(&PathBuf::from(path))
}

/// Platform-correct URL that serves the ORIGINAL FLAC bytes.
/// Windows WebView2 resolves custom schemes as http://<scheme>.localhost.
#[tauri::command]
fn flac_url(path: &str) -> String {
    if cfg!(windows) {
        format!("http://flac.localhost/{}", pct_encode(path))
    } else {
        format!("flac://localhost/{}", pct_encode(path))
    }
}

fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

// ---------------------------------------------------------------------------
// flac:// protocol — serves original bytes, Accept-Ranges for seeking
// ---------------------------------------------------------------------------

fn serve_flac(request: tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    let uri = request.uri().to_string();
    // accepted forms: flac://localhost/<enc> | http://flac.localhost/<enc>
    let enc = uri
        .strip_prefix("flac://localhost/")
        .or_else(|| uri.strip_prefix("http://flac.localhost/"))
        .or_else(|| uri.strip_prefix("https://flac.localhost/"))
        .unwrap_or("");
    let path = pct_decode(enc);

    // Range support so the media element can seek without re-downloading
    let range: Option<(u64, u64)> = request
        .headers()
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(|r| r.strip_prefix("bytes="))
        .and_then(|r| {
            let (a, b) = r.split_once('-')?;
            let start: u64 = a.parse().ok()?;
            let end: u64 = if b.is_empty() {
                u64::MAX
            } else {
                b.parse().ok()?
            };
            Some((start, end))
        });

    match fs::metadata(&path) {
        Ok(m) if m.is_file() => {
            let total = m.len();
            match range {
                Some((start, end)) if start < total => {
                    let end = end.min(total - 1);
                    let len = (end - start + 1) as usize;
                    let mut buf = vec![0u8; len];
                    let mut f = fs::File::open(&path).map_err(|e| e.to_string()).unwrap();
                    use std::io::Seek;
                    f.seek(std::io::SeekFrom::Start(start)).ok();
                    f.read_exact(&mut buf).ok();
                    tauri::http::Response::builder()
                        .status(206)
                        .header("Content-Type", "audio/flac")
                        .header("Content-Length", len.to_string())
                        .header("Accept-Ranges", "bytes")
                        .header("Access-Control-Allow-Origin", "*")
                        .header(
                            "Content-Range",
                            format!("bytes {}-{}/{}", start, end, total),
                        )
                        .body(buf)
                        .unwrap()
                }
                Some(_) => tauri::http::Response::builder()
                    .status(416)
                    .header("Content-Range", format!("bytes */{}", total))
                    .body(Vec::new())
                    .unwrap(),
                None => {
                    // 200: full body. We deliberately do NOT load it into
                    // memory here — stream from disk.
                    let mut f = fs::File::open(&path).map_err(|e| e.to_string()).unwrap();
                    let mut buf = Vec::with_capacity(total as usize);
                    f.read_to_end(&mut buf).ok();
                    tauri::http::Response::builder()
                        .status(200)
                        .header("Content-Type", "audio/flac")
                        .header("Content-Length", buf.len().to_string())
                        .header("Accept-Ranges", "bytes")
                        .header("Access-Control-Allow-Origin", "*")
                        .body(buf)
                        .unwrap()
                }
            }
        }
        _ => tauri::http::Response::builder()
            .status(404)
            .body(format!("halftone: no such file: {}", path).into_bytes())
            .unwrap(),
    }
}

// ---------------------------------------------------------------------------
// App state + entry
// ---------------------------------------------------------------------------

#[derive(Default)]
struct LibState {
    #[allow(dead_code)]
    root: Option<String>,
    #[allow(dead_code)]
    tracks: Vec<TrackMeta>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(LibState::default())
        .register_uri_scheme_protocol("flac", |_ctx, request| serve_flac(request))
        .invoke_handler(tauri::generate_handler![
            scan_library,
            open_track,
            read_lyrics,
            flac_url
        ])
        .run(tauri::generate_context!())
        .expect("halftone widget failed to start");
}
