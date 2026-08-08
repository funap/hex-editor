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
