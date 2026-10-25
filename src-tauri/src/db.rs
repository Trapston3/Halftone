//! db.rs — Persistent Library Database, Stable Track IDs, Non-Destructive
//! Metadata Overlays, Playlists + M3U8 Import/Export, Smart Playlists Rule
//! Engine, Play History & Listening Statistics, Library Health Diagnostics,
//! Portable Mode, and Single-File DB Backup/Restore.
//!
//! Per Sections 7.2, 8.2, 9.2, 13.7, and 13.8 of the Halftone v1.1 Build Specification.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::{TrackMeta, write_atomic};

const HEAD_TAIL_WINDOW: u64 = 16 * 1024;

/// Compute a fast, deterministic Stable Track ID (`<size_hex>-<fnv64_head_tail_hex>`)
/// from the file size plus the first 16 KiB and last 16 KiB of the file.
/// Moving or renaming a file preserves its exact `stable_id`.
pub fn compute_stable_id(path: &Path) -> Option<String> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut h: u64 = 0xcbf29ce484222325 ^ len;
    let head_len = len.min(HEAD_TAIL_WINDOW) as usize;
    let mut buf = vec![0u8; head_len];
    if f.read_exact(&mut buf).is_ok() {
        for &b in &buf {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    if len > HEAD_TAIL_WINDOW {
        let tail_start = len.saturating_sub(HEAD_TAIL_WINDOW);
        let tail_len = (len - tail_start) as usize;
        let mut tbuf = vec![0u8; tail_len];
        if f.seek(SeekFrom::Start(tail_start)).is_ok() && f.read_exact(&mut tbuf).is_ok() {
            for &b in &tbuf {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
    }
    Some(format!("{len:x}-{h:016x}"))
}

/// Portable mode check: if `halftone.portable` exists next to the executable,
/// store all database/config state in `<exe_dir>/portable_data/`.
pub fn is_portable_mode() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    if dir.join("halftone.portable").exists() {
        Some(dir.join("portable_data"))
    } else {
        None
    }
}

pub fn data_root_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    if let Some(p) = is_portable_mode() {
        let _ = fs::create_dir_all(&p);
        return Some(p);
    }
    app.path().app_data_dir().ok()
}

fn db_file_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    data_root_dir(app).map(|d| d.join("halftone_db.json"))
}

fn now_sec() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Data Models
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetadataOverlay {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disc_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lyrics_offset_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrackStats {
    pub stable_id: String,
    pub last_known_path: String,
    pub play_count: u32,
    pub skip_count: u32,
    pub total_listen_sec: f64,
    pub last_played_ts: u64,
    pub date_added_ts: u64,
    pub liked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayEvent {
    pub stable_id: String,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub listen_sec: f64,
    pub timestamp_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPlaylist {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tracks: Vec<String>,
    pub created_ts: u64,
    pub updated_ts: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartRule {
    /// "artist" | "album" | "title" | "genre" | "year" | "format" | "lossless"
    /// | "sample_rate" | "bit_depth" | "bitrate" | "bpm" | "play_count"
    /// | "last_played_days" | "date_added_days" | "liked" | "duration"
    pub field: String,
    /// "contains" | "equals" | "starts_with" | "gt" | "gte" | "lt" | "lte" | "is_true" | "is_false"
    pub op: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartPlaylistDef {
    pub id: String,
    pub name: String,
    pub match_all: bool,
    pub rules: Vec<SmartRule>,
    pub sort_by: String,
    pub sort_desc: bool,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HalftoneDatabase {
    /// Keyed by `stable_id` (or normalized path fallback)
    pub overlays: HashMap<String, MetadataOverlay>,
    /// Keyed by `stable_id`
    pub stats: HashMap<String, TrackStats>,
    /// Path -> `stable_id` lookup index
    pub path_to_id: HashMap<String, String>,
    /// Chronological play history (capped at 10,000 events)
    pub play_events: Vec<PlayEvent>,
    /// User manual playlists
    pub playlists: Vec<UserPlaylist>,
    /// Smart playlists
    pub smart_playlists: Vec<SmartPlaylistDef>,
}

static DB_STATE: Mutex<Option<HalftoneDatabase>> = Mutex::new(None);

fn default_smart_playlists() -> Vec<SmartPlaylistDef> {
    vec![
        SmartPlaylistDef {
            id: "smart-hi-res".into(),
            name: "Hi-Res Lossless (24-bit+)".into(),
            match_all: true,
            rules: vec![
                SmartRule { field: "lossless".into(), op: "is_true".into(), value: "true".into() },
                SmartRule { field: "bit_depth".into(), op: "gte".into(), value: "24".into() },
            ],
            sort_by: "album".into(),
            sort_desc: false,
            limit: Some(500),
        },
        SmartPlaylistDef {
            id: "smart-heavy-rotation".into(),
            name: "Heavy Rotation".into(),
            match_all: true,
            rules: vec![
                SmartRule { field: "play_count".into(), op: "gte".into(), value: "2".into() },
            ],
            sort_by: "play_count".into(),
            sort_desc: true,
            limit: Some(100),
        },
        SmartPlaylistDef {
            id: "smart-loved".into(),
            name: "Loved & Lossless".into(),
            match_all: true,
            rules: vec![
                SmartRule { field: "liked".into(), op: "is_true".into(), value: "true".into() },
                SmartRule { field: "lossless".into(), op: "is_true".into(), value: "true".into() },
            ],
            sort_by: "title".into(),
            sort_desc: false,
            limit: None,
        },
    ]
}

pub fn with_db<F, R>(app: &tauri::AppHandle, mutate: bool, f: F) -> R
where
    F: FnOnce(&mut HalftoneDatabase) -> R,
{
    let mut guard = DB_STATE.lock().expect("db mutex");
    let db = guard.get_or_insert_with(|| {
        let mut loaded: HalftoneDatabase = db_file_path(app)
            .and_then(|p| fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if loaded.smart_playlists.is_empty() {
            loaded.smart_playlists = default_smart_playlists();
        }
        loaded
    });
    let out = f(db);
    if mutate {
        if let Some(p) = db_file_path(app) {
            if let Ok(bytes) = serde_json::to_vec_pretty(db) {
                let _ = write_atomic(&p, &bytes);
            }
        }
    }
    out
}

/// Apply any non-destructive `MetadataOverlay` and `TrackStats` onto a `TrackMeta`
pub fn enrich_track_meta(db: &mut HalftoneDatabase, meta: &mut TrackMeta) {
    let sid = db
        .path_to_id
        .get(&meta.path)
        .cloned()
        .or_else(|| compute_stable_id(Path::new(&meta.path)))
        .unwrap_or_else(|| format!("path-{:016x}", crate::lyrics::fnv(&meta.path)));

    db.path_to_id.insert(meta.path.clone(), sid.clone());
    let now = now_sec();
    let entry = db.stats.entry(sid.clone()).or_insert_with(|| TrackStats {
        stable_id: sid.clone(),
        last_known_path: meta.path.clone(),
        play_count: 0,
        skip_count: 0,
        total_listen_sec: 0.0,
        last_played_ts: 0,
        date_added_ts: now,
        liked: false,
    });
    entry.last_known_path = meta.path.clone();

    if let Some(ov) = db.overlays.get(&sid).or_else(|| db.overlays.get(&meta.path)) {
        if let Some(t) = &ov.title {
            if !t.trim().is_empty() {
                meta.title = t.clone();
            }
        }
        if let Some(a) = &ov.artist {
            if !a.trim().is_empty() {
                meta.artist = a.clone();
            }
        }
        if let Some(al) = &ov.album {
            if !al.trim().is_empty() {
                meta.album = al.clone();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Smart Playlist Rule Evaluator
// ---------------------------------------------------------------------------

pub fn eval_rule(rule: &SmartRule, track: &TrackMeta, stats: Option<&TrackStats>, ov: Option<&MetadataOverlay>) -> bool {
    let field = rule.field.as_str();
    let op = rule.op.as_str();
    let val_str = rule.value.trim().to_lowercase();

    let match_text = |actual: &str| -> bool {
        let a = actual.to_lowercase();
        match op {
            "equals" => a == val_str,
            "starts_with" => a.starts_with(&val_str),
            "not_contains" => !a.contains(&val_str),
            _ => a.contains(&val_str),
        }
    };

    let match_num = |actual: f64| -> bool {
        let target = val_str.parse::<f64>().unwrap_or(0.0);
        match op {
            "equals" | "eq" => (actual - target).abs() < 1e-3,
            "gt" => actual > target,
            "gte" => actual >= target,
            "lt" => actual < target,
            "lte" => actual <= target,
            _ => actual >= target,
        }
    };

    let match_bool = |actual: bool| -> bool {
        match op {
            "is_false" => !actual,
            "is_true" => actual,
            _ => actual == (val_str == "true" || val_str == "1" || val_str == "yes"),
        }
    };

    match field {
        "title" => match_text(&track.title),
        "artist" => match_text(&track.artist),
        "album" => match_text(&track.album),
        "genre" => match_text(ov.and_then(|o| o.genre.as_deref()).unwrap_or("")),
        "format" => match_text(&track.format_tag),
        "lossless" => match_bool(track.lossless),
        "liked" => match_bool(stats.map(|s| s.liked).unwrap_or(false)),
        "sample_rate" => match_num(track.streaminfo.sample_rate as f64),
        "bit_depth" => match_num(track.streaminfo.bits as f64),
        "bitrate" => match_num(track.bitrate_kbps as f64),
        "duration" => match_num(track.duration),
        "year" => match_num(ov.and_then(|o| o.year).unwrap_or(0) as f64),
        "bpm" => match_num(ov.and_then(|o| o.bpm).unwrap_or(0.0) as f64),
        "play_count" => match_num(stats.map(|s| s.play_count).unwrap_or(0) as f64),
        "last_played_days" => {
            let last = stats.map(|s| s.last_played_ts).unwrap_or(0);
            if last == 0 {
                return false;
            }
            let days = (now_sec().saturating_sub(last) as f64) / 86400.0;
            match_num(days)
        }
        "date_added_days" => {
            let added = stats.map(|s| s.date_added_ts).unwrap_or(0);
            let days = (now_sec().saturating_sub(added) as f64) / 86400.0;
            match_num(days)
        }
        _ => true,
    }
}

pub fn evaluate_smart_playlist(
    def: &SmartPlaylistDef,
    tracks: &[TrackMeta],
    db: &HalftoneDatabase,
) -> Vec<String> {
    let mut matched: Vec<&TrackMeta> = tracks
        .iter()
        .filter(|t| {
            let sid = db.path_to_id.get(&t.path);
            let st = sid.and_then(|id| db.stats.get(id));
            let ov = sid.and_then(|id| db.overlays.get(id)).or_else(|| db.overlays.get(&t.path));
            if def.rules.is_empty() {
                return true;
            }
            if def.match_all {
                def.rules.iter().all(|r| eval_rule(r, t, st, ov))
            } else {
                def.rules.iter().any(|r| eval_rule(r, t, st, ov))
            }
        })
        .collect();

    matched.sort_by(|a, b| {
        let sa = db.path_to_id.get(&a.path).and_then(|id| db.stats.get(id));
        let sb = db.path_to_id.get(&b.path).and_then(|id| db.stats.get(id));
        let cmp = match def.sort_by.as_str() {
            "play_count" => sa.map(|s| s.play_count).unwrap_or(0).cmp(&sb.map(|s| s.play_count).unwrap_or(0)),
            "last_played" => sa.map(|s| s.last_played_ts).unwrap_or(0).cmp(&sb.map(|s| s.last_played_ts).unwrap_or(0)),
            "date_added" => sa.map(|s| s.date_added_ts).unwrap_or(0).cmp(&sb.map(|s| s.date_added_ts).unwrap_or(0)),
            "duration" => a.duration.partial_cmp(&b.duration).unwrap_or(std::cmp::Ordering::Equal),
            "artist" => a.artist.to_lowercase().cmp(&b.artist.to_lowercase()),
            "album" => a.album.to_lowercase().cmp(&b.album.to_lowercase()),
            _ => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
        };
        if def.sort_desc { cmp.reverse() } else { cmp }
    });

    if let Some(lim) = def.limit {
        matched.truncate(lim);
    }
    matched.into_iter().map(|t| t.path.clone()).collect()
}

// ---------------------------------------------------------------------------
// M3U / M3U8 Import & Export
// ---------------------------------------------------------------------------

pub fn parse_m3u8(content: &str, base_dir: Option<&Path>) -> Vec<String> {
    let mut out = Vec::new();
    for line in content.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let p = Path::new(l);
        if p.is_absolute() {
            out.push(l.to_string());
        } else if let Some(base) = base_dir {
            out.push(base.join(p).to_string_lossy().to_string());
        } else {
            out.push(l.to_string());
        }
    }
    out
}

pub fn format_m3u8(name: &str, tracks: &[TrackMeta]) -> String {
    let mut s = format!("#EXTM3U\n#PLAYLIST:{name}\n");
    for t in tracks {
        let dur = t.duration.round() as i64;
        s.push_str(&format!("#EXTINF:{dur},{} - {}\n{}\n", t.artist, t.title, t.path));
    }
    s
}

// ---------------------------------------------------------------------------
// Library Health Diagnostics & Listening Statistics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct LibraryHealthReport {
    pub total_tracks: usize,
    pub missing_art: Vec<String>,
    pub missing_tags: Vec<String>,
    pub duplicate_groups: Vec<Vec<String>>,
    pub missing_files: Vec<String>,
    pub suspect_transcodes: Vec<String>,
    pub lossless_count: usize,
    pub lossy_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListeningStatsReport {
    pub total_plays: u64,
    pub total_listen_sec: f64,
    pub unique_tracks_played: usize,
    pub liked_count: usize,
    pub top_tracks: Vec< serde_json::Value >,
    pub top_artists: Vec< serde_json::Value >,
    pub top_albums: Vec< serde_json::Value >,
    /// 24 hourly buckets (0..23)
    pub hourly_heatmap: [u32; 24],
    /// 7 weekday buckets (0=Sun..6=Sat)
    pub weekday_heatmap: [u32; 7],
    pub current_streak_days: u32,
}

// ---------------------------------------------------------------------------
// Tauri Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn db_get_state(app: tauri::AppHandle) -> serde_json::Value {
    with_db(&app, false, |db| {
        serde_json::json!({
            "portable": is_portable_mode().is_some(),
            "overlays": db.overlays,
            "stats": db.stats,
            "path_to_id": db.path_to_id,
            "playlists": db.playlists,
            "smart_playlists": db.smart_playlists,
        })
    })
}

#[tauri::command]
pub fn db_set_overlay(
    app: tauri::AppHandle,
    path: String,
    overlay: MetadataOverlay,
) -> Result<MetadataOverlay, String> {
    Ok(with_db(&app, true, |db| {
        let key = db
            .path_to_id
            .get(&path)
            .cloned()
            .or_else(|| compute_stable_id(Path::new(&path)))
            .unwrap_or_else(|| path.clone());
        db.path_to_id.insert(path.clone(), key.clone());
        db.overlays.insert(key, overlay.clone());
        overlay
    }))
}

#[tauri::command]
pub fn db_toggle_like(app: tauri::AppHandle, path: String, liked: Option<bool>) -> bool {
    with_db(&app, true, |db| {
        let sid = db
            .path_to_id
            .get(&path)
            .cloned()
            .or_else(|| compute_stable_id(Path::new(&path)))
            .unwrap_or_else(|| path.clone());
        db.path_to_id.insert(path.clone(), sid.clone());
        let entry = db.stats.entry(sid.clone()).or_insert_with(|| TrackStats {
            stable_id: sid,
            last_known_path: path,
            date_added_ts: now_sec(),
            ..Default::default()
        });
        entry.liked = liked.unwrap_or(!entry.liked);
        entry.liked
    })
}

#[tauri::command]
pub fn db_record_play(
    app: tauri::AppHandle,
    path: String,
    title: String,
    artist: String,
    album: String,
    listen_sec: f64,
    skipped: bool,
) -> TrackStats {
    with_db(&app, true, |db| {
        let sid = db
            .path_to_id
            .get(&path)
            .cloned()
            .or_else(|| compute_stable_id(Path::new(&path)))
            .unwrap_or_else(|| path.clone());
        db.path_to_id.insert(path.clone(), sid.clone());
        let ts = now_sec();
        let entry = db.stats.entry(sid.clone()).or_insert_with(|| TrackStats {
            stable_id: sid.clone(),
            last_known_path: path.clone(),
            date_added_ts: ts,
            ..Default::default()
        });
        entry.last_known_path = path.clone();
        if skipped {
            entry.skip_count += 1;
        } else {
            entry.play_count += 1;
            entry.last_played_ts = ts;
        }
        entry.total_listen_sec += listen_sec.max(0.0);
        let out = entry.clone();

        if !skipped {
            db.play_events.push(PlayEvent {
                stable_id: sid,
                path,
                title,
                artist,
                album,
                listen_sec: listen_sec.max(1.0),
                timestamp_sec: ts,
            });
            if db.play_events.len() > 10_000 {
                let drain = db.play_events.len() - 10_000;
                db.play_events.drain(0..drain);
            }
        }
        out
    })
}

#[tauri::command]
pub fn db_save_playlist(
    app: tauri::AppHandle,
    id: Option<String>,
    name: String,
    description: Option<String>,
    tracks: Vec<String>,
) -> UserPlaylist {
    with_db(&app, true, |db| {
        let ts = now_sec();
        let pid = id.unwrap_or_else(|| format!("pl-{ts}-{}", db.playlists.len() + 1));
        if let Some(existing) = db.playlists.iter_mut().find(|p| p.id == pid) {
            existing.name = name;
            if let Some(d) = description {
                existing.description = d;
            }
            existing.tracks = tracks;
            existing.updated_ts = ts;
            return existing.clone();
        }
        let pl = UserPlaylist {
            id: pid,
            name,
            description: description.unwrap_or_default(),
            tracks,
            created_ts: ts,
            updated_ts: ts,
        };
        db.playlists.push(pl.clone());
        pl
    })
}

#[tauri::command]
pub fn db_delete_playlist(app: tauri::AppHandle, id: String) -> bool {
    with_db(&app, true, |db| {
        let before = db.playlists.len();
        db.playlists.retain(|p| p.id != id);
        db.smart_playlists.retain(|p| p.id != id);
        db.playlists.len() != before
    })
}

#[tauri::command]
pub fn db_save_smart_playlist(
    app: tauri::AppHandle,
    def: SmartPlaylistDef,
) -> SmartPlaylistDef {
    with_db(&app, true, |db| {
        if let Some(existing) = db.smart_playlists.iter_mut().find(|p| p.id == def.id) {
            *existing = def.clone();
        } else {
            db.smart_playlists.push(def.clone());
        }
        def
    })
}

#[tauri::command]
pub fn db_eval_smart_playlist(
    app: tauri::AppHandle,
    def: SmartPlaylistDef,
) -> Vec<String> {
    let tracks = crate::library_snapshot_internal();
    with_db(&app, false, |db| evaluate_smart_playlist(&def, &tracks, db))
}

#[tauri::command]
pub fn db_import_m3u(app: tauri::AppHandle, path: String) -> Result<UserPlaylist, String> {
    let p = Path::new(&path);
    let content = fs::read_to_string(p).map_err(|e| e.to_string())?;
    let name = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Imported Playlist")
        .to_string();
    let tracks = parse_m3u8(&content, p.parent());
    Ok(db_save_playlist(app, None, name, Some("Imported from M3U8".into()), tracks))
}

#[tauri::command]
pub fn db_export_m3u(path: String, name: String, track_paths: Vec<String>) -> Result<String, String> {
    let lib = crate::library_snapshot_internal();
    let by_path: HashMap<&str, &TrackMeta> = lib.iter().map(|t| (t.path.as_str(), t)).collect();
    let mut resolved = Vec::new();
    for tp in &track_paths {
        if let Some(&m) = by_path.get(tp.as_str()) {
            resolved.push(m.clone());
        }
    }
    let body = format_m3u8(&name, &resolved);
    fs::write(&path, body).map_err(|e| e.to_string())?;
    Ok(path)
}

#[tauri::command]
pub fn db_get_library_health(app: tauri::AppHandle) -> LibraryHealthReport {
    let tracks = crate::library_snapshot_internal();
    with_db(&app, false, |db| {
        let mut missing_art = Vec::new();
        let mut missing_tags = Vec::new();
        let mut lossless_count = 0usize;
        let mut lossy_count = 0usize;
        let mut by_sig: HashMap<String, Vec<String>> = HashMap::new();

        for t in &tracks {
            if t.lossless {
                lossless_count += 1;
            } else {
                lossy_count += 1;
            }
            if !t.has_cover {
                missing_art.push(t.path.clone());
            }
            if t.artist == "Unknown artist" || t.album == "Unknown album" || t.title.trim().is_empty() {
                missing_tags.push(t.path.clone());
            }
            let sig = format!(
                "{}|{}|{}",
                t.artist.trim().to_lowercase(),
                t.title.trim().to_lowercase(),
                t.duration.round() as i64
            );
            by_sig.entry(sig).or_default().push(t.path.clone());
        }

        let duplicate_groups: Vec<Vec<String>> = by_sig
            .into_values()
            .filter(|v| v.len() > 1)
            .collect();

        let active_paths: HashSet<&str> = tracks.iter().map(|t| t.path.as_str()).collect();
        let missing_files: Vec<String> = db
            .stats
            .values()
            .filter(|s| (s.play_count > 0 || s.liked) && !active_paths.contains(s.last_known_path.as_str()) && !Path::new(&s.last_known_path).exists())
            .map(|s| s.last_known_path.clone())
            .collect();

        LibraryHealthReport {
            total_tracks: tracks.len(),
            missing_art,
            missing_tags,
            duplicate_groups,
            missing_files,
            suspect_transcodes: Vec::new(),
            lossless_count,
            lossy_count,
        }
    })
}

/// Automatically relink moved/renamed files by matching their `stable_id` against
/// the current scanned library.
#[tauri::command]
pub fn db_relink_missing(app: tauri::AppHandle) -> usize {
    let tracks = crate::library_snapshot_internal();
    with_db(&app, true, |db| {
        let mut id_to_new_path: HashMap<String, String> = HashMap::new();
        for t in &tracks {
            if let Some(sid) = compute_stable_id(Path::new(&t.path)) {
                id_to_new_path.insert(sid.clone(), t.path.clone());
                db.path_to_id.insert(t.path.clone(), sid);
            }
        }
        let mut relinked = 0usize;
        for (sid, st) in db.stats.iter_mut() {
            if let Some(new_p) = id_to_new_path.get(sid) {
                if &st.last_known_path != new_p {
                    let old_p = st.last_known_path.clone();
                    st.last_known_path = new_p.clone();
                    for pl in db.playlists.iter_mut() {
                        for item in pl.tracks.iter_mut() {
                            if item == &old_p {
                                *item = new_p.clone();
                            }
                        }
                    }
                    relinked += 1;
                }
            }
        }
        relinked
    })
}

#[tauri::command]
pub fn db_get_listening_stats(app: tauri::AppHandle) -> ListeningStatsReport {
    let tracks = crate::library_snapshot_internal();
    let by_path: HashMap<&str, &TrackMeta> = tracks.iter().map(|t| (t.path.as_str(), t)).collect();

    with_db(&app, false, |db| {
        let mut total_plays = 0u64;
        let mut total_listen_sec = 0.0f64;
        let mut unique_tracks_played = 0usize;
        let mut liked_count = 0usize;

        let mut track_scores: Vec<(u32, f64, String, String, String, String)> = Vec::new();
        let mut artist_counts: HashMap<String, (u32, f64)> = HashMap::new();
        let mut album_counts: HashMap<String, (u32, f64, String)> = HashMap::new();

        for st in db.stats.values() {
            if st.liked {
                liked_count += 1;
            }
            if st.play_count > 0 {
                unique_tracks_played += 1;
                total_plays += st.play_count as u64;
                total_listen_sec += st.total_listen_sec;

                let (title, artist, album) = if let Some(m) = by_path.get(st.last_known_path.as_str()) {
                    (m.title.clone(), m.artist.clone(), m.album.clone())
                } else {
                    (
                        Path::new(&st.last_known_path)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("Unknown")
                            .to_string(),
                        "Unknown artist".into(),
                        "Unknown album".into(),
                    )
                };
                track_scores.push((
                    st.play_count,
                    st.total_listen_sec,
                    st.last_known_path.clone(),
                    title,
                    artist.clone(),
                    album.clone(),
                ));
                let ae = artist_counts.entry(artist.clone()).or_insert((0, 0.0));
                ae.0 += st.play_count;
                ae.1 += st.total_listen_sec;

                let ale = album_counts.entry(album).or_insert((0, 0.0, artist));
                ale.0 += st.play_count;
                ale.1 += st.total_listen_sec;
            }
        }

        track_scores.sort_by(|a, b| b.0.cmp(&a.0));
        let top_tracks: Vec<serde_json::Value> = track_scores
            .into_iter()
            .take(15)
            .map(|(plays, sec, path, title, artist, album)| {
                serde_json::json!({
                    "plays": plays,
                    "listen_sec": sec,
                    "path": path,
                    "title": title,
                    "artist": artist,
                    "album": album,
                })
            })
            .collect();

        let mut artists_vec: Vec<(String, u32, f64)> = artist_counts
            .into_iter()
            .map(|(k, (c, s))| (k, c, s))
            .collect();
        artists_vec.sort_by(|a, b| b.1.cmp(&a.1));
        let top_artists: Vec<serde_json::Value> = artists_vec
            .into_iter()
            .take(10)
            .map(|(artist, plays, sec)| serde_json::json!({ "artist": artist, "plays": plays, "listen_sec": sec }))
            .collect();

        let mut albums_vec: Vec<(String, u32, f64, String)> = album_counts
            .into_iter()
            .map(|(k, (c, s, ar))| (k, c, s, ar))
            .collect();
        albums_vec.sort_by(|a, b| b.1.cmp(&a.1));
        let top_albums: Vec<serde_json::Value> = albums_vec
            .into_iter()
            .take(10)
            .map(|(album, plays, sec, artist)| serde_json::json!({ "album": album, "artist": artist, "plays": plays, "listen_sec": sec }))
            .collect();

        let mut hourly_heatmap = [0u32; 24];
        let mut weekday_heatmap = [0u32; 7];
        let mut days_set: HashSet<u64> = HashSet::new();

        for ev in &db.play_events {
            let hour = ((ev.timestamp_sec / 3600) % 24) as usize;
            let day_idx = (ev.timestamp_sec / 86400) as u64;
            let weekday = ((day_idx + 4) % 7) as usize;
            hourly_heatmap[hour] += 1;
            weekday_heatmap[weekday] += 1;
            days_set.insert(day_idx);
        }

        let mut streak = 0u32;
        if !days_set.is_empty() {
            let mut cur_day = now_sec() / 86400;
            if !days_set.contains(&cur_day) && days_set.contains(&cur_day.saturating_sub(1)) {
                cur_day = cur_day.saturating_sub(1);
            }
            while days_set.contains(&cur_day) {
                streak += 1;
                if cur_day == 0 {
                    break;
                }
                cur_day -= 1;
            }
        }

        ListeningStatsReport {
            total_plays,
            total_listen_sec,
            unique_tracks_played,
            liked_count,
            top_tracks,
            top_artists,
            top_albums,
            hourly_heatmap,
            weekday_heatmap,
            current_streak_days: streak,
        }
    })
}

#[tauri::command]
pub fn db_export_bundle(app: tauri::AppHandle, dest_path: String) -> Result<String, String> {
    let settings = crate::settings_load_internal(&app);
    let bundle = with_db(&app, false, |db| {
        serde_json::json!({
            "version": "1.1.0",
            "exported_at": now_sec(),
            "database": db,
            "settings": settings,
        })
    });
    let bytes = serde_json::to_vec_pretty(&bundle).map_err(|e| e.to_string())?;
    write_atomic(Path::new(&dest_path), &bytes).map_err(|e| e.to_string())?;
    Ok(dest_path)
}

#[tauri::command]
pub fn db_import_bundle(app: tauri::AppHandle, src_path: String) -> Result<bool, String> {
    let bytes = fs::read(&src_path).map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if let Some(db_val) = v.get("database") {
        let imported: HalftoneDatabase =
            serde_json::from_value(db_val.clone()).map_err(|e| e.to_string())?;
        with_db(&app, true, |db| {
            *db = imported;
        });
    }
    if let Some(settings_val) = v.get("settings") {
        if !settings_val.is_null() {
            let _ = crate::settings_save_internal(&app, settings_val.clone());
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn stable_id_survives_rename() {
        let p1 = std::env::temp_dir().join(format!("ht_sid_1_{}.bin", std::process::id()));
        let p2 = std::env::temp_dir().join(format!("ht_sid_2_{}.bin", std::process::id()));
        let payload = vec![42u8; 40_000];
        File::create(&p1).unwrap().write_all(&payload).unwrap();
        let id1 = compute_stable_id(&p1).unwrap();
        fs::rename(&p1, &p2).unwrap();
        let id2 = compute_stable_id(&p2).unwrap();
        assert_eq!(id1, id2, "renamed file must keep identical stable_id");
        let _ = fs::remove_file(&p2);
    }

    #[test]
    fn smart_playlist_rule_engine_filters_and_sorts() {
        let mut db = HalftoneDatabase::default();
        db.path_to_id.insert("a.flac".into(), "id-a".into());
        db.path_to_id.insert("b.mp3".into(), "id-b".into());
        db.stats.insert(
            "id-a".into(),
            TrackStats {
                stable_id: "id-a".into(),
                last_known_path: "a.flac".into(),
                play_count: 10,
                liked: true,
                ..Default::default()
            },
        );
        let t1 = TrackMeta {
            path: "a.flac".into(),
            title: "Alpha".into(),
            artist: "Boards of Canada".into(),
            album: "Music Has the Right to Children".into(),
            streaminfo: crate::StreamInfo { sample_rate: 96000, bits: 24, channels: 2, total_samples: 960000 },
            duration: 10.0,
            cover: None,
            block_types: vec![],
            replaygain: None,
            format_tag: "FLAC".into(),
            lossless: true,
            bitrate_kbps: 2800,
            has_cover: true,
            embedded_lyrics: None,
        };
        let t2 = TrackMeta {
            path: "b.mp3".into(),
            title: "Beta".into(),
            artist: "Other".into(),
            album: "Album".into(),
            streaminfo: crate::StreamInfo { sample_rate: 44100, bits: 16, channels: 2, total_samples: 441000 },
            duration: 10.0,
            cover: None,
            block_types: vec![],
            replaygain: None,
            format_tag: "MP3".into(),
            lossless: false,
            bitrate_kbps: 320,
            has_cover: false,
            embedded_lyrics: None,
        };
        let def = SmartPlaylistDef {
            id: "test".into(),
            name: "Hi-Res".into(),
            match_all: true,
            rules: vec![
                SmartRule { field: "lossless".into(), op: "is_true".into(), value: "true".into() },
                SmartRule { field: "bit_depth".into(), op: "gte".into(), value: "24".into() },
                SmartRule { field: "liked".into(), op: "is_true".into(), value: "true".into() },
            ],
            sort_by: "play_count".into(),
            sort_desc: true,
            limit: None,
        };
        let res = evaluate_smart_playlist(&def, &[t1, t2], &db);
        assert_eq!(res, vec!["a.flac".to_string()]);
    }
}
