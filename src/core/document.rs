#![allow(dead_code)]

use crate::core::bookmark::{BookmarkColor, BookmarkItem};
use crate::core::buffer::Buffer;
use crate::core::hex_import::AddressMap;
use crate::core::history::History;
use crate::core::structure::{KsyDefinition, ParseResult};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Represents a document processing unit that bundles a file buffer and its edit history.
pub struct Document {
    pub path: PathBuf,
    pub buffer: Buffer,
    pub history: History,
    pub last_saved_version: usize,
    read_only: bool,
    pub address_map: AddressMap,
    pub bookmarks: Arc<RwLock<Vec<BookmarkItem>>>,
    pub ksy_definition: Arc<RwLock<Option<Arc<KsyDefinition>>>>,
    pub parse_result: Arc<RwLock<Option<Arc<ParseResult>>>>,
    pub custom_breaks: Arc<RwLock<BTreeSet<usize>>>,
    pub custom_joins: Arc<RwLock<BTreeSet<usize>>>,
    pub empty_lines: Arc<RwLock<BTreeMap<usize, usize>>>,
    pub hidden_bookmark_colors: Arc<RwLock<HashSet<BookmarkColor>>>,
    pub hidden_bookmark_ids: Arc<RwLock<HashSet<String>>>,
    pub hide_unbookmarked: Arc<RwLock<bool>>,
}

impl Document {
    pub fn new(path: PathBuf, buffer: Buffer) -> Self {
        Self {
            path,
            buffer,
            history: History::new(),
            last_saved_version: 0,
            read_only: false,
            address_map: AddressMap::default(),
            bookmarks: Arc::new(RwLock::new(Vec::new())),
            ksy_definition: Arc::new(RwLock::new(None)),
            parse_result: Arc::new(RwLock::new(None)),
            custom_breaks: Arc::new(RwLock::new(BTreeSet::new())),
            custom_joins: Arc::new(RwLock::new(BTreeSet::new())),
            empty_lines: Arc::new(RwLock::new(BTreeMap::new())),
            hidden_bookmark_colors: Arc::new(RwLock::new(HashSet::new())),
            hidden_bookmark_ids: Arc::new(RwLock::new(HashSet::new())),
            hide_unbookmarked: Arc::new(RwLock::new(false)),
        }
    }

    /// Creates a document that starts in read-only mode.
    pub fn new_read_only(path: PathBuf, buffer: Buffer) -> Self {
        let mut document = Self::new(path, buffer);
        document.read_only = true;
        document
    }

    /// Sets the address map for this document and returns self.
    pub fn with_address_map(mut self, address_map: AddressMap) -> Self {
        self.address_map = address_map;
        self
    }

    /// Returns the base address of the document.
    pub fn base_address(&self) -> usize {
        self.address_map.base_address()
    }

    /// Converts a linear buffer offset to its physical memory address.
    pub fn offset_to_address(&self, offset: usize) -> usize {
        self.address_map.offset_to_address(offset)
    }

    /// Converts a physical memory address to a linear buffer offset.
    pub fn address_to_offset(&self, address: usize) -> Option<usize> {
        self.address_map.address_to_offset(address)
    }

    /// Returns the maximum contiguous slice of bytes starting at `offset` up to `count` bytes.
    ///
    /// If `address_map` defines memory segments, the returned slice will not exceed the current
    /// segment's boundary, ensuring that unmapped address gaps are not bridged.
    pub fn read_contiguous_bytes(&self, offset: usize, count: usize) -> &[u8] {
        let data = self.buffer.data();
        if offset >= data.len() || count == 0 {
            return &[];
        }

        let max_end = if self.address_map.segments.is_empty() {
            data.len()
        } else {
            match self.address_map.segment_at_offset(offset) {
                Some(seg) => seg.end_buffer_offset(),
                None => return &[],
            }
        };

        let end = offset.saturating_add(count).min(max_end).min(data.len());
        if offset < end { &data[offset..end] } else { &[] }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Changes the file path used by subsequent save operations.
    pub fn set_path(&mut self, path: PathBuf) {
        self.path = path;
    }

    /// Returns true if the document has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.history.state_id() != self.last_saved_version
    }

    /// Returns whether editing and normal saves are currently disabled.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Changes whether the document accepts edits and normal saves.
    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }

    /// Toggles read-only mode and returns the new state.
    pub fn toggle_read_only(&mut self) -> bool {
        self.read_only = !self.read_only;
        self.read_only
    }

    /// Marks the document as saved, updating the last saved version.
    pub fn mark_as_saved(&mut self) {
        self.last_saved_version = self.history.state_id();
    }

    /// Computes the active folded regions (as a start -> end map).
    ///
    /// Hidden bookmarks and unbookmarked gaps (when `hide_unbookmarked` is enabled)
    /// are calculated as separate, non-overlapping folded intervals.
    pub fn computed_folded_regions(&self) -> BTreeMap<usize, usize> {
        let total_size = self.buffer.len();
        if total_size == 0 {
            return BTreeMap::new();
        }

        let is_hide_unbookmarked = *self.hide_unbookmarked.read().expect("hide_unbookmarked read lock");
        let hidden_colors = self.hidden_bookmark_colors.read().expect("hidden_bookmark_colors read lock");
        let hidden_ids = self.hidden_bookmark_ids.read().expect("hidden_bookmark_ids read lock");
        let bookmarks = self.bookmarks.read().expect("bookmarks read lock");

        let mut bookmarked_ranges = Vec::new();
        let mut hidden_ranges = Vec::new();

        for item in bookmarks.iter() {
            if item.size > 0 {
                let start = item.offset.min(total_size);
                let end = item.offset.saturating_add(item.size).min(total_size);
                if start < end {
                    bookmarked_ranges.push((start, end));
                    if hidden_colors.contains(&item.color) || hidden_ids.contains(&item.id) {
                        hidden_ranges.push((start, end));
                    }
                }
            }
        }
        drop(bookmarks);
        drop(hidden_colors);
        drop(hidden_ids);

        let mut folds = BTreeMap::new();

        // 1. Hidden bookmark ranges become folds
        if !hidden_ranges.is_empty() {
            hidden_ranges.sort_unstable_by_key(|&(s, e)| (s, e));
            let mut cur_start = hidden_ranges[0].0;
            let mut cur_end = hidden_ranges[0].1;
            for &(s, e) in &hidden_ranges[1..] {
                if s < cur_end {
                    cur_end = cur_end.max(e);
                } else {
                    folds.insert(cur_start, cur_end);
                    cur_start = s;
                    cur_end = e;
                }
            }
            folds.insert(cur_start, cur_end);
        }

        // 2. If hide_unbookmarked is enabled, unbookmarked gaps also become folds (separate from bookmarks)
        if is_hide_unbookmarked {
            if bookmarked_ranges.is_empty() {
                folds.insert(0, total_size);
            } else {
                bookmarked_ranges.sort_unstable_by_key(|&(s, e)| (s, e));
                let mut merged_bm = Vec::new();
                let mut cur_start = bookmarked_ranges[0].0;
                let mut cur_end = bookmarked_ranges[0].1;
                for &(s, e) in &bookmarked_ranges[1..] {
                    if s <= cur_end {
                        cur_end = cur_end.max(e);
                    } else {
                        merged_bm.push((cur_start, cur_end));
                        cur_start = s;
                        cur_end = e;
                    }
                }
                merged_bm.push((cur_start, cur_end));

                let mut cursor = 0;
                for (bm_s, bm_e) in merged_bm {
                    if bm_s > cursor {
                        folds.insert(cursor, bm_s);
                    }
                    cursor = bm_e;
                }
                if cursor < total_size {
                    folds.insert(cursor, total_size);
                }
            }
        }

        folds
    }

    /// Returns summary details for a folded region starting at `offset`.
    pub fn fold_bookmark_summary_at(&self, offset: usize) -> Option<FoldedBookmarkSummary> {
        let folded = self.computed_folded_regions();
        let fold_end = folded.get(&offset).copied()?;
        let hidden_colors = self.hidden_bookmark_colors.read().expect("hidden_bookmark_colors read lock");
        let hidden_ids = self.hidden_bookmark_ids.read().expect("hidden_bookmark_ids read lock");
        let bookmarks = self.bookmarks.read().expect("bookmarks read lock");

        let mut matched_items = Vec::new();
        for item in bookmarks.iter() {
            if (hidden_colors.contains(&item.color) || hidden_ids.contains(&item.id))
                && item.offset < fold_end
                && item.offset.saturating_add(item.size) > offset
            {
                matched_items.push(item);
            }
        }

        let is_unbookmarked = matched_items.is_empty();
        let primary = matched_items.first().copied();
        let color = primary.map(|it| it.color).unwrap_or_default();
        let comment = primary
            .map(|it| it.comment.clone())
            .unwrap_or_else(|| if is_unbookmarked { "Unbookmarked".to_string() } else { String::new() });
        let bookmark_ids = matched_items.iter().map(|it| it.id.clone()).collect();

        Some(FoldedBookmarkSummary {
            start_offset: offset,
            end_offset: fold_end,
            size: fold_end.saturating_sub(offset),
            color,
            comment,
            bookmark_ids,
            is_unbookmarked,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FoldedBookmarkSummary {
    pub start_offset: usize,
    pub end_offset: usize,
    pub size: usize,
    pub color: BookmarkColor,
    pub comment: String,
    pub bookmark_ids: Vec<String>,
    pub is_unbookmarked: bool,
}

impl Drop for Document {
    fn drop(&mut self) {
        let old_definition = self.ksy_definition.write().ok().and_then(|mut definition| definition.take());
        let old_parse_result = self.parse_result.write().ok().and_then(|mut result| result.take());
        if old_definition.is_some() || old_parse_result.is_some() {
            std::thread::spawn(move || {
                drop((old_definition, old_parse_result));
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::command::Command;
    use crate::core::editor::Editor;

    struct MockCommand;

    impl Command for MockCommand {
        fn execute(&mut self, _editor: &mut Editor) {}
        fn undo(&mut self, _editor: &mut Editor) {}
    }

    #[test]
    fn test_is_dirty() {
        let mut doc = Document::new(PathBuf::from("test"), Buffer::empty());

        // Initially clean
        assert!(!doc.is_dirty());

        // Simulate modification (push command to history)
        doc.history.push(Box::new(MockCommand));
        assert!(doc.is_dirty());

        // Simulate save
        doc.mark_as_saved();
        assert!(!doc.is_dirty());

        // Modify again
        doc.history.push(Box::new(MockCommand));
        assert!(doc.is_dirty());

        // Undo
        // Note: history.pop_undo() returns the command and removes it from undo stack
        doc.history.pop_undo();
        assert!(!doc.is_dirty());
    }

    #[test]
    fn test_read_only_state() {
        let mut doc = Document::new_read_only(PathBuf::from("test"), Buffer::empty());
        assert!(doc.is_read_only());

        doc.set_read_only(false);
        assert!(!doc.is_read_only());
        assert!(doc.toggle_read_only());
        assert!(doc.is_read_only());
    }

    #[test]
    fn test_read_contiguous_bytes_with_segments() {
        use crate::core::hex_import::{AddressMap, MemorySegment};

        let data = b"Hello 0Hello 1".to_vec();
        let map = AddressMap::from_segments(vec![
            MemorySegment {
                buffer_offset: 0,
                address: 0x0000,
                length: 7,
            },
            MemorySegment {
                buffer_offset: 7,
                address: 0x1000,
                length: 7,
            },
        ]);
        let doc = Document::new(PathBuf::from("test.mot"), Buffer::new(data)).with_address_map(map);

        // Near end of segment 0 (offset 5, reading 8 bytes): must clamp to segment 0 boundary (offset 7)
        let bytes = doc.read_contiguous_bytes(5, 8);
        assert_eq!(bytes, b" 0");

        // Within segment 1: reading 4 bytes
        let bytes = doc.read_contiguous_bytes(7, 4);
        assert_eq!(bytes, b"Hell");

        // Out of bounds
        assert!(doc.read_contiguous_bytes(14, 8).is_empty());
    }
}
