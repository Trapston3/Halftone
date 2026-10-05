// alac.rs — ALAC-in-M4A lossless decode to WAV via symphonia.
//
// Scope (Phase 1): ALAC audio stored in an MP4/M4A container ONLY. The MP4
// demux and ALAC decode are symphonia's (no hand-rolled decoder); this module
// is the bounded glue: in-memory MediaSource, explicit 512 MiB output cap,
// narrowing to the source bit depth, and a canonical 44-byte-header RIFF/WAVE
// writer.
//
// symphonia's ALAC decoder always yields an AudioBuffer<i32>; the stream's
// real depth comes from codec_params.bits_per_sample. For byte-exact output
// (compared against ffmpeg in tests) we narrow the i32 samples back to the
// source depth: i16 (>>16), i24 (>>8, packed LE 3 bytes), i32 (as-is).
//
// The output WAV (header + little-endian interleaved PCM) is byte-compared in
// tests against ffmpeg's independent decoder, so any asymmetry fails CI.

use symphonia::core::audio::{SampleBuffer, SignalSpec};
use symphonia::core::codecs::{CodecType, DecoderOptions, CODEC_TYPE_ALAC, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSource;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::sample::i24;

/// Hard cap on the decoded output (header + PCM bytes). A lossless
/// 192 kHz / 24-bit / 8-channel hour-long file would be ~3.3 GB; the cap
/// turns that into an explicit error instead of an OOM.
pub const MAX_OUTPUT_BYTES: u64 = 512 * 1024 * 1024;

/// In-memory, seekable MediaSource over the caller's bytes.
struct CursorSource {
    data: Vec<u8>,
    pos: u64,
}

impl MediaSource for CursorSource {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.data.len() as u64)
    }
}

impl std::io::Read for CursorSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let avail = self.data.len() as u64 - self.pos;
        let n = (avail as usize).min(buf.len());
        buf[..n].copy_from_slice(&self.data[self.pos as usize..self.pos as usize + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl std::io::Seek for CursorSource {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        let len = self.data.len() as i64;
        let new_pos = match pos {
            std::io::SeekFrom::Start(s) => s as i64,
            std::io::SeekFrom::End(e) => len + e,
            std::io::SeekFrom::Current(d) => self.pos as i64 + d,
        };
        if new_pos < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek before start",
            ));
        }
        self.pos = new_pos as u64;
        Ok(self.pos)
    }
}

/// Decode an ALAC-in-M4A file into a complete RIFF/WAVE file (44-byte header
/// + interleaved little-endian integer PCM at the source rate/depth).
///
/// `Err` for non-ALAC codecs, undecodable input, or when the decoded output
/// would exceed the 512 MiB limit. Never panics.
pub fn decode(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if bytes.is_empty() {
        return Err("alac: empty input".into());
    }
    let mss = symphonia::core::io::MediaSourceStream::new(
        Box::new(CursorSource { data: bytes, pos: 0 }),
        symphonia::core::io::MediaSourceStreamOptions::default(),
    );

    // isomp4 is the only container feature enabled; the hint is belt & braces.
    let mut hint = Hint::new();
    hint.with_extension("m4a");

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("alac: cannot demux (is this ALAC-in-M4A?): {e}"))?;

    let mut format_reader = probed.format;

    // First track with a real codec; require it to be ALAC.
    let track = format_reader
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .cloned()
        .ok_or_else(|| "alac: no audio track in container".to_string())?;
    if track.codec_params.codec != CODEC_TYPE_ALAC {
        return Err(format!(
            "alac: container codec is not ALAC — refusing to decode (codec id {:?})",
            track.codec_params.codec
        ));
    }

    // Source bit depth (symphonia's ALAC output buffer is always i32).
    let bits_per_sample: u8 = match track.codec_params.bits_per_sample {
        Some(8) => 8,
        Some(16) => 16,
        Some(20) | Some(24) => 24, // 20-bit ALAC ships in 24-bit containers
        Some(32) => 32,
        Some(other) => {
            return Err(format!("alac: unsupported bit depth {other}"));
        }
        None => 16, // ALAC default when absent (16-bit files)
    };

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("alac: cannot create ALAC decoder: {e}"))?;

    let track_id = track.id;
    let mut pcm_out: Vec<u8> = Vec::new();
    let mut spec_seen: Option<SignalSpec> = None;

    loop {
        let packet = match format_reader.next_packet() {
            Ok(p) => p,
            // symphonia signals clean end-of-stream as an UnexpectedEof IoError
            Err(SymphoniaError::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(SymphoniaError::ResetRequired) => {
                return Err("alac: mid-stream reset required (unsupported edit list)".into())
            }
            Err(e) => return Err(format!("alac: demux error: {e}")),
        };
        if packet.track_id() != track_id {
            continue; // ignore non-audio / secondary tracks
        }

        let decoded = match decoder.decode(&packet) {
            Ok(r) => r,
            Err(SymphoniaError::DecodeError(_)) => continue, // skip bad packet
            Err(e) => return Err(format!("alac: decode error: {e}")),
        };

        let spec = *decoded.spec();
        let n_frames = decoded.frames() as u64;
        if n_frames == 0 {
            continue;
        }
        spec_seen = Some(spec);

        // Symphonia hands ALAC back as i32 in every case; copy into an
        // interleaved raw buffer, then narrow to the source depth.
        let mut sb = SampleBuffer::<i32>::new(n_frames, spec);
        sb.copy_interleaved_ref(decoded);

        let converted: Vec<u8> = match bits_per_sample {
            8 => sb.samples().iter().map(|&s| (s >> 24) as i8).collect::<Vec<i8>>()
                .iter().flat_map(|v| v.to_le_bytes()).collect(),
            16 => sb.samples().iter().map(|&s| (s >> 16) as i16)
                .flat_map(|v| v.to_le_bytes()).collect(),
            24 => sb.samples().iter().map(|&s| {
                let v = (s >> 8) as i32; // keep sign; take low 3 bytes LE
                [v as u8, (v >> 8) as u8, (v >> 16) as u8]
            }).flatten().collect(),
            _ => sb.samples().iter().flat_map(|v| v.to_le_bytes()).collect(), // 32-bit
        };

        if (pcm_out.len() + converted.len()) as u64 > MAX_OUTPUT_BYTES {
            return Err(format!(
                "alac: decoded PCM would exceed the 512 MiB memory limit (at {} bytes)",
                pcm_out.len() + converted.len()
            ));
        }
        pcm_out.extend_from_slice(&converted);
    }

    let spec = spec_seen.ok_or_else(|| "alac: no audio decoded (zero packets)".to_string())?;
    let sample_rate = spec.rate;
    let channels = spec.channels.count() as u8;
    if sample_rate == 0 || channels == 0 {
        return Err("alac: invalid stream parameters".into());
    }
    if pcm_out.is_empty() {
        return Err("alac: no samples decoded".into());
    }

    Ok(build_wav(&pcm_out, sample_rate, channels, bits_per_sample))
}

/// Canonical little-endian RIFF/WAVE writer (44-byte canonical header).
pub(crate) fn build_wav(pcm: &[u8], sample_rate: u32, channels: u8, bits: u8) -> Vec<u8> {
    let bytes_per_sample = (bits / 8) as usize;
    let block_align = channels as usize * bytes_per_sample;
    let data_len = pcm.len() as u32;
    let riff_len = 4 + 8 + 16 + 8 + data_len;

    let mut out = Vec::with_capacity(8 + riff_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_len.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&(channels as u16).to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * block_align as u32).to_le_bytes()); // byte rate
    out.extend_from_slice(&(block_align as u16).to_le_bytes());
    out.extend_from_slice(&(bits as u16).to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(pcm);
    out
}
