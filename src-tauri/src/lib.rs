//! Halftone audio backend — direct-bytes pipeline, now format-neutral.
//!
//! Contract (docs/audio-pipeline.md): metadata + cover art come out of ONE
//! header read per track. Playback serves the file's ORIGINAL bytes via the
//! media:// protocol (ALAC is the one documented exception: decoded once to
//! PCM in Rust, no lossy transcode). FLAC has its own verified parser;
//! WAV/MP3/AAC/ALAC live in formats.rs/alac.rs.

use serde::Serialize;
use tauri::Manager;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

// Phase 1 format parsers + ALAC decode (authored once, #[path]-included into both the
// standalone TDD harness and this crate). They must NOT depend on tauri/serde.
#[path = "formats.rs"]
mod formats;
mod probe;
mod lyrics;
mod media_source;
#[path = "alac.rs"]
mod alac;
#[path = "m4a.rs"]
mod m4a;

// OTA update channel: latest.json published as a release asset. Override the
// source for QA with HALFTONE_OTA_URL; disable the check with HALFTONE_NO_OTA.
const OTA_LATEST_URL: &str =
    "https://github.com/Trapston3/Halftone/releases/latest/download/latest.json";

/// Normalize a user-pasted folder path: trim whitespace, then strip the
/// surrounding quotes Explorer's "Copy as path" adds when the path contains
/// spaces ("C:\Users\Sashankar J\..."). A plain drive root "C:\" survives.
fn sanitize_dir(dir: &str) -> String {
    let t = dir.trim();
    let t = t.trim_matches('"').trim_matches('\'').trim();
    let t = if t.chars().count() > 3 { t.trim_end_matches(['\\', '/']) } else { t };
    t.trim().to_string()
}

pub fn base64_decode_pub(s: &str) -> Option<Vec<u8>> {
    base64_decode(s)
}

/// How much of the file we read for metadata. All FLAC metadata blocks sit
/// before the first audio frame; 8 MiB is generous (largest known PICTURE
/// block in the test library is ~360 KiB). Falls back to a full read if a
/// metadata block happens to extend past this.
const HEADER_READ: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct StreamInfo {
    pub sample_rate: u32,
    pub bits: u8,
    pub channels: u8,
    pub total_samples: u64,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct Picture {
    pub mime: String,
    pub data_b64: String,
    pub width: u32,
    pub height: u32,
    pub data_len: u32,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct TrackMeta {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub streaminfo: StreamInfo,
    #[serde(rename = "duration_s")]
    pub duration: f64,
    pub cover: Option<Picture>,
    pub block_types: Vec<u8>,
    /// REPLAYGAIN_TRACK_GAIN / REPLAYGAIN_ALBUM_GAIN from VORBIS_COMMENT
    /// (already parsed in the same single read — zero extra I/O).
    pub replaygain: Option<ReplayGain>,
    /// Format tag: "FLAC" | "WAV" | "MP3" | "AAC" | "ALAC"
    #[serde(rename = "format")]
    pub format_tag: String,
    /// True for FLAC/WAV/ALAC, false for MP3/AAC
    pub lossless: bool,
    /// Average bitrate in kbps (rounded)
    pub bitrate_kbps: u32,
    /// Embedded art exists. Scans never ship the bytes (they made the scan
    /// payload enormous); the UI loads art via the `/cover/` media route.
    #[serde(default)]
    pub has_cover: bool,
    /// Lyrics stored in the file's own tags (LYRICS / USLT / ©lyr). Only
    /// filled by open_track; scans drop it like the cover.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedded_lyrics: Option<String>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct ReplayGain {
    pub track_gain: Option<f64>,
    pub album_gain: Option<f64>,
}

fn parse_rg_db(tag: &str) -> Option<f64> {
    // tags like "-7.20 dB" / "+3.5 dB" / "-6.4"
    let t = tag.trim();
    let num: String = t.chars().take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '+' || *c == '.').collect();
    num.parse::<f64>().ok()
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanResult {
    pub tracks: Vec<TrackMeta>,
    pub skipped: Vec<String>,
    /// Files with audio extensions Halftone can't play yet (FLAC-only scope).
    pub unsupported: usize,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
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

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut out = Vec::new();
    for c in s.bytes() {
        if c == b'=' {
            break;
        }
        let v = match B64.iter().position(|&b| b == c) {
            Some(idx) => idx as u32,
            None => return None,
        };
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
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
    track_from_flac_buf(&buf, path)
}

/// FLAC TrackMeta from a buffer holding (at least) the metadata blocks.
pub(crate) fn track_from_flac_buf(buf: &[u8], path: &Path) -> Result<TrackMeta, String> {
    let (si, tags, mut pics, block_types) = walk_flac(buf)?;
    let duration_s = if si.sample_rate > 0 {
        si.total_samples as f64 / si.sample_rate as f64
    } else {
        0.0
    };
    let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?").to_string();
    let replaygain = if tags.contains_key("REPLAYGAIN_TRACK_GAIN") || tags.contains_key("REPLAYGAIN_ALBUM_GAIN") {
        Some(ReplayGain {
            track_gain: tags.get("REPLAYGAIN_TRACK_GAIN").and_then(|t| parse_rg_db(t)),
            album_gain: tags.get("REPLAYGAIN_ALBUM_GAIN").and_then(|t| parse_rg_db(t)),
        })
    } else {
        None
    };
    Ok(TrackMeta {
        path: path.to_string_lossy().to_string(),
        title: tags.get("TITLE").cloned().unwrap_or(file_stem),
        artist: tags.get("ARTIST").cloned().unwrap_or_else(|| "Unknown artist".into()),
        album: tags.get("ALBUM").cloned().unwrap_or_else(|| "Unknown album".into()),
        streaminfo: si,
        duration: duration_s,
        cover: if pics.is_empty() { None } else { Some(pics.remove(0)) },
        block_types,
        replaygain,
        format_tag: "FLAC".into(),
        lossless: true,
        bitrate_kbps: 0, // FLAC bitrate varies per frame; 0 = unknown/VBR
        has_cover: !pics.is_empty(),
        embedded_lyrics: tags.get("LYRICS").or_else(|| tags.get("UNSYNCEDLYRICS")).cloned(),
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
    parse_lrc(&text)
}

/// LRC text -> sorted lines (multi-timestamp lines expand to one per stamp).
pub fn parse_lrc(text: &str) -> Vec<LyricLine> {
    let mut out: Vec<LyricLine> = Vec::new();
    for line in text.lines() {
        let mut rest = line;
        let mut stamps: Vec<f64> = Vec::new();
        loop {
            let t = rest.trim_start();
            let Some(after) = t.strip_prefix('[') else { break };
            let Some(close) = after.find(']') else { break };
            let tag = &after[..close];
            if let Some((mm, ss)) = tag.split_once(':') {
                if let (Ok(m), Ok(s)) = (mm.trim().parse::<f64>(), ss.trim().parse::<f64>()) {
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

/// App handle captured at setup; watcher + any backend->UI emit uses it.
static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

/// Watch the library folder; on any change, emit halftone:lib-changed so
/// the owner UI auto-rescans. One dedicated watcher thread per scan.
fn watch_folder(app: tauri::AppHandle, dir: &str) {
    let d = dir.to_string();
    let h = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = try_watch(h, d) {
            eprintln!("halftone watch: {e}");
        }
    });
}

fn try_watch(app: tauri::AppHandle, dir: String) -> Result<(), String> {
    use notify::Watcher as _;
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::RecommendedWatcher::new(tx, notify::Config::default())
        .map_err(|e| e.to_string())?;
    watcher.watch(Path::new(&dir), notify::RecursiveMode::NonRecursive)
        .map_err(|e| e.to_string())?;
    // watcher must outlive the loop; debounce bursts then notify the owner UI
    loop {
        match rx.recv_timeout(std::time::Duration::from_secs(86400)) {
            Ok(_) => {
                while rx.recv_timeout(std::time::Duration::from_millis(700)).is_ok() {}
                use tauri::Emitter;
                let _ = app.emit("halftone:lib-changed", {});
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------
// Persistent scan index: path -> (size, mtime, meta). Rescans of an
// unchanged library parse nothing.
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize, serde::Deserialize)]
struct ScanEntry {
    size: u64,
    mtime: u64,
    meta: TrackMeta,
}

static LIBRARY: std::sync::Mutex<Vec<TrackMeta>> = std::sync::Mutex::new(Vec::new());

fn scan_index_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("scan_index.json"))
}

fn scan_index_load(app: &tauri::AppHandle) -> HashMap<String, ScanEntry> {
    scan_index_path(app)
        .and_then(|p| fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn scan_index_save(app: &tauri::AppHandle, idx: &HashMap<String, ScanEntry>) {
    let Some(p) = scan_index_path(app) else { return };
    let _ = write_atomic(&p, &serde_json::to_vec(idx).unwrap_or_default());
}

pub(crate) fn write_atomic(p: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(d) = p.parent() {
        fs::create_dir_all(d)?;
    }
    let tmp = p.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, p)
}

/// Last scan result (no covers) so viewer windows can fetch the library
/// from Rust instead of receiving it inside every sync broadcast.
#[tauri::command]
fn library_snapshot() -> Vec<TrackMeta> {
    LIBRARY.lock().map(|l| l.clone()).unwrap_or_default()
}

/// Extensions WebView2 can likely play but Halftone does not support yet.
const UNSUPPORTED_AUDIO: &[&str] = &["ogg", "oga", "opus", "wma", "aiff", "aif", "ape", "wv"];

/// Format counters for per-format scan reporting.
#[derive(Debug, Clone, Serialize)]
pub struct ScanCounts {
    pub flac: usize,
    pub wav: usize,
    pub mp3: usize,
    pub aac: usize,
    pub alac: usize,
    pub unsupported: usize,
}

impl ScanCounts {
    fn new() -> Self {
        Self { flac: 0, wav: 0, mp3: 0, aac: 0, alac: 0, unsupported: 0 }
    }
}

#[tauri::command]
fn scan_library(app: tauri::AppHandle, dir: &str) -> Result<ScanResult, String> {
    use tauri::Emitter;

    let dir = sanitize_dir(dir);
    if dir.is_empty() {
        return Err("folder not found: (empty path)".into());
    }
    if !Path::new(&dir).is_dir() {
        return Err(format!("folder not found: {}", dir));
    }
    let mut skipped = Vec::new();
    let mut counts = ScanCounts::new();
    let t0 = std::time::Instant::now();
    let _ = CANCEL_SCAN.swap(false, std::sync::atomic::Ordering::Relaxed);

    // 1. Collect candidate paths (recursive, Artist/Album/ layouts). Only
    //    audio extensions are opened; magic bytes still decide the format.
    fn collect(dir: &Path, depth: usize, out: &mut Vec<(PathBuf, u64, u64)>, unsupported: &mut usize) {
        if depth > 16 || CANCEL_SCAN.load(std::sync::atomic::Ordering::Relaxed) {
            return; // pathological nesting guard / cancelled
        }
        let Ok(rd) = fs::read_dir(dir) else { return };
        for entry in rd.flatten() {
            let p = entry.path();
            let Ok(md) = entry.metadata() else { continue };
            if md.is_dir() {
                collect(&p, depth + 1, out, unsupported);
            } else if md.is_file() {
                if probe::is_candidate(&p) {
                    let mtime = md.modified().ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs()).unwrap_or(0);
                    out.push((p, md.len(), mtime));
                } else if p.extension().and_then(|e| e.to_str())
                    .map(|e| UNSUPPORTED_AUDIO.contains(&e.to_ascii_lowercase().as_str()))
                    .unwrap_or(false)
                {
                    *unsupported += 1;
                }
            }
        }
    }
    let mut cands = Vec::new();
    collect(Path::new(&dir), 0, &mut cands, &mut counts.unsupported);

    // 2. Parse in parallel with bounded reads; unchanged files (same size +
    //    mtime) come straight from the persistent scan index.
    let index = scan_index_load(&app);
    let done = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<(String, u64, u64, Result<TrackMeta, String>)> = {
        use rayon::prelude::*;
        cands.par_iter().filter_map(|(p, size, mtime)| {
            if CANCEL_SCAN.load(std::sync::atomic::Ordering::Relaxed) {
                return None;
            }
            let key = p.to_string_lossy().to_string();
            let r = match index.get(&key) {
                Some(e) if e.size == *size && e.mtime == *mtime => Ok(e.meta.clone()),
                _ => probe::read_meta(p, false).map(|(m, _, _)| m),
            };
            let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if n % 50 == 0 {
                let _ = app.emit("halftone:scan-progress", serde_json::json!({
                    "found": n, "total": cands.len(),
                    "folder": p.parent().map(|d| d.display().to_string()).unwrap_or_default(),
                    "done": false,
                }));
            }
            Some((key, *size, *mtime, r))
        }).collect()
    };

    let mut tracks = Vec::with_capacity(results.len());
    let mut new_index = HashMap::with_capacity(results.len());
    for (key, size, mtime, r) in results {
        match r {
            Ok(m) => {
                match m.format_tag.as_str() {
                    "FLAC" => counts.flac += 1,
                    "WAV" => counts.wav += 1,
                    "MP3" => counts.mp3 += 1,
                    "ALAC" => counts.alac += 1,
                    _ => counts.aac += 1,
                }
                new_index.insert(key, ScanEntry { size, mtime, meta: m.clone() });
                tracks.push(m);
            }
            // a .mp3-named text file etc: report it, never silently drop
            Err(e) => skipped.push(format!("{}: {}", key, e)),
        }
    }
    let cancelled = CANCEL_SCAN.load(std::sync::atomic::Ordering::Relaxed);
    if !cancelled {
        scan_index_save(&app, &new_index);
    }
    tracks.sort_by(|a, b| a.path.cmp(&b.path));

    // Emit final progress with per-format breakdown
    let _ = app.emit(
        "halftone:scan-progress",
        serde_json::json!({
            "found": tracks.len(),
            "skipped": skipped.len(),
            "counts": {
                "flac": counts.flac,
                "wav": counts.wav,
                "mp3": counts.mp3,
                "aac": counts.aac,
                "alac": counts.alac,
                "unsupported": counts.unsupported,
            },
            "done": true,
            "ms": t0.elapsed().as_millis()
        }),
    );
    if let Some(app) = APP.get() {
        watch_folder(app.clone(), &dir);
    }
    if let Ok(mut lib) = LIBRARY.lock() {
        *lib = tracks.clone();
    }
    Ok(ScanResult { tracks, skipped, unsupported: counts.unsupported })
}

// ---------------------------------------------------------------------------
// Native folder picker (no plugin — one dialog, returns an owned String)
// ---------------------------------------------------------------------------

#[tauri::command]
fn pick_folder() -> Option<String> {
    let picked = rfd::FileDialog::new()
        .set_title("Choose your music folder")
        .pick_folder();
    picked.and_then(|p| p.to_str().map(|s| s.to_string()))
}

// ---------------------------------------------------------------------------
// OTA self-update — download a new exe, then replace after clean exit.
// Cannot touch the running image on Windows, so the new binary is staged as
// Halftone.update.next to the exe's folder and swap_install() finishes the
// job from a detached helper process once this one is gone.
// ---------------------------------------------------------------------------

fn ota_url() -> String {
    std::env::var("HALFTONE_OTA_URL").unwrap_or_else(|_| OTA_LATEST_URL.to_string())
}

/// Fetch + parse latest.json -> (version, exe url)
fn ota_latest() -> Result<(String, String), String> {
    let b = ota_fetch(&ota_url())?;
    let v: serde_json::Value = serde_json::from_slice(&b).map_err(|e| format!("bad latest.json: {e}"))?;
    let ver = v["version"].as_str().unwrap_or("").to_string();
    let url = v["url"].as_str().unwrap_or("").to_string();
    if ver.is_empty() || url.is_empty() {
        return Err("bad latest.json (missing version/url)".into());
    }
    Ok((ver, url))
}

#[derive(Serialize, Clone)]
struct OtaStatus {
    supported: bool,
    checking: bool,
    disabled: bool,
    current: String,
    available: Option<String>, // new version when one is found
    downloading: bool,
    ready: bool,       // staged, applied on next start
    error: Option<String>,
}

fn ota_paths() -> Result<(PathBuf, PathBuf), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no parent dir")?.to_path_buf();
    Ok((exe, dir.join("Halftone.update.next")))
}

#[tauri::command]
fn ota_check() -> OtaStatus {
    let mut st = OtaStatus {
        supported: true,
        checking: false,
        disabled: std::env::var("HALFTONE_NO_OTA").is_ok(),
        current: env!("CARGO_PKG_VERSION").to_string(),
        available: None,
        downloading: false,
        ready: ota_paths().map(|(_, s)| s.exists()).unwrap_or(false),
        error: None,
    };
    if st.disabled || st.ready {
        return st; // update already staged or check disabled (QA/dev)
    }
    match ota_latest() {
        Ok((ver, _url)) => {
            if ver != env!("CARGO_PKG_VERSION") {
                st.available = Some(ver);
            }
        }
        Err(e) => st.error = Some(e),
    }
    st
}

/// Fetch a URL over HTTPS with a 30s timeout and a UA. Small redirect
/// follower (github release links redirect to objects.githubusercontent).
fn http_get(url: &str, max_redirects: usize) -> Result<reqwest::blocking::Response, String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("halftone-ota")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let mut resp = client.get(url).send().map_err(|e| e.to_string())?;
    // reqwest follows redirects by default; this loop is a safety net.
    if resp.status().is_redirection() && max_redirects > 0 {
        if let Some(loc) = resp.headers().get(reqwest::header::LOCATION) {
            let loc = loc.to_str().map_err(|e| e.to_string())?.to_string();
            resp = http_get(&loc, max_redirects - 1)?;
        }
    }
    resp.error_for_status().map_err(|e| e.to_string())
}

fn ota_fetch(url: &str) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let resp = http_get(url, 5)?;
    let mut out = Vec::new();
    resp.take(64 * 1024 * 1024)
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[tauri::command]
fn ota_download() -> OtaStatus {
    let mut st = ota_check();
    if st.disabled || st.available.is_none() || st.ready {
        return st;
    }
    let ver = st.available.clone().unwrap_or_default();
    let url = match ota_latest() {
        Ok((v, u)) if v == ver => u,
        Ok(_) => {
            st.error = Some("latest.json changed mid-update".into());
            return st;
        }
        Err(e) => {
            st.error = Some(e);
            return st;
        }
    };
    let (_exe_path, stage) = match ota_paths() {
        Ok(p) => p,
        Err(e) => {
            st.error = Some(e);
            return st;
        }
    };
    match ota_fetch(&url) {
        Ok(bytes) => {
            if bytes.len() < 1_000_000 {
                st.error = Some(format!("downloaded exe too small ({} B)", bytes.len()));
                return st;
            }
            if std::fs::write(&stage, &bytes).is_err() {
                st.error = Some("cannot write update next to the exe".into());
                return st;
            }
            // sanity: PE images start with "MZ"
            if !bytes.starts_with(b"MZ") {
                let _ = std::fs::remove_file(&stage);
                st.error = Some("downloaded file is not a Windows exe".into());
                return st;
            }
            st.downloading = false;
            st.ready = true;
            st.available = Some(ver);
            st
        }
        Err(e) => {
            let _ = std::fs::remove_file(&stage);
            st.error = Some(format!("download failed: {e}"));
            st
        }
    }
}

#[tauri::command]
fn ota_apply() -> Result<String, String> {
    let (exe_path, stage) = ota_paths()?;
    if !stage.exists() {
        return Err("no update staged".into());
    }
    // sanity: PE images start with "MZ"
    let mut f = fs::File::open(&stage).map_err(|e| e.to_string())?;
    let mut magic = [0u8; 2];
    f.read_exact(&mut magic).map_err(|e| e.to_string())?;
    drop(f);
    if &magic != b"MZ" {
        let _ = std::fs::remove_file(&stage);
        return Err("staged file is not a Windows exe".into());
    }
    let exe = exe_path.to_string_lossy().to_string();
    let stage_s = stage.to_string_lossy().to_string();
    /* Helper as a generated .cmd file — nested cmd /C start /C "..." quoting is
       fragile (cmd's quote stripping mangles it); a batch file is deterministic.
       %~f0 self-deletes the script; the app must be gone before copy, hence the
       2s wait (process::exit below takes effect immediately). */
    let bat = exe_path.parent().ok_or("no parent dir")?.join("Halftone.update.cmd");
    let script = format!(
        "@echo off\r\ntimeout /T 2 /NOBREAK >NUL\r\ncopy /Y \"{st}\" \"{ex}\" >NUL\r\nif errorlevel 1 exit 1\r\ndel /Q \"{st}\"\r\nstart \"\" \"{ex}\"\r\ndel /Q \"%~f0\"\r\n",
        st = stage_s,
        ex = exe
    );
    fs::write(&bat, script).map_err(|e| format!("cannot write update helper: {e}"))?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd")
            .arg("/C")
            .arg(&bat)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(windows))]
    std::process::Command::new("sh").arg("-c").arg(&script).spawn().map_err(|e| e.to_string())?;
    std::process::exit(0);
}


static CANCEL_SCAN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
fn scan_cancel() {
    CANCEL_SCAN.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[tauri::command]
fn open_track(path: &str) -> Result<TrackMeta, String> {
    // Bounded reads; detect by MAGIC bytes, not extension. Full cover kept
    // (SMTC + now-playing need it) — this is one track, not the library.
    probe::read_meta(Path::new(path), true).map(|(m, _, _)| m)
}

#[tauri::command]
fn read_lyrics(path: &str) -> Vec<LyricLine> {
    read_lrc_file(&PathBuf::from(path))
}

/// Format-neutral URL that serves the file's ORIGINAL bytes (Phase 1:
/// FLAC/WAV/MP3/AAC direct, ALAC as decoded-PCM WAV wrapper — see
/// docs/audio-pipeline.md). Windows WebView2 resolves custom schemes as
/// http://<scheme>.localhost.
#[tauri::command]
fn media_url(path: &str) -> String {
    if cfg!(windows) {
        format!("http://media.localhost/{}", pct_encode(path))
    } else {
        format!("media://localhost/{}", pct_encode(path))
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

// LRCLIB auto-fetch: search for a track, download its synced .lrc if present,
// and save it next to the FLAC so the normal read_lyrics path picks it up.
/// Legacy button path: force a fresh LRCLIB lookup, and (as before) also
/// save a sidecar `.lrc` next to the track when synced lyrics were found.
#[tauri::command]
async fn lrc_fetch(app: tauri::AppHandle, path: String, artist: String, title: String) -> Result<String, String> {
    let dur = {
        let p = PathBuf::from(&path);
        tauri::async_runtime::spawn_blocking(move || probe::read_meta(&p, false).map(|(m, _, _)| m.duration).unwrap_or(0.0))
            .await
            .unwrap_or(0.0)
    };
    let r = lyrics::resolve(&app, &path, &artist, &title, "", dur, true, true).await?;
    if !r.synced {
        return Err(if r.plain.is_some() { "only unsynced lyrics on lrclib".into() } else { "no lyrics found on lrclib".into() });
    }
    let lrc: String = r.lines.iter().map(|l| {
        let m = (l.t / 60.0).floor();
        format!("[{:02}:{:05.2}]{}\n", m as u64, l.t - m * 60.0, l.text)
    }).collect();
    let lrc_path = Path::new(&path).with_extension("lrc");
    fs::write(&lrc_path, lrc).map_err(|e| format!("cannot write {}: {e}", lrc_path.display()))?;
    Ok(lrc_path.to_string_lossy().to_string())
}

/// Automatic lyrics for the playing track (see lyrics.rs for the order).
#[tauri::command]
async fn lyrics_get(
    app: tauri::AppHandle,
    path: String,
    artist: String,
    title: String,
    album: String,
    duration: f64,
    allow_net: bool,
    force: bool,
) -> Result<lyrics::LyricsResult, String> {
    lyrics::resolve(&app, &path, &artist, &title, &album, duration, allow_net, force).await
}

/// URL of the track's embedded art via the media protocol (404 if none).
#[tauri::command]
fn cover_url(path: &str) -> String {
    if cfg!(windows) {
        format!("http://media.localhost/cover/{}", pct_encode(path))
    } else {
        format!("media://localhost/cover/{}", pct_encode(path))
    }
}

/// One settings file shared by both windows (localStorage is per-webview
/// on some platforms and gets wiped with WebView2 caches).
#[tauri::command]
fn settings_load(app: tauri::AppHandle) -> serde_json::Value {
    app.path()
        .app_config_dir()
        .ok()
        .and_then(|d| fs::read(d.join("settings.json")).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::Value::Null)
}

#[tauri::command]
fn settings_save(app: tauri::AppHandle, v: serde_json::Value) -> Result<(), String> {
    let d = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(&v).map_err(|e| e.to_string())?;
    write_atomic(&d.join("settings.json"), &bytes).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// SMTC bridge commands (owner/UI only)
// ---------------------------------------------------------------------------

/// Longest body returned for an open-ended range (`bytes=N-`). The media
/// element asks for the rest; nothing ever loads a whole file per request.
const RANGE_CAP: usize = 2 * 1024 * 1024;

/// One decoded ALAC file (path, mtime) -> WAV bytes. Decoding is the
/// expensive part; ranges are then sliced out of memory.
static ALAC_CACHE: std::sync::Mutex<Option<(PathBuf, std::time::SystemTime, std::sync::Arc<Vec<u8>>)>> =
    std::sync::Mutex::new(None);

/// Small per-file facts (format, ALAC?) so every range request doesn't
/// re-probe. Keyed by path + mtime.
static MEDIA_KIND: std::sync::Mutex<Option<HashMap<PathBuf, (std::time::SystemTime, probe::Kind, bool)>>> =
    std::sync::Mutex::new(None);

/// Embedded-art LRU for the `/cover/` route.
static COVER_CACHE: std::sync::Mutex<Vec<(PathBuf, std::time::SystemTime, std::sync::Arc<(String, Vec<u8>)>)>> =
    std::sync::Mutex::new(Vec::new());
const COVER_CACHE_MAX: usize = 256;

fn http(status: u16, body: Vec<u8>) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(status)
        .header("Access-Control-Allow-Origin", "*")
        .body(body)
        .unwrap_or_default()
}

fn media_kind(path: &Path, mtime: std::time::SystemTime) -> Result<(probe::Kind, bool), String> {
    if let Ok(mut g) = MEDIA_KIND.lock() {
        let map = g.get_or_insert_with(HashMap::new);
        if let Some((t, k, a)) = map.get(path) {
            if *t == mtime {
                return Ok((*k, *a));
            }
        }
        let mut f = fs::File::open(path).map_err(|e| e.to_string())?;
        let head = probe::sniff_file(&mut f)?;
        let kind = probe::kind_of(&head).ok_or("unrecognized audio format")?;
        let alac = kind == probe::Kind::M4a
            && probe::read_meta(path, false).map(|(_, _, a)| a).unwrap_or(false);
        if map.len() > 64 {
            map.clear();
        }
        map.insert(path.to_path_buf(), (mtime, kind, alac));
        return Ok((kind, alac));
    }
    Err("media state poisoned".into())
}

fn alac_wav(path: &Path, mtime: std::time::SystemTime) -> Result<std::sync::Arc<Vec<u8>>, String> {
    let mut g = ALAC_CACHE.lock().map_err(|_| "alac cache poisoned")?;
    if let Some((p, t, w)) = g.as_ref() {
        if p == path && *t == mtime {
            return Ok(w.clone());
        }
    }
    let wav = std::sync::Arc::new(alac::decode(fs::read(path).map_err(|e| e.to_string())?)?);
    *g = Some((path.to_path_buf(), mtime, wav.clone()));
    Ok(wav)
}

fn serve_cover(path: &Path) -> tauri::http::Response<Vec<u8>> {
    let Ok(mtime) = fs::metadata(path).and_then(|m| m.modified()) else {
        return http(404, b"no such file".to_vec());
    };
    let hit = COVER_CACHE.lock().ok().and_then(|mut c| {
        let i = c.iter().position(|(p, t, _)| p == path && *t == mtime)?;
        let e = c.remove(i);
        let v = e.2.clone();
        c.push(e); // most recently used at the end
        Some(v)
    });
    let art = match hit {
        Some(a) => a,
        None => {
            let pic = probe::read_meta(path, true).ok().and_then(|(m, _, _)| m.cover);
            let Some(pic) = pic else { return http(404, b"no embedded art".to_vec()) };
            let Some(bytes) = base64_decode(&pic.data_b64) else { return http(500, b"bad art".to_vec()) };
            let a = std::sync::Arc::new((pic.mime, bytes));
            if let Ok(mut c) = COVER_CACHE.lock() {
                if c.len() >= COVER_CACHE_MAX {
                    c.remove(0);
                }
                c.push((path.to_path_buf(), mtime, a.clone()));
            }
            a
        }
    };
    tauri::http::Response::builder()
        .status(200)
        .header("Content-Type", if art.0.is_empty() { "image/jpeg" } else { art.0.as_str() })
        .header("Cache-Control", "max-age=31536000")
        .header("Access-Control-Allow-Origin", "*")
        .body(art.1.clone())
        .unwrap_or_default()
}

fn range_response(
    total: usize,
    range: Option<&str>,
    content_type: &str,
    read: impl FnOnce(usize, usize) -> Result<Vec<u8>, String>,
) -> tauri::http::Response<Vec<u8>> {
    let r = match media_source::select_range(range, total) {
        Ok(r) => r,
        Err(()) => {
            return tauri::http::Response::builder()
                .status(416)
                .header("Content-Range", format!("bytes */{}", total))
                .body(Vec::new())
                .unwrap_or_default()
        }
    };
    let r = media_source::cap_range(r, RANGE_CAP);
    let body = match read(r.start, r.end) {
        Ok(b) => b,
        Err(e) => return http(500, e.into_bytes()),
    };
    let mut b = tauri::http::Response::builder()
        .header("Content-Type", content_type)
        .header("Content-Length", body.len().to_string())
        .header("Accept-Ranges", "bytes")
        .header("Access-Control-Allow-Origin", "*");
    b = if r.partial {
        b.status(206).header("Content-Range", format!("bytes {}-{}/{}", r.start, r.end - 1, total))
    } else {
        b.status(200)
    };
    b.body(body).unwrap_or_default()
}

fn serve_media(request: tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    let uri = request.uri().to_string();
    let enc = uri
        .strip_prefix("media://localhost/")
        .or_else(|| uri.strip_prefix("http://media.localhost/"))
        .or_else(|| uri.strip_prefix("https://media.localhost/"))
        .or_else(|| uri.strip_prefix("flac://localhost/"))          // legacy fallback
        .or_else(|| uri.strip_prefix("http://flac.localhost/"))     // legacy fallback
        .unwrap_or("");
    if let Some(c) = enc.strip_prefix("cover/") {
        return serve_cover(Path::new(&pct_decode(c)));
    }
    let path = PathBuf::from(pct_decode(enc));
    let range = request.headers().get("range").and_then(|v| v.to_str().ok());

    let md = match fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => return http(404, format!("halftone: cannot read {}: {}", path.display(), e).into_bytes()),
    };
    let mtime = md.modified().unwrap_or(std::time::UNIX_EPOCH);
    let (kind, is_alac) = match media_kind(&path, mtime) {
        Ok(k) => k,
        Err(e) => return http(415, e.into_bytes()),
    };

    if is_alac {
        // Lossless decompression (see audio-pipeline.md), decoded ONCE.
        return match alac_wav(&path, mtime) {
            Ok(wav) => range_response(wav.len(), range, "audio/wav", |a, b| Ok(wav[a..b].to_vec())),
            Err(_) => http(500, b"alac decode failed".to_vec()),
        };
    }
    // Original bytes: seek + read only the requested window.
    range_response(md.len() as usize, range, kind.mime(), |a, b| {
        use std::io::{Seek, SeekFrom};
        let mut f = fs::File::open(&path).map_err(|e| e.to_string())?;
        f.seek(SeekFrom::Start(a as u64)).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; b - a];
        f.read_exact(&mut buf).map_err(|e| e.to_string())?;
        Ok(buf)
    })
}

// ---------------------------------------------------------------------------
// App entry
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Tray icon + native menu: show/hide widget, show/hide main,
            // transport, quit. QUIT IS THE TERMINATE PATH (the main window's
            // close only hides — it is the audio owner; killing it kills audio).
            {
                use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
                use tauri::tray::TrayIconBuilder;

                let m_show_widget = MenuItem::with_id(app, "show_widget", "Show Widget", true, None::<&str>)?;
                let m_hide_widget = MenuItem::with_id(app, "hide_widget", "Hide Widget", true, None::<&str>)?;
                let m_show_main = MenuItem::with_id(app, "show_main", "Show Main Window", true, None::<&str>)?;
                let m_sep1 = PredefinedMenuItem::separator(app)?;
                let m_play = MenuItem::with_id(app, "play", "Play / Pause", true, None::<&str>)?;
                let m_next = MenuItem::with_id(app, "next", "Next", true, None::<&str>)?;
                let m_prev = MenuItem::with_id(app, "prev", "Previous", true, None::<&str>)?;
                let m_sep2 = PredefinedMenuItem::separator(app)?;
                let m_quit = MenuItem::with_id(app, "quit", "Quit Halftone", true, None::<&str>)?;

                let menu = Menu::with_items(
                    app,
                    &[&m_show_widget, &m_hide_widget, &m_show_main, &m_sep1, &m_play, &m_next, &m_prev, &m_sep2, &m_quit],
                )?;

                let _tray = TrayIconBuilder::with_id("halftone-tray")
                    .icon(app.default_window_icon().unwrap().clone())
                    .tooltip("Halftone")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| {
                        use tauri::Emitter;
                        match event.id().as_ref() {
                            "quit" => {
                                std::process::exit(0);
                            }
                            "show_widget" => {
                                if let Some(w) = app.get_webview_window("widget") {
                                    let _ = w.show();
                                    let _ = w.set_focus();
                                }
                            }
                            "hide_widget" => {
                                if let Some(w) = app.get_webview_window("widget") {
                                    let _ = w.hide();
                                }
                            }
                            "show_main" => {
                                if let Some(w) = app.get_webview_window("main") {
                                    let _ = w.show();
                                    let _ = w.set_focus();
                                }
                            }
                            "play" => {
                                let _ = app.emit("smtc-button", "toggle");
                            }
                            "next" => {
                                let _ = app.emit("smtc-button", "next");
                            }
                            "prev" => {
                                let _ = app.emit("smtc-button", "prev");
                            }
                            _ => {}
                        }
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let tauri::tray::TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        } = event
                        {
                            // left-click toggles widget visibility
                            if let Some(w) = tray.app_handle().get_webview_window("widget") {
                                if w.is_visible().unwrap_or(false) {
                                    let _ = w.hide();
                                } else {
                                    let _ = w.show();
                                    let _ = w.set_focus();
                                }
                            }
                        }
                    })
                    .build(app)?;
            }

            // Widget size constraints: enforced at the window level. (JS setMinSize
            // proved unreliable on this Tauri/Windows combo — the OS would accept
            // 200x120 despite min being set.) Logical pixels: 380x260 .. 760x560.
            // Deferred: webview windows from config register slightly after setup starts.
            let w_app = app.handle().clone();
            std::thread::spawn(move || {
                use tauri::Manager;
                for _ in 0..50 {
                    if let Some(w) = w_app.get_webview_window("widget") {
                        let r1 = w.set_min_size(Some(tauri::LogicalSize::new(380.0, 260.0)));
                        let r2 = w.set_max_size(Some(tauri::LogicalSize::new(760.0, 560.0)));
                        eprintln!("halftone bounds: min={:?} max={:?}", r1.is_ok(), r2.is_ok());
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            });
            let _ = APP.set(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // LOAD-BEARING: the main window is the audio OWNER. Its close
            // button only HIDES — destroying it would kill app-wide audio.
            // Only the widget's close / tray quit terminates the process.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .register_uri_scheme_protocol("media", |_ctx, request| serve_media(request))
        .register_uri_scheme_protocol("flac", |_ctx, request| serve_media(request)) // legacy
        .invoke_handler(tauri::generate_handler![
            scan_library,
            scan_cancel,
            open_track,
            read_lyrics,
            media_url,
            pick_folder,
            lrc_fetch,
            lyrics_get,
            cover_url,
            library_snapshot,
            settings_load,
            settings_save,
            ota_check,
            ota_download,
            ota_apply
        ])
        .run(tauri::generate_context!())
        .expect("halftone widget failed to start");
}
