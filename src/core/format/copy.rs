use std::fmt::Write as _;

const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyFormat {
    HexDump,
    CppArray,
    HexStream,
    HexWithSpaces,
    PrintableText,
    Base64,
    EscapedString,
    Binary,
    RustArray,
    JsonArray,
}

/// Convert a slice of bytes into Base64 encoded string (RFC 4648 standard) without external dependencies.
pub fn to_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    if bytes.is_empty() {
        return String::new();
    }
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        result.push(TABLE[(b0 >> 2) as usize] as char);
        result.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            result.push(TABLE[(((b1 & 0x0F) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(TABLE[(b2 & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

/// Format bytes into Wireshark / canonical hexdump format.
///
/// Output format:
/// `00000000  50 4b 03 04 14 00 00 00  08 00 84 8b 5b 58 21 82  |PK..........[X!.|`
pub fn format_hexdump(bytes: &[u8], start_offset: usize) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let line_count = bytes.len().div_ceil(16);
    let mut result = String::with_capacity(line_count * 78);
    for (chunk_idx, chunk) in bytes.chunks(16).enumerate() {
        if chunk_idx > 0 {
            result.push('\n');
        }
        let line_offset = start_offset + chunk_idx * 16;
        let _ = write!(result, "{:08x}  ", line_offset);

        // Hex representation (16 bytes, split into two 8-byte groups)
        for i in 0..16 {
            if i < chunk.len() {
                let b = chunk[i];
                result.push(HEX_CHARS[(b >> 4) as usize] as char);
                result.push(HEX_CHARS[(b & 0x0F) as usize] as char);
            } else {
                result.push_str("  ");
            }
            if i == 7 {
                result.push_str("  ");
            } else if i < 15 {
                result.push(' ');
            }
        }

        result.push_str("  |");
        for &b in chunk {
            if (32..=126).contains(&b) {
                result.push(b as char);
            } else {
                result.push('.');
            }
        }
        result.push('|');
    }
    result
}

/// Format bytes as a C/C++ array declaration.
pub fn format_cpp_array(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "const unsigned char data[0] = {};".to_string();
    }
    let len = bytes.len();
    let mut result = String::with_capacity(64 + len * 6);
    let _ = write!(result, "/* Length: {} (0x{:X}) bytes */\nconst unsigned char data[{}] = {{\n", len, len, len);
    for (chunk_idx, chunk) in bytes.chunks(12).enumerate() {
        result.push_str("    ");
        for (i, &b) in chunk.iter().enumerate() {
            result.push_str("0x");
            result.push(HEX_CHARS[(b >> 4) as usize] as char);
            result.push(HEX_CHARS[(b & 0x0F) as usize] as char);
            let is_last_byte = chunk_idx * 12 + i + 1 == len;
            if !is_last_byte {
                result.push_str(", ");
            }
        }
        if chunk_idx * 12 + chunk.len() < len {
            result.push('\n');
        }
    }
    result.push_str("\n};");
    result
}

/// Format bytes as a Rust array declaration.
pub fn format_rust_array(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "const DATA: [u8; 0] = [];".to_string();
    }
    let len = bytes.len();
    let mut result = String::with_capacity(32 + len * 6);
    let _ = writeln!(result, "const DATA: [u8; {}] = [", len);
    for (chunk_idx, chunk) in bytes.chunks(12).enumerate() {
        result.push_str("    ");
        for (i, &b) in chunk.iter().enumerate() {
            result.push_str("0x");
            result.push(HEX_CHARS[(b >> 4) as usize] as char);
            result.push(HEX_CHARS[(b & 0x0F) as usize] as char);
            let is_last_byte = chunk_idx * 12 + i + 1 == len;
            if !is_last_byte {
                result.push_str(", ");
            } else {
                result.push(',');
            }
        }
        result.push('\n');
    }
    result.push_str("];");
    result
}

/// Format bytes as raw continuous hex stream (e.g. `504b0304...`).
pub fn format_hex_stream(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        result.push(HEX_CHARS[(b >> 4) as usize] as char);
        result.push(HEX_CHARS[(b & 0x0F) as usize] as char);
    }
    result
}

/// Format bytes as space-separated hex bytes (e.g. `50 4b 03 04...`).
pub fn format_hex_spaces(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len().saturating_mul(3));
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 {
            result.push(' ');
        }
        result.push(HEX_CHARS[(b >> 4) as usize] as char);
        result.push(HEX_CHARS[(b & 0x0F) as usize] as char);
    }
    result
}

/// Format bytes as printable text (non-printable bytes rendered as `.`) using the specified encoding.
pub fn format_printable_text(bytes: &[u8], encoding: crate::core::encoding::Encoding) -> String {
    encoding.format_preview(bytes, 0, bytes.len())
}

/// Format bytes as escaped string (e.g. `\x50\x4b\x03\x04...`).
pub fn format_escaped_string(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 4);
    for &b in bytes {
        result.push_str("\\x");
        result.push(HEX_CHARS[(b >> 4) as usize] as char);
        result.push(HEX_CHARS[(b & 0x0F) as usize] as char);
    }
    result
}

/// Format bytes as 8-bit binary representation separated by spaces (e.g. `01010000 01001011...`).
pub fn format_binary(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len().saturating_mul(9));
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 {
            result.push(' ');
        }
        let _ = write!(result, "{:08b}", b);
    }
    result
}

/// Format bytes as JSON array of byte values (e.g. `[80, 75, 3, 4]`).
pub fn format_json_array(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len().saturating_mul(4) + 2);
    result.push('[');
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 {
            result.push_str(", ");
        }
        let _ = write!(result, "{}", b);
    }
    result.push(']');
    result
}

/// General entry point to format bytes in any supported `CopyFormat`.
pub fn format_bytes(bytes: &[u8], start_offset: usize, format: CopyFormat, encoding: crate::core::encoding::Encoding) -> String {
    match format {
        CopyFormat::HexDump => format_hexdump(bytes, start_offset),
        CopyFormat::CppArray => format_cpp_array(bytes),
        CopyFormat::HexStream => format_hex_stream(bytes),
        CopyFormat::HexWithSpaces => format_hex_spaces(bytes),
        CopyFormat::PrintableText => format_printable_text(bytes, encoding),
        CopyFormat::Base64 => to_base64(bytes),
        CopyFormat::EscapedString => format_escaped_string(bytes),
        CopyFormat::Binary => format_binary(bytes),
        CopyFormat::RustArray => format_rust_array(bytes),
        CopyFormat::JsonArray => format_json_array(bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_empty() {
        let enc = crate::core::encoding::Encoding::Ascii;
        assert_eq!(format_bytes(&[], 0, CopyFormat::HexDump, enc), "");
        assert_eq!(format_bytes(&[], 0, CopyFormat::HexStream, enc), "");
        assert_eq!(format_bytes(&[], 0, CopyFormat::HexWithSpaces, enc), "");
        assert_eq!(format_bytes(&[], 0, CopyFormat::PrintableText, enc), "");
        assert_eq!(format_bytes(&[], 0, CopyFormat::Base64, enc), "");
        assert_eq!(format_bytes(&[], 0, CopyFormat::EscapedString, enc), "");
        assert_eq!(format_bytes(&[], 0, CopyFormat::Binary, enc), "");
        assert_eq!(format_bytes(&[], 0, CopyFormat::JsonArray, enc), "[]");
        assert_eq!(format_bytes(&[], 0, CopyFormat::CppArray, enc), "const unsigned char data[0] = {};");
        assert_eq!(format_bytes(&[], 0, CopyFormat::RustArray, enc), "const DATA: [u8; 0] = [];");
    }

    #[test]
    fn test_hexdump_formatting() {
        let sample = b"Hello, World!\x00\x01\x02\x03\x04";
        let out = format_hexdump(sample, 0x10);
        let expected_line1 = "00000010  48 65 6c 6c 6f 2c 20 57  6f 72 6c 64 21 00 01 02  |Hello, World!...|";
        let expected_line2 = "00000020  03 04                                             |..|";
        assert_eq!(out, format!("{}\n{}", expected_line1, expected_line2));
    }

    #[test]
    fn test_cpp_array_formatting() {
        let sample = [0x50, 0x4B, 0x03, 0x04];
        let out = format_cpp_array(&sample);
        assert!(out.contains("const unsigned char data[4] = {"));
        assert!(out.contains("0x50, 0x4b, 0x03, 0x04"));
        assert!(out.ends_with("\n};"));
    }

    #[test]
    fn test_rust_array_formatting() {
        let sample = [0x50, 0x4B, 0x03, 0x04];
        let out = format_rust_array(&sample);
        assert!(out.contains("const DATA: [u8; 4] = ["));
        assert!(out.contains("0x50, 0x4b, 0x03, 0x04,"));
        assert!(out.ends_with("];"));
    }

    #[test]
    fn test_hex_stream() {
        let sample = [0xDE, 0xAD, 0xBE, 0xEF];
        assert_eq!(format_hex_stream(&sample), "deadbeef");
    }

    #[test]
    fn test_hex_spaces() {
        let sample = [0xDE, 0xAD, 0xBE, 0xEF];
        assert_eq!(format_hex_spaces(&sample), "de ad be ef");
    }

    #[test]
    fn test_printable_text() {
        let sample = b"Hi\x00\x1f \x7e\x7f\x80!";
        assert_eq!(format_printable_text(sample, crate::core::encoding::Encoding::Ascii), "Hi.. ~..!".to_string());

        // Shift-JIS printable text
        let sjis_sample = [0x82, 0xB1, 0x82, 0xF1, 0x00, 0x41];
        assert_eq!(
            format_printable_text(&sjis_sample, crate::core::encoding::Encoding::ShiftJis),
            "こん.A".to_string()
        );
    }

    #[test]
    fn test_base64() {
        assert_eq!(to_base64(b""), "");
        assert_eq!(to_base64(b"f"), "Zg==");
        assert_eq!(to_base64(b"fo"), "Zm8=");
        assert_eq!(to_base64(b"foo"), "Zm9v");
        assert_eq!(to_base64(b"foob"), "Zm9vYg==");
        assert_eq!(to_base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(to_base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn test_escaped_string() {
        let sample = [0x00, 0xFF, 0x41];
        assert_eq!(format_escaped_string(&sample), "\\x00\\xff\\x41");
    }

    #[test]
    fn test_binary() {
        let sample = [0x05, 0x80];
        assert_eq!(format_binary(&sample), "00000101 10000000");
    }

    #[test]
    fn test_json_array() {
        let sample = [1, 2, 3, 255];
        assert_eq!(format_json_array(&sample), "[1, 2, 3, 255]");
    }
}
