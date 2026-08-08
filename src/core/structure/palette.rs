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
