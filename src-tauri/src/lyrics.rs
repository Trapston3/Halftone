//! lyrics.rs — automatic lyrics resolution for the playing track.
//!
//! Order: sidecar `.lrc` -> tags embedded in the file -> app cache ->
//! LRCLIB (exact `/api/get`, then `/api/search` matched by duration).
//! Network results are cached in the app cache dir — never written next to
//! the music (libraries are often on read-only or synced drives). Misses
//! are cached as `.none` for a week so we don't hammer LRCLIB.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use tauri::Manager;

use crate::{parse_lrc, probe, read_lrc_file, LyricLine};

const NEG_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
const UA: &str = "Halftone/0.2 (https://github.com/Trapston3/Halftone)";

#[derive(Debug, Clone, Serialize)]
pub struct LyricsResult {
    /// "sidecar" | "embedded" | "cache" | "lrclib" | "none"
    pub source: String,
    pub synced: bool,
    pub lines: Vec<LyricLine>,
    /// Unsynced lyrics text, when that is all that exists.
    pub plain: Option<String>,
}

impl LyricsResult {
    fn none() -> Self {
        Self { source: "none".into(), synced: false, lines: Vec::new(), plain: None }
    }
    /// LRC-looking text becomes synced lines; anything else stays plain.
    fn from_text(source: &str, text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let lines = parse_lrc(text);
        Some(if lines.is_empty() {
            Self { source: source.into(), synced: false, lines, plain: Some(text.to_string()) }
        } else {
            Self { source: source.into(), synced: true, lines, plain: None }
        })
    }
}

/// FNV-1a 64 — stable across Rust releases (DefaultHasher is not).
fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn cache_key(artist: &str, title: &str, duration: f64) -> String {
    format!(
        "{:016x}",
        fnv(&format!("{}|{}|{}", artist.trim().to_lowercase(), title.trim().to_lowercase(), duration.round() as i64))
    )
}

/// "Song (feat. X) [Remastered 2011] - Live" -> "Song"
pub fn simplify_title(t: &str) -> String {
    let mut s = t.to_string();
    for (open, close) in [('(', ')'), ('[', ']')] {
        while let (Some(a), Some(b)) = (s.find(open), s.find(close)) {
            if b <= a {
                break;
            }
            s.replace_range(a..=b, "");
        }
    }
    let lower = s.to_lowercase();
    for cut in [" feat.", " ft.", " featuring ", " - "] {
        if let Some(i) = lower.find(cut) {
            s.truncate(i);
            break;
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn cache_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_cache_dir().ok().map(|d| d.join("lyrics"))
}

fn read_cache(dir: &Path, key: &str, force: bool) -> Option<Option<LyricsResult>> {
    for ext in ["lrc", "txt"] {
        if let Ok(t) = fs::read_to_string(dir.join(format!("{key}.{ext}"))) {
            return Some(LyricsResult::from_text("cache", &t));
        }
    }
    if !force {
        let neg = dir.join(format!("{key}.none"));
        let fresh = fs::metadata(&neg)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .map_or(false, |age| age < NEG_TTL);
        if fresh {
            return Some(None);
        }
    }
    None
}

/// Pick the best LRCLIB record: synced within ±3 s, any synced, then plain
/// closest by duration.
fn pick(results: &[serde_json::Value], duration: f64) -> Option<(bool, String)> {
    let dur_ok = |r: &serde_json::Value| {
        duration <= 0.0 || r["duration"].as_f64().map_or(true, |d| (d - duration).abs() <= 3.0)
    };
    let synced = |r: &serde_json::Value| {
        r["syncedLyrics"].as_str().filter(|s| !s.trim().is_empty()).map(str::to_string)
    };
    if let Some(s) = results.iter().filter(|r| dur_ok(r)).find_map(synced) {
        return Some((true, s));
    }
    if let Some(s) = results.iter().find_map(synced) {
        return Some((true, s));
    }
    let mut plain: Vec<(f64, String)> = results
        .iter()
        .filter_map(|r| {
            let p = r["plainLyrics"].as_str().filter(|s| !s.trim().is_empty())?;
            let dd = r["duration"].as_f64().map_or(1e9, |d| (d - duration).abs());
            Some((dd, p.to_string()))
        })
        .collect();
    plain.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    plain.into_iter().next().map(|(_, p)| (false, p))
}

async fn lrclib(artist: &str, title: &str, album: &str, duration: f64) -> Result<Option<(bool, String)>, String> {
    let client = reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    // 1. exact signature match
    if duration > 0.0 {
        let r = client
            .get("https://lrclib.net/api/get")
            .query(&[
                ("artist_name", artist),
                ("track_name", title),
                ("album_name", album),
                ("duration", &format!("{}", duration.round() as i64)),
            ])
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if r.status().is_success() {
            let v: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
            if let Some(hit) = pick(std::slice::from_ref(&v), duration) {
                return Ok(Some(hit));
            }
        }
    }
    // 2. search, then a simplified title ("Song (feat. X)" -> "Song")
    let simple = simplify_title(title);
    let mut titles = vec![title.to_string()];
    if !simple.is_empty() && simple != title {
        titles.push(simple);
    }
    for t in titles {
        let r = client
            .get("https://lrclib.net/api/search")
            .query(&[("artist_name", artist), ("track_name", t.as_str())])
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !r.status().is_success() {
            continue;
        }
        let v: Vec<serde_json::Value> = r.json().await.map_err(|e| e.to_string())?;
        if let Some(hit) = pick(&v, duration) {
            return Ok(Some(hit));
        }
    }
    Ok(None)
}

pub async fn resolve(
    app: &tauri::AppHandle,
    path: &str,
    artist: &str,
    title: &str,
    album: &str,
    duration: f64,
    allow_net: bool,
    force: bool,
) -> Result<LyricsResult, String> {
    let p = Path::new(path);
    if !force {
        let side = read_lrc_file(p);
        if !side.is_empty() {
            return Ok(LyricsResult { source: "sidecar".into(), synced: true, lines: side, plain: None });
        }
        let pp = p.to_path_buf();
        let emb = tauri::async_runtime::spawn_blocking(move || {
            probe::read_meta(&pp, true).ok().and_then(|(m, _, _)| m.embedded_lyrics)
        })
        .await
        .ok()
        .flatten();
        if let Some(r) = emb.and_then(|t| LyricsResult::from_text("embedded", &t)) {
            return Ok(r);
        }
    }
    let key = cache_key(artist, title, duration);
    let dir = cache_dir(app);
    if let Some(d) = &dir {
        if !force {
            match read_cache(d, &key, force) {
                Some(Some(r)) => return Ok(r),
                Some(None) => return Ok(LyricsResult::none()), // recent miss
                None => {}
            }
        }
    }
    if !allow_net || title.trim().is_empty() {
        return Ok(LyricsResult::none());
    }
    let hit = lrclib(artist, title, album, duration).await?;
    if let Some(d) = &dir {
        let _ = fs::create_dir_all(d);
        match &hit {
            Some((synced, text)) => {
                let _ = fs::write(d.join(format!("{key}.{}", if *synced { "lrc" } else { "txt" })), text);
                let _ = fs::remove_file(d.join(format!("{key}.none")));
            }
            None => {
                let _ = fs::write(d.join(format!("{key}.none")), b"");
            }
        }
    }
    Ok(match hit {
        Some((_, text)) => LyricsResult::from_text("lrclib", &text).unwrap_or_else(LyricsResult::none),
        None => LyricsResult::none(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_stable_and_normalized() {
        assert_eq!(cache_key("Artist ", "Song", 200.4), cache_key("artist", "song", 199.6));
        assert_ne!(cache_key("a", "b", 100.0), cache_key("a", "b", 101.0));
        assert_eq!(cache_key("a", "b", 1.0), format!("{:016x}", fnv("a|b|1")));
    }

    #[test]
    fn simplify() {
        assert_eq!(simplify_title("Song (feat. X) [Remastered 2011]"), "Song");
        assert_eq!(simplify_title("Song - Live at Wembley"), "Song");
        assert_eq!(simplify_title("Song ft. Someone"), "Song");
        assert_eq!(simplify_title("Plain"), "Plain");
    }

    #[test]
    fn text_classification() {
        let s = LyricsResult::from_text("x", "[00:01.00]hello\n[00:02.50]world").unwrap();
        assert!(s.synced);
        assert_eq!(s.lines.len(), 2);
        let p = LyricsResult::from_text("x", "just words\nmore words").unwrap();
        assert!(!p.synced);
        assert!(p.plain.is_some());
        assert!(LyricsResult::from_text("x", "  ").is_none());
    }

    #[test]
    fn pick_prefers_synced_close_duration() {
        let v: Vec<serde_json::Value> = serde_json::from_str(r#"[
          {"duration": 300, "syncedLyrics": "[00:01.00]far", "plainLyrics": "far"},
          {"duration": 201, "syncedLyrics": "[00:01.00]near", "plainLyrics": "near"},
          {"duration": 200, "syncedLyrics": null, "plainLyrics": "plain"}
        ]"#).unwrap();
        assert_eq!(pick(&v, 200.0), Some((true, "[00:01.00]near".into())));
        let only_plain: Vec<serde_json::Value> =
            serde_json::from_str(r#"[{"duration": 200, "syncedLyrics": "", "plainLyrics": "p"}]"#).unwrap();
        assert_eq!(pick(&only_plain, 200.0), Some((false, "p".into())));
        assert_eq!(pick(&[], 1.0), None);
    }
}
