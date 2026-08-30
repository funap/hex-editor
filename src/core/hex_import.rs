use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Represents a contiguous range of memory from an imported object file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySegment {
    /// Starting offset within the linear buffer
    pub buffer_offset: usize,
    /// Physical memory address in the original source file
    pub address: usize,
    /// Byte length of this segment
    pub length: usize,
}

impl MemorySegment {
    #[inline]
    pub fn end_address(&self) -> usize {
        self.address.saturating_add(self.length)
    }

    #[inline]
    pub fn end_buffer_offset(&self) -> usize {
        self.buffer_offset.saturating_add(self.length)
    }

    #[inline]
    #[allow(dead_code)]
    pub fn contains_buffer_offset(&self, offset: usize) -> bool {
        offset >= self.buffer_offset && offset < self.end_buffer_offset()
    }

    #[inline]
    pub fn contains_address(&self, addr: usize) -> bool {
        addr >= self.address && addr < self.end_address()
    }
}

/// Formatting options and metadata for exporting Motorola S-Record and Intel HEX files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HexFormatOptions {
    /// Number of data bytes per record line (default 20 / 0x14 for Motorola S-Record, 16 / 0x10 for Intel HEX)
    pub record_data_length: usize,
    /// Explicit address width in bytes (2 = S1, 3 = S2, 4 = S3; 0 = auto-detect)
    pub address_width: usize,
    /// Header string for S0 record (None = no S0 record)
    pub header: Option<String>,
    /// Optional execution entry point address for termination record
    pub entry_point: Option<usize>,
    /// Whether an S5/S6 record count was present
    pub has_count_record: bool,
    /// Preferred line ending (CRLF or LF)
    pub crlf: bool,
}

impl Default for HexFormatOptions {
    fn default() -> Self {
        Self {
            record_data_length: 20, // standard 0x14 for Motorola S-Record
            address_width: 0,       // auto-detect
            header: None,
            entry_point: None,
            has_count_record: false,
            crlf: false,
        }
    }
}

/// Maps buffer offsets to physical memory addresses across one or more segments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressMap {
    pub segments: Vec<MemorySegment>,
    pub format_options: HexFormatOptions,
}

impl Default for AddressMap {
    fn default() -> Self {
        Self {
            segments: vec![MemorySegment {
                buffer_offset: 0,
                address: 0,
                length: usize::MAX,
            }],
            format_options: HexFormatOptions::default(),
        }
    }
}

impl AddressMap {
    /// Creates a single contiguous address map with a custom base address.
    #[allow(dead_code)]
    pub fn single_segment(base_address: usize, total_size: usize) -> Self {
        Self {
            segments: vec![MemorySegment {
                buffer_offset: 0,
                address: base_address,
                length: total_size,
            }],
            format_options: HexFormatOptions::default(),
        }
    }

    /// Creates an AddressMap from a list of memory segments.
    #[allow(dead_code)]
    pub fn from_segments(segments: Vec<MemorySegment>) -> Self {
        Self::from_segments_with_options(segments, HexFormatOptions::default())
    }

    /// Creates an AddressMap from memory segments and format options.
    pub fn from_segments_with_options(segments: Vec<MemorySegment>, format_options: HexFormatOptions) -> Self {
        if segments.is_empty() {
            Self {
                format_options,
                ..Self::default()
            }
        } else {
            Self { segments, format_options }
        }
    }

    /// Returns the lowest base address of the first segment.
    pub fn base_address(&self) -> usize {
        self.segments.first().map(|s| s.address).unwrap_or(0)
    }

    /// Returns true if there are multiple segments with address gaps between them.
    pub fn has_gaps(&self) -> bool {
        if self.segments.len() <= 1 {
            return false;
        }
        for window in self.segments.windows(2) {
            if window[0].end_address() < window[1].address {
                return true;
            }
        }
        false
    }

    /// Converts a linear buffer offset to its physical memory address.
    pub fn offset_to_address(&self, offset: usize) -> usize {
        if self.segments.is_empty() {
            return offset;
        }

        // Binary search for the segment containing `offset`
        match self.segments.binary_search_by(|seg| {
            if offset < seg.buffer_offset {
                std::cmp::Ordering::Greater
            } else if offset >= seg.end_buffer_offset() {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        }) {
            Ok(idx) => {
                let seg = &self.segments[idx];
                seg.address + (offset - seg.buffer_offset)
            }
            Err(idx) => {
                if idx == 0 {
                    self.segments[0].address
                } else if idx >= self.segments.len() {
                    let last = &self.segments[self.segments.len() - 1];
                    last.address + (offset.saturating_sub(last.buffer_offset))
                } else {
                    let seg = &self.segments[idx];
                    seg.address
                }
            }
        }
    }

    /// Converts a physical memory address to a linear buffer offset.
    pub fn address_to_offset(&self, address: usize) -> Option<usize> {
        if self.segments.is_empty() {
            return Some(address);
        }

        for seg in &self.segments {
            if seg.contains_address(address) {
                return Some(seg.buffer_offset + (address - seg.address));
            }
        }

        // If not directly inside a segment, find the closest segment containing or after this address
        for seg in &self.segments {
            if address < seg.address {
                return Some(seg.buffer_offset);
            }
        }

        self.segments.last().map(|s| s.end_buffer_offset().saturating_sub(1))
    }

    /// Checks if a gap immediately precedes the given buffer offset, and if so returns `Some((gap_start_addr, gap_end_addr))`.
    pub fn gap_before_offset(&self, offset: usize) -> Option<(usize, usize)> {
        if offset == 0 || self.segments.len() <= 1 {
            return None;
        }
        for window in self.segments.windows(2) {
            if window[1].buffer_offset == offset && window[0].end_address() < window[1].address {
                return Some((window[0].end_address(), window[1].address));
            }
        }
        None
    }

    /// Collects layout break events corresponding to segment boundaries.
    pub fn collect_segment_breaks(&self, breaks: &mut BTreeSet<usize>) {
        for seg in &self.segments {
            if seg.buffer_offset > 0 {
                breaks.insert(seg.buffer_offset);
            }
        }
    }

    /// Collects empty line counts for address gaps between segments so layout reserves a row for the gap bar.
    pub fn collect_gap_lines(&self, empty_lines: &mut BTreeMap<usize, usize>) {
        for window in self.segments.windows(2) {
            if window[0].end_address() < window[1].address && window[1].buffer_offset > 0 {
                *empty_lines.entry(window[1].buffer_offset).or_default() += 1;
            }
        }
    }

    /// Returns the recommended default file extension ("mot", "hex", or "bin") based on the current path and address map structure.
    pub fn default_extension(&self, current_path: &std::path::Path) -> &'static str {
        if is_mot_extension(current_path) {
            "mot"
        } else if is_hex_extension(current_path) {
            "hex"
        } else if self.has_gaps() || self.format_options.record_data_length == 20 {
            "mot"
        } else if self.format_options.record_data_length == 16 && self.format_options.header.is_none() && self.base_address() > 0 {
            "hex"
        } else {
            "bin"
        }
    }

    /// Adjusts segment buffer offsets, lengths, and addresses after an edit at `[start..start + old_len)` replaced with `new_len` bytes.
    pub fn adjust_after_edit(&mut self, start: usize, old_len: usize, new_len: usize) {
        if self.segments.is_empty() {
            return;
        }

        // Fast path for single segment starting at 0
        if self.segments.len() == 1 && self.segments[0].address == 0 && self.segments[0].buffer_offset == 0 {
            if self.segments[0].length != usize::MAX {
                if new_len >= old_len {
                    self.segments[0].length = self.segments[0].length.saturating_add(new_len - old_len);
                } else {
                    self.segments[0].length = self.segments[0].length.saturating_sub(old_len - new_len);
                }
            }
            return;
        }

        let old_end = start.saturating_add(old_len);
        let mut new_segments: Vec<MemorySegment> = Vec::with_capacity(self.segments.len());
        let mut current_buffer_offset = 0;
        let mut inserted_remaining = new_len;

        let num_segs = self.segments.len();
        for (i, seg) in self.segments.iter().enumerate() {
            let is_last_seg = i + 1 == num_segs;
            let seg_start = seg.buffer_offset;
            let seg_end = seg.end_buffer_offset();

            if old_len == 0 {
                // Pure insertion at `start`
                let insert_into_this = if is_last_seg {
                    start >= seg_start
                } else if start == 0 && seg_start == 0 {
                    true
                } else {
                    start >= seg_start && start < seg_end
                };

                let to_insert = if insert_into_this && inserted_remaining > 0 {
                    let ins = inserted_remaining;
                    inserted_remaining = 0;
                    ins
                } else {
                    0
                };

                let new_seg_len = seg.length.saturating_add(to_insert);
                new_segments.push(MemorySegment {
                    buffer_offset: current_buffer_offset,
                    address: seg.address,
                    length: new_seg_len,
                });
                current_buffer_offset += new_seg_len;
            } else {
                // Replacement or deletion
                if old_end <= seg_start {
                    // Edit was completely before this segment
                    new_segments.push(MemorySegment {
                        buffer_offset: current_buffer_offset,
                        address: seg.address,
                        length: seg.length,
                    });
                    current_buffer_offset += seg.length;
                } else if start >= seg_end && !is_last_seg {
                    // Edit is completely after this segment
                    new_segments.push(MemorySegment {
                        buffer_offset: current_buffer_offset,
                        address: seg.address,
                        length: seg.length,
                    });
                    current_buffer_offset += seg.length;
                } else {
                    // Overlaps with this segment
                    let kept_before = start.saturating_sub(seg_start).min(seg.length);
                    let deleted_prefix = if start <= seg_start {
                        old_end.saturating_sub(seg_start).min(seg.length)
                    } else {
                        0
                    };
                    let kept_after = seg_end.saturating_sub(old_end.max(seg_start));

                    let insert_into_this = if is_last_seg {
                        start >= seg_start || (start <= seg_start && kept_after > 0)
                    } else {
                        start >= seg_start && start <= seg_end
                    };

                    let to_insert = if insert_into_this && inserted_remaining > 0 {
                        let ins = inserted_remaining;
                        inserted_remaining = 0;
                        ins
                    } else {
                        0
                    };

                    let new_seg_len = kept_before + to_insert + kept_after;
                    if new_seg_len > 0 {
                        let new_address = seg.address.saturating_add(deleted_prefix);
                        new_segments.push(MemorySegment {
                            buffer_offset: current_buffer_offset,
                            address: new_address,
                            length: new_seg_len,
                        });
                        current_buffer_offset += new_seg_len;
                    }
                }
            }
        }

        if inserted_remaining > 0 {
            if let Some(last) = new_segments.last_mut() {
                last.length += inserted_remaining;
            } else {
                let base_addr = self.base_address();
                new_segments.push(MemorySegment {
                    buffer_offset: 0,
                    address: base_addr,
                    length: inserted_remaining,
                });
            }
        }

        if new_segments.is_empty() {
            let base_addr = self.base_address();
            new_segments.push(MemorySegment {
                buffer_offset: 0,
                address: base_addr,
                length: 0,
            });
        }

        self.segments = new_segments;
    }
}

/// Identifies the format of the parsed hex/mot file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HexFormat {
    MotorolaS19,
    MotorolaS28,
    MotorolaS37,
    IntelHex,
}

impl HexFormat {
    pub fn label(&self) -> &'static str {
        match self {
            HexFormat::MotorolaS19 => "Motorola S-Record (S19)",
            HexFormat::MotorolaS28 => "Motorola S-Record (S28)",
            HexFormat::MotorolaS37 => "Motorola S-Record (S37)",
            HexFormat::IntelHex => "Intel HEX",
        }
    }
}

/// Result of importing a Motorola S-Record or Intel HEX file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HexImportResult {
    /// Combined contiguous data buffer of all segments
    pub data: Vec<u8>,
    /// Address map containing all memory segments and physical addresses
    pub address_map: AddressMap,
    /// Format of the imported file
    pub format: HexFormat,
    /// Optional execution start address / entry point
    pub entry_point: Option<usize>,
    /// Optional header string from S0 record
    pub header: Option<String>,
}

/// Error type for parsing S-Record or Intel HEX files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HexImportError {
    EmptyInput,
    InvalidRecordStart { line: usize, char: char },
    InvalidHexDigits { line: usize, content: String },
    LineTooShort { line: usize },
    ChecksumMismatch { line: usize, expected: u8, actual: u8 },
    UnsupportedRecordType { line: usize, record_type: String },
    NoDataRecords,
    UnknownFormat,
}

impl fmt::Display for HexImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HexImportError::EmptyInput => write!(f, "File is empty"),
            HexImportError::InvalidRecordStart { line, char } => {
                write!(f, "Line {}: invalid record start character '{}'", line, char)
            }
            HexImportError::InvalidHexDigits { line, content } => {
                write!(f, "Line {}: invalid hex string '{}'", line, content)
            }
            HexImportError::LineTooShort { line } => write!(f, "Line {}: line is too short", line),
            HexImportError::ChecksumMismatch { line, expected, actual } => {
                write!(
                    f,
                    "Line {}: checksum mismatch (calculated: 0x{:02X}, in file: 0x{:02X})",
                    line, expected, actual
                )
            }
            HexImportError::UnsupportedRecordType { line, record_type } => {
                write!(f, "Line {}: unsupported record type '{}'", line, record_type)
            }
            HexImportError::NoDataRecords => write!(f, "No data records found in file"),
            HexImportError::UnknownFormat => write!(f, "Unrecognized format (not valid Motorola S-Record or Intel HEX)"),
        }
    }
}

impl std::error::Error for HexImportError {}

/// Parse a two-character hex slice into a `u8`.
fn parse_hex_byte(s: &str, line_idx: usize) -> Result<u8, HexImportError> {
    u8::from_str_radix(s, 16).map_err(|_| HexImportError::InvalidHexDigits {
        line: line_idx + 1,
        content: s.to_string(),
    })
}

/// Parse a Motorola S-Record file from string content.
pub fn parse_motorola_srec(content: &str) -> Result<HexImportResult, HexImportError> {
    let mut raw_chunks: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut entry_point = None;
    let mut header = None;
    let mut max_addr_width = 2; // 2 = S19, 3 = S28, 4 = S37
    let mut chunk_length_counts: BTreeMap<usize, usize> = BTreeMap::new();
    let mut has_count_record = false;
    let crlf = content.contains("\r\n");

    let mut line_count = 0;
    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        line_count += 1;

        if !trimmed.starts_with('S') && !trimmed.starts_with('s') {
            return Err(HexImportError::InvalidRecordStart {
                line: line_idx + 1,
                char: trimmed.chars().next().unwrap_or(' '),
            });
        }

        if trimmed.len() < 4 {
            return Err(HexImportError::LineTooShort { line: line_idx + 1 });
        }

        let type_char = trimmed.chars().nth(1).unwrap();
        let count_str = &trimmed[2..4];
        let byte_count = parse_hex_byte(count_str, line_idx)? as usize;

        // Total expected characters after 'S<type>': byte_count * 2
        let data_str = &trimmed[4..];
        if data_str.len() < byte_count * 2 {
            return Err(HexImportError::LineTooShort { line: line_idx + 1 });
        }

        // Checksum verification: sum of byte_count + all following bytes + checksum == 0xFF
        let mut sum: u32 = byte_count as u32;
        let mut record_bytes = Vec::with_capacity(byte_count);
        for chunk in data_str.as_bytes()[..byte_count * 2].chunks_exact(2) {
            let hex_slice = std::str::from_utf8(chunk).map_err(|_| HexImportError::InvalidHexDigits {
                line: line_idx + 1,
                content: String::from_utf8_lossy(chunk).to_string(),
            })?;
            let b = parse_hex_byte(hex_slice, line_idx)?;
            sum += b as u32;
            record_bytes.push(b);
        }

        let expected_checksum = !((sum - record_bytes.last().copied().unwrap_or(0) as u32) as u8);
        let actual_checksum = record_bytes.pop().unwrap_or(0);
        if expected_checksum != actual_checksum {
            return Err(HexImportError::ChecksumMismatch {
                line: line_idx + 1,
                expected: expected_checksum,
                actual: actual_checksum,
            });
        }

        match type_char {
            '0' => {
                // S0: Header
                if record_bytes.len() >= 2 {
                    // Skip 2-byte address 0x0000
                    let header_data = &record_bytes[2..];
                    if let Ok(text) = std::str::from_utf8(header_data) {
                        header = Some(text.trim_matches('\0').to_string());
                    }
                }
            }
            '1' => {
                // S1: 16-bit address (2 bytes) + data
                if record_bytes.len() < 2 {
                    return Err(HexImportError::LineTooShort { line: line_idx + 1 });
                }
                max_addr_width = max_addr_width.max(2);
                let addr = ((record_bytes[0] as usize) << 8) | (record_bytes[1] as usize);
                let data = record_bytes[2..].to_vec();
                if !data.is_empty() {
                    *chunk_length_counts.entry(data.len()).or_insert(0) += 1;
                    raw_chunks.push((addr, data));
                }
            }
            '2' => {
                // S2: 24-bit address (3 bytes) + data
                if record_bytes.len() < 3 {
                    return Err(HexImportError::LineTooShort { line: line_idx + 1 });
                }
                max_addr_width = max_addr_width.max(3);
                let addr = ((record_bytes[0] as usize) << 16) | ((record_bytes[1] as usize) << 8) | (record_bytes[2] as usize);
                let data = record_bytes[3..].to_vec();
                if !data.is_empty() {
                    *chunk_length_counts.entry(data.len()).or_insert(0) += 1;
                    raw_chunks.push((addr, data));
                }
            }
            '3' => {
                // S3: 32-bit address (4 bytes) + data
                if record_bytes.len() < 4 {
                    return Err(HexImportError::LineTooShort { line: line_idx + 1 });
                }
                max_addr_width = max_addr_width.max(4);
                let addr =
                    ((record_bytes[0] as usize) << 24) | ((record_bytes[1] as usize) << 16) | ((record_bytes[2] as usize) << 8) | (record_bytes[3] as usize);
                let data = record_bytes[4..].to_vec();
                if !data.is_empty() {
                    *chunk_length_counts.entry(data.len()).or_insert(0) += 1;
                    raw_chunks.push((addr, data));
                }
            }
            '5' | '6' => {
                // S5 / S6: Record count
                has_count_record = true;
            }
            '7' => {
                // S7: 32-bit termination / entry address
                if record_bytes.len() >= 4 {
                    let addr = ((record_bytes[0] as usize) << 24)
                        | ((record_bytes[1] as usize) << 16)
                        | ((record_bytes[2] as usize) << 8)
                        | (record_bytes[3] as usize);
                    entry_point = Some(addr);
                }
            }
            '8' => {
                // S8: 24-bit termination / entry address
                if record_bytes.len() >= 3 {
                    let addr = ((record_bytes[0] as usize) << 16) | ((record_bytes[1] as usize) << 8) | (record_bytes[2] as usize);
                    entry_point = Some(addr);
                }
            }
            '9' => {
                // S9: 16-bit termination / entry address
                if record_bytes.len() >= 2 {
                    let addr = ((record_bytes[0] as usize) << 8) | (record_bytes[1] as usize);
                    entry_point = Some(addr);
                }
            }
            _ => {
                return Err(HexImportError::UnsupportedRecordType {
                    line: line_idx + 1,
                    record_type: format!("S{}", type_char),
                });
            }
        }
    }

    if line_count == 0 {
        return Err(HexImportError::EmptyInput);
    }
    if raw_chunks.is_empty() {
        return Err(HexImportError::NoDataRecords);
    }

    let format = match max_addr_width {
        4 => HexFormat::MotorolaS37,
        3 => HexFormat::MotorolaS28,
        _ => HexFormat::MotorolaS19,
    };

    let record_data_length = chunk_length_counts
        .into_iter()
        .max_by_key(|&(_, count)| count)
        .map(|(len, _)| len)
        .unwrap_or(20);

    let format_options = HexFormatOptions {
        record_data_length,
        address_width: max_addr_width,
        header: header.clone(),
        entry_point,
        has_count_record,
        crlf,
    };

    assemble_chunks_into_result(raw_chunks, format, entry_point, header, format_options)
}

/// Parse an Intel HEX file from string content.
pub fn parse_intel_hex(content: &str) -> Result<HexImportResult, HexImportError> {
    let mut raw_chunks: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut base_addr: usize = 0;
    let mut entry_point = None;
    let mut chunk_length_counts: BTreeMap<usize, usize> = BTreeMap::new();
    let crlf = content.contains("\r\n");

    let mut line_count = 0;
    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        line_count += 1;

        if !trimmed.starts_with(':') {
            return Err(HexImportError::InvalidRecordStart {
                line: line_idx + 1,
                char: trimmed.chars().next().unwrap_or(' '),
            });
        }

        if trimmed.len() < 11 {
            return Err(HexImportError::LineTooShort { line: line_idx + 1 });
        }

        let byte_count = parse_hex_byte(&trimmed[1..3], line_idx)? as usize;
        let addr_hi = parse_hex_byte(&trimmed[3..5], line_idx)?;
        let addr_lo = parse_hex_byte(&trimmed[5..7], line_idx)?;
        let record_type = parse_hex_byte(&trimmed[7..9], line_idx)?;
        let record_addr = ((addr_hi as usize) << 8) | (addr_lo as usize);

        let data_and_checksum_len = (byte_count + 1) * 2;
        if trimmed.len() < 9 + data_and_checksum_len {
            return Err(HexImportError::LineTooShort { line: line_idx + 1 });
        }

        // Checksum verification: sum of all bytes in line modulo 256 == 0
        let mut sum: u32 = (byte_count as u32) + (addr_hi as u32) + (addr_lo as u32) + (record_type as u32);
        let mut record_bytes = Vec::with_capacity(byte_count);

        let data_str = &trimmed[9..9 + byte_count * 2];
        for chunk in data_str.as_bytes().chunks_exact(2) {
            let hex_slice = std::str::from_utf8(chunk).map_err(|_| HexImportError::InvalidHexDigits {
                line: line_idx + 1,
                content: String::from_utf8_lossy(chunk).to_string(),
            })?;
            let b = parse_hex_byte(hex_slice, line_idx)?;
            sum += b as u32;
            record_bytes.push(b);
        }

        let checksum_str = &trimmed[9 + byte_count * 2..9 + (byte_count + 1) * 2];
        let actual_checksum = parse_hex_byte(checksum_str, line_idx)?;
        let expected_checksum = ((!sum + 1) & 0xFF) as u8;

        if actual_checksum != expected_checksum {
            return Err(HexImportError::ChecksumMismatch {
                line: line_idx + 1,
                expected: expected_checksum,
                actual: actual_checksum,
            });
        }

        match record_type {
            0x00 => {
                // Data record
                let full_addr = base_addr.saturating_add(record_addr);
                if !record_bytes.is_empty() {
                    *chunk_length_counts.entry(record_bytes.len()).or_insert(0) += 1;
                    raw_chunks.push((full_addr, record_bytes));
                }
            }
            0x01 => {
                // End of File
                break;
            }
            0x02 => {
                // Extended Segment Address: segment << 4
                if record_bytes.len() >= 2 {
                    let seg = ((record_bytes[0] as usize) << 8) | (record_bytes[1] as usize);
                    base_addr = seg << 4;
                }
            }
            0x03 => {
                // Start Segment Address (CS:IP)
                if record_bytes.len() >= 4 {
                    let cs = ((record_bytes[0] as usize) << 8) | (record_bytes[1] as usize);
                    let ip = ((record_bytes[2] as usize) << 8) | (record_bytes[3] as usize);
                    entry_point = Some((cs << 4) + ip);
                }
            }
            0x04 => {
                // Extended Linear Address: upper 16 bits
                if record_bytes.len() >= 2 {
                    let upper = ((record_bytes[0] as usize) << 8) | (record_bytes[1] as usize);
                    base_addr = upper << 16;
                }
            }
            0x05 => {
                // Start Linear Address (EIP)
                if record_bytes.len() >= 4 {
                    let eip = ((record_bytes[0] as usize) << 24)
                        | ((record_bytes[1] as usize) << 16)
                        | ((record_bytes[2] as usize) << 8)
                        | (record_bytes[3] as usize);
                    entry_point = Some(eip);
                }
            }
            _ => {
                return Err(HexImportError::UnsupportedRecordType {
                    line: line_idx + 1,
                    record_type: format!("{:02X}", record_type),
                });
            }
        }
    }

    if line_count == 0 {
        return Err(HexImportError::EmptyInput);
    }
    if raw_chunks.is_empty() {
        return Err(HexImportError::NoDataRecords);
    }

    let record_data_length = chunk_length_counts
        .into_iter()
        .max_by_key(|&(_, count)| count)
        .map(|(len, _)| len)
        .unwrap_or(16);

    let format_options = HexFormatOptions {
        record_data_length,
        address_width: 0,
        header: None,
        entry_point,
        has_count_record: false,
        crlf,
    };

    assemble_chunks_into_result(raw_chunks, HexFormat::IntelHex, entry_point, None, format_options)
}

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

/// Assembles sorted data chunks into contiguous MemorySegments and compact buffer payload.
fn assemble_chunks_into_result(
    mut raw_chunks: Vec<(usize, Vec<u8>)>,
    format: HexFormat,
    entry_point: Option<usize>,
    header: Option<String>,
    format_options: HexFormatOptions,
) -> Result<HexImportResult, HexImportError> {
    // Sort chunks by starting address
    raw_chunks.sort_by_key(|(addr, _)| *addr);

    // Merge contiguous and overlapping chunks into distinct memory segments
    struct MergedBlock {
        address: usize,
        data: Vec<u8>,
    }

    let mut merged_blocks: Vec<MergedBlock> = Vec::new();

    for (chunk_addr, chunk_data) in raw_chunks {
        if let Some(last) = merged_blocks.last_mut() {
            let last_end = last.address + last.data.len();
            if chunk_addr <= last_end {
                // Overlap or strictly contiguous
                let offset_in_last = chunk_addr - last.address;
                let needed_len = offset_in_last + chunk_data.len();
                if needed_len > last.data.len() {
                    last.data.resize(needed_len, 0xFF);
                }
                last.data[offset_in_last..offset_in_last + chunk_data.len()].copy_from_slice(&chunk_data);
                continue;
            }
        }
        merged_blocks.push(MergedBlock {
            address: chunk_addr,
            data: chunk_data,
        });
    }

    if merged_blocks.is_empty() {
        return Err(HexImportError::NoDataRecords);
    }

    let total_bytes: usize = merged_blocks.iter().map(|b| b.data.len()).sum();
    let mut combined_data = Vec::with_capacity(total_bytes);
    let mut segments = Vec::with_capacity(merged_blocks.len());

    let mut current_buffer_offset = 0;
    for block in merged_blocks {
        let len = block.data.len();
        segments.push(MemorySegment {
            buffer_offset: current_buffer_offset,
            address: block.address,
            length: len,
        });
        combined_data.extend_from_slice(&block.data);
        current_buffer_offset += len;
    }

    Ok(HexImportResult {
        data: combined_data,
        address_map: AddressMap::from_segments_with_options(segments, format_options),
        format,
        entry_point,
        header,
    })
}

/// Exports binary data and its AddressMap to Motorola S-Record (`.mot` / `.srec`) string format.
pub fn export_motorola_srec(data: &[u8], address_map: &AddressMap) -> String {
    let mut out = String::new();
    let line_ending = if address_map.format_options.crlf { "\r\n" } else { "\n" };

    // S0 Header record: only if header was present in source or specified
    if let Some(hdr) = &address_map.format_options.header {
        let hdr_bytes = hdr.as_bytes();
        let byte_count = 2 + hdr_bytes.len() + 1;
        let mut sum: u32 = byte_count as u32;
        out.push_str(&format!("S0{:02X}0000", byte_count));
        for &b in hdr_bytes {
            sum += b as u32;
            out.push_str(&format!("{:02X}", b));
        }
        let checksum = !(sum as u8);
        out.push_str(&format!("{:02X}{}", checksum, line_ending));
    }

    let segments = if address_map.segments.is_empty() {
        vec![MemorySegment {
            buffer_offset: 0,
            address: 0,
            length: data.len(),
        }]
    } else {
        address_map.segments.clone()
    };

    let max_address = segments.iter().map(|s| s.address.saturating_add(s.length)).max().unwrap_or(0);

    let address_bytes = if address_map.format_options.address_width >= 2 {
        address_map.format_options.address_width.min(4)
    } else if max_address <= 0xFFFF {
        2
    } else if max_address <= 0xFF_FFFF {
        3
    } else {
        4
    };

    let mut record_count: usize = 0;
    let chunk_size = if address_map.format_options.record_data_length > 0 {
        address_map.format_options.record_data_length
    } else {
        20 // standard 0x14 for Motorola S-Record
    };

    for seg in &segments {
        let seg_start = seg.buffer_offset.min(data.len());
        let seg_end = (seg.buffer_offset + seg.length).min(data.len());
        if seg_start >= seg_end {
            continue;
        }

        let seg_data = &data[seg_start..seg_end];
        let mut cur_addr = seg.address;

        for chunk in seg_data.chunks(chunk_size) {
            record_count += 1;
            let byte_count = address_bytes + chunk.len() + 1;
            let mut sum: u32 = byte_count as u32;

            out.push('S');
            match address_bytes {
                2 => out.push('1'),
                3 => out.push('2'),
                _ => out.push('3'),
            }

            out.push_str(&format!("{:02X}", byte_count));

            match address_bytes {
                2 => {
                    let a1 = ((cur_addr >> 8) & 0xFF) as u8;
                    let a0 = (cur_addr & 0xFF) as u8;
                    sum += a1 as u32 + a0 as u32;
                    out.push_str(&format!("{:02X}{:02X}", a1, a0));
                }
                3 => {
                    let a2 = ((cur_addr >> 16) & 0xFF) as u8;
                    let a1 = ((cur_addr >> 8) & 0xFF) as u8;
                    let a0 = (cur_addr & 0xFF) as u8;
                    sum += a2 as u32 + a1 as u32 + a0 as u32;
                    out.push_str(&format!("{:02X}{:02X}{:02X}", a2, a1, a0));
                }
                _ => {
                    let a3 = ((cur_addr >> 24) & 0xFF) as u8;
                    let a2 = ((cur_addr >> 16) & 0xFF) as u8;
                    let a1 = ((cur_addr >> 8) & 0xFF) as u8;
                    let a0 = (cur_addr & 0xFF) as u8;
                    sum += a3 as u32 + a2 as u32 + a1 as u32 + a0 as u32;
                    out.push_str(&format!("{:02X}{:02X}{:02X}{:02X}", a3, a2, a1, a0));
                }
            }

            for &b in chunk {
                sum += b as u32;
                out.push_str(&format!("{:02X}", b));
            }

            let checksum = !(sum as u8);
            out.push_str(&format!("{:02X}{}", checksum, line_ending));

            cur_addr += chunk.len();
        }
    }

    if address_map.format_options.has_count_record && record_count <= 0xFFFF {
        let count_u16 = record_count as u16;
        let c1 = ((count_u16 >> 8) & 0xFF) as u8;
        let c0 = (count_u16 & 0xFF) as u8;
        let sum: u32 = 3 + c1 as u32 + c0 as u32;
        let checksum = !(sum as u8);
        out.push_str(&format!("S503{:04X}{:02X}{}", count_u16, checksum, line_ending));
    }

    let start_addr = address_map
        .format_options
        .entry_point
        .unwrap_or_else(|| segments.first().map(|s| s.address).unwrap_or(0));

    match address_bytes {
        2 => {
            let a1 = ((start_addr >> 8) & 0xFF) as u8;
            let a0 = (start_addr & 0xFF) as u8;
            let sum: u32 = 3 + a1 as u32 + a0 as u32;
            let checksum = !(sum as u8);
            out.push_str(&format!("S903{:04X}{:02X}{}", start_addr & 0xFFFF, checksum, line_ending));
        }
        3 => {
            let a2 = ((start_addr >> 16) & 0xFF) as u8;
            let a1 = ((start_addr >> 8) & 0xFF) as u8;
            let a0 = (start_addr & 0xFF) as u8;
            let sum: u32 = 4 + a2 as u32 + a1 as u32 + a0 as u32;
            let checksum = !(sum as u8);
            out.push_str(&format!("S804{:06X}{:02X}{}", start_addr & 0xFF_FFFF, checksum, line_ending));
        }
        _ => {
            let a3 = ((start_addr >> 24) & 0xFF) as u8;
            let a2 = ((start_addr >> 16) & 0xFF) as u8;
            let a1 = ((start_addr >> 8) & 0xFF) as u8;
            let a0 = (start_addr & 0xFF) as u8;
            let sum: u32 = 5 + a3 as u32 + a2 as u32 + a1 as u32 + a0 as u32;
            let checksum = !(sum as u8);
            out.push_str(&format!("S705{:08X}{:02X}{}", start_addr, checksum, line_ending));
        }
    }

    out
}

/// Exports binary data and its AddressMap to Intel HEX string format.
pub fn export_intel_hex(data: &[u8], address_map: &AddressMap) -> String {
    let mut out = String::new();
    let line_ending = if address_map.format_options.crlf { "\r\n" } else { "\n" };

    let segments = if address_map.segments.is_empty() {
        vec![MemorySegment {
            buffer_offset: 0,
            address: 0,
            length: data.len(),
        }]
    } else {
        address_map.segments.clone()
    };

    let mut current_upper_16: Option<u16> = None;
    let chunk_size = if address_map.format_options.record_data_length > 0 {
        address_map.format_options.record_data_length
    } else {
        16 // standard 0x10 for Intel HEX
    };

    for seg in &segments {
        let seg_start = seg.buffer_offset.min(data.len());
        let seg_end = (seg.buffer_offset + seg.length).min(data.len());
        if seg_start >= seg_end {
            continue;
        }

        let seg_data = &data[seg_start..seg_end];
        let mut cur_addr = seg.address;

        for chunk in seg_data.chunks(chunk_size) {
            let upper_16 = ((cur_addr >> 16) & 0xFFFF) as u16;
            let lower_16 = (cur_addr & 0xFFFF) as u16;

            if current_upper_16 != Some(upper_16) {
                current_upper_16 = Some(upper_16);
                let u1 = ((upper_16 >> 8) & 0xFF) as u8;
                let u0 = (upper_16 & 0xFF) as u8;
                let sum: u32 = 2 + 4 + u1 as u32 + u0 as u32;
                let checksum = (!((sum & 0xFF) as u8)).wrapping_add(1);
                out.push_str(&format!(":02000004{:04X}{:02X}{}", upper_16, checksum, line_ending));
            }

            let byte_count = chunk.len() as u8;
            let a1 = ((lower_16 >> 8) & 0xFF) as u8;
            let a0 = (lower_16 & 0xFF) as u8;
            let mut sum: u32 = byte_count as u32 + a1 as u32 + a0 as u32;

            out.push_str(&format!(":{:02X}{:04X}00", byte_count, lower_16));
            for &b in chunk {
                sum += b as u32;
                out.push_str(&format!("{:02X}", b));
            }
            let checksum = (!((sum & 0xFF) as u8)).wrapping_add(1);
            out.push_str(&format!("{:02X}{}", checksum, line_ending));

            cur_addr += chunk.len();
        }
    }

    if let Some(entry) = address_map.format_options.entry_point {
        let e3 = ((entry >> 24) & 0xFF) as u8;
        let e2 = ((entry >> 16) & 0xFF) as u8;
        let e1 = ((entry >> 8) & 0xFF) as u8;
        let e0 = (entry & 0xFF) as u8;
        let sum: u32 = 4 + 5 + e3 as u32 + e2 as u32 + e1 as u32 + e0 as u32;
        let checksum = (!((sum & 0xFF) as u8)).wrapping_add(1);
        out.push_str(&format!(":04000005{:08X}{:02X}{}", entry, checksum, line_ending));
    }

    out.push_str(&format!(":00000001FF{}", line_ending));
    out
}

/// Checks if the file path has a Motorola S-Record extension.
pub fn is_mot_extension(path: &std::path::Path) -> bool {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "mot" | "srec" | "s19" | "s28" | "s37")
}

/// Checks if the file path has an Intel HEX extension.
pub fn is_hex_extension(path: &std::path::Path) -> bool {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "hex" | "ihex" | "ihx")
}

/// Reconstructs a full linear binary image from the given buffer and its AddressMap,
/// placing each segment at its physical memory address and filling unmapped regions and gaps with `fill_byte` (default 0x00).
pub fn export_raw_binary(data: &[u8], address_map: &AddressMap, fill_byte: u8) -> Vec<u8> {
    if address_map.segments.is_empty() || (address_map.segments.len() == 1 && address_map.segments[0].address == 0) {
        return data.to_vec();
    }

    let max_address = address_map
        .segments
        .iter()
        .map(|s| s.address.saturating_add(s.length))
        .max()
        .unwrap_or(data.len());

    let mut out = vec![fill_byte; max_address];

    for seg in &address_map.segments {
        let seg_start = seg.buffer_offset.min(data.len());
        let seg_end = (seg.buffer_offset + seg.length).min(data.len());
        if seg_start >= seg_end {
            continue;
        }

        let seg_data = &data[seg_start..seg_end];
        let target_start = seg.address.min(out.len());
        let target_end = (seg.address + seg_data.len()).min(out.len());
        let copy_len = target_end.saturating_sub(target_start);
        if copy_len > 0 {
            out[target_start..target_start + copy_len].copy_from_slice(&seg_data[..copy_len]);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_motorola_srec_s19() {
        let srec = "S0030000FC\nS107100001020304DE\nS9030000FC\n";
        let res = parse_motorola_srec(srec).expect("valid srec");
        assert_eq!(res.format, HexFormat::MotorolaS19);
        assert_eq!(res.data, vec![0x01, 0x02, 0x03, 0x04]);
        assert_eq!(res.address_map.base_address(), 0x1000);
        assert_eq!(res.address_map.offset_to_address(0), 0x1000);
        assert_eq!(res.address_map.offset_to_address(3), 0x1003);
    }

    #[test]
    fn test_parse_motorola_srec_with_gaps() {
        let srec = "S0030000FC\nS3090001000001020304EB\nS3090002000005060708DA\nS70500000000FA\n";
        let res = parse_motorola_srec(srec).expect("valid srec with gaps");
        assert_eq!(res.format, HexFormat::MotorolaS37);
        assert_eq!(res.data, vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
        assert_eq!(res.address_map.segments.len(), 2);
        assert_eq!(res.address_map.segments[0].address, 0x0001_0000);
        assert_eq!(res.address_map.segments[0].length, 4);
        assert_eq!(res.address_map.segments[1].address, 0x0002_0000);
        assert_eq!(res.address_map.segments[1].length, 4);
        assert!(res.address_map.has_gaps());

        assert_eq!(res.address_map.offset_to_address(0), 0x0001_0000);
        assert_eq!(res.address_map.offset_to_address(3), 0x0001_0003);
        assert_eq!(res.address_map.offset_to_address(4), 0x0002_0000);
        assert_eq!(res.address_map.offset_to_address(7), 0x0002_0003);

        assert_eq!(res.address_map.address_to_offset(0x0001_0002), Some(2));
        assert_eq!(res.address_map.address_to_offset(0x0002_0001), Some(5));
    }

    #[test]
    fn test_export_and_import_motorola_srec_roundtrip() {
        let original_data = vec![0x11, 0x22, 0x33, 0x44, 0xAA, 0xBB, 0xCC, 0xDD];
        let map = AddressMap::from_segments(vec![
            MemorySegment {
                buffer_offset: 0,
                address: 0x0004_0000,
                length: 4,
            },
            MemorySegment {
                buffer_offset: 4,
                address: 0x0008_0000,
                length: 4,
            },
        ]);

        let exported = export_motorola_srec(&original_data, &map);
        let imported = parse_motorola_srec(&exported).expect("re-parse exported srec");

        assert_eq!(imported.data, original_data);
        assert_eq!(imported.address_map.segments.len(), 2);
        assert_eq!(imported.address_map.segments[0].address, 0x0004_0000);
        assert_eq!(imported.address_map.segments[1].address, 0x0008_0000);
    }

    #[test]
    fn test_export_and_import_intel_hex_roundtrip() {
        let original_data = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02];
        let map = AddressMap::from_segments(vec![
            MemorySegment {
                buffer_offset: 0,
                address: 0x0800_0000,
                length: 4,
            },
            MemorySegment {
                buffer_offset: 4,
                address: 0x0800_0010,
                length: 2,
            },
        ]);

        let exported = export_intel_hex(&original_data, &map);
        let imported = parse_intel_hex(&exported).expect("re-parse exported intel hex");

        assert_eq!(imported.data, original_data);
        assert_eq!(imported.address_map.segments.len(), 2);
        assert_eq!(imported.address_map.segments[0].address, 0x0800_0000);
        assert_eq!(imported.address_map.segments[1].address, 0x0800_0010);
    }

    #[test]
    fn test_parse_intel_hex_basic() {
        // :04 0000 00 01020304 F2
        // :00 0000 01 FF
        let ihex = ":0400000001020304F2\n:00000001FF\n";
        let res = parse_intel_hex(ihex).expect("valid intel hex");
        assert_eq!(res.format, HexFormat::IntelHex);
        assert_eq!(res.data, vec![0x01, 0x02, 0x03, 0x04]);
        assert_eq!(res.address_map.base_address(), 0x0000);
        assert_eq!(res.address_map.offset_to_address(0), 0x0000);
        assert_eq!(res.address_map.offset_to_address(3), 0x0003);
    }

    #[test]
    fn test_parse_intel_hex_extended_linear_with_gaps() {
        // Extended linear addr: upper 16-bit = 0x0800 -> chk = 0xF2
        // Data at 0x0800_0000: DE AD BE EF -> chk = 0xC4
        // Extended linear addr: upper 16-bit = 0x2000 -> chk = 0xDA
        // Data at 0x2000_0010: CA FE -> chk = 0x07 (2 + 0 + 0x10 + 0 + 0xCA + 0xFE = 0x1F8 -> (!0xF8 + 1) = 0x08... wait: 2 + 0 + 16 + 0 + 202 + 254 = 474 = 0x1DA -> 2's complement of 0xDA is 0x26)
        // Let's calculate: 2 + 0 + 0x10 + 0 + 0xCA + 0xFE = 474 = 0x1DA. (!0xDA + 1) & 0xFF = 0x26.
        // :00 0000 01 FF
        let ihex = ":020000040800F2\n:04000000DEADBEEFC4\n:020000042000DA\n:02001000CAFE26\n:00000001FF\n";
        let res = parse_hex_or_mot(ihex).expect("valid intel hex with extended address");
        assert_eq!(res.format, HexFormat::IntelHex);
        assert_eq!(res.data, vec![0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE]);
        assert_eq!(res.address_map.segments.len(), 2);
        assert_eq!(res.address_map.segments[0].address, 0x0800_0000);
        assert_eq!(res.address_map.segments[0].length, 4);
        assert_eq!(res.address_map.segments[1].address, 0x2000_0010);
        assert_eq!(res.address_map.segments[1].length, 2);
        assert!(res.address_map.has_gaps());
        assert_eq!(res.address_map.offset_to_address(4), 0x2000_0010);
        assert_eq!(res.address_map.offset_to_address(5), 0x2000_0011);
    }

    #[test]
    fn test_checksum_failure() {
        let invalid_srec = "S10710000102030400\n";
        let err = parse_motorola_srec(invalid_srec).unwrap_err();
        match err {
            HexImportError::ChecksumMismatch { line, expected, actual } => {
                assert_eq!(line, 1);
                assert_eq!(actual, 0x00);
                assert_eq!(expected, 0xDE);
            }
            _ => panic!("expected checksum mismatch"),
        }
    }

    #[test]
    fn test_export_raw_binary_with_offset_and_gaps() {
        let data = vec![0xAA, 0xBB, 0xCC, 0xDD];
        let map = AddressMap::from_segments(vec![
            MemorySegment {
                buffer_offset: 0,
                address: 4,
                length: 2,
            },
            MemorySegment {
                buffer_offset: 2,
                address: 8,
                length: 2,
            },
        ]);

        let raw = export_raw_binary(&data, &map, 0x00);
        assert_eq!(raw.len(), 10);
        assert_eq!(raw, vec![0, 0, 0, 0, 0xAA, 0xBB, 0, 0, 0xCC, 0xDD]);
    }

    #[test]
    fn test_reexport_preserves_exact_mot_structure() {
        let srec = "S31900FD00000064213403200A000000000100050A0005DC05DC31\nS31900FD001405DC05050164000A000000003D4CCCCD3D4CCCCD37\nS70501000400F5\n";
        let res = parse_motorola_srec(srec).expect("valid srec");
        assert_eq!(res.address_map.format_options.record_data_length, 20);
        assert_eq!(res.address_map.format_options.entry_point, Some(0x0100_0400));
        assert_eq!(res.address_map.format_options.header, None);
        assert!(!res.address_map.format_options.crlf);

        let reexported = export_motorola_srec(&res.data, &res.address_map);
        assert_eq!(reexported, srec);
    }

    #[test]
    fn test_address_map_adjust_after_edit_insert_preserves_subsequent_segment_addresses() {
        let mut map = AddressMap::from_segments(vec![
            MemorySegment {
                buffer_offset: 0,
                address: 0x00FD_0000,
                length: 10,
            },
            MemorySegment {
                buffer_offset: 10,
                address: 0x0100_0000,
                length: 10,
            },
        ]);

        // Insert 2 bytes at offset 5 (inside first segment)
        map.adjust_after_edit(5, 0, 2);

        assert_eq!(map.segments.len(), 2);
        // Segment 0 length grew by 2
        assert_eq!(map.segments[0].buffer_offset, 0);
        assert_eq!(map.segments[0].address, 0x00FD_0000);
        assert_eq!(map.segments[0].length, 12);

        // Segment 1 buffer_offset shifted by 2, address remains EXACTLY 0x0100_0000!
        assert_eq!(map.segments[1].buffer_offset, 12);
        assert_eq!(map.segments[1].address, 0x0100_0000);
        assert_eq!(map.segments[1].length, 10);

        // Verify offset_to_address mappings
        assert_eq!(map.offset_to_address(0), 0x00FD_0000);
        assert_eq!(map.offset_to_address(5), 0x00FD_0005);
        assert_eq!(map.offset_to_address(6), 0x00FD_0006);
        assert_eq!(map.offset_to_address(11), 0x00FD_000B);
        assert_eq!(map.offset_to_address(12), 0x0100_0000);
        assert_eq!(map.offset_to_address(21), 0x0100_0009);
    }

    #[test]
    fn test_address_map_adjust_after_edit_delete_preserves_subsequent_segment_addresses() {
        let mut map = AddressMap::from_segments(vec![
            MemorySegment {
                buffer_offset: 0,
                address: 0x00FD_0000,
                length: 10,
            },
            MemorySegment {
                buffer_offset: 10,
                address: 0x0100_0000,
                length: 10,
            },
        ]);

        // Delete 3 bytes at offset 4 (inside first segment)
        map.adjust_after_edit(4, 3, 0);

        assert_eq!(map.segments.len(), 2);
        assert_eq!(map.segments[0].buffer_offset, 0);
        assert_eq!(map.segments[0].address, 0x00FD_0000);
        assert_eq!(map.segments[0].length, 7);

        assert_eq!(map.segments[1].buffer_offset, 7);
        assert_eq!(map.segments[1].address, 0x0100_0000);
        assert_eq!(map.segments[1].length, 10);

        assert_eq!(map.offset_to_address(6), 0x00FD_0006);
        assert_eq!(map.offset_to_address(7), 0x0100_0000);
        assert_eq!(map.offset_to_address(16), 0x0100_0009);
    }
}
