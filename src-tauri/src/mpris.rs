//! mpris.rs — OS media-integration bridge.
//!
//! Windows: WebView2 exposes the owner window's `<audio>` element as the OS
//! media session through `navigator.mediaSession` (see ui/main.html). The
//! Rust side adds nothing there — commands are no-ops — because a Rust SMTC
//! interop session would fight WebView2's own session (the "zombie session"
//! problem; see docs/WHAT_WE_BUILT.md Phase 0).
//!
//! Linux: there is no WebView2 media-session bridge, so this module runs a
//! real MPRIS D-Bus player (org.mpris.MediaPlayer2) via souvlaki, mirroring
//! the SMTC surface: metadata (title/artist/album/cover + duration),
//! playback state + position, and transport buttons that emit the SAME
//! `smtc-button` events the frontend already handles ("toggle"/"next"/
//! "prev"). The UI pushes state with the `smtc_update` command (no-op on
//! Windows, where mediaSession does the job).
//!
//! Every command name exists on all OSes; everything here compiles only on
//! Linux and the non-Linux shims are trivial.

/// What the UI wants the OS media surface to show right now.
/// (On Windows the struct is constructed by the no-op command path; fields
/// are read by the Linux MPRIS implementation.)
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Debug)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Embedded art already written to a temp file, as a file:// URL
    /// (MPRIS mpris:artUrl wants a URL; local files must be file://).
    pub cover_url: Option<String>,
    pub duration_s: f64,
    pub playing: bool,
    pub position_s: f64,
}

#[cfg(target_os = "linux")]
mod imp {
    use super::NowPlaying;
    use crate::TrackMeta;
    use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig};
    use tauri::Emitter;
    use std::sync::Mutex;
    use std::time::Duration;

    static CONTROLS: Mutex<Option<MediaControls>> = Mutex::new(None);

    /// Emit events via the captured app handle (set in `run()` setup).
    fn with_controls(f: &mut dyn FnMut(&mut MediaControls) -> Result<(), souvlaki::Error>) {
        let mut g = match CONTROLS.lock() {
            Ok(g) => g,
            Err(_) => return, // poisoned: never panic into a command
        };
        // Lazily create the MPRIS service on first use (needs no window on
        // Linux; display/dbus names identify us on the session bus).
        if g.is_none() {
            let Some(app) = crate::app_handle() else {
                eprintln!("halftone mpris: app handle not ready");
                return;
            };
            let config = PlatformConfig {
                display_name: "Halftone",
                dbus_name: "halftone",
                hwnd: None, // Linux does not use it
            };
            match MediaControls::new(config) {
                Ok(mut c) => {
                    // Transport events -> the same smtc-button payload the
                    // frontend already listens for (tray menu does this on
                    // Windows too).
                    let handler = move |ev: MediaControlEvent| {
                        let msg = match ev {
                            MediaControlEvent::Play => "play",
                            MediaControlEvent::Pause => "pause",
                            MediaControlEvent::Toggle => "toggle",
                            MediaControlEvent::Next => "next",
                            MediaControlEvent::Previous => "prev",
                            MediaControlEvent::Stop => "pause",
                            MediaControlEvent::SetPosition(p) => {
                                let _ = app.emit("smtc-seek", p.0.as_secs_f64());
                                return;
                            }
                            MediaControlEvent::SeekBy(dir, d) => {
                                let _ = app.emit(
                                    "smtc-seek-by",
                                    (matches!(dir, souvlaki::SeekDirection::Forward), d.as_secs_f64()),
                                );
                                return;
                            }
                            MediaControlEvent::Seek(_) => return, // undetermined amount
                            MediaControlEvent::SetVolume(_) => return, // handled by UI volume, not mapped
                            MediaControlEvent::OpenUri(_) => return,
                            MediaControlEvent::Raise => return,
                            MediaControlEvent::Quit => return,
                        };
                        let _ = app.emit("smtc-button", msg);
                    };
                    if let Err(e) = c.attach(handler) {
                        eprintln!("halftone mpris: attach failed: {e:?}");
                        return;
                    }
                    *g = Some(c);
                }
                Err(e) => {
                    // No session bus (CI, SSH without dbus): keep playing,
                    // just without MPRIS.
                    eprintln!("halftone mpris: unavailable: {e:?}");
                    return;
                }
            }
        }
        if let Some(c) = g.as_mut() {
            if let Err(e) = f(c) {
                eprintln!("halftone mpris: update failed: {e:?}");
            }
        }
    }

    pub fn update(np: &NowPlaying) {
        with_controls(&mut |c| {
            let meta = MediaMetadata {
                title: Some(np.title.as_str()),
                album: Some(np.album.as_str()),
                artist: Some(np.artist.as_str()),
                cover_url: np.cover_url.as_deref(),
                duration: (np.duration_s > 0.0)
                    .then(|| Duration::from_secs_f64(np.duration_s)),
            };
            c.set_metadata(meta)?;
            let progress = Some(MediaPosition(Duration::from_secs_f64(np.position_s)));
            let _ = c.set_playback(if np.playing {
                MediaPlayback::Playing { progress }
            } else {
                MediaPlayback::Paused { progress }
            });
            Ok(())
        });
    }

    pub fn clear() {
        with_controls(&mut |c| {
            c.set_playback(MediaPlayback::Stopped)
        });
    }

impl NowPlaying {
    /// Build the now-playing state from parsed track metadata, with the
    /// embedded cover exported to a temp file (Linux MPRIS needs a URL).
    /// Called from the UI-side wiring (open_track/position ticks); unused
    /// until that lands, hence the allow — the command surface is what
    /// matters on all OSes.
    #[allow(dead_code)]
    pub fn from_meta(
        meta: &TrackMeta,
        playing: bool,
        position_s: f64,
    ) -> Option<Self> {
        Some(Self {
            title: if meta.title.is_empty() { track_name_of(&meta.path) } else { meta.title.clone() },
            artist: meta.artist.clone(),
            album: meta.album.clone(),
            cover_url: super::cover_temp_url(meta),
            duration_s: meta.duration,
            playing,
            position_s,
        })
    }
}

    /// Fall back to the file stem when the metadata has no TITLE tag.
    #[allow(dead_code)] // part of the update path planned for the UI wiring; kept for parity
    fn track_name_of(p: &str) -> String {
        std::path::Path::new(p)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.to_string())
    }
}

// ---------------------------------------------------------------------------
// Non-Linux: commands exist and do nothing (mediaSession owns SMTC).
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::NowPlaying;
    pub fn update(_np: &NowPlaying) {}
    pub fn clear() {}
}

// ---------------------------------------------------------------------------
// OS-neutral command surface (registered on every platform)
// ---------------------------------------------------------------------------

/// Push the current now-playing state to the OS media surface.
/// Linux: updates the MPRIS player. Windows: no-op (the UI's
/// navigator.mediaSession integration already drives SMTC).
#[tauri::command]
pub fn smtc_update(
    title: String,
    artist: String,
    album: String,
    cover_url: Option<String>,
    duration_s: f64,
    playing: bool,
    position_s: f64,
) {
    imp::update(&NowPlaying {
        title,
        artist,
        album,
        cover_url,
        duration_s,
        playing,
        position_s,
    });
}

/// The library stopped (end of queue, window closing): drop the OS entry.
#[tauri::command]
pub fn smtc_clear() {
    imp::clear();
}

/// Write embedded art to a temp file and return its file:// URL (Linux MPRIS
/// wants a URL; the decoded PNG/JPEG bytes are already in TrackMeta).
#[cfg(target_os = "linux")]
pub fn cover_temp_url(meta: &crate::TrackMeta) -> Option<String> {
    let pic = meta.cover.as_ref()?;
    let ext = if pic.mime.contains("png") { "png" } else { "jpg" };
    let data = crate::base64_decode_pub(&pic.data_b64)?;
    let dir = std::env::temp_dir().join("halftone-covers");
    std::fs::create_dir_all(&dir).ok()?;
    // One file per track path hash: stable across pushes, no unbounded growth
    // (a hash of the path — same track overwrites its own file).
    let name = format!("{}.{ext}", hash_path(&meta.path));
    let p = dir.join(name);
    std::fs::write(&p, data).ok()?;
    let url = tauri::Url::from_file_path(&p).ok()?;
    Some(url.to_string())
}

#[cfg(target_os = "linux")]
fn hash_path(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(all(target_os = "linux", test))]
mod mpris_tests {
    use super::*;
 
    #[test]
    fn cover_temp_url_roundtrip() {
        let m = TrackMeta {
            path: "/x/test.ogg".into(),
            title: "t".into(),
            artist: "a".into(),
            album: "l".into(),
            streaminfo: crate::StreamInfo { sample_rate: 44100, bits: 16, channels: 2, total_samples: 44100 },
            duration: 1.0,
            cover: Some(crate::Picture {
                mime: "image/png".into(),
                data_b64: crate::base64_encode(b"PNGDATA"),
                width: 8,
                height: 8,
                data_len: 7,
            }),
            block_types: vec![],
            replaygain: None,
            format_tag: "OGG".into(),
            lossless: false,
            bitrate_kbps: 0,
            has_cover: true,
            embedded_lyrics: None,
        };
        let url = cover_temp_url(&m).expect("cover temp url");
        assert!(url.starts_with("file://"), "{url}");
        // The same track maps to the same temp file (stable hash).
        let url2 = cover_temp_url(&m).unwrap();
        assert_eq!(url, url2);
    }
}
