#[allow(unused_imports)]
pub use crate::core::address_map::*;
#[allow(unused_imports)]
pub use crate::core::format::*;

/// Automatically detects format (Motorola S-Record or Intel HEX) and parses the input.
pub fn parse_hex_or_mot(content: &str) -> Result<HexImportResult, HexImportError> {
    let first_non_empty_line = content.lines().map(|l| l.trim()).find(|l| !l.is_empty());
    match first_non_empty_line {
        Some(line) => {
            if line.starts_with('S') || line.starts_with('s') {
                parse_motorola_srec(content)
            } else if line.starts_with(':') {
                parse_intel_hex(content)
            } else {
                Err(HexImportError::UnknownFormat)
            }
        }
        None => Err(HexImportError::EmptyInput),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_or_mot_dispatch() {
        let srec = "S0030000FC\nS107100001020304DE\nS9030000FC\n";
        let res = parse_hex_or_mot(srec).expect("srec dispatch");
        assert_eq!(res.format, HexFormat::MotorolaS19);

        let ihex = ":0400000001020304F2\n:00000001FF\n";
        let res2 = parse_hex_or_mot(ihex).expect("ihex dispatch");
        assert_eq!(res2.format, HexFormat::IntelHex);

        assert_eq!(parse_hex_or_mot("").unwrap_err(), HexImportError::EmptyInput);
        assert_eq!(parse_hex_or_mot("INVALID").unwrap_err(), HexImportError::UnknownFormat);
    }
}
