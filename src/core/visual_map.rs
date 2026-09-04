//! Pure data and rendering logic for visual map / byte distribution visualizations.
//!
//! Provides color categorization, color map LUT generation, and pixel buffer
//! rendering independent of any GUI framework.

use crate::core::color::RgbaColor;
use serde::{Deserialize, Serialize};
use std::cmp;

/// Visual display color modes for byte map rendering.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum VisualMapColorMode {
    Grayscale,
    DataCategory,
    Rainbow,
    Entropy,
}

/// Categorization of byte values into semantic character and data groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ByteCategory {
    Null,
    Control,
    Space,
    Ascii,
    Extended,
}

impl ByteCategory {
    /// Categorizes a single byte.
    #[inline]
    pub fn of(byte: u8) -> Self {
        match byte {
            0 => ByteCategory::Null,
            1..=31 | 127 => ByteCategory::Control,
            32 => ByteCategory::Space,
            33..=126 => ByteCategory::Ascii,
            _ => ByteCategory::Extended,
        }
    }

    /// Returns human-readable label for this category.
    pub fn label(self) -> &'static str {
        match self {
            ByteCategory::Null => "Null (00)",
            ByteCategory::Control => "Control",
            ByteCategory::Space => "Space (20)",
            ByteCategory::Ascii => "ASCII",
            ByteCategory::Extended => "Extended",
        }
    }
}

/// Color palette (in BGRA `[b, g, r, a]` format) for the 5 byte categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CategoryPalette {
    pub null: [u8; 4],
    pub control: [u8; 4],
    pub space: [u8; 4],
    pub ascii: [u8; 4],
    pub extended: [u8; 4],
}

impl Default for CategoryPalette {
    fn default() -> Self {
        Self {
            null: [120, 120, 120, 46],      // muted (dim)
            control: [40, 40, 220, 191],    // red-ish
            space: [220, 140, 40, 140],     // blue-ish
            ascii: [40, 200, 40, 217],      // green-ish
            extended: [200, 100, 180, 204], // accent/purple
        }
    }
}

/// Generates a 256-entry BGRA lookup table for `VisualMapColorMode::Grayscale`.
pub fn grayscale_bgra_lut() -> [[u8; 4]; 256] {
    let mut lut = [[0u8; 4]; 256];
    for byte in 0..=255 {
        let val = byte as f32 / 255.0;
        let lum = ((val * 0.8 + 0.1) * 255.0).clamp(0.0, 255.0) as u8;
        lut[byte as usize] = [lum, lum, lum, 255];
    }
    lut
}

/// Generates a 256-entry BGRA lookup table for `VisualMapColorMode::Rainbow`.
pub fn rainbow_bgra_lut() -> [[u8; 4]; 256] {
    let mut lut = [[0u8; 4]; 256];
    for byte in 0..=255 {
        let val = byte as f32 / 255.0;
        let rgba = RgbaColor::from_hsla_f32(val, 0.8, 0.5, 1.0);
        lut[byte as usize] = [rgba.b, rgba.g, rgba.r, rgba.a];
    }
    lut
}

/// Generates a 256-entry BGRA lookup table for `VisualMapColorMode::DataCategory` using the given palette.
pub fn category_bgra_lut(palette: &CategoryPalette) -> [[u8; 4]; 256] {
    let mut lut = [[0u8; 4]; 256];
    for byte in 0..=255 {
        lut[byte as usize] = match ByteCategory::of(byte) {
            ByteCategory::Null => palette.null,
            ByteCategory::Control => palette.control,
            ByteCategory::Space => palette.space,
            ByteCategory::Ascii => palette.ascii,
            ByteCategory::Extended => palette.extended,
        };
    }
    lut
}

/// Parameters for rendering a visual map pixel buffer.
#[derive(Clone, Debug)]
pub struct VisualMapRenderParams {
    pub cols: usize,
    pub start_row: usize,
    pub visible_rows: usize,
    pub max_visible_cols: usize,
    pub cell_width: usize,
    pub cell_height: usize,
    pub physical_width: usize,
    pub physical_height: usize,
    pub color_mode: VisualMapColorMode,
    pub entropy_window: usize,
    pub custom_lut: Option<[[u8; 4]; 256]>,
}

/// Renders a raw BGRA pixel buffer (`Vec<u8>`) from binary data according to the given parameters.
pub fn render_visual_map_bgra(buffer: &[u8], params: &VisualMapRenderParams) -> Vec<u8> {
    let buffer_len = buffer.len();
    let physical_width = params.physical_width;
    let physical_height = params.physical_height;

    if buffer_len == 0 || physical_width == 0 || physical_height == 0 {
        return Vec::new();
    }

    let mut pixels = vec![0u8; physical_width * physical_height * 4];
    let total_rows = buffer_len.div_ceil(params.cols);
    let start_row = params.start_row;
    let end_row = (start_row + params.visible_rows).min(total_rows);

    if params.color_mode == VisualMapColorMode::Entropy {
        let visible_start_offset = start_row * params.cols;
        let visible_end_offset = cmp::min(buffer_len, end_row * params.cols);

        let entropies = crate::core::entropy::compute_sliding_entropy(buffer, visible_start_offset, visible_end_offset, params.entropy_window);
        let lut = crate::core::entropy::entropy_bgra_lut();

        for r in start_row..end_row {
            let row_y = r - start_row;
            let row_offset = r * params.cols;
            let chunk_len = cmp::min(params.cols, buffer_len.saturating_sub(row_offset));
            let chunk_len = cmp::min(chunk_len, params.max_visible_cols);
            if chunk_len == 0 {
                break;
            }

            for c in 0..chunk_len {
                let byte_idx = row_offset + c;
                let color = if byte_idx >= visible_start_offset && byte_idx < visible_end_offset && (byte_idx - visible_start_offset) < entropies.len() {
                    let norm = entropies[byte_idx - visible_start_offset];
                    let lut_idx = crate::core::entropy::normalized_to_lut_index(norm);
                    lut[lut_idx]
                } else {
                    [0, 0, 0, 255]
                };

                blit_cell(&mut pixels, row_y, c, params, color);
            }
        }
    } else {
        let bgra_lut = if let Some(custom) = params.custom_lut {
            custom
        } else {
            match params.color_mode {
                VisualMapColorMode::Grayscale => grayscale_bgra_lut(),
                VisualMapColorMode::Rainbow => rainbow_bgra_lut(),
                VisualMapColorMode::DataCategory => category_bgra_lut(&CategoryPalette::default()),
                VisualMapColorMode::Entropy => unreachable!(),
            }
        };

        for r in start_row..end_row {
            let row_y = r - start_row;
            let row_offset = r * params.cols;
            let chunk_len = cmp::min(params.cols, buffer_len.saturating_sub(row_offset));
            let chunk_len = cmp::min(chunk_len, params.max_visible_cols);
            if chunk_len == 0 {
                break;
            }

            let chunk_end = (row_offset + chunk_len).min(buffer_len);
            let chunk = &buffer[row_offset..chunk_end];

            for (c, &byte) in chunk.iter().enumerate() {
                let color = bgra_lut[byte as usize];
                blit_cell(&mut pixels, row_y, c, params, color);
            }
        }
    }

    pixels
}

#[inline]
fn blit_cell(pixels: &mut [u8], row_y: usize, col_x: usize, params: &VisualMapRenderParams, color: [u8; 4]) {
    let cell_height = params.cell_height;
    let cell_width = params.cell_width;
    let physical_height = params.physical_height;
    let physical_width = params.physical_width;

    for dy in 0..cell_height {
        let py = row_y * cell_height + dy;
        if py >= physical_height {
            continue;
        }
        for dx in 0..cell_width {
            let px_idx = col_x * cell_width + dx;
            if px_idx >= physical_width {
                continue;
            }
            let pixel_offset = (py * physical_width + px_idx) * 4;
            pixels[pixel_offset] = color[0];
            pixels[pixel_offset + 1] = color[1];
            pixels[pixel_offset + 2] = color[2];
            pixels[pixel_offset + 3] = color[3];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_category_mapping() {
        assert_eq!(ByteCategory::of(0), ByteCategory::Null);
        assert_eq!(ByteCategory::of(1), ByteCategory::Control);
        assert_eq!(ByteCategory::of(31), ByteCategory::Control);
        assert_eq!(ByteCategory::of(127), ByteCategory::Control);
        assert_eq!(ByteCategory::of(32), ByteCategory::Space);
        assert_eq!(ByteCategory::of(33), ByteCategory::Ascii);
        assert_eq!(ByteCategory::of(b'A'), ByteCategory::Ascii);
        assert_eq!(ByteCategory::of(126), ByteCategory::Ascii);
        assert_eq!(ByteCategory::of(128), ByteCategory::Extended);
        assert_eq!(ByteCategory::of(255), ByteCategory::Extended);
    }

    #[test]
    fn test_grayscale_lut() {
        let lut = grayscale_bgra_lut();
        assert_eq!(lut.len(), 256);
        // Alpha is 255
        assert_eq!(lut[0][3], 255);
        assert_eq!(lut[255][3], 255);
        // R=G=B
        assert_eq!(lut[128][0], lut[128][1]);
        assert_eq!(lut[128][1], lut[128][2]);
    }

    #[test]
    fn test_rainbow_lut() {
        let lut = rainbow_bgra_lut();
        assert_eq!(lut.len(), 256);
        assert_eq!(lut[0][3], 255);
        assert_eq!(lut[255][3], 255);
    }

    #[test]
    fn test_category_lut() {
        let palette = CategoryPalette {
            null: [1, 2, 3, 4],
            control: [5, 6, 7, 8],
            space: [9, 10, 11, 12],
            ascii: [13, 14, 15, 16],
            extended: [17, 18, 19, 20],
        };
        let lut = category_bgra_lut(&palette);
        assert_eq!(lut[0], [1, 2, 3, 4]);
        assert_eq!(lut[10], [5, 6, 7, 8]);
        assert_eq!(lut[32], [9, 10, 11, 12]);
        assert_eq!(lut[b'x' as usize], [13, 14, 15, 16]);
        assert_eq!(lut[200], [17, 18, 19, 20]);
    }

    #[test]
    fn test_render_empty_buffer() {
        let params = VisualMapRenderParams {
            cols: 16,
            start_row: 0,
            visible_rows: 10,
            max_visible_cols: 16,
            cell_width: 2,
            cell_height: 2,
            physical_width: 32,
            physical_height: 20,
            color_mode: VisualMapColorMode::Grayscale,
            entropy_window: 64,
            custom_lut: None,
        };
        let pixels = render_visual_map_bgra(&[], &params);
        assert!(pixels.is_empty());
    }

    #[test]
    fn test_render_visual_map_pixels() {
        let data = vec![0u8, 32, b'A', 255];
        let params = VisualMapRenderParams {
            cols: 2,
            start_row: 0,
            visible_rows: 2,
            max_visible_cols: 2,
            cell_width: 1,
            cell_height: 1,
            physical_width: 2,
            physical_height: 2,
            color_mode: VisualMapColorMode::Grayscale,
            entropy_window: 64,
            custom_lut: None,
        };
        let pixels = render_visual_map_bgra(&data, &params);
        assert_eq!(pixels.len(), 2 * 2 * 4);
    }
}
