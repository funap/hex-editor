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

/// Finds printable strings across multiple buffer segments while limiting the total results.
///
/// If `ranges` is empty, the entire `data` slice is scanned as a single contiguous range.
/// When `ranges` are provided, each range is scanned independently, preventing strings
/// from falsely joining across segment boundaries or address gaps.
pub fn find_strings_segmented_limited(
    data: &[u8],
    ranges: &[std::ops::Range<usize>],
    encoding: Encoding,
    min_chars: usize,
    max_results: usize,
) -> (Vec<StringMatch>, bool) {
    if ranges.is_empty() {
        return find_strings_limited(data, encoding, min_chars, max_results);
    }

    if max_results == 0 || min_chars == 0 || data.is_empty() {
        return (Vec::new(), false);
    }

    let mut all_matches = Vec::new();
    let mut is_truncated = false;

    for range in ranges {
        let start = range.start.min(data.len());
        let end = range.end.min(data.len());
        if start >= end {
            continue;
        }

        let slice = &data[start..end];
        let remaining = if max_results == usize::MAX {
            usize::MAX
        } else {
            let rem = max_results.saturating_sub(all_matches.len());
            if rem == 0 {
                is_truncated = true;
                break;
            }
            rem
        };

        let (mut seg_matches, seg_truncated) = find_strings_limited(slice, encoding, min_chars, remaining);
        for m in &mut seg_matches {
            m.offset += start;
        }
        all_matches.extend(seg_matches);

        if seg_truncated || (max_results != usize::MAX && all_matches.len() >= max_results) {
            is_truncated = true;
            break;
        }
    }

    (all_matches, is_truncated)
}

/// Finds all printable strings across multiple buffer segments.
#[allow(dead_code)]
pub fn find_strings_segmented(data: &[u8], ranges: &[std::ops::Range<usize>], encoding: Encoding, min_chars: usize) -> Vec<StringMatch> {
    find_strings_segmented_limited(data, ranges, encoding, min_chars, usize::MAX).0
}

fn find_strings_impl(data: &[u8], encoding: Encoding, min_chars: usize, max_results: Option<usize>) -> Vec<StringMatch> {
    if data.is_empty() || min_chars == 0 {
        return Vec::new();
    }

    let alignment = encoding.alignment();
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
    fn finds_shift_jis_strings() {
        // "こんにちは" in Shift-JIS: 0x82 0xB1, 0x82 0xF1, 0x82 0xC9, 0x82 0xBF, 0x82 0xCD
        let mut data = vec![0x00];
        data.extend([0x82, 0xB1, 0x82, 0xF1, 0x82, 0xC9, 0x82, 0xBF, 0x82, 0xCD]);
        data.push(0x00);

        assert_eq!(
            find_strings(&data, Encoding::ShiftJis, 4),
            vec![StringMatch {
                offset: 1,
                byte_len: 10,
                text: "こんにちは".to_string(),
            }]
        );
    }

    #[test]
    fn finds_iso8859_strings() {
        // "café" in ISO-8859-1: b"caf\xE9"
        let data = b"\0caf\xE9\0x";
        assert_eq!(
            find_strings(data, Encoding::Iso8859_1, 4),
            vec![StringMatch {
                offset: 1,
                byte_len: 4,
                text: "café".to_string(),
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

    #[test]
    fn prevents_merging_strings_across_segments() {
        // Without segment ranges, "Hello 0Hello 1Hello 2" is merged into one string.
        let data = b"Hello 0Hello 1Hello 2";
        let unsegmented = find_strings(data, Encoding::Ascii, 4);
        assert_eq!(unsegmented.len(), 1);
        assert_eq!(unsegmented[0].text, "Hello 0Hello 1Hello 2");

        // With segment ranges representing address gaps, each string is kept separate.
        let ranges = vec![0..7, 7..14, 14..21];
        let segmented = find_strings_segmented(data, &ranges, Encoding::Ascii, 4);
        assert_eq!(segmented.len(), 3);
        assert_eq!(segmented[0].offset, 0);
        assert_eq!(segmented[0].byte_len, 7);
        assert_eq!(segmented[0].text, "Hello 0");

        assert_eq!(segmented[1].offset, 7);
        assert_eq!(segmented[1].byte_len, 7);
        assert_eq!(segmented[1].text, "Hello 1");

        assert_eq!(segmented[2].offset, 14);
        assert_eq!(segmented[2].byte_len, 7);
        assert_eq!(segmented[2].text, "Hello 2");
    }

    #[test]
    fn segmented_scan_handles_truncation() {
        let data = b"Hello 0Hello 1Hello 2";
        let ranges = vec![0..7, 7..14, 14..21];
        let (matches, is_truncated) = find_strings_segmented_limited(data, &ranges, Encoding::Ascii, 4, 2);
        assert_eq!(matches.len(), 2);
        assert!(is_truncated);
        assert_eq!(matches[0].text, "Hello 0");
        assert_eq!(matches[1].text, "Hello 1");
    }

    #[test]
    fn segmented_scan_empty_ranges_falls_back_to_full() {
        let data = b"Hello 0\0Hello 1";
        let (matches, is_truncated) = find_strings_segmented_limited(data, &[], Encoding::Ascii, 4, 10);
        assert_eq!(matches.len(), 2);
        assert!(!is_truncated);
        assert_eq!(matches[0].text, "Hello 0");
        assert_eq!(matches[1].text, "Hello 1");
    }
}
