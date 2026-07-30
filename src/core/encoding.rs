// This file will be responsible for converting byte sequences into strings corresponding to a specified encoding
// (e.g., UTF-8, Shift JIS). It will also include logic for detecting the encoding.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Ascii,
    Utf8,
    Utf16Le,
    Utf16Be,
}

impl Encoding {
    pub fn decode_char_at(&self, buffer: &[u8], offset: usize) -> Option<(char, usize)> {
        match self {
            Encoding::Ascii => {
                if offset < buffer.len() {
                    let b = buffer[offset];
                    if b >= 32 && b <= 126 { Some((b as char, 1)) } else { None }
                } else {
                    None
                }
            }
            Encoding::Utf8 => {
                if offset >= buffer.len() {
                    return None;
                }
                let b = buffer[offset];
                let len = if b & 0x80 == 0 {
                    1
                } else if b & 0xE0 == 0xC0 {
                    2
                } else if b & 0xF0 == 0xE0 {
                    3
                } else if b & 0xF8 == 0xF0 {
                    4
                } else {
                    return None;
                }; // Invalid start byte or continuation byte

                if offset + len <= buffer.len() {
                    if let Ok(s) = std::str::from_utf8(&buffer[offset..offset + len]) {
                        let c = s.chars().next().unwrap();
                        let is_printable = !c.is_control() && c != '\u{FFFD}';
                        if is_printable {
                            return Some((c, len));
                        }
                    }
                }
                None
            }
            Encoding::Utf16Le | Encoding::Utf16Be => {
                let is_le = *self == Encoding::Utf16Le;
                if offset % 2 != 0 {
                    return None;
                }
                if offset + 2 <= buffer.len() {
                    let u1 = if is_le {
                        u16::from_le_bytes([buffer[offset], buffer[offset + 1]])
                    } else {
                        u16::from_be_bytes([buffer[offset], buffer[offset + 1]])
                    };

                    if (0xD800..=0xDBFF).contains(&u1) {
                        // High surrogate
                        if offset + 4 <= buffer.len() {
                            let u2 = if is_le {
                                u16::from_le_bytes([buffer[offset + 2], buffer[offset + 3]])
                            } else {
                                u16::from_be_bytes([buffer[offset + 2], buffer[offset + 3]])
                            };
                            if (0xDC00..=0xDFFF).contains(&u2) {
                                // Low surrogate
                                if let Some(c) = std::char::decode_utf16([u1, u2]).next().and_then(|r| r.ok()) {
                                    let is_printable = !c.is_control() && c != '\u{FFFD}';
                                    if is_printable {
                                        return Some((c, 4));
                                    }
                                }
                            }
                        }
                    } else if !(0xDC00..=0xDFFF).contains(&u1) {
                        // Not a low surrogate
                        if let Some(c) = std::char::decode_utf16([u1]).next().and_then(|r| r.ok()) {
                            let is_printable = !c.is_control() && c != '\u{FFFD}';
                            if is_printable {
                                return Some((c, 2));
                            }
                        }
                    }
                }
                None
            }
        }
    }

    pub fn is_continuation_byte(&self, buffer: &[u8], offset: usize) -> bool {
        if offset >= buffer.len() {
            return false;
        }
        match self {
            Encoding::Ascii => false,
            Encoding::Utf8 => {
                if buffer[offset] & 0xC0 != 0x80 {
                    return false;
                }
                for i in 1..=3 {
                    if offset >= i {
                        let start_idx = offset - i;
                        if buffer[start_idx] & 0xC0 != 0x80 {
                            if let Some((_, len)) = self.decode_char_at(buffer, start_idx) {
                                return start_idx + len > offset;
                            } else {
                                return false;
                            }
                        }
                    }
                }
                false
            }
            Encoding::Utf16Le | Encoding::Utf16Be => {
                if offset % 2 != 0 {
                    let start_idx = offset - 1;
                    if let Some((_, len)) = self.decode_char_at(buffer, start_idx) {
                        return start_idx + len > offset;
                    }
                    if start_idx >= 2 {
                        let prev_start = start_idx - 2;
                        if let Some((_, len)) = self.decode_char_at(buffer, prev_start) {
                            return prev_start + len > offset;
                        }
                    }
                    false
                } else {
                    if offset >= 2 {
                        let prev_start = offset - 2;
                        if let Some((_, len)) = self.decode_char_at(buffer, prev_start) {
                            return prev_start + len > offset;
                        }
                    }
                    false
                }
            }
        }
    }
}

impl Default for Encoding {
    fn default() -> Self {
        Encoding::Ascii
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::components::data_inspector::format_hex_values;

    #[test]
    fn test_format_hex_values() {
        let (h8, h16, h32, h64) = format_hex_values(&[], false);
        assert_eq!(h8, "--");
        assert_eq!(h16, "--");
        assert_eq!(h32, "--");
        assert_eq!(h64, "--");

        let (h8_p, h16_p, h32_p, h64_p) = format_hex_values(&[0x12, 0x34], false);
        assert_eq!(h8_p, "0x12");
        assert_eq!(h16_p, "0x3412");
        assert_eq!(h32_p, "--");
        assert_eq!(h64_p, "--");

        let bytes = [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF];

        // Little Endian
        let (h8, h16, h32, h64) = format_hex_values(&bytes, false);
        assert_eq!(h8, "0x01");
        assert_eq!(h16, "0x2301");
        assert_eq!(h32, "0x67452301");
        assert_eq!(h64, "0xEFCDAB8967452301");

        // Big Endian
        let (h8_be, h16_be, h32_be, h64_be) = format_hex_values(&bytes, true);
        assert_eq!(h8_be, "0x01");
        assert_eq!(h16_be, "0x0123");
        assert_eq!(h32_be, "0x01234567");
        assert_eq!(h64_be, "0x0123456789ABCDEF");
    }

    #[test]
    fn test_encoding_decode_char_at() {
        use super::Encoding;

        let ascii_bytes = b"Hello World";
        assert_eq!(Encoding::Ascii.decode_char_at(ascii_bytes, 0), Some(('H', 1)));

        let utf8_bytes = "こんにちは".as_bytes();
        assert_eq!(Encoding::Utf8.decode_char_at(utf8_bytes, 0), Some(('こ', 3)));

        let invalid_utf8 = vec![0xFF, 0xFE];
        assert_eq!(Encoding::Utf8.decode_char_at(&invalid_utf8, 0), None);

        let utf16le = vec![0x41, 0x00, 0x42, 0x00];
        assert_eq!(Encoding::Utf16Le.decode_char_at(&utf16le, 0), Some(('A', 2)));
        assert_eq!(Encoding::Utf16Le.decode_char_at(&utf16le, 2), Some(('B', 2)));

        let utf16be = vec![0x00, 0x41, 0x00, 0x42];
        assert_eq!(Encoding::Utf16Be.decode_char_at(&utf16be, 0), Some(('A', 2)));
        assert_eq!(Encoding::Utf16Be.decode_char_at(&utf16be, 2), Some(('B', 2)));
    }
}
