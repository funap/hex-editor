use crate::core::color::RgbaColor;

pub const DEFAULT_PALETTE: [&str; 12] = [
    "#FF6B6B", "#4ECDC4", "#45B7D1", "#96CEB4", "#FFEAA7", "#DDA0DD", "#98D8C8", "#F7DC6F", "#BB8FCE", "#85C1E9", "#F0B27A", "#AED6F1",
];

pub fn hex_to_color(hex: &str) -> Option<RgbaColor> {
    RgbaColor::from_hex(hex)
}

pub fn color(index: usize) -> RgbaColor {
    let hex = DEFAULT_PALETTE[index % DEFAULT_PALETTE.len()];
    hex_to_color(hex).unwrap_or(RgbaColor::rgb(255, 255, 255))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rgb_and_rgba_hex_colors() {
        let rgb = hex_to_color("#FF0000").expect("valid RGB color");
        assert_eq!(rgb, hex_to_color("FF0000").expect("valid RGB color without #"));
        assert_eq!(rgb.a, 255);

        let rgba = hex_to_color("#0080FF80").expect("valid RGBA color");
        assert_eq!(rgba.a, 128);
    }

    #[test]
    fn rejects_invalid_hex_and_wraps_palette_indices() {
        assert!(hex_to_color("").is_none());
        assert!(hex_to_color("#12345").is_none());
        assert!(hex_to_color("#GG0000").is_none());
        assert_eq!(color(0), color(DEFAULT_PALETTE.len()));
    }
}
