use std::fmt;

/// The radix mode used for interpreting undecorated numeric offset inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GotoRadix {
    #[default]
    Hex,
    Dec,
}

#[allow(dead_code)]
impl GotoRadix {
    pub fn label(&self) -> &'static str {
        match self {
            GotoRadix::Hex => "Hex",
            GotoRadix::Dec => "Dec",
        }
    }
}

/// The jump origin / mode of the parsed offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GotoOrigin {
    Absolute,
    RelativeForward,
    RelativeBackward,
    FromEnd,
    Percentage,
    Line,
}

/// The result of parsing a goto offset expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedGotoOffset {
    /// The target byte offset clamped to the document bounds.
    pub target_offset: usize,
    /// The raw unclamped target offset calculated from the expression.
    pub raw_target: usize,
    /// The origin / interpretation mode used for this offset.
    pub origin: GotoOrigin,
    /// True if the raw target offset exceeded the document size.
    pub is_out_of_bounds: bool,
}

/// Errors that can occur when parsing a goto offset expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GotoParseError {
    Empty,
    InvalidFormat(String),
}

impl fmt::Display for GotoParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GotoParseError::Empty => write!(f, "Please enter an offset"),
            GotoParseError::InvalidFormat(msg) => write!(f, "Invalid offset: {}", msg),
        }
    }
}

impl std::error::Error for GotoParseError {}

/// Parses a number string with optional base prefix/suffix or default radix.
fn parse_number_with_base(s: &str, default_radix: GotoRadix) -> Result<usize, GotoParseError> {
    let clean: String = s.chars().filter(|c| *c != '_' && !c.is_whitespace()).collect();
    if clean.is_empty() {
        return Err(GotoParseError::Empty);
    }

    let lower = clean.to_ascii_lowercase();

    // 0x / 0X prefix
    if let Some(rest) = lower.strip_prefix("0x") {
        if rest.is_empty() {
            return Err(GotoParseError::InvalidFormat("Missing digits after 0x".into()));
        }
        return usize::from_str_radix(rest, 16).map_err(|_| GotoParseError::InvalidFormat(format!("'{}' is not a valid hex number", clean)));
    }

    // $ prefix (hex)
    if let Some(rest) = lower.strip_prefix('$') {
        if rest.is_empty() {
            return Err(GotoParseError::InvalidFormat("Missing digits after $".into()));
        }
        return usize::from_str_radix(rest, 16).map_err(|_| GotoParseError::InvalidFormat(format!("'{}' is not a valid hex number", clean)));
    }

    // 0d prefix or # prefix (decimal)
    if let Some(rest) = lower.strip_prefix("0d").or_else(|| lower.strip_prefix('#')) {
        if rest.is_empty() {
            return Err(GotoParseError::InvalidFormat("Missing digits after decimal prefix".into()));
        }
        return rest
            .parse::<usize>()
            .map_err(|_| GotoParseError::InvalidFormat(format!("'{}' is not a valid decimal number", clean)));
    }

    // 0o prefix (octal)
    if let Some(rest) = lower.strip_prefix("0o") {
        if rest.is_empty() {
            return Err(GotoParseError::InvalidFormat("Missing digits after 0o".into()));
        }
        return usize::from_str_radix(rest, 8).map_err(|_| GotoParseError::InvalidFormat(format!("'{}' is not a valid octal number", clean)));
    }

    // 0b prefix (binary)
    if let Some(rest) = lower.strip_prefix("0b") {
        if rest.is_empty() {
            return Err(GotoParseError::InvalidFormat("Missing digits after 0b".into()));
        }
        return usize::from_str_radix(rest, 2).map_err(|_| GotoParseError::InvalidFormat(format!("'{}' is not a valid binary number", clean)));
    }

    // h suffix (hex)
    if let Some(rest) = lower.strip_suffix('h')
        && !rest.is_empty()
        && rest.chars().all(|c| c.is_ascii_hexdigit())
    {
        return usize::from_str_radix(rest, 16).map_err(|_| GotoParseError::InvalidFormat(format!("'{}' is not a valid hex number", clean)));
    }

    // Auto-detect hex if contains a-f
    let contains_hex_letter = clean.chars().any(|c| matches!(c, 'a'..='f' | 'A'..='F'));
    if contains_hex_letter {
        return usize::from_str_radix(&clean, 16).map_err(|_| GotoParseError::InvalidFormat(format!("'{}' is not a valid hex number", clean)));
    }

    // Fallback to default radix
    match default_radix {
        GotoRadix::Hex => usize::from_str_radix(&clean, 16).map_err(|_| GotoParseError::InvalidFormat(format!("'{}' is not a valid hex number", clean))),
        GotoRadix::Dec => clean
            .parse::<usize>()
            .map_err(|_| GotoParseError::InvalidFormat(format!("'{}' is not a valid decimal number", clean))),
    }
}

/// Parses a goto offset expression from user input.
///
/// Supports:
/// - Hexadecimal (`0x1000`, `1A0`, `$100`, `1000h`)
/// - Decimal (`256`, `0d256`, `#256`)
/// - Octal (`0o777`) and Binary (`0b10101010`)
/// - Relative forwards (`+0x100`, `+50`)
/// - Relative backwards (`-0x20`, `-10`)
/// - Relative from end (`end-0x10`, `eof-50`)
/// - Named positions (`begin`, `start`, `first`, `end`, `eof`, `last`)
/// - Percentage (`50%`, `75.5%`, `100%`)
/// - Line / Row syntax (`L10`, `line 10`, `:10`)
/// - Segment:Offset syntax (`0000:0100`)
pub fn parse_goto_offset(input: &str, current_cursor: usize, total_size: usize, default_radix: GotoRadix) -> Result<ParsedGotoOffset, GotoParseError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(GotoParseError::Empty);
    }

    let lower = trimmed.to_ascii_lowercase();

    // Named positions
    if matches!(lower.as_str(), "begin" | "start" | "first") {
        return Ok(ParsedGotoOffset {
            target_offset: 0,
            raw_target: 0,
            origin: GotoOrigin::Absolute,
            is_out_of_bounds: false,
        });
    }
    if matches!(lower.as_str(), "end" | "eof" | "last") {
        let target = total_size.saturating_sub(1);
        return Ok(ParsedGotoOffset {
            target_offset: target,
            raw_target: target,
            origin: GotoOrigin::FromEnd,
            is_out_of_bounds: false,
        });
    }

    // Percentage: "50%", "75.5%"
    if let Some(pct_str) = trimmed.strip_suffix('%') {
        let pct_clean = pct_str.trim();
        let pct: f64 = pct_clean
            .parse()
            .map_err(|_| GotoParseError::InvalidFormat(format!("'{}' is not a valid percentage", pct_clean)))?;
        if pct < 0.0 {
            return Err(GotoParseError::InvalidFormat("Percentage cannot be negative".into()));
        }
        let raw = ((total_size as f64) * (pct / 100.0)).round() as usize;
        let clamped = if total_size == 0 { 0 } else { raw.min(total_size.saturating_sub(1)) };
        let is_out_of_bounds = raw >= total_size && total_size > 0;
        return Ok(ParsedGotoOffset {
            target_offset: clamped,
            raw_target: raw,
            origin: GotoOrigin::Percentage,
            is_out_of_bounds,
        });
    }

    // Line / Row syntax: "L10", "l 10", ":10", "line 10"
    let line_str = if let Some(rest) = lower.strip_prefix("line") {
        Some(rest.trim())
    } else if let Some(rest) = lower.strip_prefix('l') {
        Some(rest.trim())
    } else {
        lower.strip_prefix(':').map(|rest| rest.trim())
    };

    if let Some(line_num_str) = line_str
        && !line_num_str.is_empty()
    {
        let line_num = parse_number_with_base(line_num_str, GotoRadix::Dec)?;
        // 1-indexed row: L1 = row 0 (offset 0), L2 = row 1 (offset 16)
        let row = line_num.saturating_sub(1);
        let raw = row.saturating_mul(16);
        let clamped = if total_size == 0 { 0 } else { raw.min(total_size.saturating_sub(1)) };
        let is_out_of_bounds = raw >= total_size && total_size > 0;
        return Ok(ParsedGotoOffset {
            target_offset: clamped,
            raw_target: raw,
            origin: GotoOrigin::Line,
            is_out_of_bounds,
        });
    }

    // Relative from end: "end-0x10", "eof-50"
    let end_relative_str = if let Some(rest) = lower.strip_prefix("end-") {
        Some(rest.trim())
    } else {
        lower.strip_prefix("eof-").map(|rest| rest.trim())
    };

    if let Some(num_str) = end_relative_str {
        let val = parse_number_with_base(num_str, default_radix)?;
        let raw = total_size.saturating_sub(val);
        let clamped = if total_size == 0 { 0 } else { raw.min(total_size.saturating_sub(1)) };
        return Ok(ParsedGotoOffset {
            target_offset: clamped,
            raw_target: raw,
            origin: GotoOrigin::FromEnd,
            is_out_of_bounds: false,
        });
    }

    // Relative forward: "+0x100", "+50"
    if let Some(num_str) = trimmed.strip_prefix('+') {
        let val = parse_number_with_base(num_str.trim(), default_radix)?;
        let raw = current_cursor.saturating_add(val);
        let clamped = if total_size == 0 { 0 } else { raw.min(total_size.saturating_sub(1)) };
        let is_out_of_bounds = raw >= total_size && total_size > 0;
        return Ok(ParsedGotoOffset {
            target_offset: clamped,
            raw_target: raw,
            origin: GotoOrigin::RelativeForward,
            is_out_of_bounds,
        });
    }

    // Relative backward: "-0x20", "-10"
    if let Some(num_str) = trimmed.strip_prefix('-') {
        let val = parse_number_with_base(num_str.trim(), default_radix)?;
        let raw = current_cursor.saturating_sub(val);
        let clamped = if total_size == 0 { 0 } else { raw.min(total_size.saturating_sub(1)) };
        return Ok(ParsedGotoOffset {
            target_offset: clamped,
            raw_target: raw,
            origin: GotoOrigin::RelativeBackward,
            is_out_of_bounds: false,
        });
    }

    // Segment:Offset syntax: "0000:0100" (both in Hex)
    if trimmed.contains(':') && !trimmed.starts_with(':') {
        let parts: Vec<&str> = trimmed.split(':').collect();
        if parts.len() == 2 {
            let seg = parse_number_with_base(parts[0].trim(), GotoRadix::Hex)?;
            let off = parse_number_with_base(parts[1].trim(), GotoRadix::Hex)?;
            let raw = seg.saturating_mul(16).saturating_add(off);
            let clamped = if total_size == 0 { 0 } else { raw.min(total_size.saturating_sub(1)) };
            let is_out_of_bounds = raw >= total_size && total_size > 0;
            return Ok(ParsedGotoOffset {
                target_offset: clamped,
                raw_target: raw,
                origin: GotoOrigin::Absolute,
                is_out_of_bounds,
            });
        }
    }

    // Standard absolute offset
    let val = parse_number_with_base(trimmed, default_radix)?;
    let clamped = if total_size == 0 { 0 } else { val.min(total_size.saturating_sub(1)) };
    let is_out_of_bounds = val >= total_size && total_size > 0;
    Ok(ParsedGotoOffset {
        target_offset: clamped,
        raw_target: val,
        origin: GotoOrigin::Absolute,
        is_out_of_bounds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_formats() {
        let total = 0x10000;
        let cursor = 0x100;

        // 0x prefix
        let res = parse_goto_offset("0x200", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 0x200);
        assert_eq!(res.origin, GotoOrigin::Absolute);
        assert!(!res.is_out_of_bounds);

        // $ prefix
        let res = parse_goto_offset("$300", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 0x300);

        // h suffix
        let res = parse_goto_offset("400h", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 0x400);

        // Contains hex letter a-f with Dec mode
        let res = parse_goto_offset("1a0", cursor, total, GotoRadix::Dec).unwrap();
        assert_eq!(res.target_offset, 0x1A0);

        // Underscores
        let res = parse_goto_offset("0x10_00", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 0x1000);
    }

    #[test]
    fn test_parse_dec_formats() {
        let total = 10000;
        let cursor = 100;

        // Plain decimal with Dec mode
        let res = parse_goto_offset("500", cursor, total, GotoRadix::Dec).unwrap();
        assert_eq!(res.target_offset, 500);

        // 0d prefix with Hex mode
        let res = parse_goto_offset("0d500", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 500);

        // # prefix
        let res = parse_goto_offset("#500", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 500);
    }

    #[test]
    fn test_parse_oct_and_bin_formats() {
        let total = 10000;
        let cursor = 100;

        // 0o prefix (octal)
        let res = parse_goto_offset("0o77", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 63);

        // 0b prefix (binary)
        let res = parse_goto_offset("0b1010", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 10);
    }

    #[test]
    fn test_parse_relative_offsets() {
        let total = 0x1000;
        let cursor = 0x100;

        // Relative forward (+)
        let res = parse_goto_offset("+0x50", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 0x150);
        assert_eq!(res.origin, GotoOrigin::RelativeForward);

        // Relative backward (-)
        let res = parse_goto_offset("-0x30", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 0xD0);
        assert_eq!(res.origin, GotoOrigin::RelativeBackward);

        // Relative backward underflow saturates to 0
        let res = parse_goto_offset("-0x200", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 0);

        // From end
        let res = parse_goto_offset("end-0x10", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 0xFF0);
        assert_eq!(res.origin, GotoOrigin::FromEnd);
    }

    #[test]
    fn test_parse_named_positions() {
        let total = 0x1000;
        let cursor = 0x100;

        let res = parse_goto_offset("begin", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 0);

        let res = parse_goto_offset("start", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 0);

        let res = parse_goto_offset("end", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 0xFFF);

        let res = parse_goto_offset("eof", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 0xFFF);
    }

    #[test]
    fn test_parse_percentage() {
        let total = 1000;
        let cursor = 0;

        let res = parse_goto_offset("50%", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 500);
        assert_eq!(res.origin, GotoOrigin::Percentage);

        let res = parse_goto_offset("100%", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 999);
    }

    #[test]
    fn test_parse_line() {
        let total = 1000;
        let cursor = 0;

        // Line 1 -> row 0 -> offset 0
        let res = parse_goto_offset("L1", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 0);
        assert_eq!(res.origin, GotoOrigin::Line);

        // Line 2 -> row 1 -> offset 16
        let res = parse_goto_offset("L2", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 16);

        // Line 10 -> row 9 -> offset 144
        let res = parse_goto_offset(":10", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 144);
    }

    #[test]
    fn test_parse_segment_offset() {
        let total = 0x10000;
        let cursor = 0;

        // 0010:0020 -> 0x10 * 16 + 0x20 = 0x120
        let res = parse_goto_offset("0010:0020", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 0x120);
    }

    #[test]
    fn test_out_of_bounds_clamping() {
        let total = 0x100;
        let cursor = 0;

        let res = parse_goto_offset("0x500", cursor, total, GotoRadix::Hex).unwrap();
        assert_eq!(res.target_offset, 0xFF);
        assert_eq!(res.raw_target, 0x500);
        assert!(res.is_out_of_bounds);
    }

    #[test]
    fn test_invalid_formats() {
        let total = 1000;
        let cursor = 0;

        assert!(matches!(parse_goto_offset("", cursor, total, GotoRadix::Hex), Err(GotoParseError::Empty)));
        assert!(matches!(parse_goto_offset("   ", cursor, total, GotoRadix::Hex), Err(GotoParseError::Empty)));
        assert!(matches!(
            parse_goto_offset("0xZZZ", cursor, total, GotoRadix::Hex),
            Err(GotoParseError::InvalidFormat(_))
        ));
        assert!(matches!(
            parse_goto_offset("xyz", cursor, total, GotoRadix::Dec),
            Err(GotoParseError::InvalidFormat(_))
        ));
    }
}
