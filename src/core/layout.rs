use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub const BYTES_PER_ROW: usize = 16;

#[derive(Clone, Debug)]
pub enum LineMap {
    Standard { total_size: usize },
    Sparse(Arc<SparseLineMap>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SparseLineMap {
    pub segments: Vec<LayoutSegment>,
    pub total_lines: usize,
    pub total_size: usize,
    pub max_bytes_per_row: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutSegment {
    pub start_offset: usize,
    pub start_line: usize,
    pub byte_len: usize,
    pub line_count: usize,
    pub kind: SegmentKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SegmentKind {
    Standard,
    Custom { starts: Arc<Vec<usize>> },
}

impl PartialEq for LineMap {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (LineMap::Standard { total_size: s1 }, LineMap::Standard { total_size: s2 }) => s1 == s2,
            (LineMap::Sparse(sm1), LineMap::Sparse(sm2)) => sm1 == sm2,
            _ => {
                if self.len() != other.len() {
                    return false;
                }
                for i in 0..self.len() {
                    if self.get(i) != other.get(i) {
                        return false;
                    }
                }
                true
            }
        }
    }
}

impl Eq for LineMap {}

impl PartialEq<Vec<usize>> for LineMap {
    fn eq(&self, other: &Vec<usize>) -> bool {
        if self.len() != other.len() {
            return false;
        }
        for (i, &item) in other.iter().enumerate().take(self.len()) {
            if self.get(i) != Some(item) {
                return false;
            }
        }
        true
    }
}

impl PartialEq<LineMap> for Vec<usize> {
    fn eq(&self, other: &LineMap) -> bool {
        other.eq(self)
    }
}

impl SparseLineMap {
    pub fn len(&self) -> usize {
        self.total_lines
    }

    pub fn is_empty(&self) -> bool {
        self.total_lines == 0
    }

    pub fn get(&self, index: usize) -> Option<usize> {
        if index >= self.total_lines {
            return None;
        }
        let seg_idx = match self.segments.binary_search_by(|seg| {
            if index < seg.start_line {
                std::cmp::Ordering::Greater
            } else if index >= seg.start_line + seg.line_count {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        }) {
            Ok(idx) => idx,
            Err(_) => return None,
        };
        let seg = &self.segments[seg_idx];
        match &seg.kind {
            SegmentKind::Standard => {
                let rel_line = index - seg.start_line;
                Some(seg.start_offset + rel_line * BYTES_PER_ROW)
            }
            SegmentKind::Custom { starts } => {
                let rel_line = index - seg.start_line;
                starts.get(rel_line).copied()
            }
        }
    }

    pub fn binary_search(&self, offset: &usize) -> Result<usize, usize> {
        if self.total_size == 0 {
            return if *offset == 0 { Ok(0) } else { Err(1) };
        }
        if *offset >= self.total_size {
            return Err(self.total_lines);
        }
        let seg_idx = match self.segments.binary_search_by(|seg| {
            if *offset < seg.start_offset {
                std::cmp::Ordering::Greater
            } else if *offset >= seg.start_offset + seg.byte_len {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        }) {
            Ok(idx) => idx,
            Err(_) => return Err(self.total_lines),
        };
        let seg = &self.segments[seg_idx];
        match &seg.kind {
            SegmentKind::Standard => {
                let rel_offset = offset - seg.start_offset;
                if rel_offset.is_multiple_of(BYTES_PER_ROW) {
                    Ok(seg.start_line + rel_offset / BYTES_PER_ROW)
                } else {
                    Err(seg.start_line + rel_offset / BYTES_PER_ROW + 1)
                }
            }
            SegmentKind::Custom { starts } => match starts.binary_search(offset) {
                Ok(idx) => Ok(seg.start_line + idx),
                Err(idx) => Err(seg.start_line + idx),
            },
        }
    }
}

impl LineMap {
    pub fn len(&self) -> usize {
        match self {
            LineMap::Standard { total_size } => {
                if *total_size == 0 {
                    1
                } else {
                    (*total_size).div_ceil(BYTES_PER_ROW)
                }
            }
            LineMap::Sparse(sparse) => sparse.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, index: usize) -> Option<usize> {
        match self {
            LineMap::Standard { .. } => {
                let len = self.len();
                if index < len { Some(index * BYTES_PER_ROW) } else { None }
            }
            LineMap::Sparse(sparse) => sparse.get(index),
        }
    }

    pub fn binary_search(&self, offset: &usize) -> Result<usize, usize> {
        match self {
            LineMap::Standard { total_size } => {
                if *total_size == 0 {
                    return if *offset == 0 { Ok(0) } else { Err(1) };
                }
                let row = *offset / BYTES_PER_ROW;
                let len = self.len();
                if row < len {
                    if (*offset).is_multiple_of(BYTES_PER_ROW) { Ok(row) } else { Err(row + 1) }
                } else {
                    Err(len)
                }
            }
            LineMap::Sparse(sparse) => sparse.binary_search(offset),
        }
    }

    pub fn max_bytes_per_row(&self) -> usize {
        match self {
            LineMap::Standard { .. } => BYTES_PER_ROW,
            LineMap::Sparse(sparse) => sparse.max_bytes_per_row,
        }
    }
}

/// Builds a line map when the layout and break events are already sorted and
/// deduplicated. The default expanded structure layout uses the same boundary
/// list for both event streams, so this avoids a second allocation and sort.
pub fn build_line_map_from_sorted_events(total_size: usize, events: &[usize], custom_joins: &BTreeSet<usize>, empty_lines: &BTreeMap<usize, usize>) -> LineMap {
    if events.is_empty() && custom_joins.is_empty() && empty_lines.is_empty() {
        return LineMap::Standard { total_size };
    }

    build_line_map_from_sorted_event_lists(total_size, events, events, custom_joins, empty_lines)
}

fn build_line_map_from_sorted_event_lists(
    total_size: usize,
    layout_events: &[usize],
    break_events: &[usize],
    custom_joins: &BTreeSet<usize>,
    empty_lines: &BTreeMap<usize, usize>,
) -> LineMap {
    let mut segments = Vec::new();

    if total_size == 0 {
        segments.push(LayoutSegment {
            start_offset: 0,
            start_line: 0,
            byte_len: 0,
            line_count: 1,
            kind: SegmentKind::Custom { starts: Arc::new(vec![0]) },
        });
    } else {
        let mut current = 0;
        let mut current_line = 0;
        let mut event_idx = 0;
        let mut break_idx = 0;

        while current < total_size {
            // Find next event > current.
            while event_idx < layout_events.len() && layout_events[event_idx] <= current {
                event_idx += 1;
            }
            let next_event = if event_idx < layout_events.len() {
                Some(layout_events[event_idx])
            } else {
                None
            };

            match next_event {
                Some(ev) if ev - current > BYTES_PER_ROW => {
                    // We can fit one or more standard lines of BYTES_PER_ROW.
                    let n = (ev - current - 1) / BYTES_PER_ROW;
                    if n > 0 {
                        let len_bytes = n * BYTES_PER_ROW;
                        segments.push(LayoutSegment {
                            start_offset: current,
                            start_line: current_line,
                            byte_len: len_bytes,
                            line_count: n,
                            kind: SegmentKind::Standard,
                        });
                        current += len_bytes;
                        current_line += n;
                        continue;
                    }
                }
                None if total_size - current >= BYTES_PER_ROW => {
                    // No more events, and we have at least one full standard line remaining.
                    let remaining_bytes = total_size - current;
                    let n = remaining_bytes / BYTES_PER_ROW;
                    let len_bytes = n * BYTES_PER_ROW;
                    segments.push(LayoutSegment {
                        start_offset: current,
                        start_line: current_line,
                        byte_len: len_bytes,
                        line_count: n,
                        kind: SegmentKind::Standard,
                    });
                    current += len_bytes;
                    current_line += n;
                    continue;
                }
                _ => {}
            }

            // Otherwise, we are too close to an event or at the end of the file.
            // Generate a custom segment using localized layout logic.
            let mut starts = Vec::new();
            let start_offset = current;
            let start_line = current_line;

            while current < total_size {
                // Check if we can transition back to Standard mode.
                if !starts.is_empty() {
                    while event_idx < layout_events.len() && layout_events[event_idx] < current {
                        event_idx += 1;
                    }
                    let next_ev = if event_idx < layout_events.len() {
                        Some(layout_events[event_idx])
                    } else {
                        None
                    };

                    let can_transition = match next_ev {
                        Some(ev) => ev - current > BYTES_PER_ROW,
                        None => total_size - current >= BYTES_PER_ROW,
                    };

                    if can_transition {
                        break;
                    }
                }

                // Process empty lines at current.
                if let Some(&count) = empty_lines.get(&current) {
                    for _ in 0..count {
                        starts.push(current);
                    }
                }

                starts.push(current);

                // Find the next event break after current (includes structure
                // field breaks and custom breaks) in O(1) amortized time.
                while break_idx < break_events.len() && break_events[break_idx] <= current {
                    break_idx += 1;
                }
                let next_event_break = break_events.get(break_idx).copied();

                // Advance in BYTES_PER_ROW increments, skipping joined boundaries.
                let mut next_pos = current + BYTES_PER_ROW;
                while custom_joins.contains(&next_pos) && next_pos < total_size {
                    next_pos += BYTES_PER_ROW;
                }

                match next_event_break {
                    Some(break_pos) if break_pos < next_pos && break_pos > current => {
                        current = break_pos;
                    }
                    _ => {
                        current = next_pos;
                    }
                }
            }

            let line_count = starts.len();
            let byte_len = current - start_offset;

            segments.push(LayoutSegment {
                start_offset,
                start_line,
                byte_len,
                line_count,
                kind: SegmentKind::Custom { starts: Arc::new(starts) },
            });
            current_line += line_count;
        }
    }

    // Compute max_bytes_per_row and total_lines once from the compact segments.
    let mut max_bytes_per_row = BYTES_PER_ROW;
    let mut total_lines = 0;
    for i in 0..segments.len() {
        let segment = &segments[i];
        total_lines += segment.line_count;
        match &segment.kind {
            SegmentKind::Standard => {
                if i + 1 == segments.len() {
                    let last_line_start = segment.start_offset + (segment.line_count - 1) * BYTES_PER_ROW;
                    let last_line_len = total_size - last_line_start;
                    max_bytes_per_row = max_bytes_per_row.max(last_line_len);
                }
            }
            SegmentKind::Custom { starts } => {
                let next_start_offset = if i + 1 < segments.len() { segments[i + 1].start_offset } else { total_size };
                for j in 0..segment.line_count {
                    let end = if j + 1 < segment.line_count { starts[j + 1] } else { next_start_offset };
                    max_bytes_per_row = max_bytes_per_row.max(end.saturating_sub(starts[j]));
                }
            }
        }
    }

    LineMap::Sparse(Arc::new(SparseLineMap {
        segments,
        total_lines,
        total_size,
        max_bytes_per_row,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_linemap() {
        let map = LineMap::Standard { total_size: 32 };
        assert_eq!(map.len(), 2);
        assert!(!map.is_empty());
        assert_eq!(map.get(0), Some(0));
        assert_eq!(map.get(1), Some(16));
        assert_eq!(map.get(2), None);
        assert_eq!(map.binary_search(&0), Ok(0));
        assert_eq!(map.binary_search(&16), Ok(1));
        assert_eq!(map.binary_search(&8), Err(1));
        assert_eq!(map.max_bytes_per_row(), 16);
    }

    #[test]
    fn test_standard_linemap_empty() {
        let map = LineMap::Standard { total_size: 0 };
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(0), Some(0));
        assert_eq!(map.binary_search(&0), Ok(0));
        assert_eq!(map.binary_search(&10), Err(1));
    }

    #[test]
    fn test_sparse_linemap() {
        let seg = LayoutSegment {
            start_offset: 0,
            start_line: 0,
            byte_len: 10,
            line_count: 1,
            kind: SegmentKind::Standard,
        };
        let sparse = SparseLineMap {
            segments: vec![seg],
            total_lines: 1,
            total_size: 10,
            max_bytes_per_row: 10,
        };
        let map = LineMap::Sparse(Arc::new(sparse));
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(0), Some(0));
        assert_eq!(map.binary_search(&0), Ok(0));
        assert_eq!(map.max_bytes_per_row(), 10);
    }
}
