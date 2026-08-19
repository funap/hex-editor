use crate::core::encoding::Encoding;

/// The default minimum number of decoded characters in a string match.
pub const DEFAULT_MIN_STRING_LENGTH: usize = 4;

/// A printable string found in a byte buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringMatch {
    /// The byte offset at which the string starts.
    pub offset: usize,
    /// The number of bytes occupied by the decoded string.
    pub byte_len: usize,
    /// The decoded printable characters.
    pub text: String,
}

/// Finds all runs of printable characters using `encoding`.
///
/// A run is returned as one match, so suffixes of a long printable run are
/// not returned as separate matches. `min_chars` counts decoded characters;
/// for UTF-16, surrogate pairs count as one character.
pub fn find_strings(data: &[u8], encoding: Encoding, min_chars: usize) -> Vec<StringMatch> {
    find_strings_impl(data, encoding, min_chars, None)
}

/// Finds printable strings while limiting the number of returned matches.
///
/// The returned boolean is true when matches were omitted because the limit
/// was reached.
pub fn find_strings_limited(data: &[u8], encoding: Encoding, min_chars: usize, max_results: usize) -> (Vec<StringMatch>, bool) {
    if max_results == usize::MAX {
        return (find_strings(data, encoding, min_chars), false);
    }

    let scan_limit = max_results.saturating_add(1);
    let mut matches = find_strings_impl(data, encoding, min_chars, Some(scan_limit));
    let is_truncated = matches.len() > max_results;
    if is_truncated {
        matches.truncate(max_results);
    }
    (matches, is_truncated)
}

fn find_strings_impl(data: &[u8], encoding: Encoding, min_chars: usize, max_results: Option<usize>) -> Vec<StringMatch> {
    if data.is_empty() || min_chars == 0 {
        return Vec::new();
    }

    let alignment = match encoding {
        Encoding::Utf16Le | Encoding::Utf16Be => 2,
        Encoding::Ascii | Encoding::Utf8 => 1,
    };
    let mut matches = Vec::new();
    let mut offset = 0;

    while offset < data.len() {
        let start = offset;
        let mut cursor = offset;
        let mut character_count = 0;
        let mut text = String::new();

        while let Some((character, byte_len)) = encoding.decode_char_at(data, cursor) {
            text.push(character);
            character_count += 1;
            cursor += byte_len;
        }

        if cursor == start {
            offset += alignment;
            continue;
        }

        if character_count >= min_chars {
            matches.push(StringMatch {
                offset: start,
                byte_len: cursor - start,
                text,
            });

            if let Some(max_results) = max_results
                && matches.len() >= max_results
            {
                break;
            }
        }

        // Skip the complete run so a long string is emitted only once.
        offset = cursor;
    }

    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_ascii_runs_and_skips_short_runs() {
        let data = b"\0abc\0hello world\0X";

        assert_eq!(
            find_strings(data, Encoding::Ascii, 4),
            vec![StringMatch {
                offset: 5,
                byte_len: 11,
                text: "hello world".to_string(),
            }]
        );
    }

    #[test]
    fn finds_utf8_strings_by_character_count() {
        let data = "\0こんにちは\0abc".as_bytes();

        assert_eq!(
            find_strings(data, Encoding::Utf8, 4),
            vec![StringMatch {
                offset: 1,
                byte_len: "こんにちは".len(),
                text: "こんにちは".to_string(),
            }]
        );
    }

    #[test]
    fn finds_utf16_strings_using_code_unit_alignment() {
        let mut data = vec![0x00, 0x00];
        for character in "Test".chars() {
            data.extend(Encoding::Utf16Le.encode_char(character).expect("UTF-16 encodes all chars"));
        }
        data.extend([0x00, 0x00]);

        assert_eq!(
            find_strings(&data, Encoding::Utf16Le, 4),
            vec![StringMatch {
                offset: 2,
                byte_len: 8,
                text: "Test".to_string(),
            }]
        );
    }

    #[test]
    fn reports_when_results_are_truncated() {
        let (matches, is_truncated) = find_strings_limited(b"one\0two2\0three", Encoding::Ascii, 3, 2);

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].text, "one");
        assert_eq!(matches[1].text, "two2");
        assert!(is_truncated);
    }

    #[test]
    fn zero_minimum_length_does_not_match() {
        assert!(find_strings(b"printable", Encoding::Ascii, 0).is_empty());
    }
}
