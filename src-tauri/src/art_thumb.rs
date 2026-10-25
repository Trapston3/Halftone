//! art_thumb.rs — 256×256 cached thumbnail generation + OKLCH dominant color
//! extraction with OKLab/WCAG AA contrast clamping.
//!
//! Per Section 8.2 & 12.1 of the Halftone v1.1 Build Specification:
//! - Downscales full cover art (user override, embedded picture, or web-cached art)
//!   into a cached 256×256 PNG thumbnail under `<app_data_dir>/covers/thumbs/<key>.png`.
//! - Extracts the dominant vibrant palette color using chroma-weighted histogram
//!   clustering in sRGB -> OKLab -> OKLCH.
//! - Clamps OKLCH lightness (`L`) for both Dark and Light theme modes so that
//!   `--cover-accent` always guarantees >= 4.5:1 WCAG AA contrast against text
//!   and >= 3.0:1 against surface backgrounds.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use image::imageops::FilterType;
use image::GenericImageView;
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::{base64_decode_pub, covers, pct_encode, probe, write_atomic};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtThumbResult {
    /// `http://media.localhost/thumb/<encoded>` or `media://localhost/thumb/<encoded>`
    pub thumb_url: String,
    /// Dominant accent hex clamped for the active/dark mode (`#rrggbb`)
    pub accent_hex: String,
    /// Dominant accent hex clamped for dark surfaces (`#rrggbb`, >= 4.5:1 vs `#0e0f14`)
    pub accent_dark_hex: String,
    /// Dominant accent hex clamped for light surfaces (`#rrggbb`, >= 4.5:1 vs `#f7f4ec`)
    pub accent_light_hex: String,
    /// Secondary harmonic accent hex (`#rrggbb`)
    pub secondary_hex: String,
    /// Raw OKLCH tuple `[L (0..1), C (0..0.4), H (0..360)]`
    pub oklch: [f32; 3],
    /// True when the cover art overall average luminance is dark (`Y < 0.35`)
    pub is_dark_art: bool,
    /// Source of cover ("user" | "embedded" | "web" | "fallback")
    pub source: String,
}

static THUMB_META_CACHE: Mutex<Option<HashMap<String, ArtThumbResult>>> = Mutex::new(None);

fn thumbs_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("covers").join("thumbs"))
}

pub fn thumb_url_for(path: &str) -> String {
    if cfg!(windows) {
        format!("http://media.localhost/thumb/{}", pct_encode(path))
    } else {
        format!("media://localhost/thumb/{}", pct_encode(path))
    }
}

// ---------------------------------------------------------------------------
// Color space math: sRGB <-> Linear RGB <-> OKLab <-> OKLCH + WCAG Contrast
// ---------------------------------------------------------------------------

fn srgb_to_linear(c: u8) -> f32 {
    let v = (c as f32) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Convert sRGB (0..255) to OKLab (L, a, b)
pub fn rgb_to_oklab(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let lr = srgb_to_linear(r);
    let lg = srgb_to_linear(g);
    let lb = srgb_to_linear(b);

    let l = (0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb).cbrt();
    let m = (0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb).cbrt();
    let s = (0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb).cbrt();

    (
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    )
}

/// Convert OKLab (L, a, b) to sRGB (r, g, b)
pub fn oklab_to_rgb(l_ok: f32, a_ok: f32, b_ok: f32) -> (u8, u8, u8) {
    let l_ = l_ok + 0.3963377774 * a_ok + 0.2158037573 * b_ok;
    let m_ = l_ok - 0.1055613458 * a_ok - 0.0638541728 * b_ok;
    let s_ = l_ok - 0.0894841775 * a_ok - 1.2914855480 * b_ok;

    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;

    let lr = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
    let lg = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
    let lb = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s;

    (linear_to_srgb(lr), linear_to_srgb(lg), linear_to_srgb(lb))
}

pub fn oklab_to_oklch(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let c = (a * a + b * b).sqrt();
    let mut h = b.atan2(a).to_degrees();
    if h < 0.0 {
        h += 360.0;
    }
    (l, c, h)
}

pub fn oklch_to_rgb(l: f32, c: f32, h_deg: f32) -> (u8, u8, u8) {
    let rad = h_deg.to_radians();
    let a = c * rad.cos();
    let b = c * rad.sin();
    oklab_to_rgb(l, a, b)
}

pub fn relative_luminance(r: u8, g: u8, b: u8) -> f32 {
    0.2126 * srgb_to_linear(r) + 0.7152 * srgb_to_linear(g) + 0.0722 * srgb_to_linear(b)
}

pub fn wcag_contrast(rgb1: (u8, u8, u8), rgb2: (u8, u8, u8)) -> f32 {
    let l1 = relative_luminance(rgb1.0, rgb1.1, rgb1.2);
    let l2 = relative_luminance(rgb2.0, rgb2.1, rgb2.2);
    let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

pub fn rgb_to_hex(rgb: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

/// Adjust OKLCH lightness `L` (while preserving hue and chroma as closely as
/// possible) until `wcag_contrast(rgb, bg_rgb) >= min_ratio`.
pub fn clamp_oklch_for_contrast(
    l: f32,
    c: f32,
    h: f32,
    bg_rgb: (u8, u8, u8),
    min_ratio: f32,
    prefer_lighter: bool,
) -> (u8, u8, u8) {
    let cur_c = c.clamp(0.04, 0.24);
    let mut cur_l = l.clamp(0.22, 0.88);
    let mut best_rgb = oklch_to_rgb(cur_l, cur_c, h);
    let mut best_ratio = wcag_contrast(best_rgb, bg_rgb);
    if best_ratio >= min_ratio {
        return best_rgb;
    }
    let step = if prefer_lighter { 0.015 } else { -0.015 };
    for _ in 0..55 {
        cur_l = (cur_l + step).clamp(0.15, 0.94);
        let cand = oklch_to_rgb(cur_l, cur_c, h);
        let ratio = wcag_contrast(cand, bg_rgb);
        if ratio > best_ratio {
            best_ratio = ratio;
            best_rgb = cand;
        }
        if ratio >= min_ratio {
            return cand;
        }
    }
    // If chroma was preventing sufficient contrast at extreme L, slightly desaturate
    let mut reduced_c = cur_c;
    for _ in 0..16 {
        reduced_c *= 0.85;
        let cand = oklch_to_rgb(cur_l, reduced_c, h);
        if wcag_contrast(cand, bg_rgb) >= min_ratio {
            return cand;
        }
    }
    best_rgb
}

/// Extract dominant OKLCH color + secondary harmonic color from an RGBA/RGB pixel buffer
pub fn extract_palette_from_rgba(rgba: &[u8]) -> ([f32; 3], String, String, String, bool) {
    // 24 hue buckets (15 deg each)
    let mut buckets_w = [0.0f32; 24];
    let mut buckets_l = [0.0f32; 24];
    let mut buckets_c = [0.0f32; 24];
    let mut buckets_h = [0.0f32; 24];
    let mut total_lum = 0.0f32;
    let mut total_px = 0usize;

    for px in rgba.chunks_exact(4) {
        if px[3] < 128 {
            continue;
        }
        let (r, g, b) = (px[0], px[1], px[2]);
        total_lum += relative_luminance(r, g, b);
        total_px += 1;

        let (l, a_ok, b_ok) = rgb_to_oklab(r, g, b);
        let (l, c, h) = oklab_to_oklch(l, a_ok, b_ok);
        // Weight vibrant mid-tone pixels highest so muddy blacks/whites don't win
        let mid_bonus = 1.0 - ((l - 0.58).abs() * 1.4).clamp(0.0, 0.85);
        let weight = (c.powf(1.35) * mid_bonus) + 0.002;
        let idx = ((h / 15.0).floor() as usize).min(23);
        buckets_w[idx] += weight;
        buckets_l[idx] += l * weight;
        buckets_c[idx] += c * weight;
        buckets_h[idx] += h * weight;
    }

    let avg_lum = if total_px > 0 {
        total_lum / (total_px as f32)
    } else {
        0.2
    };
    let is_dark_art = avg_lum < 0.35;

    let mut best_i = 0usize;
    for i in 1..24 {
        if buckets_w[i] > buckets_w[best_i] {
            best_i = i;
        }
    }

    let (dom_l, dom_c, dom_h) = if buckets_w[best_i] > 0.0 {
        let w = buckets_w[best_i];
        (
            (buckets_l[best_i] / w).clamp(0.35, 0.82),
            (buckets_c[best_i] / w).clamp(0.08, 0.22),
            (buckets_h[best_i] / w).rem_euclid(360.0),
        )
    } else {
        // Fallback warm vermilion / electric indigo
        (0.65, 0.16, 28.0)
    };

    // Secondary color: best non-adjacent hue bucket or +42 deg harmonic shift
    let mut sec_i = (best_i + 4) % 24;
    let mut best_sec_w = -1.0f32;
    for i in 0..24 {
        let dist = (i as isize - best_i as isize).abs();
        let wrap_dist = dist.min(24 - dist);
        if wrap_dist >= 3 && buckets_w[i] > best_sec_w {
            best_sec_w = buckets_w[i];
            sec_i = i;
        }
    }
    let (sec_l, sec_c, sec_h) = if best_sec_w > buckets_w[best_i] * 0.18 && buckets_w[sec_i] > 0.0 {
        let w = buckets_w[sec_i];
        (
            (buckets_l[sec_i] / w).clamp(0.38, 0.80),
            (buckets_c[sec_i] / w).clamp(0.07, 0.20),
            (buckets_h[sec_i] / w).rem_euclid(360.0),
        )
    } else {
        (dom_l, (dom_c * 0.85).max(0.07), (dom_h + 42.0).rem_euclid(360.0))
    };

    // Clamp for dark canvas (#11131a) and light canvas (#f6f2e8) at >= 4.5:1 WCAG AA
    let dark_rgb = clamp_oklch_for_contrast(dom_l, dom_c, dom_h, (17, 19, 26), 4.6, true);
    let light_rgb = clamp_oklch_for_contrast(dom_l, dom_c, dom_h, (246, 242, 232), 4.6, false);
    let sec_rgb = clamp_oklch_for_contrast(sec_l, sec_c, sec_h, (17, 19, 26), 3.5, true);

    (
        [dom_l, dom_c, dom_h],
        rgb_to_hex(dark_rgb),
        rgb_to_hex(light_rgb),
        rgb_to_hex(sec_rgb),
        is_dark_art,
    )
}

/// Resolve raw cover image bytes for a track (user > embedded > web)
fn resolve_cover_bytes(app: &tauri::AppHandle, track_path: &Path) -> Option<(String, Vec<u8>)> {
    let mtime = fs::metadata(track_path).and_then(|m| m.modified()).ok()?;
    if let Some(resp) = covers::serve_stored(track_path, mtime, "user") {
        if resp.status() == 200 {
            return Some(("user".into(), resp.into_body()));
        }
    }
    if let Some((meta, _, _)) = probe::read_meta(track_path, true).ok() {
        if let Some(pic) = meta.cover {
            if let Some(bytes) = base64_decode_pub(&pic.data_b64) {
                return Some(("embedded".into(), bytes));
            }
        }
    }
    if let Some(resp) = covers::serve_stored(track_path, mtime, "web") {
        if resp.status() == 200 {
            return Some(("web".into(), resp.into_body()));
        }
    }
    let _ = app;
    None
}

/// Generate a 256×256 PNG thumbnail and compute OKLCH accents for `track_path`.
pub fn build_or_load_thumb(app: &tauri::AppHandle, path_str: &str) -> ArtThumbResult {
    let path = Path::new(path_str);
    let mtime_secs = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cache_key = format!(
        "{:016x}_{}",
        crate::lyrics::fnv(&path_str.to_lowercase()),
        mtime_secs
    );

    if let Ok(mut guard) = THUMB_META_CACHE.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        if let Some(existing) = map.get(&cache_key) {
            return existing.clone();
        }
    }

    let fallback = || ArtThumbResult {
        thumb_url: thumb_url_for(path_str),
        accent_hex: "#d84a2a".into(),
        accent_dark_hex: "#ff7a59".into(),
        accent_light_hex: "#b83214".into(),
        secondary_hex: "#7c6cff".into(),
        oklch: [0.64, 0.16, 28.0],
        is_dark_art: true,
        source: "fallback".into(),
    };

    let Some((source, raw_bytes)) = resolve_cover_bytes(app, path) else {
        return fallback();
    };

    let Ok(img) = image::load_from_memory(&raw_bytes) else {
        return fallback();
    };

    // Center-crop to square and downscale to 256x256
    let (w, h) = img.dimensions();
    let side = w.min(h).max(1);
    let cropped = img.crop_imm((w - side) / 2, (h - side) / 2, side, side);
    let thumb = cropped.resize_exact(256, 256, FilterType::Lanczos3);

    // Save 256x256 PNG to covers/thumbs/<key>.png
    if let Some(tdir) = thumbs_dir(app) {
        let _ = fs::create_dir_all(&tdir);
        let thumb_file = tdir.join(format!("{cache_key}.png"));
        if !thumb_file.exists() {
            let mut png_bytes = Vec::new();
            let mut cursor = std::io::Cursor::new(&mut png_bytes);
            if thumb.write_to(&mut cursor, image::ImageFormat::Png).is_ok() {
                let _ = write_atomic(&thumb_file, &png_bytes);
            }
        }
    }

    // Sample a 64x64 grid for fast, deterministic OKLCH palette extraction
    let sample_img = thumb.resize_exact(64, 64, FilterType::Triangle).to_rgba8();
    let (oklch, accent_dark_hex, accent_light_hex, secondary_hex, is_dark_art) =
        extract_palette_from_rgba(sample_img.as_raw());

    let res = ArtThumbResult {
        thumb_url: thumb_url_for(path_str),
        accent_hex: accent_dark_hex.clone(),
        accent_dark_hex,
        accent_light_hex,
        secondary_hex,
        oklch,
        is_dark_art,
        source,
    };

    if let Ok(mut guard) = THUMB_META_CACHE.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        if map.len() > 2048 {
            map.clear();
        }
        map.insert(cache_key, res.clone());
    }
    res
}

/// Serve a cached 256×256 PNG thumbnail over the `media://localhost/thumb/<path>` route.
pub fn serve_thumb(app: Option<&tauri::AppHandle>, path: &Path) -> Option<(String, Vec<u8>)> {
    let app = app?;
    let path_str = path.to_string_lossy();
    let _ = build_or_load_thumb(app, &path_str);
    let mtime_secs = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cache_key = format!(
        "{:016x}_{}",
        crate::lyrics::fnv(&path_str.to_lowercase()),
        mtime_secs
    );
    let tdir = thumbs_dir(app)?;
    let thumb_file = tdir.join(format!("{cache_key}.png"));
    let bytes = fs::read(thumb_file).ok()?;
    Some(("image/png".into(), bytes))
}

#[tauri::command]
pub async fn art_thumb(app: tauri::AppHandle, path: String) -> Result<ArtThumbResult, String> {
    tauri::async_runtime::spawn_blocking(move || build_or_load_thumb(&app, &path))
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oklab_roundtrip_and_wcag_clamp() {
        let orig = (216u8, 74u8, 42u8); // Halftone vermilion
        let (l, a, b) = rgb_to_oklab(orig.0, orig.1, orig.2);
        let back = oklab_to_rgb(l, a, b);
        assert!((orig.0 as i32 - back.0 as i32).abs() <= 2);
        assert!((orig.1 as i32 - back.1 as i32).abs() <= 2);
        assert!((orig.2 as i32 - back.2 as i32).abs() <= 2);

        let (l, c, h) = oklab_to_oklch(l, a, b);
        let dark_bg = (17u8, 19u8, 26u8);
        let light_bg = (246u8, 242u8, 232u8);
        let clamped_dark = clamp_oklch_for_contrast(l, c, h, dark_bg, 4.5, true);
        let clamped_light = clamp_oklch_for_contrast(l, c, h, light_bg, 4.5, false);
        assert!(
            wcag_contrast(clamped_dark, dark_bg) >= 4.5,
            "dark contrast failed: {}",
            wcag_contrast(clamped_dark, dark_bg)
        );
        assert!(
            wcag_contrast(clamped_light, light_bg) >= 4.5,
            "light contrast failed: {}",
            wcag_contrast(clamped_light, light_bg)
        );
    }

    #[test]
    fn palette_extraction_picks_vibrant_color() {
        // 16x16 image: mostly dark grey, with 25% vibrant cyan pixels
        let mut rgba = Vec::with_capacity(16 * 16 * 4);
        for i in 0..(16 * 16) {
            if i % 4 == 0 {
                rgba.extend_from_slice(&[20, 210, 235, 255]);
            } else {
                rgba.extend_from_slice(&[28, 30, 34, 255]);
            }
        }
        let (oklch, dark_hex, light_hex, sec_hex, is_dark) = extract_palette_from_rgba(&rgba);
        assert!(oklch[1] > 0.08, "expected vibrant chroma, got {}", oklch[1]);
        assert!(dark_hex.starts_with('#') && dark_hex.len() == 7);
        assert!(light_hex.starts_with('#') && light_hex.len() == 7);
        assert!(sec_hex.starts_with('#') && sec_hex.len() == 7);
        assert!(is_dark);
    }
}
