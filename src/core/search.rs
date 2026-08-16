#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SearchMode {
    #[default]
    Hex,
    Text,
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub enum SearchLimit {
    /// Limit to a maximum number of results
    Count(usize),
    /// Limit to results within N bytes from the first match
    Range(usize),
    /// No limit
    Unlimited,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct SearchOptions {
    pub mode: SearchMode,
    pub limit: SearchLimit,
    pub range: Option<std::ops::Range<usize>>,
}

#[allow(dead_code)]
impl SearchOptions {
    pub fn new(mode: SearchMode) -> Self {
        Self {
            mode,
            limit: SearchLimit::Unlimited,
            range: None,
        }
    }

    pub fn with_count_limit(mode: SearchMode, max_results: usize) -> Self {
        Self {
            mode,
            limit: SearchLimit::Count(max_results),
            range: None,
        }
    }

    pub fn with_range_limit(mode: SearchMode, range_bytes: usize) -> Self {
        Self {
            mode,
            limit: SearchLimit::Range(range_bytes),
            range: None,
        }
    }

    pub fn with_range(mode: SearchMode, range: std::ops::Range<usize>) -> Self {
        Self {
            mode,
            limit: SearchLimit::Unlimited,
            range: Some(range),
        }
    }
}

/// A pattern matcher byte, specifying value and mask.
/// E.g., for exact matching `value = B, mask = 0xFF`.
/// For wildcard `value = 0, mask = 0`.
/// For half-byte wildcard `value = 0x40, mask = 0xF0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatternByte {
    pub value: u8,
    pub mask: u8,
}

impl PatternByte {
    /// Create a PatternByte requiring an exact match of the byte value.
    pub fn new_exact(value: u8) -> Self {
        Self { value, mask: 0xFF }
    }

    /// Create a PatternByte that matches any byte value.
    pub fn new_wildcard() -> Self {
        Self { value: 0, mask: 0 }
    }

    /// Check if this PatternByte matches the given byte under its mask.
    #[inline]
    pub fn matches(&self, byte: u8) -> bool {
        (byte & self.mask) == self.value
    }
}

/// Parses a hex search pattern containing hex digits, wildcards (`?` or `*`),
/// and optional spaces/separators into a sequence of `PatternByte` matchers.
/// Returns None if the pattern contains invalid characters.
pub fn parse_hex_pattern(query: &str) -> Option<Vec<PatternByte>> {
    let mut pattern = Vec::new();

    for token in query.split_whitespace() {
        let chars: Vec<char> = token.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c1 = chars[i];
            if i + 1 < chars.len() {
                let c2 = chars[i + 1];
                let is_c1_wild = c1 == '?' || c1 == '*';
                let is_c2_wild = c2 == '?' || c2 == '*';
                let is_c1_hex = c1.is_ascii_hexdigit();
                let is_c2_hex = c2.is_ascii_hexdigit();

                if is_c1_wild && is_c2_wild {
                    pattern.push(PatternByte::new_wildcard());
                } else if is_c1_hex && is_c2_hex {
                    let val_high = c1.to_digit(16).expect("valid hex digit") as u8;
                    let val_low = c2.to_digit(16).expect("valid hex digit") as u8;
                    pattern.push(PatternByte::new_exact((val_high << 4) | val_low));
                } else if is_c1_hex && is_c2_wild {
                    let val_high = c1.to_digit(16).expect("valid hex digit") as u8;
                    pattern.push(PatternByte {
                        value: val_high << 4,
                        mask: 0xF0,
                    });
                } else if is_c1_wild && is_c2_hex {
                    let val_low = c2.to_digit(16).expect("valid hex digit") as u8;
                    pattern.push(PatternByte { value: val_low, mask: 0x0F });
                } else {
                    return None;
                }
                i += 2;
            } else {
                if c1 == '?' || c1 == '*' {
                    pattern.push(PatternByte::new_wildcard());
                } else if c1.is_ascii_hexdigit() {
                    let val = c1.to_digit(16).expect("valid hex digit") as u8;
                    pattern.push(PatternByte::new_exact(val));
                } else {
                    return None;
                }
                i += 1;
            }
        }
    }

    if pattern.is_empty() { None } else { Some(pattern) }
}

/// A stateless function to find occurrences of a pattern in a byte slice.
pub fn find_occurrences(data: &[u8], pattern: &[PatternByte], limit: SearchLimit, range: Option<std::ops::Range<usize>>) -> Vec<usize> {
    if pattern.is_empty() || pattern.len() > data.len() {
        return Vec::new();
    }

    let mut results = Vec::new();
    let pattern_len = pattern.len();
    let data_len = data.len();

    // Determine search range
    let (start, end) = if let Some(r) = range {
        (r.start.min(data_len), r.end.min(data_len))
    } else {
        (0, data_len)
    };

    if start >= end || end < pattern_len {
        return Vec::new();
    }

    use bstr::ByteSlice as _;

    let search_end = end - pattern_len;
    let mut first_match: Option<usize> = None;
    let mut i = start.min(search_end + 1);

    while i <= search_end {
        if pattern[0].mask == 0xFF {
            let target = pattern[0].value;
            match data[i..=search_end].find_byte(target) {
                Some(pos) => {
                    i += pos;
                }
                None => break,
            }
        } else if !pattern[0].matches(data[i]) {
            i += 1;
            continue;
        }

        let mut matched = true;
        for j in 1..pattern_len {
            if !pattern[j].matches(data[i + j]) {
                matched = false;
                break;
            }
        }

        if matched {
            // Track first match for range-based limiting
            if first_match.is_none() {
                first_match = Some(i);
            }

            // Check limit
            match limit {
                SearchLimit::Count(max) => {
                    results.push(i);
                    if results.len() >= max {
                        break;
                    }
                }
                SearchLimit::Range(range_bytes) => {
                    if let Some(first) = first_match
                        && i >= first + range_bytes
                    {
                        break;
                    }
                    results.push(i);
                }
                SearchLimit::Unlimited => {
                    results.push(i);
                }
            }
        }

        i += 1;
    }

    results
}

/// Helper to find a single occurrence scanning forward in range `start..end`.
fn find_single_forward(data: &[u8], pattern: &[PatternByte], start: usize, end: usize) -> Option<usize> {
    if pattern.is_empty() || pattern.len() > data.len() {
        return None;
    }
    let data_len = data.len();
    let start = start.min(data_len);
    let end = end.min(data_len);
    let pattern_len = pattern.len();

    if start >= end || end < pattern_len {
        return None;
    }

    use bstr::ByteSlice as _;
    let search_end = end - pattern_len;
    let mut i = start.min(search_end + 1);

    while i <= search_end {
        if pattern[0].mask == 0xFF {
            let target = pattern[0].value;
            match data[i..=search_end].find_byte(target) {
                Some(pos) => {
                    i += pos;
                }
                None => break,
            }
        } else if !pattern[0].matches(data[i]) {
            i += 1;
            continue;
        }

        let mut matched = true;
        for j in 1..pattern_len {
            if !pattern[j].matches(data[i + j]) {
                matched = false;
                break;
            }
        }

        if matched {
            return Some(i);
        }

        i += 1;
    }

    None
}

/// Helper to find a single occurrence scanning backward in range `start..end`.
fn find_single_backward(data: &[u8], pattern: &[PatternByte], start: usize, end: usize) -> Option<usize> {
    if pattern.is_empty() || pattern.len() > data.len() {
        return None;
    }
    let data_len = data.len();
    let start = start.min(data_len);
    let end = end.min(data_len);
    let pattern_len = pattern.len();

    if start >= end || end < pattern_len {
        return None;
    }

    use bstr::ByteSlice as _;
    let search_end = end - pattern_len;
    if start > search_end {
        return None;
    }

    let mut i = search_end;
    loop {
        if pattern[0].mask == 0xFF {
            let target = pattern[0].value;
            match data[start..=i].rfind_byte(target) {
                Some(pos) => {
                    i = start + pos;
                }
                None => break,
            }
        } else if !pattern[0].matches(data[i]) {
            if i <= start {
                break;
            }
            i -= 1;
            continue;
        }

        let mut matched = true;
        for j in 1..pattern_len {
            if !pattern[j].matches(data[i + j]) {
                matched = false;
                break;
            }
        }

        if matched {
            return Some(i);
        }

        if i <= start {
            break;
        }
        i -= 1;
    }

    None
}

/// Finds the next occurrence of `pattern` starting at `from_offset`, wrapping around to 0 if needed.
pub fn find_next_occurrence(data: &[u8], pattern: &[PatternByte], from_offset: usize) -> Option<usize> {
    if data.is_empty() || pattern.is_empty() || pattern.len() > data.len() {
        return None;
    }

    // 1. Search forward from `from_offset` to end of data
    if let Some(pos) = find_single_forward(data, pattern, from_offset, data.len()) {
        return Some(pos);
    }

    // 2. Wrap-around: search from 0 to `from_offset`
    if from_offset > 0 {
        let wrap_end = (from_offset + pattern.len().saturating_sub(1)).min(data.len());
        return find_single_forward(data, pattern, 0, wrap_end);
    }

    None
}

/// Finds the previous occurrence of `pattern` strictly starting before `before_offset`, wrapping around to the end of data if needed.
pub fn find_prev_occurrence(data: &[u8], pattern: &[PatternByte], before_offset: usize) -> Option<usize> {
    if data.is_empty() || pattern.is_empty() || pattern.len() > data.len() {
        return None;
    }

    // 1. Search backward strictly before `before_offset` if `before_offset > 0`
    if before_offset > 0 {
        let end = (before_offset + pattern.len().saturating_sub(1)).min(data.len());
        if let Some(pos) = find_single_backward(data, pattern, 0, end) {
            return Some(pos);
        }
    }

    // 2. Wrap-around: search backward from end of data
    find_single_backward(data, pattern, 0, data.len())
}

/// Finds all occurrences of `pattern` within the specified `range` of `data`.
pub fn find_occurrences_in_range(data: &[u8], pattern: &[PatternByte], range: std::ops::Range<usize>) -> Vec<usize> {
    find_occurrences(data, pattern, SearchLimit::Unlimited, Some(range))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_exact_pattern(bytes: &[u8]) -> Vec<PatternByte> {
        bytes.iter().map(|&b| PatternByte::new_exact(b)).collect()
    }

    #[test]
    fn test_find_occurrences_text() {
        let data = b"Hello World Hello";
        let pattern = to_exact_pattern(b"Hello");
        let results = find_occurrences(data, &pattern, SearchLimit::Unlimited, None);
        assert_eq!(results, vec![0, 12]);
    }

    #[test]
    fn test_find_occurrences_limit_count() {
        let data = b"AA AA AA AA";
        let pattern = to_exact_pattern(b"AA");
        let results = find_occurrences(data, &pattern, SearchLimit::Count(2), None);
        assert_eq!(results, vec![0, 3]);
    }

    #[test]
    fn test_find_occurrences_limit_range() {
        let data = b"AA..AA....AA";
        let pattern = to_exact_pattern(b"AA");
        let results = find_occurrences(data, &pattern, SearchLimit::Range(5), None);
        assert_eq!(results, vec![0, 4]);
    }

    #[test]
    fn test_find_occurrences_range_restriction() {
        let data = b"0123456789";
        let pattern = to_exact_pattern(b"34");

        let results = find_occurrences(data, &pattern, SearchLimit::Unlimited, Some(2..6));
        assert_eq!(results, vec![3]);

        let results = find_occurrences(data, &pattern, SearchLimit::Unlimited, Some(5..8));
        assert!(results.is_empty());

        let results = find_occurrences(data, &pattern, SearchLimit::Unlimited, Some(0..3));
        assert!(results.is_empty());
    }

    #[test]
    fn test_empty_pattern_or_data() {
        assert!(find_occurrences(b"", &[], SearchLimit::Unlimited, None).is_empty());
        assert!(find_occurrences(b"data", &[], SearchLimit::Unlimited, None).is_empty());
        assert_eq!(find_next_occurrence(b"", &[], 0), None);
        assert_eq!(find_prev_occurrence(b"", &[], 0), None);
    }

    #[test]
    fn test_find_next_and_prev_occurrence() {
        let data = b"abc 123 abc 456 abc";
        let pattern = to_exact_pattern(b"abc");

        // Forward from 0 -> 0
        assert_eq!(find_next_occurrence(data, &pattern, 0), Some(0));
        // Forward from 1 -> 8
        assert_eq!(find_next_occurrence(data, &pattern, 1), Some(8));
        // Forward from 9 -> 16
        assert_eq!(find_next_occurrence(data, &pattern, 9), Some(16));
        // Forward from 17 -> wraps to 0
        assert_eq!(find_next_occurrence(data, &pattern, 17), Some(0));

        // Backward from 16 (matches at 0, 8, 16) -> strictly before 16 is 8
        assert_eq!(find_prev_occurrence(data, &pattern, 16), Some(8));
        // Backward before 8 is 0
        assert_eq!(find_prev_occurrence(data, &pattern, 8), Some(0));
        // Backward before 0 wraps to 16
        assert_eq!(find_prev_occurrence(data, &pattern, 0), Some(16));
    }

    #[test]
    fn test_find_occurrences_in_range() {
        let data = b"XX--XX--XX--XX";
        let pattern = to_exact_pattern(b"XX");

        let matches = find_occurrences_in_range(data, &pattern, 4..11);
        assert_eq!(matches, vec![4, 8]);
    }

    #[test]
    fn test_parse_hex_pattern() {
        let parsed = parse_hex_pattern("48 89 ?? 24 ?8").unwrap();
        assert_eq!(parsed.len(), 5);
        assert_eq!(parsed[0], PatternByte::new_exact(0x48));
        assert_eq!(parsed[1], PatternByte::new_exact(0x89));
        assert_eq!(parsed[2], PatternByte::new_wildcard());
        assert_eq!(parsed[3], PatternByte::new_exact(0x24));
        assert_eq!(parsed[4], PatternByte { value: 0x08, mask: 0x0F });

        // Contiguous
        let parsed2 = parse_hex_pattern("4889??24?8").unwrap();
        assert_eq!(parsed2, parsed);

        // Asterisk wildcard
        let parsed3 = parse_hex_pattern("48 89 ** 24 *8").unwrap();
        assert_eq!(parsed3, parsed);

        // Half-byte high-nibble wildcard
        let parsed4 = parse_hex_pattern("4?").unwrap();
        assert_eq!(parsed4.len(), 1);
        assert_eq!(parsed4[0], PatternByte { value: 0x40, mask: 0xF0 });

        // Single digit padding
        let parsed5 = parse_hex_pattern("A").unwrap();
        assert_eq!(parsed5.len(), 1);
        assert_eq!(parsed5[0], PatternByte::new_exact(0x0A));
    }

    #[test]
    fn test_find_occurrences_wildcard() {
        let data = &[0x48, 0x89, 0x54, 0x24, 0x08, 0x48, 0x89, 0x4c, 0x24, 0x18];
        let pattern = parse_hex_pattern("48 89 ?? 24 ?8").unwrap();
        let results = find_occurrences(data, &pattern, SearchLimit::Unlimited, None);
        assert_eq!(results, vec![0, 5]);

        let pattern_half = parse_hex_pattern("24 ?8").unwrap();
        let results_half = find_occurrences(data, &pattern_half, SearchLimit::Unlimited, None);
        assert_eq!(results_half, vec![3, 8]);
    }
}
