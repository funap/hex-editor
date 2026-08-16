//! Parsing helpers for byte-oriented clipboard operations.

/// Parses common hex-editor clipboard representations into bytes.
///
/// The parser accepts continuous or separated hexadecimal values, C/Rust
/// style `0xNN` arrays, binary octets, escaped `\xNN` strings, JSON-like
/// decimal arrays, and finally falls back to UTF-8 text bytes. The fallback
/// makes pasting ordinary text into the ASCII column useful without making
/// the clipboard format platform-specific.
pub fn parse_paste_bytes(input: &str) -> Option<Vec<u8>> {
    let text = input.trim();
    if text.is_empty() {
        return Some(Vec::new());
    }

    if let Some(bytes) = parse_escaped_bytes(text) {
        return Some(bytes);
    }

    let is_array = (text.starts_with('[') && text.ends_with(']')) || (text.starts_with('{') && text.ends_with('}'));
    if is_array && let Some(bytes) = parse_decimal_array(&text[1..text.len() - 1]) {
        return Some(bytes);
    }

    if let Some(bytes) = parse_prefixed_hex(text)
        && !bytes.is_empty()
    {
        return Some(bytes);
    }

    if let Some(bytes) = parse_binary_tokens(text)
        && !bytes.is_empty()
    {
        return Some(bytes);
    }

    if let Some(bytes) = parse_hex(text)
        && !bytes.is_empty()
    {
        return Some(bytes);
    }

    Some(text.as_bytes().to_vec())
}

fn parse_escaped_bytes(text: &str) -> Option<Vec<u8>> {
    if !text.contains("\\x") && !text.contains("\\X") {
        return None;
    }

    let bytes = text.as_bytes();
    let mut result = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if index + 3 < bytes.len() && bytes[index] == b'\\' && (bytes[index + 1] == b'x' || bytes[index + 1] == b'X') {
            let high = hex_digit(bytes[index + 2])?;
            let low = hex_digit(bytes[index + 3])?;
            result.push((high << 4) | low);
            index += 4;
        } else {
            result.extend(&bytes[index..]);
            break;
        }
    }
    Some(result)
}

fn parse_decimal_array(text: &str) -> Option<Vec<u8>> {
    let tokens: Vec<&str> = text
        .split(|c: char| c == ',' || c.is_ascii_whitespace())
        .filter(|token| !token.is_empty())
        .collect();
    if tokens.is_empty() {
        return Some(Vec::new());
    }

    let mut result = Vec::with_capacity(tokens.len());
    for token in tokens {
        let value = if let Some(hex) = token.strip_prefix("0x").or_else(|| token.strip_prefix("0X")) {
            u16::from_str_radix(hex, 16).ok()?
        } else {
            token.parse::<u16>().ok()?
        };
        result.push(u8::try_from(value).ok()?);
    }
    Some(result)
}

fn parse_prefixed_hex(text: &str) -> Option<Vec<u8>> {
    let mut result = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index + 2 <= bytes.len() {
        if bytes[index] != b'0' || !matches!(bytes[index + 1], b'x' | b'X') {
            index += 1;
            continue;
        }

        let value_start = index + 2;
        let mut value_end = value_start;
        while value_end < bytes.len() && bytes[value_end].is_ascii_hexdigit() {
            value_end += 1;
        }
        let digits = &bytes[value_start..value_end];
        if !(1..=2).contains(&digits.len()) {
            return None;
        }
        result.push(u8::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()?);
        index = value_end;
    }

    (!result.is_empty()).then_some(result)
}

fn parse_binary_tokens(text: &str) -> Option<Vec<u8>> {
    let tokens: Vec<&str> = text
        .split(|c: char| c == ',' || c.is_ascii_whitespace())
        .filter(|token| !token.is_empty())
        .collect();
    if tokens.is_empty() || tokens.iter().any(|token| token.len() != 8 || token.chars().any(|c| c != '0' && c != '1')) {
        return None;
    }
    tokens.into_iter().map(|token| u8::from_str_radix(token, 2).ok()).collect()
}

fn parse_hex(text: &str) -> Option<Vec<u8>> {
    let mut result = Vec::new();
    let mut saw_hex = false;

    for line in text.lines() {
        let line = line.split('|').next().unwrap_or(line);
        let mut tokens = line.split_ascii_whitespace();
        let first = tokens.next();
        let remaining: Vec<&str> = tokens.collect();

        // Skip the address column of the application's conventional hex dump.
        let tokens: Vec<&str> = if first.is_some_and(|token| token.len() >= 6 && is_hex(token)) && !remaining.is_empty() {
            remaining
        } else {
            first.into_iter().chain(remaining).collect()
        };

        for token in tokens {
            let token = token.trim_matches(|c| c == ',' || c == ';');
            if token.is_empty() {
                continue;
            }
            if token.len() == 2 && is_hex(token) {
                result.push(u8::from_str_radix(token, 16).ok()?);
                saw_hex = true;
            } else if token.len() > 2 && token.len().is_multiple_of(2) && is_hex(token) {
                for pair in token.as_bytes().chunks_exact(2) {
                    result.push(u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?);
                }
                saw_hex = true;
            } else if token.contains(':') || token.contains('-') {
                continue;
            } else {
                return None;
            }
        }
    }

    saw_hex.then_some(result)
}

fn is_hex(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_paste_bytes;

    #[test]
    fn parses_common_hex_formats() {
        assert_eq!(parse_paste_bytes("deadbeef"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
        assert_eq!(parse_paste_bytes("DE AD\nBE EF"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
        assert_eq!(parse_paste_bytes("{ 0x41, 0x42 }"), Some(vec![0x41, 0x42]));
        assert_eq!(parse_paste_bytes("01000001 01000010"), Some(vec![0x41, 0x42]));
    }

    #[test]
    fn parses_hex_dump_and_arrays() {
        let dump = "00000000  48 65 6c 6c 6f  |Hello|\n00000005  21              |!|";
        assert_eq!(parse_paste_bytes(dump), Some(b"Hello!".to_vec()));
        assert_eq!(parse_paste_bytes("[65, 66, 255]"), Some(vec![65, 66, 255]));
        assert_eq!(parse_paste_bytes("{65, 66}"), Some(vec![65, 66]));
        assert_eq!(parse_paste_bytes("const unsigned char data[] = {0x41, 0x42};"), Some(vec![0x41, 0x42]));
        assert_eq!(parse_paste_bytes("const DATA: [u8; 2] = [0x41, 0x42];"), Some(vec![0x41, 0x42]));
        assert_eq!(parse_paste_bytes("\\x48\\x69"), Some(b"Hi".to_vec()));
    }

    #[test]
    fn falls_back_to_text() {
        assert_eq!(parse_paste_bytes("hello"), Some(b"hello".to_vec()));
    }
}
