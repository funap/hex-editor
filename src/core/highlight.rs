use gpui::Hsla;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fs;
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static HIGHLIGHT_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn generate_highlight_id() -> String {
    let id = HIGHLIGHT_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("hl-{}", id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HighlightColor {
    Red,
    Orange,
    #[default]
    Yellow,
    Green,
    Cyan,
    Blue,
    Purple,
    Pink,
    Custom {
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    },
}

impl HighlightColor {
    pub const ALL_PRESETS: &'static [HighlightColor] = &[
        HighlightColor::Red,
        HighlightColor::Orange,
        HighlightColor::Yellow,
        HighlightColor::Green,
        HighlightColor::Cyan,
        HighlightColor::Blue,
        HighlightColor::Purple,
        HighlightColor::Pink,
    ];

    pub fn to_hsla(self) -> Hsla {
        match self {
            HighlightColor::Red => gpui::hsla(0.0, 0.75, 0.55, 0.35),
            HighlightColor::Orange => gpui::hsla(30.0 / 360.0, 0.85, 0.55, 0.35),
            HighlightColor::Yellow => gpui::hsla(50.0 / 360.0, 0.85, 0.50, 0.35),
            HighlightColor::Green => gpui::hsla(120.0 / 360.0, 0.65, 0.45, 0.35),
            HighlightColor::Cyan => gpui::hsla(180.0 / 360.0, 0.70, 0.45, 0.35),
            HighlightColor::Blue => gpui::hsla(215.0 / 360.0, 0.75, 0.55, 0.35),
            HighlightColor::Purple => gpui::hsla(280.0 / 360.0, 0.70, 0.55, 0.35),
            HighlightColor::Pink => gpui::hsla(330.0 / 360.0, 0.75, 0.55, 0.35),
            HighlightColor::Custom { r, g, b, a } => {
                let rf = r as f32 / 255.0;
                let gf = g as f32 / 255.0;
                let bf = b as f32 / 255.0;
                let af = a as f32 / 255.0;
                gpui::Rgba { r: rf, g: gf, b: bf, a: af }.into()
            }
        }
    }

    /// Solid / opaque color for badges, icon swatches, or UI indicator dots.
    pub fn to_badge_hsla(self) -> Hsla {
        match self {
            HighlightColor::Red => gpui::hsla(0.0, 0.85, 0.60, 1.0),
            HighlightColor::Orange => gpui::hsla(30.0 / 360.0, 0.90, 0.60, 1.0),
            HighlightColor::Yellow => gpui::hsla(50.0 / 360.0, 0.90, 0.55, 1.0),
            HighlightColor::Green => gpui::hsla(120.0 / 360.0, 0.75, 0.50, 1.0),
            HighlightColor::Cyan => gpui::hsla(180.0 / 360.0, 0.80, 0.50, 1.0),
            HighlightColor::Blue => gpui::hsla(215.0 / 360.0, 0.85, 0.60, 1.0),
            HighlightColor::Purple => gpui::hsla(280.0 / 360.0, 0.80, 0.60, 1.0),
            HighlightColor::Pink => gpui::hsla(330.0 / 360.0, 0.85, 0.60, 1.0),
            HighlightColor::Custom { r, g, b, .. } => {
                let rf = r as f32 / 255.0;
                let gf = g as f32 / 255.0;
                let bf = b as f32 / 255.0;
                gpui::Rgba { r: rf, g: gf, b: bf, a: 1.0 }.into()
            }
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            HighlightColor::Red => "Red",
            HighlightColor::Orange => "Orange",
            HighlightColor::Yellow => "Yellow",
            HighlightColor::Green => "Green",
            HighlightColor::Cyan => "Cyan",
            HighlightColor::Blue => "Blue",
            HighlightColor::Purple => "Purple",
            HighlightColor::Pink => "Pink",
            HighlightColor::Custom { .. } => "Custom",
        }
    }

    pub fn from_hsla(hsla: Hsla) -> Self {
        let h_deg = hsla.h * 360.0;
        if (0.0..15.0).contains(&h_deg) || (345.0..=360.0).contains(&h_deg) {
            HighlightColor::Red
        } else if (15.0..40.0).contains(&h_deg) {
            HighlightColor::Orange
        } else if (40.0..80.0).contains(&h_deg) {
            HighlightColor::Yellow
        } else if (80.0..150.0).contains(&h_deg) {
            HighlightColor::Green
        } else if (150.0..200.0).contains(&h_deg) {
            HighlightColor::Cyan
        } else if (200.0..250.0).contains(&h_deg) {
            HighlightColor::Blue
        } else if (250.0..310.0).contains(&h_deg) {
            HighlightColor::Purple
        } else {
            HighlightColor::Pink
        }
    }
}

impl Serialize for HighlightColor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            HighlightColor::Red => serializer.serialize_str("red"),
            HighlightColor::Orange => serializer.serialize_str("orange"),
            HighlightColor::Yellow => serializer.serialize_str("yellow"),
            HighlightColor::Green => serializer.serialize_str("green"),
            HighlightColor::Cyan => serializer.serialize_str("cyan"),
            HighlightColor::Blue => serializer.serialize_str("blue"),
            HighlightColor::Purple => serializer.serialize_str("purple"),
            HighlightColor::Pink => serializer.serialize_str("pink"),
            HighlightColor::Custom { r, g, b, a } => {
                if *a == 255 {
                    serializer.serialize_str(&format!("#{:02x}{:02x}{:02x}", r, g, b))
                } else {
                    serializer.serialize_str(&format!("#{:02x}{:02x}{:02x}{:02x}", r, g, b, a))
                }
            }
        }
    }
}

impl<'de> Deserialize<'de> for HighlightColor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ColorVisitor;

        impl<'de> serde::de::Visitor<'de> for ColorVisitor {
            type Value = HighlightColor;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a color string (e.g. 'red', 'blue', '#ff0000') or an RGBA object")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let lower = v.trim().to_lowercase();
                match lower.as_str() {
                    "red" => Ok(HighlightColor::Red),
                    "orange" => Ok(HighlightColor::Orange),
                    "yellow" => Ok(HighlightColor::Yellow),
                    "green" => Ok(HighlightColor::Green),
                    "cyan" => Ok(HighlightColor::Cyan),
                    "blue" => Ok(HighlightColor::Blue),
                    "purple" => Ok(HighlightColor::Purple),
                    "pink" => Ok(HighlightColor::Pink),
                    hex if hex.starts_with('#') => {
                        let hex_body = &hex[1..];
                        match hex_body.len() {
                            3 => {
                                let r = u8::from_str_radix(&hex_body[0..1].repeat(2), 16).map_err(E::custom)?;
                                let g = u8::from_str_radix(&hex_body[1..2].repeat(2), 16).map_err(E::custom)?;
                                let b = u8::from_str_radix(&hex_body[2..3].repeat(2), 16).map_err(E::custom)?;
                                Ok(HighlightColor::Custom { r, g, b, a: 255 })
                            }
                            6 => {
                                let r = u8::from_str_radix(&hex_body[0..2], 16).map_err(E::custom)?;
                                let g = u8::from_str_radix(&hex_body[2..4], 16).map_err(E::custom)?;
                                let b = u8::from_str_radix(&hex_body[4..6], 16).map_err(E::custom)?;
                                Ok(HighlightColor::Custom { r, g, b, a: 255 })
                            }
                            8 => {
                                let r = u8::from_str_radix(&hex_body[0..2], 16).map_err(E::custom)?;
                                let g = u8::from_str_radix(&hex_body[2..4], 16).map_err(E::custom)?;
                                let b = u8::from_str_radix(&hex_body[4..6], 16).map_err(E::custom)?;
                                let a = u8::from_str_radix(&hex_body[6..8], 16).map_err(E::custom)?;
                                Ok(HighlightColor::Custom { r, g, b, a })
                            }
                            _ => Err(E::custom(format!("Invalid hex color format: {}", hex))),
                        }
                    }
                    _ => Err(E::custom(format!("Unknown highlight color: {}", lower))),
                }
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                let mut r = None;
                let mut g = None;
                let mut b = None;
                let mut a = Some(255u8);

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "r" => r = Some(map.next_value()?),
                        "g" => g = Some(map.next_value()?),
                        "b" => b = Some(map.next_value()?),
                        "a" => a = Some(map.next_value()?),
                        _ => {
                            let _ = map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }

                let r = r.ok_or_else(|| serde::de::Error::missing_field("r"))?;
                let g = g.ok_or_else(|| serde::de::Error::missing_field("g"))?;
                let b = b.ok_or_else(|| serde::de::Error::missing_field("b"))?;
                let a = a.unwrap_or(255);

                Ok(HighlightColor::Custom { r, g, b, a })
            }
        }

        deserializer.deserialize_any(ColorVisitor)
    }
}

fn default_highlight_color() -> HighlightColor {
    HighlightColor::Yellow
}

fn deserialize_offset_or_hex<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    struct OffsetVisitor;

    impl<'de> serde::de::Visitor<'de> for OffsetVisitor {
        type Value = usize;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an integer or hex string (e.g. 1024 or '0x400')")
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(v as usize)
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            if v < 0 { Err(E::custom("offset cannot be negative")) } else { Ok(v as usize) }
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            let trimmed = v.trim();
            if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
                usize::from_str_radix(hex, 16).map_err(E::custom)
            } else {
                trimmed.parse::<usize>().map_err(E::custom)
            }
        }
    }

    deserializer.deserialize_any(OffsetVisitor)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HighlightItem {
    #[serde(default = "generate_highlight_id", skip_serializing)]
    pub id: String,
    #[serde(deserialize_with = "deserialize_offset_or_hex")]
    pub offset: usize,
    #[serde(deserialize_with = "deserialize_offset_or_hex")]
    pub size: usize,
    #[serde(default = "default_highlight_color")]
    pub color: HighlightColor,
    #[serde(default)]
    pub comment: String,
}

impl HighlightItem {
    pub fn new(offset: usize, size: usize, color: HighlightColor, comment: impl Into<String>) -> Self {
        Self {
            id: generate_highlight_id(),
            offset,
            size,
            color,
            comment: comment.into(),
        }
    }

    pub fn range(&self) -> Range<usize> {
        self.offset..self.offset.saturating_add(self.size)
    }

    pub fn hsla_color(&self) -> Hsla {
        self.color.to_hsla()
    }

    pub fn format_offset(&self) -> String {
        format!("0x{:08X}", self.offset)
    }

    #[allow(dead_code)]
    pub fn format_size(&self) -> String {
        if self.size == 1 {
            "1 byte".to_string()
        } else if self.size < 1024 {
            format!("{} bytes", self.size)
        } else if self.size < 1024 * 1024 {
            format!("{:.1} KB", self.size as f64 / 1024.0)
        } else {
            format!("{:.2} MB", self.size as f64 / (1024.0 * 1024.0))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighlightFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    pub highlights: Vec<HighlightItem>,
}

fn default_version() -> u32 {
    1
}

impl HighlightFile {
    pub fn from_json(json: &str) -> anyhow::Result<Vec<HighlightItem>> {
        // First attempt: HighlightFile wrapped format
        let mut items = if let Ok(file) = serde_json::from_str::<HighlightFile>(json) {
            file.highlights
        } else if let Ok(items) = serde_json::from_str::<Vec<HighlightItem>>(json) {
            items
        } else {
            let err = serde_json::from_str::<HighlightFile>(json).unwrap_err();
            anyhow::bail!("Failed to parse highlights JSON: {}", err)
        };

        // Guarantee distinct, fresh unique runtime IDs for all loaded items
        for item in &mut items {
            item.id = generate_highlight_id();
        }
        Ok(items)
    }

    pub fn to_json(highlights: &[HighlightItem], file_path: Option<&Path>) -> anyhow::Result<String> {
        let file = HighlightFile {
            version: 1,
            file_path: file_path.map(|p| p.to_string_lossy().to_string()),
            highlights: highlights.to_vec(),
        };
        Ok(serde_json::to_string_pretty(&file)?)
    }

    pub fn save_to_path(path: &Path, highlights: &[HighlightItem], file_path: Option<&Path>) -> anyhow::Result<()> {
        let json = Self::to_json(highlights, file_path)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load_from_path(path: &Path) -> anyhow::Result<Vec<HighlightItem>> {
        let content = fs::read_to_string(path)?;
        Self::from_json(&content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_color_presets_and_names() {
        for color in HighlightColor::ALL_PRESETS {
            let hsla = color.to_hsla();
            assert!(hsla.a > 0.0);
            let badge = color.to_badge_hsla();
            assert_eq!(badge.a, 1.0);
            assert!(!color.name().is_empty());
        }
    }

    #[test]
    fn test_highlight_color_serde() {
        let json = serde_json::to_string(&HighlightColor::Red).unwrap();
        assert_eq!(json, "\"red\"");
        let color: HighlightColor = serde_json::from_str("\"blue\"").unwrap();
        assert_eq!(color, HighlightColor::Blue);

        let hex_color: HighlightColor = serde_json::from_str("\"#112233\"").unwrap();
        assert_eq!(
            hex_color,
            HighlightColor::Custom {
                r: 0x11,
                g: 0x22,
                b: 0x33,
                a: 255
            }
        );

        let rgba_color: HighlightColor = serde_json::from_str(r#"{"r": 10, "g": 20, "b": 30, "a": 128}"#).unwrap();
        assert_eq!(rgba_color, HighlightColor::Custom { r: 10, g: 20, b: 30, a: 128 });
    }

    #[test]
    fn test_highlight_item_serde_with_hex_offsets() {
        let json = r#"[
            {
                "offset": "0x0010",
                "size": "0x20",
                "color": "green",
                "comment": "Header section"
            },
            {
                "offset": 100,
                "size": 4,
                "color": "pink",
                "comment": "Magic number"
            }
        ]"#;

        let items = HighlightFile::from_json(json).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].offset, 16);
        assert_eq!(items[0].size, 32);
        assert_eq!(items[0].color, HighlightColor::Green);
        assert_eq!(items[0].comment, "Header section");

        assert_eq!(items[1].offset, 100);
        assert_eq!(items[1].size, 4);
        assert_eq!(items[1].color, HighlightColor::Pink);
        assert_eq!(items[1].comment, "Magic number");
    }

    #[test]
    fn test_highlight_file_roundtrip() {
        let items = vec![
            HighlightItem::new(0, 16, HighlightColor::Red, "File header"),
            HighlightItem::new(64, 4, HighlightColor::Cyan, "Checksum"),
        ];

        let json = HighlightFile::to_json(&items, Some(Path::new("test.bin"))).unwrap();
        let loaded = HighlightFile::from_json(&json).unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].offset, 0);
        assert_eq!(loaded[0].size, 16);
        assert_eq!(loaded[0].color, HighlightColor::Red);
        assert_eq!(loaded[0].comment, "File header");
        assert_eq!(loaded[1].offset, 64);
        assert_eq!(loaded[1].size, 4);
        assert_eq!(loaded[1].color, HighlightColor::Cyan);
        assert_eq!(loaded[1].comment, "Checksum");
    }

    #[test]
    fn test_highlight_file_disk_io() {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join("test_xvw_highlights.json");

        let items = vec![
            HighlightItem::new(1024, 256, HighlightColor::Yellow, "Data block"),
            HighlightItem::new(2048, 128, HighlightColor::Purple, "Signature"),
        ];

        HighlightFile::save_to_path(&temp_path, &items, Some(Path::new("firmware.bin"))).unwrap();
        assert!(temp_path.exists());

        let loaded = HighlightFile::load_from_path(&temp_path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].offset, 1024);
        assert_eq!(loaded[0].size, 256);
        assert_eq!(loaded[0].color, HighlightColor::Yellow);
        assert_eq!(loaded[0].comment, "Data block");
        assert_eq!(loaded[1].offset, 2048);
        assert_eq!(loaded[1].size, 128);
        assert_eq!(loaded[1].color, HighlightColor::Purple);
        assert_eq!(loaded[1].comment, "Signature");

        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn test_highlight_item_formatting() {
        let item = HighlightItem::new(0x1234, 16, HighlightColor::Green, "Test");
        assert_eq!(item.format_offset(), "0x00001234");
        assert_eq!(item.format_size(), "16 bytes");

        let single = HighlightItem::new(0, 1, HighlightColor::Blue, "");
        assert_eq!(single.format_size(), "1 byte");

        let kb = HighlightItem::new(0, 2048, HighlightColor::Orange, "");
        assert_eq!(kb.format_size(), "2.0 KB");
    }
}
