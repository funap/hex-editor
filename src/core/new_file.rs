/// Maximum buffer size limit (1 GB) to prevent out-of-memory allocations.
pub const MAX_BUFFER_SIZE: usize = 1024 * 1024 * 1024;

/// Parses user-provided size text into a byte count.
/// Supports plain numbers (`1024`), hex (`0x400`), and units (`1K`, `4KB`, `64KB`, `1MB`, `1.5MB`, `1GB`, etc.).
pub fn parse_buffer_size(input: &str) -> Result<usize, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Size cannot be empty".to_string());
    }

    let lower = trimmed.to_ascii_lowercase();

    // Check for hex prefix (e.g. 0x1000)
    if let Some(hex_str) = lower.strip_prefix("0x") {
        let hex_clean = hex_str.replace('_', "");
        if hex_clean.is_empty() {
            return Err("Invalid hex size".to_string());
        }
        let val = usize::from_str_radix(&hex_clean, 16).map_err(|_| "Invalid hex number".to_string())?;
        if val > MAX_BUFFER_SIZE {
            return Err("Size exceeds maximum limit of 1 GB".to_string());
        }
        return Ok(val);
    }

    // Check for units
    let (num_str, multiplier) = if lower.ends_with("gib") || lower.ends_with("gb") {
        let end = if lower.ends_with("gib") { lower.len() - 3 } else { lower.len() - 2 };
        (&trimmed[..end], 1024 * 1024 * 1024)
    } else if lower.ends_with('g') {
        (&trimmed[..trimmed.len() - 1], 1024 * 1024 * 1024)
    } else if lower.ends_with("mib") || lower.ends_with("mb") {
        let end = if lower.ends_with("mib") { lower.len() - 3 } else { lower.len() - 2 };
        (&trimmed[..end], 1024 * 1024)
    } else if lower.ends_with('m') {
        (&trimmed[..trimmed.len() - 1], 1024 * 1024)
    } else if lower.ends_with("kib") || lower.ends_with("kb") {
        let end = if lower.ends_with("kib") { lower.len() - 3 } else { lower.len() - 2 };
        (&trimmed[..end], 1024)
    } else if lower.ends_with('k') {
        (&trimmed[..trimmed.len() - 1], 1024)
    } else if lower.ends_with('b') {
        (&trimmed[..trimmed.len() - 1], 1)
    } else {
        (trimmed, 1)
    };

    let num_clean = num_str.trim().replace('_', "");
    if num_clean.is_empty() {
        return Err("Missing number before unit".to_string());
    }

    // Allow float inputs for units (e.g. 1.5 MB)
    let bytes = if num_clean.contains('.') {
        let float_val = num_clean.parse::<f64>().map_err(|_| "Invalid size number".to_string())?;
        if float_val < 0.0 {
            return Err("Size cannot be negative".to_string());
        }
        let total = float_val * (multiplier as f64);
        if total > (MAX_BUFFER_SIZE as f64) {
            return Err("Size exceeds maximum limit of 1 GB".to_string());
        }
        total as usize
    } else {
        let int_val = num_clean.parse::<usize>().map_err(|_| "Invalid size number".to_string())?;
        int_val.checked_mul(multiplier).ok_or_else(|| "Size overflow".to_string())?
    };

    if bytes > MAX_BUFFER_SIZE {
        return Err("Size exceeds maximum limit of 1 GB".to_string());
    }

    Ok(bytes)
}

/// Parses a fill byte specification into a `u8`.
/// Supports hex (`0x00`, `FF`, `0x20`), decimal (`0`..`255`), binary (`0b11110000`), or char (`' '`).
pub fn parse_fill_byte(input: &str) -> Result<u8, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(0x00);
    }

    let lower = trimmed.to_ascii_lowercase();

    // Check single character in quotes (e.g. 'A' or ' ')
    if (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() == 3)
        || (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() == 3)
    {
        let ch = trimmed.as_bytes()[1];
        return Ok(ch);
    }

    // Check binary prefix (0b...)
    if let Some(bin_str) = lower.strip_prefix("0b") {
        let bin_clean = bin_str.replace('_', "");
        return u8::from_str_radix(&bin_clean, 2).map_err(|_| "Invalid binary byte (e.g. 0b10101010)".to_string());
    }

    // Check hex prefix (0x...)
    if let Some(hex_str) = lower.strip_prefix("0x") {
        let hex_clean = hex_str.replace('_', "");
        return u8::from_str_radix(&hex_clean, 16).map_err(|_| "Invalid hex byte (0x00..0xFF)".to_string());
    }

    // Try decimal parse
    if let Ok(dec_val) = trimmed.parse::<u64>() {
        if dec_val <= 255 {
            return Ok(dec_val as u8);
        }
        return Err("Decimal value exceeds 255 (0..255)".to_string());
    }

    // Try 1-2 char hex without prefix (e.g. FF, 00, aa, 20)
    if trimmed.len() <= 2
        && trimmed.chars().all(|c| c.is_ascii_hexdigit())
        && let Ok(hex_val) = u8::from_str_radix(trimmed, 16)
    {
        return Ok(hex_val);
    }

    Err("Invalid byte value (use hex e.g. 0x00, FF, or dec 0-255)".to_string())
}

/// Formats a byte size into a human-readable summary.
pub fn format_size_preview(size: usize) -> String {
    if size == 0 {
        return "0 bytes (Empty)".to_string();
    }
    if size < 1024 {
        format!("{size} bytes (0x{size:X})")
    } else if size < 1024 * 1024 {
        let kb = (size as f64) / 1024.0;
        format!("{size} bytes (0x{size:X}, {kb:.2} KB)")
    } else {
        let mb = (size as f64) / (1024.0 * 1024.0);
        format!("{size} bytes (0x{size:X}, {mb:.2} MB)")
    }
}

/// Formats a fill byte into a human-readable preview.
pub fn format_fill_preview(byte: u8) -> String {
    let ascii_repr = match byte {
        0x00 => "0x00 | Dec: 0 (Null / \\0)".to_string(),
        0x20 => "0x20 | Dec: 32 (' ' Space)".to_string(),
        0x09 => "0x09 | Dec: 9 (\\t Tab)".to_string(),
        0x0A => "0x0A | Dec: 10 (\\n LF)".to_string(),
        0x0D => "0x0D | Dec: 13 (\\r CR)".to_string(),
        0xFF => "0xFF | Dec: 255 (All 1s)".to_string(),
        b if b.is_ascii_graphic() => format!("0x{b:02X} | Dec: {b} ('{}')", b as char),
        b => format!("0x{b:02X} | Dec: {b}"),
    };
    format!("Byte: {ascii_repr}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_buffer_size() {
        assert_eq!(parse_buffer_size("0").unwrap(), 0);
        assert_eq!(parse_buffer_size("1024").unwrap(), 1024);
        assert_eq!(parse_buffer_size("  256  ").unwrap(), 256);
        assert_eq!(parse_buffer_size("0x100").unwrap(), 256);
        assert_eq!(parse_buffer_size("0X1000").unwrap(), 4096);
        assert_eq!(parse_buffer_size("1K").unwrap(), 1024);
        assert_eq!(parse_buffer_size("1KB").unwrap(), 1024);
        assert_eq!(parse_buffer_size("4 kb").unwrap(), 4096);
        assert_eq!(parse_buffer_size("64KB").unwrap(), 65536);
        assert_eq!(parse_buffer_size("1MB").unwrap(), 1048576);
        assert_eq!(parse_buffer_size("1.5MB").unwrap(), 1572864);
        assert_eq!(parse_buffer_size("1GB").unwrap(), 1073741824);

        assert!(parse_buffer_size("").is_err());
        assert!(parse_buffer_size("abc").is_err());
        assert!(parse_buffer_size("2GB").is_err()); // Exceeds 1GB limit
        assert!(parse_buffer_size("-10").is_err());
    }

    #[test]
    fn test_parse_fill_byte() {
        assert_eq!(parse_fill_byte("").unwrap(), 0x00);
        assert_eq!(parse_fill_byte("0").unwrap(), 0x00);
        assert_eq!(parse_fill_byte("0x00").unwrap(), 0x00);
        assert_eq!(parse_fill_byte("0x20").unwrap(), 0x20);
        assert_eq!(parse_fill_byte("32").unwrap(), 0x20);
        assert_eq!(parse_fill_byte("0xFF").unwrap(), 0xFF);
        assert_eq!(parse_fill_byte("255").unwrap(), 0xFF);
        assert_eq!(parse_fill_byte("FF").unwrap(), 0xFF);
        assert_eq!(parse_fill_byte("ff").unwrap(), 0xFF);
        assert_eq!(parse_fill_byte("0xAA").unwrap(), 0xAA);
        assert_eq!(parse_fill_byte("aa").unwrap(), 0xAA);
        assert_eq!(parse_fill_byte("0b11110000").unwrap(), 0xF0);
        assert_eq!(parse_fill_byte("'A'").unwrap(), b'A');
        assert_eq!(parse_fill_byte("' '").unwrap(), b' ');

        assert!(parse_fill_byte("256").is_err());
        assert!(parse_fill_byte("0x100").is_err());
        assert!(parse_fill_byte("xyz").is_err());
    }

    #[test]
    fn test_format_size_preview() {
        assert_eq!(format_size_preview(0), "0 bytes (Empty)");
        assert_eq!(format_size_preview(256), "256 bytes (0x100)");
        assert_eq!(format_size_preview(1024), "1024 bytes (0x400, 1.00 KB)");
        assert_eq!(format_size_preview(1048576), "1048576 bytes (0x100000, 1.00 MB)");
    }

    #[test]
    fn test_format_fill_preview() {
        assert!(format_fill_preview(0x00).contains("0x00"));
        assert!(format_fill_preview(0x20).contains("Space"));
        assert!(format_fill_preview(0xFF).contains("255"));
    }
}
