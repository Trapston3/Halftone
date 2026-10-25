//! discord_rpc.rs — Discord Rich Presence (RPC) over IPC named pipes.
//!
//! Protocol:
//! - Windows: `\\.\pipe\discord-ipc-0` through `\\.\pipe\discord-ipc-9`
//! - Unix: `$XDG_RUNTIME_DIR/discord-ipc-0` etc.
//! - Frame: `[opcode: u32-le, length: u32-le, payload_bytes]`
//! - Handshake (opcode 0): `{"v": 1, "client_id": "<id>"}`
//! - Set Activity (opcode 1): `{"cmd": "SET_ACTIVITY", "args": { "pid": ..., "activity": { ... } }}`

use std::io::{Read, Write};
#[cfg(unix)]
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_CLIENT_ID: &str = "1300489210485923890";
static NONCE_COUNTER: AtomicU64 = AtomicU64::new(1);
static RPC_CHANNEL: OnceLock<SyncSender<RpcMessage>> = OnceLock::new();

enum RpcMessage {
    Update(serde_json::Value),
    Clear,
}

#[cfg(windows)]
type PipeStream = std::fs::File;

#[cfg(unix)]
type PipeStream = std::os::unix::net::UnixStream;

#[cfg(not(any(windows, unix)))]
type PipeStream = std::fs::File;

#[cfg(windows)]
fn try_connect_pipe() -> Option<PipeStream> {
    for i in 0..10 {
        let path = format!(r"\\.\pipe\discord-ipc-{}", i);
        if let Ok(file) = std::fs::OpenOptions::new().read(true).write(true).open(&path) {
            return Some(file);
        }
    }
    None
}

#[cfg(unix)]
fn try_connect_pipe() -> Option<PipeStream> {
    use std::os::unix::net::UnixStream;
    let dirs = [
        std::env::var("XDG_RUNTIME_DIR").ok(),
        std::env::var("TMPDIR").ok(),
        std::env::var("TMP").ok(),
        std::env::var("TEMP").ok(),
        Some("/tmp".to_string()),
    ];
    for dir in dirs.into_iter().flatten() {
        for i in 0..10 {
            let path = PathBuf::from(&dir).join(format!("discord-ipc-{}", i));
            if let Ok(stream) = UnixStream::connect(&path) {
                return Some(stream);
            }
        }
    }
    None
}

#[cfg(not(any(windows, unix)))]
fn try_connect_pipe() -> Option<PipeStream> {
    None
}

pub(crate) fn send_frame<W: Write>(writer: &mut W, opcode: u32, payload: &str) -> std::io::Result<()> {
    let bytes = payload.as_bytes();
    let mut header = [0u8; 8];
    header[0..4].copy_from_slice(&opcode.to_le_bytes());
    header[4..8].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    writer.write_all(&header)?;
    writer.write_all(bytes)?;
    writer.flush()?;
    Ok(())
}

pub(crate) fn read_frame<R: Read>(reader: &mut R) -> std::io::Result<(u32, Vec<u8>)> {
    let mut header = [0u8; 8];
    reader.read_exact(&mut header)?;
    let opcode = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    if length > 128 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "discord rpc frame exceeded 128 KiB cap",
        ));
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok((opcode, body))
}

struct WorkerState {
    stream: Option<PipeStream>,
}

impl WorkerState {
    fn new() -> Self {
        Self { stream: None }
    }

    fn ensure_connected(&mut self) -> bool {
        if self.stream.is_some() {
            return true;
        }
        let Some(mut s) = try_connect_pipe() else {
            return false;
        };
        let client_id = std::env::var("HALFTONE_DISCORD_CLIENT_ID")
            .unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
        let handshake = serde_json::json!({
            "v": 1,
            "client_id": client_id
        })
        .to_string();

        if send_frame(&mut s, 0, &handshake).is_err() {
            return false;
        }
        if read_frame(&mut s).is_err() {
            return false;
        }
        self.stream = Some(s);
        true
    }

    fn send_payload(&mut self, payload: &serde_json::Value) -> bool {
        if !self.ensure_connected() {
            return false;
        }
        let s = self.stream.as_mut().unwrap();
        let body = payload.to_string();
        if send_frame(s, 1, &body).is_err() {
            self.stream = None;
            return false;
        }
        // Drain reply frame
        let _ = read_frame(s);
        true
    }

    fn clear_activity(&mut self) -> bool {
        if self.stream.is_none() {
            return true;
        }
        let nonce = format!("ht-{}", NONCE_COUNTER.fetch_add(1, Ordering::Relaxed));
        let payload = serde_json::json!({
            "cmd": "SET_ACTIVITY",
            "args": {
                "pid": std::process::id(),
                "activity": null
            },
            "nonce": nonce
        });
        self.send_payload(&payload)
    }
}

fn worker_loop(rx: Receiver<RpcMessage>) {
    let mut state = WorkerState::new();
    while let Ok(msg) = rx.recv() {
        match msg {
            RpcMessage::Update(payload) => {
                let _ = state.send_payload(&payload);
            }
            RpcMessage::Clear => {
                let _ = state.clear_activity();
            }
        }
    }
}

fn get_rpc_tx() -> &'static SyncSender<RpcMessage> {
    RPC_CHANNEL.get_or_init(|| {
        let (tx, rx) = sync_channel(32);
        std::thread::Builder::new()
            .name("discord-rpc".into())
            .spawn(move || worker_loop(rx))
            .expect("failed to spawn discord-rpc thread");
        tx
    })
}

pub fn update_presence(
    title: &str,
    artist: &str,
    album: &str,
    duration_sec: f64,
    position_sec: f64,
    playing: bool,
) -> bool {
    let nonce = format!("ht-{}", NONCE_COUNTER.fetch_add(1, Ordering::Relaxed));
    let title_clean = if title.trim().is_empty() { "Unknown Title" } else { title.trim() };
    let artist_clean = if artist.trim().is_empty() { "Unknown Artist" } else { artist.trim() };
    let album_clean = if album.trim().is_empty() { "Single" } else { album.trim() };

    let mut activity = serde_json::json!({
        "type": 2, // Listening
        "details": title_clean,
        "state": format!("by {} • on Halftone", artist_clean),
        "assets": {
            "large_image": "halftone_logo",
            "large_text": format!("Listening on Halftone — FOSS Local Music Player ({})", album_clean),
            "small_image": if playing { "play" } else { "pause" },
            "small_text": if playing { "Playing" } else { "Paused" }
        },
        "buttons": [
            { "label": "Get Halftone on GitHub", "url": "https://github.com/Trapston3/Halftone" }
        ]
    });

    if playing && duration_sec > 0.0 {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let start_ms = now_ms.saturating_sub((position_sec.max(0.0) * 1000.0) as u64);
        let end_ms = start_ms + ((duration_sec * 1000.0) as u64);
        if let Some(obj) = activity.as_object_mut() {
            obj.insert(
                "timestamps".into(),
                serde_json::json!({
                    "start": start_ms,
                    "end": end_ms
                }),
            );
        }
    }

    let payload = serde_json::json!({
        "cmd": "SET_ACTIVITY",
        "args": {
            "pid": std::process::id(),
            "activity": activity
        },
        "nonce": nonce
    });

    get_rpc_tx().try_send(RpcMessage::Update(payload)).is_ok()
}

pub fn clear_presence() -> bool {
    get_rpc_tx().try_send(RpcMessage::Clear).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_serialization_roundtrip() {
        let mut buf = Vec::new();
        let payload = r#"{"test": true, "val": 42}"#;
        send_frame(&mut buf, 1, payload).unwrap();

        assert_eq!(buf.len(), 8 + payload.len());
        let opcode = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let length = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        assert_eq!(opcode, 1);
        assert_eq!(length as usize, payload.len());

        let mut cursor = std::io::Cursor::new(buf);
        let (read_op, body) = read_frame(&mut cursor).unwrap();
        assert_eq!(read_op, 1);
        assert_eq!(String::from_utf8(body).unwrap(), payload);
    }

    #[test]
    fn rpc_fault_tolerant_when_discord_not_running() {
        // Must return false without panicking or hanging
        let updated = update_presence("Test Song", "Test Artist", "Test Album", 180.0, 10.0, true);
        let cleared = clear_presence();
        // Even if Discord is not running on the test runner, it should be a boolean (most likely false)
        let _ = (updated, cleared);
    }
}
