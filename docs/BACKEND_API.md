# Halftone Backend API

The Rust side (`src-tauri/`) exposes three surfaces to the UI. This document is
the binding contract for `ui/` — command names, argument names, return shapes,
event names and payload shapes are exactly as implemented in
`src-tauri/src/lib.rs`, `src-tauri/src/mpris.rs`, `src-tauri/src/lyrics.rs`,
and `src-tauri/src/probe.rs`.

- **Invoke commands** — `window.__TAURI__.core.invoke(name, args)`. Tauri 2
  expects **camelCase argument keys** in JS for snake_case Rust parameters
  (`duration_s` → `durationS`).
- **Events** — backend → UI via `listen(name, cb)`
  (`window.__TAURI__.event.listen`).
- **Protocol route** — the `media` custom URI scheme, used for audio bytes and
  embedded art.

Every command exists and behaves identically on Windows and Linux (Linux-only
behavior is marked; no command name is platform-conditional).

---

## Data shapes

All JSON shapes below are what arrives in JS (serde-serialized Rust structs).

### TrackMeta

```jsonc
{
  "path": "C:\\Music\\Artist\\Album\\01 Song.flac",   // absolute OS path
  "title": "Song",
  "artist": "Artist",
  "album": "Album",
  "streaminfo": {
    "sample_rate": 44100,
    "bits": 16,                 // 16 for lossy formats (convention)
    "channels": 2,
    "total_samples": 44100
  },
  "duration_s": 123.45,         // seconds, f64
  "cover": null,                // Picture | null — only set by open_track/cover_url paths
  "block_types": [0, 4],        // FLAC metadata block types seen (other formats: [])
  "replaygain": { "track_gain": -7.2, "album_gain": -7.0 },  // dB, or null
  "format": "FLAC",             // "FLAC" | "WAV" | "MP3" | "AAC" | "ALAC" | "OGG" | "OPUS"
  "lossless": true,             // false for MP3/AAC/OGG/OPUS
  "bitrate_kbps": 953,          // rounded average
  "has_cover": true,            // embedded art exists (scans never ship the bytes)
  "embedded_lyrics": "[00:01.00]..."  // LYRICS/USLT/©lyr tag, or absent (open_track only)
}
```

Notes:

- Scans (`scan_library`, `library_snapshot`) **omit `cover` bytes** (always
  `null`) and `embedded_lyrics` — the payload would be enormous. Art is loaded
  through the `/cover/` protocol route; embedded lyrics through `lyrics_get`.
- `embedded_lyrics` is `skip_serializing_if = Option::is_none`: absent key =
  no embedded lyrics, don't guess `null`.
- `duration_s` for Opus is the **playable** duration: last-page granule minus
  pre-skip, divided by 48000 (ffprobe's raw granule includes pre-skip).

### Picture

```jsonc
{ "mime": "image/jpeg", "data_b64": "<base64>", "width": 800, "height": 800, "data_len": 102400 }
```

### LyricLine

```jsonc
{ "t": 12.34, "text": "line" }   // t = seconds (f64), sorted, deduped
```

### LyricsResult (returned by `lyrics_get`)

```jsonc
{
  "source": "sidecar",    // "sidecar" | "embedded" | "cache" | "lrclib" | "none"
  "synced": true,
  "lines": [ { "t": 1.0, "text": "..." } ],
  "plain": null           // unsynced text when that's all there is, else null
}
```

### ScanResult (returned by `scan_library`)

```jsonc
{
  "tracks": [ TrackMeta... ],          // sorted by path
  "skipped": ["C:\\bad.mp3: parse error"],  // files that failed to parse
  "unsupported": 3                     // count of wma/aiff/aif/ape/wv files
}
```

### OtaStatus (returned by `ota_check` / `ota_download`)

```jsonc
{
  "supported": true,
  "checking": false,
  "disabled": false,        // HALFTONE_NO_OTA set
  "current": "0.1.2",
  "available": "0.2.0",     // null when up to date
  "downloading": false,
  "ready": false,           // staged, applies on next start
  "error": null
}
```

---

## Commands

### scan_library(dir) → ScanResult

Recursively scan a folder (depth ≤ 16) with bounded reads and a persistent
scan index (unchanged files by size+mtime come from the index). Also starts
the folder watcher and caches the library in Rust (for `library_snapshot`).

```js
const r = await window.__TAURI__.core.invoke('scan_library', { dir: 'C:\\Music' });
```

Errors: `"folder not found: <dir>"` when the path is empty/missing.

### scan_cancel() → null

Set the cancel flag; a running scan stops parsing files and discards the
index update.

```js
await window.__TAURI__.core.invoke('scan_cancel');
```

### library_snapshot() → TrackMeta[]

The last scan's tracks (cached in Rust; no covers, no embedded lyrics).
Empty array before the first scan.

```js
const tracks = await window.__TAURI__.core.invoke('library_snapshot');
```

### open_track(path) → TrackMeta

Full metadata for ONE track, including `cover` (Picture with base64 bytes)
and `embedded_lyrics` when present. Bounded head read + FLAC cover read.

```js
const m = await window.__TAURI__.core.invoke('open_track', { path: p });
```

### read_lyrics(path) → LyricLine[]

Parses the sidecar `.lrc` next to the track (same name, `.lrc` extension).
No network. Returns `[]` when absent.

```js
const lines = await window.__TAURI__.core.invoke('read_lyrics', { path: p });
```

### lyrics_get(path, artist, title, album, duration, allowNet, force) → LyricsResult

The lyrics resolver (see `docs/audio-pipeline.md`): sidecar `.lrc` → embedded
tag → LRCLIB cache dir → LRCLIB network (when `allowNet`) → `none`. `force`
bypasses the negative cache for a fresh network lookup.

```js
const r = await window.__TAURI__.core.invoke('lyrics_get', {
  path: p, artist: 'A', title: 'T', album: 'L',
  duration: 213.5, allowNet: true, force: false,
});
```

### lrc_fetch(path, artist, title) → string (written file path)

Legacy explicit button: force LRCLIB lookup and save a sidecar `.lrc` next to
the track. Errors `"no lyrics found on lrclib"` / `"only unsynced lyrics on
lrclib"`.

```js
const lrcPath = await window.__TAURI__.core.invoke('lrc_fetch', {
  path: p, artist: 'A', title: 'T',
});
```

### media_url(path) → string

URL that serves the track's bytes through the `media` protocol with Range
support. Per-OS form (the UI can also build these directly):

- Windows: `http://media.localhost/<pct-encoded path>`
- Linux: `media://localhost/<pct-encoded path>`

The path is percent-encoded (every byte not in `A-Za-z0-9-_.~` is `%XX`;
backslashes included, so pass the string through `encodeURI`-style logic is
NOT needed — the returned URL is ready to assign to `audio.src`).

```js
audio.src = await window.__TAURI__.core.invoke('media_url', { path: p });
```

### cover_url(path) → string

URL of the track's embedded art via the same protocol (`/cover/` route,
404 when the file has none; `Content-Type` from the PICTURE mime).

- Windows: `http://media.localhost/cover/<pct-encoded path>`
- Linux: `media://localhost/cover/<pct-encoded path>`

```js
img.src = await window.__TAURI__.core.invoke('cover_url', { path: p });
```

### pick_folder() → string | null

Native folder picker. Resolves `null` on cancel.

```js
const dir = await window.__TAURI__.core.invoke('pick_folder');
```

### settings_load() → any (JSON value)

Shared settings file (app config dir `settings.json`), shared by both
windows. Returns JSON `null` when never saved.

```js
const s = await window.__TAURI__.core.invoke('settings_load');
```

### settings_save(v) → null (errors as string)

Atomically (tmp + rename) writes the settings file.

```js
await window.__TAURI__.core.invoke('settings_save', { v: { volume: 0.8 } });
```

### smtc_update(title, artist, album, coverUrl, durationS, playing, positionS) → null

Push now-playing state to the OS media surface.

- **Linux**: updates the MPRIS D-Bus player (metadata + playback status +
  position). `coverUrl` should be a `file://` URL for MPRIS `mpris:artUrl`.
- **Windows**: no-op (the UI's `navigator.mediaSession` integration already
  drives SMTC through WebView2).

All arguments required; call it on every track change / play-state change /
seek (Linux MPRIS position only updates when pushed).

```js
await window.__TAURI__.core.invoke('smtc_update', {
  title: 'Song', artist: 'Artist', album: 'Album',
  coverUrl: 'file:///tmp/halftone-covers/123.png',  // or null
  durationS: 213.5, playing: true, positionS: 42.0,
});
```

### smtc_clear() → null

Playback stopped / nothing loaded. Linux: MPRIS → `PlaybackStatus Stopped`.
Windows: no-op.

```js
await window.__TAURI__.core.invoke('smtc_clear');
```

### ota_check() → OtaStatus

Checks the update channel (`HALFTONE_OTA_URL` override, `HALFTONE_NO_OTA`
disable). Network call — don't call on every tick.

### ota_download() → OtaStatus

Downloads the new exe and stages it next to the current one. Check `error` /
`ready` in the returned status.

### ota_apply() → string

Writes the updater helper and relaunches. Resolves only on success (the
process exits during the swap).

---

## Events (backend → UI)

`listen` from `window.__TAURI__.event` (or the `@tauri-apps/api` equivalent).
Payloads are JSON.

### `halftone:scan-progress`

Emitted every 50 parsed files while a scan runs, then once with `"done": true`
(final, includes per-format counts and duration):

```jsonc
// during
{ "found": 150, "total": 900, "folder": "C:\\Music\\A", "done": false }
// final
{
  "found": 900, "skipped": 2,
  "counts": { "flac": 500, "wav": 20, "mp3": 300, "aac": 40, "alac": 25,
              "ogg": 10, "opus": 5, "unsupported": 3 },
  "done": true,
  "ms": 1234
}
```

### `halftone:lib-changed`

The watched folder changed on disk (debounced 700 ms). Payload `{}`. The
owner UI should rescan (`scan_library`) on this.

### `smtc-button`

Transport intent from OUTSIDE the webview (tray menu on Windows, MPRIS
transport buttons on Linux). Payload is one of the strings:

```
"toggle" | "play" | "pause" | "next" | "prev"
```

(`"play"`/`"pause"` come only from MPRIS Play/Pause; the tray Play/Pause item
emits `"toggle"`.)

### `smtc-seek` (Linux MPRIS only)

MPRIS `SetPosition`: payload is the target position in **seconds** (f64).

### `smtc-seek-by` (Linux MPRIS only)

MPRIS `SeekBy`: payload is `[forward: boolean, seconds: f64]`.

---

## Protocol route (`media` scheme)

Serves ORIGINAL file bytes (with HTTP Range) and embedded art. One handler,
registered under the `media` scheme (plus the legacy `flac` alias):

| | URL form |
|---|---|
| Windows (WebView2 resolves custom schemes as http) | `http://media.localhost/<pct-encoded-path>` |
| Linux (webkit2gtk keeps the scheme) | `media://localhost/<pct-encoded-path>` |

Sub-routes:

- `<path>` — original audio bytes. Range support: `Accept-Ranges: bytes`,
  `206` + `Content-Range` for partial requests, `416` for invalid ranges.
  Open-ended ranges (`bytes=N-`) are served but capped at **2 MiB** per
  response (`RANGE_CAP`) — the media element re-requests as it plays; nothing
  ever loads a whole file per request.
- `cover/<path>` — embedded art (full bytes, no Range), `Content-Type` from
  the PICTURE mime (default `image/jpeg`), `Cache-Control: max-age=31536000`,
  LRU-cached (256 entries). Resolution order (see "Cover art" below):
  **user override > embedded art > web-fetched cache > `404`**. Query strings
  (`?v=…` cache busters) are stripped before decoding the path.
- ALAC-in-M4A: served as a decoded-PCM WAV wrapper (`audio/wav`) — the decode
  happens once per file (mtime-keyed single-entry cache), then ranges slice
  the WAV in memory. Other M4A (AAC) and all supported formats serve original
  bytes.

Error status: `404` file missing, `415` unrecognized audio format, `416`
bad range, `500` decode/read failure.

---

## Cover art

Auto-fetch + user overrides for tracks without embedded art. New module:
`src-tauri/src/covers.rs`.

### Storage

- Directory: `app_data_dir()/covers/` — images stored as `<key>.<ext>`
  (`jpg`/`png`/`webp`), `index.json` mapping album keys to entries, written
  atomically. Nothing is ever written next to the music files.
- Album key: FNV-1a hex (same hash as lyrics.rs) of lowercase
  `albumartist-or-artist|album`. When the album tag is missing or
  `Unknown album`, the key falls back to the track path (per-track art).
- `index.json` entry: `{ "source": "user"|"web"|"none"|"miss", "file":
  "<key>.jpg"?, "url": "<source-url>"?, "ts": <unix-seconds> }`
  - `user` — user picked/imported art (wins over everything)
  - `web` — auto-fetched from the web, used only when the file has no
    embedded art
  - `none` — user chose "no art" for this album
  - `miss` — auto-fetch found nothing; retried after **7 days**
    (negative cache, mirrors the lyrics pattern)

### Commands

All camelCase args in JS. Network behavior: 10 s timeout, polite UA
(`Halftone/0.2 (https://github.com/Trapston3/Halftone)`), downloaded bytes
validated by magic (JPEG/PNG/WebP only, ≤ 8 MiB). Sources, in rank order
(exact album+artist match first, case/punctuation-insensitive):

1. iTunes Search API (`artworkUrl100` upscaled to 600×600)
2. MusicBrainz release search → Cover Art Archive front image
   (≤ 1 req/s, globally throttled)

#### `cover_search({artist, album, limit?})` → `CoverCandidate[]`

```jsonc
[{ "url": "https://…600x600bb.jpg", "thumb": "https://…100x100bb.jpg",
   "source": "itunes",  // "itunes" | "musicbrainz"
   "title": "The Dark Side of the Moon", "artist": "Pink Floyd",
   "width": 600, "height": 600 }]   // width/height null for musicbrainz
```

No download happens here. `limit` defaults to 12 (1–30).

#### `cover_apply_url({path, url})` → `string`

Downloads + validates + stores the chosen candidate as the **user override**
for the track's album. An empty `url` records "user chose no art" (`none`).
Returns the new `cover_url` for the track. Throws on download/validation
failure.

#### `cover_import({path, file?})` → `string | null`

`file` omitted → opens a native image picker (jpg/jpeg/png/webp filter) on a
blocking thread. Copies + validates the image as the user override. Returns
the new `cover_url`, or `null` when the picker was cancelled.

#### `cover_reset({path})` → `string`

Removes the override (and any auto-fetched cache) for the track's album —
resolution falls back to embedded art / future auto-fetch. Returns the new
`cover_url`.

#### `cover_auto({path, artist, album, allowNet})` → `CoverAutoResult`

```jsonc
{ "status": "have",     // "have" | "fetched" | "none"
  "url": "http://media.localhost/cover/<pct>?v=1759…",   // null on "none"
  "error": null }       // network errors land here, never a throw
```

- embedded art, an override/cache, or a no-art choice already present →
  `{status:"have", url}` (no network)
- else `allowNet` and no fresh miss → searches, stores the best candidate as
  `web`, returns `{status:"fetched", url}`
- else `{status:"none"}` (+ optional `error`). Concurrent calls for the same
  album are deduped (in-flight set) — the UI may fire it for every row.

#### `cover_info({path})` → `CoverInfo`

```jsonc
{ "source": "user",     // "user" | "embedded" | "web" | "none"
  "url": "http://media.localhost/cover/<pct>?v=1759…" }
```

What the `/cover/` route will currently serve.

### Cache busting

`cover_url` appends `?v=<ts>` (ts = covers index last-change). After any
change (apply/import/reset/auto), re-request the URL — the query change
forces the webview to reload the image. The route strips the query before
decoding the path, so no URL scheme change was needed.

### OTA/compat

New files live only under `app_data_dir()/covers/` — nothing next to the
music, no schema change to the scan payload, `has_cover` unchanged.

---

## Per-OS URL summary (for the UI)

| Thing | Windows | Linux |
|---|---|---|
| Track bytes | `http://media.localhost/<pct>` | `media://localhost/<pct>` |
| Cover art | `http://media.localhost/cover/<pct>` | `media://localhost/cover/<pct>` |
| OS media session | `navigator.mediaSession` (WebView2 native) — do NOT call `smtc_update` on Windows for the OS surface; it's a no-op | `smtc_update` / `smtc_clear` + `smtc-button`/`smtc-seek*` events |

Both commands and events exist on every OS; only the transport underneath
differs. Code the UI once.

## Supported formats (backend)

| format | extension(s) | lossless | Range serving |
|---|---|---|---|
| FLAC | `.flac` | yes | original bytes |
| WAV | `.wav`, `.wave` | yes | original bytes |
| MP3 | `.mp3` | no (VBR via Xing/VBRI, CBR fallback) | original bytes |
| AAC (ADTS) | `.aac` | no | original bytes |
| ALAC in M4A | `.m4a`, `.mp4`, `.alac` | yes | decoded WAV |
| AAC in M4A | `.m4a`, `.mp4` | no | original bytes |
| Ogg Vorbis | `.ogg`, `.oga`, `.oggvorbis` | no | original bytes |
| Ogg Opus | `.opus`, `.ogg`, `.oga` | no | original bytes |

Format detection is by MAGIC BYTES, never extension: extensions only decide
what gets opened at scan time. Files that parse-but-fail land in
`ScanResult.skipped` with the reason.
