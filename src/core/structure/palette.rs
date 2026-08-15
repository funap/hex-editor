use gpui::Hsla;

pub const DEFAULT_PALETTE: [&str; 12] = [
    "#FF6B6B", "#4ECDC4", "#45B7D1", "#96CEB4", "#FFEAA7", "#DDA0DD", "#98D8C8", "#F7DC6F", "#BB8FCE", "#85C1E9", "#F0B27A", "#AED6F1",
];

pub fn hex_to_hsla(hex: &str) -> Option<Hsla> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 && hex.len() != 8 {
        return None;
    }

    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    let a = if hex.len() == 8 {
        u8::from_str_radix(&hex[6..8], 16).ok()? as f32 / 255.0
    } else {
        1.0
    };

    let rgba8 = gpui::Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a,
    };
    Some(rgba8.into())
}

pub fn color(index: usize) -> Hsla {
    let hex = DEFAULT_PALETTE[index % DEFAULT_PALETTE.len()];
    hex_to_hsla(hex).unwrap_or_else(gpui::white)
}

pub fn get_color(index: usize) -> Hsla {
    color(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rgb_and_rgba_hex_colors() {
        let rgb = hex_to_hsla("#FF0000").expect("valid RGB color");
        assert_eq!(rgb, hex_to_hsla("FF0000").expect("valid RGB color without #"));
        assert_eq!(rgb.a, 1.0);

        let rgba = hex_to_hsla("#0080FF80").expect("valid RGBA color");
        assert!((rgba.a - (128.0 / 255.0)).abs() < f32::EPSILON);
        assert!(rgba.s > 0.0);
    }

    #[test]
    fn rejects_invalid_hex_and_wraps_palette_indices() {
        assert!(hex_to_hsla("").is_none());
        assert!(hex_to_hsla("#12345").is_none());
        assert!(hex_to_hsla("#GG0000").is_none());
        assert_eq!(color(0), color(DEFAULT_PALETTE.len()));
        assert_eq!(get_color(1), color(1));
    }
}
