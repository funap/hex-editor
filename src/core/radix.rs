use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DisplayRadix {
    #[default]
    Hexadecimal,
    Decimal,
    Octal,
    Binary,
}

#[allow(dead_code)]
impl DisplayRadix {
    pub const ALL: [DisplayRadix; 4] = [DisplayRadix::Hexadecimal, DisplayRadix::Decimal, DisplayRadix::Octal, DisplayRadix::Binary];

    pub fn label(&self) -> &'static str {
        match self {
            DisplayRadix::Hexadecimal => "Hexadecimal",
            DisplayRadix::Decimal => "Decimal",
            DisplayRadix::Octal => "Octal",
            DisplayRadix::Binary => "Binary",
        }
    }

    pub fn short_label(&self) -> &'static str {
        match self {
            DisplayRadix::Hexadecimal => "HEX",
            DisplayRadix::Decimal => "DEC",
            DisplayRadix::Octal => "OCT",
            DisplayRadix::Binary => "BIN",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ByteGroupSize {
    #[default]
    One = 1,
    Two = 2,
    Four = 4,
    Eight = 8,
}

#[allow(dead_code)]
impl ByteGroupSize {
    pub const ALL: [ByteGroupSize; 4] = [ByteGroupSize::One, ByteGroupSize::Two, ByteGroupSize::Four, ByteGroupSize::Eight];

    pub fn byte_count(&self) -> usize {
        match self {
            ByteGroupSize::One => 1,
            ByteGroupSize::Two => 2,
            ByteGroupSize::Four => 4,
            ByteGroupSize::Eight => 8,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ByteGroupSize::One => "1 Byte (8-bit)",
            ByteGroupSize::Two => "2 Bytes (16-bit)",
            ByteGroupSize::Four => "4 Bytes (32-bit)",
            ByteGroupSize::Eight => "8 Bytes (64-bit)",
        }
    }

    pub fn short_label(&self) -> &'static str {
        match self {
            ByteGroupSize::One => "1B",
            ByteGroupSize::Two => "2B",
            ByteGroupSize::Four => "4B",
            ByteGroupSize::Eight => "8B",
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ByteOrder {
    #[default]
    LittleEndian,
    BigEndian,
}

#[allow(dead_code)]
impl ByteOrder {
    pub fn is_big_endian(&self) -> bool {
        matches!(self, ByteOrder::BigEndian)
    }

    pub fn label(&self) -> &'static str {
        match self {
            ByteOrder::LittleEndian => "Little Endian",
            ByteOrder::BigEndian => "Big Endian",
        }
    }

    pub fn short_label(&self) -> &'static str {
        match self {
            ByteOrder::LittleEndian => "LE",
            ByteOrder::BigEndian => "BE",
        }
    }
}

/// Returns the fixed character width for a given radix and group size combination.
pub fn digit_count(radix: DisplayRadix, group_size: ByteGroupSize) -> usize {
    match (group_size, radix) {
        (ByteGroupSize::One, DisplayRadix::Hexadecimal) => 2,
        (ByteGroupSize::One, DisplayRadix::Decimal) => 3,
        (ByteGroupSize::One, DisplayRadix::Octal) => 3,
        (ByteGroupSize::One, DisplayRadix::Binary) => 8,

        (ByteGroupSize::Two, DisplayRadix::Hexadecimal) => 4,
        (ByteGroupSize::Two, DisplayRadix::Decimal) => 5,
        (ByteGroupSize::Two, DisplayRadix::Octal) => 6,
        (ByteGroupSize::Two, DisplayRadix::Binary) => 16,

        (ByteGroupSize::Four, DisplayRadix::Hexadecimal) => 8,
        (ByteGroupSize::Four, DisplayRadix::Decimal) => 10,
        (ByteGroupSize::Four, DisplayRadix::Octal) => 11,
        (ByteGroupSize::Four, DisplayRadix::Binary) => 32,

        (ByteGroupSize::Eight, DisplayRadix::Hexadecimal) => 16,
        (ByteGroupSize::Eight, DisplayRadix::Decimal) => 20,
        (ByteGroupSize::Eight, DisplayRadix::Octal) => 22,
        (ByteGroupSize::Eight, DisplayRadix::Binary) => 64,
    }
}

/// Formats a group slice of bytes based on the selected Radix, Group Size, Endianness, and starting slot within the group.
///
/// If `start_slot == 0 && bytes.len() == group_size.byte_count()`, the integer value is formatted with exact zero-padding.
/// If `bytes.len() < group_size.byte_count()` or `start_slot > 0` (e.g. midway line break), each byte slot `k` (0..group_size)
/// is formatted at its exact positional offset, with missing slots padded with '.' so byte position can be identified.
pub fn format_group(bytes: &[u8], start_slot: usize, radix: DisplayRadix, group_size: ByteGroupSize, is_big_endian: bool) -> String {
    let expected = group_size.byte_count();
    let total_digits = digit_count(radix, group_size);

    if bytes.is_empty() {
        return ".".repeat(total_digits);
    }

    if start_slot == 0 && bytes.len() >= expected {
        match (group_size, radix) {
            (ByteGroupSize::One, DisplayRadix::Hexadecimal) => format!("{:02x}", bytes[0]),
            (ByteGroupSize::One, DisplayRadix::Decimal) => format!("{:03}", bytes[0]),
            (ByteGroupSize::One, DisplayRadix::Octal) => format!("{:03o}", bytes[0]),
            (ByteGroupSize::One, DisplayRadix::Binary) => format!("{:08b}", bytes[0]),

            (ByteGroupSize::Two, DisplayRadix::Hexadecimal) => {
                let arr: [u8; 2] = [bytes[0], bytes[1]];
                let val = if is_big_endian { u16::from_be_bytes(arr) } else { u16::from_le_bytes(arr) };
                format!("{:04x}", val)
            }
            (ByteGroupSize::Two, DisplayRadix::Decimal) => {
                let arr: [u8; 2] = [bytes[0], bytes[1]];
                let val = if is_big_endian { u16::from_be_bytes(arr) } else { u16::from_le_bytes(arr) };
                format!("{:05}", val)
            }
            (ByteGroupSize::Two, DisplayRadix::Octal) => {
                let arr: [u8; 2] = [bytes[0], bytes[1]];
                let val = if is_big_endian { u16::from_be_bytes(arr) } else { u16::from_le_bytes(arr) };
                format!("{:06o}", val)
            }
            (ByteGroupSize::Two, DisplayRadix::Binary) => {
                let arr: [u8; 2] = [bytes[0], bytes[1]];
                let val = if is_big_endian { u16::from_be_bytes(arr) } else { u16::from_le_bytes(arr) };
                format!("{:016b}", val)
            }

            (ByteGroupSize::Four, DisplayRadix::Hexadecimal) => {
                let arr: [u8; 4] = [bytes[0], bytes[1], bytes[2], bytes[3]];
                let val = if is_big_endian { u32::from_be_bytes(arr) } else { u32::from_le_bytes(arr) };
                format!("{:08x}", val)
            }
            (ByteGroupSize::Four, DisplayRadix::Decimal) => {
                let arr: [u8; 4] = [bytes[0], bytes[1], bytes[2], bytes[3]];
                let val = if is_big_endian { u32::from_be_bytes(arr) } else { u32::from_le_bytes(arr) };
                format!("{:010}", val)
            }
            (ByteGroupSize::Four, DisplayRadix::Octal) => {
                let arr: [u8; 4] = [bytes[0], bytes[1], bytes[2], bytes[3]];
                let val = if is_big_endian { u32::from_be_bytes(arr) } else { u32::from_le_bytes(arr) };
                format!("{:011o}", val)
            }
            (ByteGroupSize::Four, DisplayRadix::Binary) => {
                let arr: [u8; 4] = [bytes[0], bytes[1], bytes[2], bytes[3]];
                let val = if is_big_endian { u32::from_be_bytes(arr) } else { u32::from_le_bytes(arr) };
                format!("{:032b}", val)
            }

            (ByteGroupSize::Eight, DisplayRadix::Hexadecimal) => {
                let arr: [u8; 8] = [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]];
                let val = if is_big_endian { u64::from_be_bytes(arr) } else { u64::from_le_bytes(arr) };
                format!("{:016x}", val)
            }
            (ByteGroupSize::Eight, DisplayRadix::Decimal) => {
                let arr: [u8; 8] = [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]];
                let val = if is_big_endian { u64::from_be_bytes(arr) } else { u64::from_le_bytes(arr) };
                format!("{:020}", val)
            }
            (ByteGroupSize::Eight, DisplayRadix::Octal) => {
                let arr: [u8; 8] = [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]];
                let val = if is_big_endian { u64::from_be_bytes(arr) } else { u64::from_le_bytes(arr) };
                format!("{:022o}", val)
            }
            (ByteGroupSize::Eight, DisplayRadix::Binary) => {
                let arr: [u8; 8] = [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]];
                let val = if is_big_endian { u64::from_be_bytes(arr) } else { u64::from_le_bytes(arr) };
                format!("{:064b}", val)
            }
        }
    } else {
        // Partial group: format each slot k in 0..expected
        let mut formatted = String::with_capacity(total_digits);
        for k in 0..expected {
            if k >= start_slot && (k - start_slot) < bytes.len() {
                let b = bytes[k - start_slot];
                match radix {
                    DisplayRadix::Hexadecimal => formatted.push_str(&format!("{:02x}", b)),
                    DisplayRadix::Decimal => formatted.push_str(&format!("{:03}", b)),
                    DisplayRadix::Octal => formatted.push_str(&format!("{:03o}", b)),
                    DisplayRadix::Binary => formatted.push_str(&format!("{:08b}", b)),
                }
            } else {
                let slot_dots = match radix {
                    DisplayRadix::Hexadecimal => 2,
                    DisplayRadix::Decimal => 3,
                    DisplayRadix::Octal => 3,
                    DisplayRadix::Binary => 8,
                };
                formatted.push_str(&".".repeat(slot_dots));
            }
        }
        if formatted.len() < total_digits {
            formatted.push_str(&".".repeat(total_digits - formatted.len()));
        } else if formatted.len() > total_digits {
            formatted.truncate(total_digits);
        }
        formatted
    }
}

/// Checks if all bytes in the slice are zero.
pub fn is_group_zero(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(|&b| b == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_digit_count() {
        assert_eq!(digit_count(DisplayRadix::Hexadecimal, ByteGroupSize::One), 2);
        assert_eq!(digit_count(DisplayRadix::Decimal, ByteGroupSize::One), 3);
        assert_eq!(digit_count(DisplayRadix::Octal, ByteGroupSize::One), 3);
        assert_eq!(digit_count(DisplayRadix::Binary, ByteGroupSize::One), 8);

        assert_eq!(digit_count(DisplayRadix::Hexadecimal, ByteGroupSize::Two), 4);
        assert_eq!(digit_count(DisplayRadix::Decimal, ByteGroupSize::Two), 5);
        assert_eq!(digit_count(DisplayRadix::Octal, ByteGroupSize::Two), 6);
        assert_eq!(digit_count(DisplayRadix::Binary, ByteGroupSize::Two), 16);

        assert_eq!(digit_count(DisplayRadix::Hexadecimal, ByteGroupSize::Four), 8);
        assert_eq!(digit_count(DisplayRadix::Decimal, ByteGroupSize::Four), 10);
        assert_eq!(digit_count(DisplayRadix::Octal, ByteGroupSize::Four), 11);
        assert_eq!(digit_count(DisplayRadix::Binary, ByteGroupSize::Four), 32);

        assert_eq!(digit_count(DisplayRadix::Hexadecimal, ByteGroupSize::Eight), 16);
        assert_eq!(digit_count(DisplayRadix::Decimal, ByteGroupSize::Eight), 20);
        assert_eq!(digit_count(DisplayRadix::Octal, ByteGroupSize::Eight), 22);
        assert_eq!(digit_count(DisplayRadix::Binary, ByteGroupSize::Eight), 64);
    }

    #[test]
    fn test_format_group_one_byte() {
        let b = &[0x2a]; // 42 decimal, 052 octal, 00101010 binary
        assert_eq!(format_group(b, 0, DisplayRadix::Hexadecimal, ByteGroupSize::One, false), "2a");
        assert_eq!(format_group(b, 0, DisplayRadix::Decimal, ByteGroupSize::One, false), "042");
        assert_eq!(format_group(b, 0, DisplayRadix::Octal, ByteGroupSize::One, false), "052");
        assert_eq!(format_group(b, 0, DisplayRadix::Binary, ByteGroupSize::One, false), "00101010");
    }

    #[test]
    fn test_format_group_two_bytes() {
        // [0x12, 0x34]
        // LE: 0x3412 = 13330 = 032022 octal
        // BE: 0x1234 = 4660 = 011064 octal
        let bytes = &[0x12, 0x34];
        assert_eq!(format_group(bytes, 0, DisplayRadix::Hexadecimal, ByteGroupSize::Two, false), "3412");
        assert_eq!(format_group(bytes, 0, DisplayRadix::Hexadecimal, ByteGroupSize::Two, true), "1234");

        assert_eq!(format_group(bytes, 0, DisplayRadix::Decimal, ByteGroupSize::Two, false), "13330");
        assert_eq!(format_group(bytes, 0, DisplayRadix::Decimal, ByteGroupSize::Two, true), "04660");

        assert_eq!(format_group(bytes, 0, DisplayRadix::Octal, ByteGroupSize::Two, false), "032022");
        assert_eq!(format_group(bytes, 0, DisplayRadix::Octal, ByteGroupSize::Two, true), "011064");

        assert_eq!(format_group(bytes, 0, DisplayRadix::Binary, ByteGroupSize::Two, false), "0011010000010010");
        assert_eq!(format_group(bytes, 0, DisplayRadix::Binary, ByteGroupSize::Two, true), "0001001000110100");
    }

    #[test]
    fn test_format_group_four_bytes() {
        let bytes = &[0x01, 0x02, 0x03, 0x04];
        // LE: 0x04030201 = 67305985
        // BE: 0x01020304 = 16909060
        assert_eq!(format_group(bytes, 0, DisplayRadix::Hexadecimal, ByteGroupSize::Four, false), "04030201");
        assert_eq!(format_group(bytes, 0, DisplayRadix::Hexadecimal, ByteGroupSize::Four, true), "01020304");

        assert_eq!(format_group(bytes, 0, DisplayRadix::Decimal, ByteGroupSize::Four, false), "0067305985");
        assert_eq!(format_group(bytes, 0, DisplayRadix::Decimal, ByteGroupSize::Four, true), "0016909060");
    }

    #[test]
    fn test_format_group_partial_positions_four_bytes() {
        // Line 1: offset 0, byte [00] -> slot 0 -> "00......"
        let b0 = &[0x00];
        assert_eq!(format_group(b0, 0, DisplayRadix::Hexadecimal, ByteGroupSize::Four, false), "00......");

        // Line 2: offset 1, byte [01] -> slot 1 -> "..01...."
        let b1 = &[0x01];
        assert_eq!(format_group(b1, 1, DisplayRadix::Hexadecimal, ByteGroupSize::Four, false), "..01....");

        // Line 3: offset 2, bytes [03, 04] -> slot 2..4 -> "....0304"
        let b23 = &[0x03, 0x04];
        assert_eq!(format_group(b23, 2, DisplayRadix::Hexadecimal, ByteGroupSize::Four, false), "....0304");

        // Single byte at slot 3 -> "......05"
        let b3 = &[0x05];
        assert_eq!(format_group(b3, 3, DisplayRadix::Hexadecimal, ByteGroupSize::Four, false), "......05");
    }

    #[test]
    fn test_format_group_partial_positions_two_bytes() {
        // Slot 0 in 2B -> "12.."
        let b = &[0x12];
        assert_eq!(format_group(b, 0, DisplayRadix::Hexadecimal, ByteGroupSize::Two, false), "12..");

        // Slot 1 in 2B -> "..12"
        assert_eq!(format_group(b, 1, DisplayRadix::Hexadecimal, ByteGroupSize::Two, false), "..12");
    }

    #[test]
    fn test_is_group_zero() {
        assert!(is_group_zero(&[0, 0, 0, 0]));
        assert!(!is_group_zero(&[0, 1, 0, 0]));
        assert!(!is_group_zero(&[]));
    }
}
