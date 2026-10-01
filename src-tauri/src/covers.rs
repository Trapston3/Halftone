//! covers.rs — cover art: user overrides + automatic web fetch.
//!
//! Store: app_data_dir()/covers/ — images as `<key>.<ext>`, plus
//! `index.json` = { "<key>": {source, file, url, ts} } written atomically.
//! Keys are per ALBUM (fnv of "artist|album", lowercased) so an override
//! covers every track of the album; "Unknown album" falls back to a
//! per-track-path key. Sources:
//!   "user"  — user picked/imported art (wins over everything)
//!   "web"   — auto-fetched from iTunes/MusicBrainz (used when the file has
//!             no embedded art)
//!   "none"  — user chose "no art" for this album
//!   "miss"  — auto-fetch found nothing; retried after MISS_TTL (7 days)
//! Resolution order in the /cover/ route: user > embedded > web > 404.
//! Never writes anything next to the music (OTA/compat rule).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::Manager;

use crate::{pct_encode, probe, write_atomic};

const UA: &str = "Halftone/0.2 (https://github.com/Trapston3/Halftone)";
/// Auto-fetch miss negative-cache TTL (mirrors lyrics.rs NEG_TTL).
const MISS_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
/// Hard cap for any downloaded/imported image.
const MAX_IMAGE: usize = 8 * 1024 * 1024;
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
/// MusicBrainz polite rate: at most one request per second.
const MB_SPACING: Duration = Duration::from_secs(1);

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// Album key: fnv hex of lowercase "artist|album" (FNV from lyrics.rs).
/// When the album tag is missing/"Unknown album", fall back to a per-track
/// path key so singletons still get their own art.
pub(crate) fn album_key(artist: &str, album: &str, path: &str) -> String {
    let a = album.trim();
    if a.is_empty() || a.to_lowercase() == "unknown album" {
        return format!(
            "{:016x}",
            crate::lyrics::fnv(&format!("track|{}", path.trim().to_lowercase()))
        );
    }
    format!(
        "{:016x}",
        crate::lyrics::fnv(&format!(
            "{}|{}",
            artist.trim().to_lowercase(),
            a.to_lowercase()
        ))
    )
}

// ---------------------------------------------------------------------------
// Index (covers/index.json)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct CoverEntry {
    /// "user" | "web" | "none" | "miss"
    pub source: String,
    /// Stored image file name inside the covers dir ("user"/"web" only).
    #[serde(default)]
    pub file: Option<String>,
    /// Source URL for web art (informational).
    #[serde(default)]
    pub url: Option<String>,
    /// Unix seconds when the entry was written.
    pub ts: u64,
}

impl PartialEq for CoverEntry {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source && self.file == other.file && self.url == other.url && self.ts == other.ts
    }
}

pub type CoverIndex = HashMap<String, CoverEntry>;

pub(crate) fn covers_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("covers"))
}

pub(crate) fn index_path(dir: &Path) -> PathBuf {
    dir.join("index.json")
}

pub(crate) fn load_index(dir: &Path) -> CoverIndex {
    fs::read(index_path(dir))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub(crate) fn save_index(dir: &Path, idx: &CoverIndex) {
    if let Ok(bytes) = serde_json::to_vec_pretty(idx) {
        let _ = write_atomic(&index_path(dir), &bytes);
        bump_version_at(dir);
    }
}

pub(crate) fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn entry_fresh_miss(e: &CoverEntry) -> bool {
    e.source == "miss"
        && now_ts()
            .checked_sub(e.ts)
            .map(|age| age < MISS_TTL.as_secs())
            .unwrap_or(true)
}

/// Store validated image bytes as `<key>.<ext>` and point `key` at them.
/// Removes a stale file when the extension changes. Returns the file name.
pub(crate) fn store_image(
    dir: &Path,
    idx: &mut CoverIndex,
    key: &str,
    bytes: &[u8],
    ext: &str,
) -> Result<String, String> {
    fs::create_dir_all(dir).map_err(|e| format!("cannot create covers dir: {e}"))?;
    let name = format!("{key}.{ext}");
    // best-effort cleanup of a replaced image with a different extension
    if let Some(old) = idx.get(key) {
        if let Some(f) = &old.file {
            if f != &name {
                let _ = fs::remove_file(dir.join(f));
            }
        }
    }
    if bytes.len() > MAX_IMAGE {
        return Err(format!("image exceeds {} MiB cap", MAX_IMAGE / (1024 * 1024)));
    }
    write_atomic(&dir.join(&name), bytes).map_err(|e| format!("cannot write cover: {e}"))?;
    Ok(name)
}

// ---------------------------------------------------------------------------
// Image validation (magic bytes; size capped by callers)
// ---------------------------------------------------------------------------

/// JPEG/PNG/WebP magic sniff -> canonical extension ("jpg"/"png"/"webp").
pub(crate) fn sniff_image(b: &[u8]) -> Option<&'static str> {
    if b.len() >= 3 && b[0] == 0xFF && b[1] == 0xD8 && b[2] == 0xFF {
        return Some("jpg");
    }
    if b.len() >= 8 && b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("png");
    }
    if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        return Some("webp");
    }
    None
}

// ---------------------------------------------------------------------------
// Web candidates
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct CoverCandidate {
    pub url: String,
    pub thumb: String,
    /// "itunes" | "musicbrainz"
    pub source: String,
    pub title: String,
    pub artist: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// iTunes Search API response -> candidates (artworkUrl100 upscaled to 600).
pub(crate) fn parse_itunes(v: &serde_json::Value, limit: usize) -> Vec<CoverCandidate> {
    let mut out = Vec::new();
    let Some(results) = v["results"].as_array() else {
        return out;
    };
    for r in results {
        let Some(art100) = r["artworkUrl100"].as_str() else { continue };
        let url = art100.replace("100x100bb", "600x600bb");
        out.push(CoverCandidate {
            url,
            thumb: art100.to_string(),
            source: "itunes".into(),
            title: r["collectionName"].as_str().unwrap_or_default().to_string(),
            artist: r["artistName"].as_str().unwrap_or_default().to_string(),
            width: Some(600),
            height: Some(600),
        });
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// MusicBrainz release search response -> candidates pointing at the Cover
/// Art Archive front image.
pub(crate) fn parse_mb(v: &serde_json::Value, limit: usize) -> Vec<CoverCandidate> {
    let mut out = Vec::new();
    let Some(releases) = v["releases"].as_array() else {
        return out;
    };
    for r in releases {
        let Some(mbid) = r["id"].as_str() else { continue };
        if mbid.is_empty() {
            continue;
        }
        let artist = r["artist-credit"]
            .as_array()
            .and_then(|ac| ac.first())
            .and_then(|a| a["name"].as_str())
            .unwrap_or_default()
            .to_string();
        let front = format!("https://coverartarchive.org/release/{mbid}/front-500");
        out.push(CoverCandidate {
            url: front.clone(),
            thumb: front,
            source: "musicbrainz".into(),
            title: r["title"].as_str().unwrap_or_default().to_string(),
            artist,
            width: None,
            height: None,
        });
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// Case/punctuation-insensitive normalization for ranking.
fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Exact-ish album+artist matches first, then album-only, then original order.
pub(crate) fn rank_candidates(mut cands: Vec<CoverCandidate>, artist: &str, album: &str) -> Vec<CoverCandidate> {
    let (na, nl) = (norm(artist), norm(album));
    let score = |c: &CoverCandidate| -> u8 {
        let album_hit = !nl.is_empty() && norm(&c.title) == nl;
        let artist_hit = !na.is_empty() && norm(&c.artist) == na;
        match (album_hit, artist_hit) {
            (true, true) => 0,
            (true, false) => 1,
            _ => 2,
        }
    };
    // stable sort keeps the API's own relevance order within a tier
    cands.sort_by_key(|c| score(c));
    cands
}

// ---------------------------------------------------------------------------
// Resolution order (pure, tested): user > embedded > web > none
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
#[allow(dead_code)] // exercised by unit tests; kept pure for testability
pub(crate) enum Resolved<'a> {
    Stored(&'a CoverEntry),
    Embedded,
    None,
}

#[allow(dead_code)] // exercised by unit tests; kept pure for testability
pub(crate) fn resolve<'a>(
    user: Option<&'a CoverEntry>,
    embedded: bool,
    web: Option<&'a CoverEntry>,
) -> Resolved<'a> {
    if let Some(e) = user {
        if e.source == "user" {
            return Resolved::Stored(e);
        }
    }
    if embedded {
        return Resolved::Embedded;
    }
    if let Some(e) = web {
        if e.source == "web" {
            return Resolved::Stored(e);
        }
    }
    Resolved::None
}

// ---------------------------------------------------------------------------
// App-side state: store dir, per-path key cache, MB throttle, in-flight set,
// and the index-version counter used as the cover_url cache buster.
// ---------------------------------------------------------------------------

static STORE_DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
static COVER_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static KEY_CACHE: std::sync::Mutex<Option<HashMap<PathBuf, (SystemTime, String)>>> =
    std::sync::Mutex::new(None);
static MB_LAST: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
static IN_FLIGHT: std::sync::Mutex<Option<HashSet<String>>> = std::sync::Mutex::new(None);

/// Pin the covers store dir (called from setup and from every command that
/// has an AppHandle — idempotent).
pub(crate) fn init(app: &tauri::AppHandle) -> Option<PathBuf> {
    let dir = STORE_DIR.get().cloned().flatten().or_else(|| covers_dir(app));
    if let Some(d) = &dir {
        let _ = fs::create_dir_all(d);
        let _ = STORE_DIR.set(Some(d.clone()));
        // Seed the buster from index.json's mtime so a change made while the
        // app was closed still busts webview caches after restart.
        let seed = fs::metadata(index_path(d))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        COVER_VERSION.fetch_max(seed, std::sync::atomic::Ordering::Relaxed);
    }
    dir
}

fn bump_version_at(dir: &Path) {
    let v = fs::metadata(dir.join("index.json"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or_else(now_ts);
    COVER_VERSION.fetch_max(v, std::sync::atomic::Ordering::Relaxed);
}

/// Cache-buster value: changes whenever the covers index changes.
pub(crate) fn version() -> u64 {
    COVER_VERSION.load(std::sync::atomic::Ordering::Relaxed)
}

/// Album key for a track file (tags read via probe, result cached by mtime).
pub(crate) fn key_for(path: &Path, mtime: SystemTime) -> Option<String> {
    if let Ok(mut g) = KEY_CACHE.lock() {
        let map = g.get_or_insert_with(HashMap::new);
        if let Some((t, k)) = map.get(path) {
            if *t == mtime {
                return Some(k.clone());
            }
        }
        let m = probe::read_meta(path, false).ok()?.0;
        let key = album_key(&m.artist, &m.album, &m.path);
        if map.len() > 512 {
            map.clear();
        }
        map.insert(path.to_path_buf(), (mtime, key.clone()));
        return Some(key);
    }
    None
}

/// Serve a stored image for the /cover/ route. `want` picks which source
/// tier we're on ("user" before embedded, "web" after). None = fall through.
pub(crate) fn serve_stored(path: &Path, mtime: SystemTime, want: &str) -> Option<tauri::http::Response<Vec<u8>>> {
    let dir = STORE_DIR.get().cloned().flatten()?;
    let key = key_for(path, mtime)?;
    let idx = load_index(&dir);
    let e = idx.get(&key)?;
    if e.source != want {
        return None;
    }
    let file = e.file.as_ref()?;
    let bytes = fs::read(dir.join(file)).ok()?;
    let mime = match file.rsplit('.').next() {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        _ => "image/jpeg",
    };
    tauri::http::Response::builder()
        .status(200)
        .header("Content-Type", mime)
        .header("Cache-Control", "max-age=31536000")
        .header("Access-Control-Allow-Origin", "*")
        .body(bytes)
        .ok()
}

// ---------------------------------------------------------------------------
// Network (10 s timeout, polite UA)
// ---------------------------------------------------------------------------

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(UA)
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())
}

async fn http_get_capped(client: &reqwest::Client, url: &str, cap: usize) -> Result<Vec<u8>, String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(format!("refusing non-http url: {url}"));
    }
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        if out.len() + chunk.len() > cap {
            return Err(format!("image exceeds {} MiB cap", cap / (1024 * 1024)));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

async fn itunes_search(
    client: reqwest::Client,
    artist: String,
    album: String,
    limit: usize,
) -> Option<Vec<CoverCandidate>> {
    let v: serde_json::Value = client
        .get("https://itunes.apple.com/search")
        .query(&[
            ("term", &format!("{artist} {album}")),
            ("entity", &"album".to_string()),
            ("limit", &format!("{limit}")),
        ])
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    Some(parse_itunes(&v, limit))
}

/// MusicBrainz (≤1 req/s — throttled inside a blocking thread so we never
/// stall the async runtime).
async fn mb_search(artist: String, album: String, limit: usize) -> Option<Vec<CoverCandidate>> {
    let q = format!("release:\"{album}\" AND artist:\"{artist}\"");
    let body = tauri::async_runtime::spawn_blocking(move || {
        {
            let mut g = MB_LAST.lock().ok()?;
            let wait = match *g {
                Some(t) => MB_SPACING.checked_sub(t.elapsed()).unwrap_or(Duration::ZERO),
                None => Duration::ZERO,
            };
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
            *g = Some(Instant::now());
        }
        let client = reqwest::blocking::Client::builder()
            .user_agent(UA)
            .timeout(HTTP_TIMEOUT)
            .build()
            .ok()?;
        let v: serde_json::Value = client
            .get("https://musicbrainz.org/ws/2/release/")
            .query(&[
                ("query", q.as_str()),
                ("fmt", "json"),
                ("limit", &limit.to_string()),
            ])
            .send()
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .ok()?;
        Some(v)
    })
    .await
    .ok()??;
    Some(parse_mb(&body, limit))
}

/// Search both sources concurrently (spawned onto tauri's runtime), rank,
/// return up to `limit` candidates. Per-source failures are skipped (partial
/// results beat none).
pub(crate) async fn search_all(artist: &str, album: &str, limit: usize) -> Vec<CoverCandidate> {
    let client = match http_client() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let (a, al) = (artist.to_string(), album.to_string());
    let h_it = tauri::async_runtime::spawn(itunes_search(client.clone(), a, al, 10));
    let h_mb = tauri::async_runtime::spawn(mb_search(artist.to_string(), album.to_string(), 5));
    let mut cands = h_it.await.ok().and_then(|r| r).unwrap_or_default();
    cands.extend(h_mb.await.ok().and_then(|r| r).unwrap_or_default());
    let ranked = rank_candidates(cands, artist, album);
    ranked.into_iter().take(limit).collect()
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub(crate) async fn cover_search(
    app: tauri::AppHandle,
    artist: String,
    album: String,
    limit: Option<u32>,
) -> Result<Vec<CoverCandidate>, String> {
    init(&app);
    let limit = limit.unwrap_or(12).clamp(1, 30) as usize;
    Ok(search_all(&artist, &album, limit).await)
}

#[derive(Debug, Serialize)]
pub struct CoverAutoResult {
    /// "have" | "fetched" | "none"
    pub status: String,
    pub url: Option<String>,
    pub error: Option<String>,
}

/// Download + validate + store a user override from a picked candidate URL.
/// An empty `url` records "user chose no art" (source "none").
#[tauri::command]
pub(crate) async fn cover_apply_url(
    app: tauri::AppHandle,
    path: String,
    url: String,
) -> Result<String, String> {
    let dir = init(&app).ok_or("no app data dir")?;
    let key = album_key_from_file(&path).await?;
    let mut idx = load_index(&dir);

    if url.trim().is_empty() {
        idx.insert(
            key.clone(),
            CoverEntry { source: "none".into(), file: None, url: None, ts: now_ts() },
        );
        save_index(&dir, &idx);
        return Ok(cover_url_for(&app, &path));
    }

    let client = http_client()?;
    let bytes = http_get_capped(&client, &url, MAX_IMAGE).await?;
    let ext = sniff_image(&bytes).ok_or("downloaded file is not a JPEG/PNG/WebP image")?;
    let name = store_image(&dir, &mut idx, &key, &bytes, ext)?;
    idx.insert(
        key.clone(),
        CoverEntry { source: "user".into(), file: Some(name), url: Some(url), ts: now_ts() },
    );
    save_index(&dir, &idx);
    Ok(cover_url_for(&app, &path))
}

/// Import an image from disk (native picker when `file` is omitted) as the
/// user override. Returns null when the picker was cancelled.
#[tauri::command]
pub(crate) async fn cover_import(
    app: tauri::AppHandle,
    path: String,
    file: Option<String>,
) -> Result<Option<String>, String> {
    let dir = init(&app).ok_or("no app data dir")?;
    let picked = match file {
        Some(f) => Some(f),
        None => {
            tauri::async_runtime::spawn_blocking(|| {
                rfd::FileDialog::new()
                    .set_title("Choose an image")
                    .add_filter("Images", &["jpg", "jpeg", "png", "webp"])
                    .pick_file()
                    .and_then(|p| p.to_str().map(|s| s.to_string()))
            })
            .await
            .map_err(|e| e.to_string())?
        }
    };
    let Some(src) = picked else { return Ok(None) };
    let bytes = tauri::async_runtime::spawn_blocking(move || fs::read(&src).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())??;
    if bytes.len() > MAX_IMAGE {
        return Err(format!("image exceeds {} MiB cap", MAX_IMAGE / (1024 * 1024)));
    }
    let ext = sniff_image(&bytes).ok_or("file is not a JPEG/PNG/WebP image")?;
    let key = album_key_from_file(&path).await?;
    let mut idx = load_index(&dir);
    let name = store_image(&dir, &mut idx, &key, &bytes, ext)?;
    idx.insert(
        key.clone(),
        CoverEntry { source: "user".into(), file: Some(name), url: None, ts: now_ts() },
    );
    save_index(&dir, &idx);
    Ok(Some(cover_url_for(&app, &path)))
}

/// Remove the user override (and any auto cache) for the track's album —
/// resolution falls back to embedded art / future auto-fetch.
#[tauri::command]
pub(crate) async fn cover_reset(app: tauri::AppHandle, path: String) -> Result<String, String> {
    let dir = init(&app).ok_or("no app data dir")?;
    let key = album_key_from_file(&path).await?;
    let mut idx = load_index(&dir);
    if let Some(old) = idx.remove(&key) {
        if let Some(f) = old.file {
            let _ = fs::remove_file(dir.join(f));
        }
        save_index(&dir, &idx);
    }
    Ok(cover_url_for(&app, &path))
}

/// Automatic art for a track. Never throws for network errors — those come
/// back as status "none" + `error`.
#[tauri::command]
pub(crate) async fn cover_auto(
    app: tauri::AppHandle,
    path: String,
    artist: String,
    album: String,
    allow_net: bool,
) -> CoverAutoResult {
    let none = |e: Option<String>| CoverAutoResult { status: "none".into(), url: None, error: e };
    let Some(dir) = init(&app) else { return none(Some("no app data dir".into())) };
    let Ok(meta) = tauri::async_runtime::spawn_blocking({
        let p = path.clone();
        move || probe::read_meta(Path::new(&p), false).map(|(m, _, _)| m)
    })
    .await
    else {
        return none(Some("track unreadable".into()));
    };
    let Ok(meta) = meta else { return none(Some("track unreadable".into())) };
    // Tags passed by the UI win when present, else the file's own tags.
    let artist = if artist.trim().is_empty() { meta.artist.clone() } else { artist };
    let album = if album.trim().is_empty() { meta.album.clone() } else { album };
    let key = album_key(&artist, &album, &path);

    // 1. Already have something? (override / no-art choice / web cache / miss)
    let idx = load_index(&dir);
    match idx.get(&key) {
        Some(e) if e.source == "user" || e.source == "none" => {
            return CoverAutoResult {
                status: "have".into(),
                url: Some(cover_url_for(&app, &path)),
                error: None,
            };
        }
        Some(e) if e.source == "web" => {
            let have_file = e.file.as_ref().map(|f| dir.join(f).exists()).unwrap_or(false);
            if have_file {
                return CoverAutoResult {
                    status: "have".into(),
                    url: Some(cover_url_for(&app, &path)),
                    error: None,
                };
            }
        }
        Some(e) if entry_fresh_miss(e) => return none(None),
        _ => {}
    }

    // 2. Embedded art? (has_cover from the bounded read, no bytes shipped)
    if meta.has_cover {
        return CoverAutoResult {
            status: "have".into(),
            url: Some(cover_url_for(&app, &path)),
            error: None,
        };
    }

    // 3. Network fetch (deduped per album key).
    if !allow_net {
        return none(None);
    }
    if !IN_FLIGHT.lock().map(|mut g| g.get_or_insert_with(HashSet::new).insert(key.clone())).unwrap_or(false) {
        // another caller is fetching this album right now; it will store the
        // result in the index — report none for this call.
        return none(None);
    }
    let result = fetch_and_store(&dir, &app, &path, &key, &artist, &album).await;
    if let Ok(mut g) = IN_FLIGHT.lock() {
        g.get_or_insert_with(HashSet::new).remove(&key);
    }
    result
}

async fn fetch_and_store(
    dir: &Path,
    app: &tauri::AppHandle,
    path: &str,
    key: &str,
    artist: &str,
    album: &str,
) -> CoverAutoResult {
    let none = |e: Option<String>| CoverAutoResult { status: "none".into(), url: None, error: e };
    let cands = search_all(artist, album, 10).await;
    if cands.is_empty() {
        let mut idx = load_index(dir);
        idx.insert(key.to_string(), CoverEntry { source: "miss".into(), file: None, url: None, ts: now_ts() });
        save_index(dir, &idx);
        return none(None);
    }
    let best = &cands[0];
    let client = match http_client() {
        Ok(c) => c,
        Err(e) => return none(Some(e)),
    };
    match http_get_capped(&client, &best.url, MAX_IMAGE).await {
        Ok(bytes) => match sniff_image(&bytes) {
            Some(ext) => {
                let mut idx = load_index(dir);
                match store_image(dir, &mut idx, key, &bytes, ext) {
                    Ok(name) => {
                        idx.insert(
                            key.to_string(),
                            CoverEntry {
                                source: "web".into(),
                                file: Some(name),
                                url: Some(best.url.clone()),
                                ts: now_ts(),
                            },
                        );
                        save_index(dir, &idx);
                        CoverAutoResult {
                            status: "fetched".into(),
                            url: Some(cover_url_for(app, path)),
                            error: None,
                        }
                    }
                    Err(e) => none(Some(e)),
                }
            }
            None => none(Some("downloaded file is not a JPEG/PNG/WebP image".into())),
        },
        Err(e) => none(Some(e)),
    }
}

#[derive(Debug, Serialize)]
pub struct CoverInfo {
    /// "user" | "embedded" | "web" | "none"
    pub source: String,
    pub url: String,
}

/// What the /cover/ route will currently serve for this track.
#[tauri::command]
pub(crate) async fn cover_info(app: tauri::AppHandle, path: String) -> Result<CoverInfo, String> {
    let dir = init(&app).ok_or("no app data dir")?;
    let key = album_key_from_file(&path).await?;
    let idx = load_index(&dir);
    let source = match idx.get(&key).map(|e| e.source.as_str()) {
        Some("user") => "user",
        Some("web") => "web",
        _ => {
            let p = path.clone();
            let has = tauri::async_runtime::spawn_blocking(move || {
                probe::read_meta(Path::new(&p), false).map(|(m, _, _)| m.has_cover).unwrap_or(false)
            })
            .await
            .unwrap_or(false);
            if has { "embedded" } else { "none" }
        }
    };
    Ok(CoverInfo { source: source.into(), url: cover_url_for(&app, &path) })
}

// ---------------------------------------------------------------------------
// Helpers shared by commands
// ---------------------------------------------------------------------------

async fn album_key_from_file(path: &str) -> Result<String, String> {
    let p = path.to_string();
    let m = tauri::async_runtime::spawn_blocking(move || {
        probe::read_meta(Path::new(&p), false).map(|(m, _, _)| m)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e)?;
    Ok(album_key(&m.artist, &m.album, path))
}

pub(crate) fn cover_url_for(app: &tauri::AppHandle, path: &str) -> String {
    init(app);
    let scheme = if cfg!(windows) {
        format!("http://media.localhost/cover/{}", pct_encode(path))
    } else {
        format!("media://localhost/cover/{}", pct_encode(path))
    };
    format!("{scheme}?v={}", version())
}

// ---------------------------------------------------------------------------
// Tests (no network)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_key_is_normalized_and_stable() {
        let k1 = album_key("Artist ", "Album", "p");
        let k2 = album_key("artist", " album ", "other-path");
        assert_eq!(k1, k2, "case/whitespace must not change the key");
        assert_eq!(k1.len(), 16); // fnv hex
        // different album -> different key
        assert_ne!(k1, album_key("artist", "other", "p"));
    }

    #[test]
    fn unknown_album_falls_back_to_track_path() {
        let k1 = album_key("A", "Unknown album", "C:\\Music\\01.flac");
        let k2 = album_key("A", "", "C:\\Music\\02.flac");
        assert_ne!(k1, k2, "each track gets its own key");
        assert_eq!(k1, album_key("B", "unknown album", "C:\\Music\\01.flac"));
        assert_eq!(k1, album_key("B", "unknown ALBUM", "C:\\Music\\01.flac"));
        // same path, different tags -> same key
        assert_eq!(k1, album_key("A", "Unknown album", "C:\\Music\\01.flac"));
    }

    #[test]
    fn index_roundtrip_and_atomic_save() {
        let dir = std::env::temp_dir().join(format!("ht_covers_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut idx = CoverIndex::new();
        idx.insert(
            "aaaaaaaaaaaaaaaa".into(),
            CoverEntry { source: "user".into(), file: Some("aaaaaaaaaaaaaaaa.jpg".into()), url: None, ts: 123 },
        );
        idx.insert(
            "bbbbbbbbbbbbbbbb".into(),
            CoverEntry { source: "web".into(), file: Some("bbbbbbbbbbbbbbbb.png".into()), url: Some("https://x/y".into()), ts: 456 },
        );
        idx.insert("cccccccccccccccc".into(), CoverEntry { source: "miss".into(), file: None, url: None, ts: 789 });
        save_index(&dir, &idx);
        let back = load_index(&dir);
        assert_eq!(back.len(), 3);
        assert_eq!(back["aaaaaaaaaaaaaaaa"].source, "user");
        assert_eq!(back["aaaaaaaaaaaaaaaa"].file.as_deref(), Some("aaaaaaaaaaaaaaaa.jpg"));
        assert_eq!(back["bbbbbbbbbbbbbbbb"].url.as_deref(), Some("https://x/y"));
        assert_eq!(back["cccccccccccccccc"].source, "miss");
        // corrupt/absent index reads as empty, never panics
        let empty = load_index(Path::new(&dir.join("nope")));
        assert!(empty.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolution_order_user_over_embedded_over_web() {
        let user = CoverEntry { source: "user".into(), file: Some("u.jpg".into()), url: None, ts: 0 };
        let web = CoverEntry { source: "web".into(), file: Some("w.jpg".into()), url: None, ts: 0 };
        let miss = CoverEntry { source: "miss".into(), file: None, url: None, ts: 0 };
        // user wins over everything
        assert_eq!(resolve(Some(&user), true, Some(&web)), Resolved::Stored(&user));
        assert_eq!(resolve(Some(&user), false, Some(&web)), Resolved::Stored(&user));
        // embedded beats web cache
        assert_eq!(resolve(Some(&web), true, Some(&web)), Resolved::Embedded);
        assert_eq!(resolve(None, true, Some(&web)), Resolved::Embedded);
        // web only without embedded
        assert_eq!(resolve(None, false, Some(&web)), Resolved::Stored(&web));
        // miss/none entries never serve bytes
        assert_eq!(resolve(None, false, Some(&miss)), Resolved::None);
        assert_eq!(resolve(Some(&miss), false, None), Resolved::None);
        assert_eq!(resolve(None, false, None), Resolved::None);
    }

    #[test]
    fn fresh_miss_expires_after_ttl() {
        let old = CoverEntry { source: "miss".into(), file: None, url: None, ts: now_ts() - 8 * 24 * 3600 };
        assert!(!entry_fresh_miss(&old), "stale miss must retry");
        let fresh = CoverEntry { source: "miss".into(), file: None, url: None, ts: now_ts() };
        assert!(entry_fresh_miss(&fresh));
        let web = CoverEntry { source: "web".into(), file: None, url: None, ts: now_ts() };
        assert!(!entry_fresh_miss(&web), "only miss entries use the TTL");
    }

    #[test]
    fn magic_validation() {
        let jpg = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        let webp = [b'R', b'I', b'F', b'F', 1, 0, 0, 0, b'W', b'E', b'B', b'P', 0];
        assert_eq!(sniff_image(&jpg), Some("jpg"));
        assert_eq!(sniff_image(&png), Some("png"));
        assert_eq!(sniff_image(&webp), Some("webp"));
        assert_eq!(sniff_image(b"GIF89a......"), None);
        assert_eq!(sniff_image(b"<html>"), None);
        assert_eq!(sniff_image(&[]), None);
        assert_eq!(sniff_image(b"RIFFxxxx"), None, "truncated RIFF rejected");
    }

    #[test]
    fn itunes_fixture_parses_to_candidates() {
        let body = r#"{
          "resultCount": 2,
          "results": [
            { "collectionName": "Led Zeppelin IV",
              "artistName": "Led Zeppelin",
              "artworkUrl100": "https://is1-ssl.mzstatic.com/image/thumb/Music/xx/source/100x100bb.jpg" },
            { "collectionName": "Other", "artistName": "X",
              "artworkUrl100": "https://is1-ssl.mzstatic.com/image/thumb/Music/yy/source/100x100bb.jpg" }
          ]
        }"#;
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        let c = parse_itunes(&v, 10);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].source, "itunes");
        assert_eq!(c[0].title, "Led Zeppelin IV");
        assert_eq!(c[0].artist, "Led Zeppelin");
        assert_eq!(
            c[0].url,
            "https://is1-ssl.mzstatic.com/image/thumb/Music/xx/source/600x600bb.jpg",
            "100x100bb upscaled to 600x600bb"
        );
        assert_eq!(c[0].thumb, "https://is1-ssl.mzstatic.com/image/thumb/Music/xx/source/100x100bb.jpg");
        assert_eq!(c[0].width, Some(600));
        // limit respected
        assert_eq!(parse_itunes(&v, 1).len(), 1);
        // garbage shapes
        assert!(parse_itunes(&serde_json::json!({}), 10).is_empty());
        assert!(parse_itunes(&serde_json::json!({"results": [{}]}), 10).is_empty());
    }

    #[test]
    fn musicbrainz_fixture_parses_mbids() {
        let body = r#"{
          "releases": [
            { "id": "7bb01b2b-9a03-3e56-a3af-0a4b6d9b8c3f",
              "title": "Led Zeppelin IV",
              "artist-credit": [ { "name": "Led Zeppelin" } ] },
            { "title": "no id release" }
          ]
        }"#;
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        let c = parse_mb(&v, 5);
        assert_eq!(c.len(), 1, "entries without an id are skipped");
        assert_eq!(c[0].source, "musicbrainz");
        assert_eq!(c[0].url, "https://coverartarchive.org/release/7bb01b2b-9a03-3e56-a3af-0a4b6d9b8c3f/front-500");
        assert_eq!(c[0].thumb, c[0].url);
        assert_eq!(c[0].title, "Led Zeppelin IV");
        assert_eq!(c[0].artist, "Led Zeppelin");
        assert!(parse_mb(&serde_json::json!({}), 5).is_empty());
    }

    #[test]
    fn ranking_prefers_exact_album_and_artist() {
        let cands = vec![
            CoverCandidate { url: "u3".into(), thumb: "t3".into(), source: "itunes".into(), title: "B-Sides".into(), artist: "Other".into(), width: None, height: None },
            CoverCandidate { url: "u1".into(), thumb: "t1".into(), source: "itunes".into(), title: "Wish You Were Here!".into(), artist: "pink-floyd".into(), width: None, height: None },
            CoverCandidate { url: "u2".into(), thumb: "t2".into(), source: "musicbrainz".into(), title: "Wish You Were Here".into(), artist: "Someone Else".into(), width: None, height: None },
        ];
        let r = rank_candidates(cands, "Pink Floyd", "Wish You Were Here");
        // case/punct-insensitive exact album+artist first
        assert_eq!(r[0].url, "u1");
        // album-only match second
        assert_eq!(r[1].url, "u2");
        assert_eq!(r[2].url, "u3");
    }

    /// Live network smoke test: a well-known album must return candidates.
    /// `cargo test --lib cover_live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn cover_live_itunes_wellknown_album() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let cands = rt.block_on(async {
            let client = http_client().unwrap();
            let v: serde_json::Value = client
                .get("https://itunes.apple.com/search")
                .query(&[
                    ("term", &"pink floyd the dark side of the moon".to_string()),
                    ("entity", &"album".to_string()),
                    ("limit", &"10".to_string()),
                ])
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap();
            parse_itunes(&v, 10)
        });
        assert!(!cands.is_empty(), "iTunes must find The Dark Side of the Moon");
        assert!(cands.iter().any(|c| c.url.contains("600x600bb")));
        println!("first candidate: {} — {}", cands[0].title, cands[0].url);
    }
}
