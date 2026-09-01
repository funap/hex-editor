//! Shannon entropy calculation and colormap utilities for binary data analysis.

use std::cmp;
use std::ops::Range;
use std::sync::LazyLock;

/// Thermal heatmap colormap stops from Low (0.0) to High (1.0).
/// Each stop is `(normalized_entropy, [R, G, B])`.
const COLOR_STOPS: &[(f32, [u8; 3])] = &[
    (0.00, [15, 23, 75]),   // Deep Navy Blue (0.0 bits - uniform / null / padding)
    (0.15, [20, 65, 150]),  // Blue (1.2 bits - very low entropy)
    (0.30, [10, 135, 175]), // Cyan / Teal (2.4 bits - low entropy)
    (0.50, [30, 175, 75]),  // Emerald Green (4.0 bits - medium-low structured)
    (0.70, [235, 185, 15]), // Gold / Yellow (5.6 bits - medium-high code & text)
    (0.85, [245, 115, 20]), // Bright Orange (6.8 bits - high entropy code blocks)
    (1.00, [235, 30, 40]),  // Vivid Crimson Red (8.0 bits - packed / encrypted / random)
];

fn interpolate_stops(normalized: f32) -> [u8; 3] {
    let t = normalized.clamp(0.0, 1.0);
    for i in 0..COLOR_STOPS.len() - 1 {
        let (t0, c0) = COLOR_STOPS[i];
        let (t1, c1) = COLOR_STOPS[i + 1];
        if t <= t1 || i == COLOR_STOPS.len() - 2 {
            let factor = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            let r = (c0[0] as f32 + factor * (c1[0] as f32 - c0[0] as f32)).round() as u8;
            let g = (c0[1] as f32 + factor * (c1[1] as f32 - c0[1] as f32)).round() as u8;
            let b = (c0[2] as f32 + factor * (c1[2] as f32 - c0[2] as f32)).round() as u8;
            return [r, g, b];
        }
    }
    COLOR_STOPS[COLOR_STOPS.len() - 1].1
}

static ENTROPY_RGBA_LUT: LazyLock<[[u8; 4]; 256]> = LazyLock::new(|| {
    let mut lut = [[0u8; 4]; 256];
    for (i, entry) in lut.iter_mut().enumerate() {
        let t = i as f32 / 255.0;
        *entry = entropy_to_rgba(t);
    }
    lut
});

static ENTROPY_BGRA_LUT: LazyLock<[[u8; 4]; 256]> = LazyLock::new(|| {
    let mut lut = [[0u8; 4]; 256];
    for (i, entry) in lut.iter_mut().enumerate() {
        let t = i as f32 / 255.0;
        *entry = entropy_to_bgra(t);
    }
    lut
});

/// Returns a precomputed 256-entry RGBA LUT for UI elements [0..=255].
pub fn entropy_lut() -> &'static [[u8; 4]; 256] {
    &ENTROPY_RGBA_LUT
}

/// Returns a precomputed 256-entry BGRA LUT for GPU image textures [0..=255].
pub fn entropy_bgra_lut() -> &'static [[u8; 4]; 256] {
    &ENTROPY_BGRA_LUT
}

/// Converts a normalized entropy value in `[0.0, 1.0]` into an index `0..=255` for LUT lookup.
#[inline(always)]
pub fn normalized_to_lut_index(normalized: f32) -> usize {
    (normalized * 255.0).round().clamp(0.0, 255.0) as usize
}

/// Maps a normalized entropy value in `[0.0, 1.0]` to RGBA `[r, g, b, a]` (for UI styling).
pub fn entropy_to_rgba(normalized: f32) -> [u8; 4] {
    let [r, g, b] = interpolate_stops(normalized);
    [r, g, b, 255]
}

/// Maps a normalized entropy value in `[0.0, 1.0]` to BGRA `[b, g, r, a]` (for GPUI / GPU textures).
pub fn entropy_to_bgra(normalized: f32) -> [u8; 4] {
    let [r, g, b] = interpolate_stops(normalized);
    [b, g, r, 255]
}

/// Returns a human-friendly label for a Shannon entropy value (in bits per byte).
pub fn entropy_level_label(entropy_bits: f64) -> &'static str {
    match entropy_bits {
        h if h < 2.0 => "Uniform (Low)",
        h if h < 4.5 => "Structured (Mid-Low)",
        h if h < 6.5 => "Code/Text (Mid)",
        h if h < 7.5 => "High Density",
        _ => "Packed/Encrypted (High)",
    }
}

#[inline(always)]
fn f_term(c: u32) -> f64 {
    if c == 0 { 0.0 } else { c as f64 * (c as f64).log2() }
}

/// Computes the Shannon entropy in bits per byte `[0.0, 8.0]` for an arbitrary slice of bytes.
pub fn shannon_entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }

    let mut counts = [0u32; 256];
    for &b in data {
        counts[b as usize] += 1;
    }

    let n = data.len() as f64;
    let log2_n = n.log2();
    let mut sum_f = 0.0;
    for &c in &counts {
        if c > 0 {
            sum_f += f_term(c);
        }
    }

    (log2_n - sum_f / n).clamp(0.0, 8.0)
}

/// Computes the normalized Shannon entropy in `[0.0, 1.0]` for an arbitrary slice of bytes.
pub fn shannon_entropy_normalized(data: &[u8]) -> f32 {
    (shannon_entropy(data) / 8.0).clamp(0.0, 1.0) as f32
}

/// Returns the window boundaries `start..end` for an `offset`, given `window_size` and `total_len`.
///
/// Ensures the window is centered on `offset` whenever possible, clamping to `0` at the start
/// and `total_len` at the end while preserving a constant window size `min(window_size, total_len)`.
pub fn entropy_window_range(offset: usize, window_size: usize, total_len: usize) -> Range<usize> {
    if total_len == 0 {
        return 0..0;
    }
    let win = window_size.max(1);
    if total_len <= win {
        return 0..total_len;
    }

    let half = win / 2;
    let start = if offset < half {
        0
    } else if offset + (win - half) >= total_len {
        total_len.saturating_sub(win)
    } else {
        offset - half
    };
    let end = cmp::min(start + win, total_len);
    start..end
}

/// Computes the Shannon entropy at a specific byte offset within `data` using a window of size `window_size`.
pub fn shannon_entropy_at(data: &[u8], offset: usize, window_size: usize) -> f64 {
    let range = entropy_window_range(offset, window_size, data.len());
    shannon_entropy(&data[range])
}

/// Computes sliding-window Shannon entropy for a subrange `start_offset..end_offset` of `data`.
///
/// Returns a `Vec<f32>` containing the normalized entropy `[0.0, 1.0]` for every byte in `start_offset..end_offset`.
///
/// Runs in amortized $O(1)$ per byte by maintaining frequency counts and only updating when the
/// window edges advance.
pub fn compute_sliding_entropy(data: &[u8], start_offset: usize, end_offset: usize, window_size: usize) -> Vec<f32> {
    let total_len = data.len();
    if total_len == 0 || start_offset >= end_offset || start_offset >= total_len {
        return Vec::new();
    }

    let end_offset = end_offset.min(total_len);
    let count = end_offset - start_offset;
    let win = window_size.max(1);

    // If buffer is smaller than window, the window is the entire buffer for all positions.
    if total_len <= win {
        let ent = shannon_entropy_normalized(data);
        return vec![ent; count];
    }

    let mut result = Vec::with_capacity(count);

    // Initialize counts for the first position
    let initial_range = entropy_window_range(start_offset, win, total_len);
    let mut counts = [0u32; 256];
    for &b in &data[initial_range.clone()] {
        counts[b as usize] += 1;
    }

    let mut sum_f = 0.0;
    for &c in &counts {
        if c > 0 {
            sum_f += f_term(c);
        }
    }

    let mut cur_start = initial_range.start;
    let mut cur_end = initial_range.end;
    let w_len = cur_end - cur_start;
    let log2_w = (w_len as f64).log2();
    let inv_w = 1.0 / (w_len as f64);

    for i in start_offset..end_offset {
        let target = entropy_window_range(i, win, total_len);

        // Advance left edge
        while cur_start < target.start {
            let b = data[cur_start] as usize;
            sum_f -= f_term(counts[b]);
            counts[b] -= 1;
            sum_f += f_term(counts[b]);
            cur_start += 1;
        }

        // Advance right edge
        while cur_end < target.end {
            let b = data[cur_end] as usize;
            sum_f -= f_term(counts[b]);
            counts[b] += 1;
            sum_f += f_term(counts[b]);
            cur_end += 1;
        }

        let h = (log2_w - sum_f * inv_w).clamp(0.0, 8.0);
        result.push((h / 8.0) as f32);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_and_uniform() {
        assert_eq!(shannon_entropy(&[]), 0.0);
        assert_eq!(shannon_entropy_normalized(&[]), 0.0);

        let zeroes = vec![0u8; 256];
        assert_eq!(shannon_entropy(&zeroes), 0.0);
        assert_eq!(shannon_entropy_normalized(&zeroes), 0.0);

        let ones = vec![0xFFu8; 100];
        assert_eq!(shannon_entropy(&ones), 0.0);
    }

    #[test]
    fn test_two_symbols_half() {
        let mut data = vec![0u8; 128];
        data.extend(vec![1u8; 128]);
        let h = shannon_entropy(&data);
        assert!((h - 1.0).abs() < 1e-6);
        assert!((shannon_entropy_normalized(&data) - 0.125).abs() < 1e-6);
    }

    #[test]
    fn test_full_range_maximum_entropy() {
        let data: Vec<u8> = (0..=255).collect();
        let h = shannon_entropy(&data);
        assert!((h - 8.0).abs() < 1e-6);
        assert!((shannon_entropy_normalized(&data) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_window_range_boundaries() {
        assert_eq!(entropy_window_range(0, 10, 0), 0..0);
        assert_eq!(entropy_window_range(5, 50, 20), 0..20);

        // total = 100, win = 10, half = 5
        assert_eq!(entropy_window_range(0, 10, 100), 0..10);
        assert_eq!(entropy_window_range(2, 10, 100), 0..10);
        assert_eq!(entropy_window_range(5, 10, 100), 0..10);
        assert_eq!(entropy_window_range(6, 10, 100), 1..11);
        assert_eq!(entropy_window_range(50, 10, 100), 45..55);
        assert_eq!(entropy_window_range(95, 10, 100), 90..100);
        assert_eq!(entropy_window_range(99, 10, 100), 90..100);
    }

    #[test]
    fn test_sliding_entropy_matches_pointwise() {
        // Create varied synthetic buffer
        let mut data = Vec::new();
        data.extend(vec![0u8; 50]); // zero entropy
        data.extend(0..=255); // high entropy
        data.extend(b"Hello world, this is typical ASCII english text with some repetitions.");
        data.extend(vec![0xAA; 80]);

        let window_size = 32;
        let sliding = compute_sliding_entropy(&data, 0, data.len(), window_size);
        assert_eq!(sliding.len(), data.len());

        for (i, &sl_val) in sliding.iter().enumerate() {
            let point_val = (shannon_entropy_at(&data, i, window_size) / 8.0) as f32;
            assert!((sl_val - point_val).abs() < 1e-4, "Mismatch at {}: sliding={}, point={}", i, sl_val, point_val);
        }
    }

    #[test]
    fn test_sliding_entropy_subrange() {
        let data: Vec<u8> = (0..500).map(|i| (i % 256) as u8).collect();
        let full = compute_sliding_entropy(&data, 0, data.len(), 64);
        let sub = compute_sliding_entropy(&data, 100, 250, 64);

        assert_eq!(sub.len(), 150);
        for i in 0..150 {
            assert!((sub[i] - full[100 + i]).abs() < 1e-5);
        }
    }

    #[test]
    fn test_colormap_lut() {
        let rgba_lut = entropy_lut();
        let bgra_lut = entropy_bgra_lut();
        assert_eq!(rgba_lut.len(), 256);
        assert_eq!(bgra_lut.len(), 256);

        // Alpha must always be 255
        for &color in rgba_lut {
            assert_eq!(color[3], 255);
        }
        for &color in bgra_lut {
            assert_eq!(color[3], 255);
        }

        // Low entropy (0) in RGBA: R=15, G=23, B=75 (Deep Navy Blue)
        let c_low_rgba = rgba_lut[0];
        assert_eq!(c_low_rgba[0], 15);
        assert_eq!(c_low_rgba[1], 23);
        assert_eq!(c_low_rgba[2], 75);

        // Low entropy (0) in BGRA: B=75, G=23, R=15
        let c_low_bgra = bgra_lut[0];
        assert_eq!(c_low_bgra[0], 75);
        assert_eq!(c_low_bgra[1], 23);
        assert_eq!(c_low_bgra[2], 15);

        // High entropy (255) in RGBA: R=235, G=30, B=40 (Crimson Red)
        let c_high_rgba = rgba_lut[255];
        assert_eq!(c_high_rgba[0], 235);
        assert_eq!(c_high_rgba[1], 30);
        assert_eq!(c_high_rgba[2], 40);

        // High entropy (255) in BGRA: B=40, G=30, R=235
        let c_high_bgra = bgra_lut[255];
        assert_eq!(c_high_bgra[0], 40);
        assert_eq!(c_high_bgra[1], 30);
        assert_eq!(c_high_bgra[2], 235);
    }
}
