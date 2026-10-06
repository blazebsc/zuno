//! Procedural album artwork. Every cover is a pure function of its seed, so
//! the eight frameworks display pixel-identical art without shipping image
//! files or touching the network.
//!
//! Realism matters for the benchmark: cover art is where a music app's memory
//! actually goes (decoded bitmaps), so the cache budget here is part of the
//! measurement. Every framework must use the shared [`ArtworkCache`] rather
//! than its own image store, so all eight pay the same artwork cost and the
//! numbers stay comparable.

use crate::rng::hash;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Curated gradient pairs — muted, album-cover-ish, no two neighbours alike.
const PAIRS: [([u8; 3], [u8; 3]); 14] = [
    ([233, 88, 106], [99, 24, 62]),    // rose
    ([252, 163, 17], [121, 68, 8]),    // amber
    ([46, 196, 182], [10, 77, 86]),    // teal
    ([131, 56, 236], [41, 12, 82]),    // violet
    ([240, 113, 103], [80, 22, 32]),   // coral
    ([67, 97, 238], [17, 27, 88]),     // ultramarine
    ([112, 214, 114], [16, 74, 44]),   // fern
    ([244, 125, 48], [92, 33, 8]),     // tangerine
    ([196, 148, 244], [61, 34, 94]),   // lilac
    ([239, 239, 240], [88, 92, 110]),  // concrete
    ([94, 96, 206], [20, 22, 68]),     // dusk blue
    ([222, 100, 121], [64, 20, 60]),   // raspberry
    ([140, 190, 178], [26, 66, 60]),   // sage
    ([250, 200, 60], [110, 40, 110]),  // gold-plum
];

/// One cover, RGBA, `size × size`, fully opaque.
///
/// Style is picked by the seed: two-tone linear gradient at a seeded angle,
/// one of six geometric overlays, and a soft vignette — abstract art that
/// reads like a real release, not a debug gradient.
pub fn cover(seed: u64, size: u32) -> Vec<u8> {
    let size = size.max(8);
    let h = hash(seed);
    let (c0, c1) = PAIRS[(h % PAIRS.len() as u64) as usize];
    let style = (h >> 8) % 6;
    // Gradient angle: one of 8 directions.
    let angle = ((h >> 12) % 8) as f32 * std::f32::consts::FRAC_PI_4;
    let (dx, dy) = (angle.cos(), angle.sin());
    // Accent: the darker end lightened — the overlay colour.
    let accent = lighten(c1, 0.18);
    let dim = darken(c0, 0.45);

    let s = size as f32;
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 / s, y as f32 / s);
            let t = ((fx - 0.5) * dx + (fy - 0.5) * dy + 0.7071) / 1.4142;
            let base = mix(c0, c1, t.clamp(0.0, 1.0));
            let mut col = match style {
                0 => rings(base, accent, fx, fy, h),
                1 => stripes(base, accent, fx, fy, h),
                2 => disc(base, accent, dim, fx, fy, h),
                3 => dots(base, accent, fx, fy, h),
                4 => wave(base, accent, dim, fx, fy, h),
                _ => bars(base, accent, dim, fx, fy, h),
            };
            // Vignette: darken the frame by up to 22%.
            let d = ((fx - 0.5).powi(2) + (fy - 0.5).powi(2)).sqrt() * 0.62;
            if d > 0.0 {
                col = darken(col, (d * 0.35).min(0.35));
            }
            px.extend_from_slice(&[col[0], col[1], col[2], 255]);
        }
    }
    px
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    [
        (a[0] as f32 * (1.0 - t) + b[0] as f32 * t) as u8,
        (a[1] as f32 * (1.0 - t) + b[1] as f32 * t) as u8,
        (a[2] as f32 * (1.0 - t) + b[2] as f32 * t) as u8,
    ]
}
fn lighten(c: [u8; 3], a: f32) -> [u8; 3] {
    [
        (c[0] as f32 + (255.0 - c[0] as f32) * a) as u8,
        (c[1] as f32 + (255.0 - c[1] as f32) * a) as u8,
        (c[2] as f32 + (255.0 - c[2] as f32) * a) as u8,
    ]
}
fn darken(c: [u8; 3], a: f32) -> [u8; 3] {
    [
        (c[0] as f32 * (1.0 - a)) as u8,
        (c[1] as f32 * (1.0 - a)) as u8,
        (c[2] as f32 * (1.0 - a)) as u8,
    ]
}

fn rings(base: [u8; 3], accent: [u8; 3], fx: f32, fy: f32, h: u64) -> [u8; 3] {
    let cx = 0.3 + (h >> 20) as f32 % 0.4;
    let cy = 0.3 + (h >> 24) as f32 % 0.4;
    let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
    let band = ((d * 14.0).sin() * 0.5 + 0.5) * 0.55;
    mix(base, accent, band)
}

fn stripes(base: [u8; 3], accent: [u8; 3], fx: f32, fy: f32, h: u64) -> [u8; 3] {
    let n = 6.0 + (h >> 20) as f32 % 5.0;
    let diag = (fx + fy) * n * 0.5;
    let s = (diag - diag.floor()) * 0.5;
    mix(base, accent, s)
}

fn disc(base: [u8; 3], accent: [u8; 3], dim: [u8; 3], fx: f32, fy: f32, h: u64) -> [u8; 3] {
    let r = 0.18 + (h >> 28) as f32 % 0.12;
    let cx = 0.5 + (((h >> 32) as f32) % 0.2) - 0.1;
    let d = ((fx - cx).powi(2) + (fy - 0.5).powi(2)).sqrt();
    if d < r {
        mix(accent, dim, (d / r) * 0.4)
    } else if d < r * 1.5 {
        darken(base, 0.12)
    } else {
        base
    }
}

fn dots(base: [u8; 3], accent: [u8; 3], fx: f32, fy: f32, h: u64) -> [u8; 3] {
    let cell = 0.08 + (h >> 22) as f32 % 0.04;
    let gx = (fx / cell).floor();
    let gy = (fy / cell).floor();
    // Pseudo-random dot presence per cell.
    let cell_id = (gx as i64).wrapping_mul(31) as u64 ^ (gy as i64).wrapping_mul(17) as u64 ^ (h >> 16);
    let on = hash(cell_id) % 3 == 0;
    let lx = (fx / cell - gx - 0.5).abs();
    let ly = (fy / cell - gy - 0.5).abs();
    if on && lx.max(ly) < 0.28 {
        accent
    } else {
        base
    }
}

fn wave(base: [u8; 3], accent: [u8; 3], dim: [u8; 3], fx: f32, fy: f32, h: u64) -> [u8; 3] {
    let ph = (h >> 26) as f32 % 6.28;
    let w = (fx * 6.2831 + ph).sin() * 0.5 + 0.5;
    if (fy - w).abs() < 0.03 {
        accent
    } else if fy < w {
        mix(base, dim, 0.25)
    } else {
        base
    }
}

fn bars(base: [u8; 3], accent: [u8; 3], dim: [u8; 3], fx: f32, fy: f32, h: u64) -> [u8; 3] {
    let n = 7.0 + (h >> 18) as f32 % 6.0;
    let col = (fx * n) as u64;
    let height = 0.25 + (hash(col ^ (h >> 12)) % 60) as f32 / 100.0;
    let x_in = (fx * n - col as f32).min(1.0 - (fx * n - col as f32));
    if x_in > 0.2 && fy > 1.0 - height {
        if (fy - (1.0 - height)).abs() < 0.05 {
            accent
        } else {
            mix(dim, accent, 0.3)
        }
    } else {
        base
    }
}

// — The shared cache ——————————————————————————————————————————

/// A fixed-budget decoded-artwork cache, shared by every framework so artwork
/// RAM is identical across the bake-off.
///
/// ponytail: one global cap of 96 decoded covers (~25 MB at 300px). If a
/// production port wants per-view budgets or disk-cached thumbnails, that
/// lives behind this same trait-shaped boundary, not in eight UIs.
pub struct ArtworkCache {
    inner: Mutex<CacheInner>,
}

struct CacheInner {
    map: HashMap<(u64, u32), Arc<Vec<u8>>>,
    order: Vec<(u64, u32)>,
    cap: usize,
}

impl ArtworkCache {
    pub fn new() -> Self {
        Self::with_capacity(96)
    }
    pub fn with_capacity(covers: usize) -> Self {
        ArtworkCache {
            inner: Mutex::new(CacheInner {
                map: HashMap::new(),
                order: Vec::new(),
                cap: covers.max(1),
            }),
        }
    }

    /// The decoded RGBA buffer for a seed, cached. First hit renders.
    pub fn get(&self, seed: u64, size: u32) -> Arc<Vec<u8>> {
        let key = (seed, size);
        let mut g = self.inner.lock().unwrap();
        if let Some(hit) = g.map.get(&key) {
            return hit.clone();
        }
        let buf = Arc::new(cover(seed, size));
        if g.order.len() >= g.cap {
            let evict = g.order.remove(0);
            g.map.remove(&evict);
        }
        g.order.push(key);
        g.map.insert(key, buf.clone());
        buf
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for ArtworkCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_are_deterministic_and_opaque() {
        let a = cover(42, 64);
        let b = cover(42, 64);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64 * 64 * 4);
        assert!(a.chunks_exact(4).all(|p| p[3] == 255));
    }

    #[test]
    fn distinct_seeds_differ() {
        assert_ne!(cover(1, 32), cover(2, 32));
    }

    #[test]
    fn cache_evicts_at_cap() {
        let c = ArtworkCache::with_capacity(4);
        for i in 0..6u64 {
            c.get(i, 16);
        }
        assert_eq!(c.len(), 4);
    }
}
