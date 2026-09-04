use crate::core::address_map::AddressMap;

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
    use crate::core::address_map::MemorySegment;

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
}
