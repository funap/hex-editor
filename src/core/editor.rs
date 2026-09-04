#![allow(dead_code)]

pub mod cursor;
pub mod layout_engine;
pub mod search_state;
pub mod structure_state;
pub mod view_options;

pub use cursor::{CursorModel, CursorState};
pub use layout_engine::LayoutEngine;
pub use search_state::SearchState;
pub use structure_state::EditorStructureState;
pub use view_options::ViewOptions;

use crate::core::bookmark::{BookmarkColor, BookmarkFile, BookmarkItem, generate_bookmark_id};
use crate::core::color::RgbaColor;
use crate::core::command::{Command, ReplaceRangeCommand};
use crate::core::document::Document;
use crate::core::encoding::Encoding;
use crate::core::radix::{ByteGroupSize, DisplayRadix};
use crate::core::selection::Selection;
use crate::core::structure::{ParseResult, ParsedField};
use std::collections::BTreeSet;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::sync::RwLock;

pub use crate::core::document::FoldedBookmarkSummary;
pub use crate::core::layout::{BYTES_PER_ROW, LineMap};

/// Represents the editor.
pub struct Editor {
    // Shared document containing buffer, history, and metadata
    pub document: Arc<RwLock<Document>>,
    pub cursor: CursorModel,
    pub layout: LayoutEngine,
    pub options: ViewOptions,
    pub search_state: SearchState,
    pub structure: EditorStructureState,
}

impl Editor {
    pub fn new(document: Arc<RwLock<Document>>) -> Self {
        let cached_layout_version = document.read().expect("document read lock").layout_version();

        Self {
            document,
            cursor: CursorModel::default(),
            layout: LayoutEngine::new(cached_layout_version),
            options: ViewOptions::default(),
            search_state: SearchState::default(),
            structure: EditorStructureState::default(),
        }
    }

    pub fn total_size(&self) -> usize {
        let binding = self.document.read().expect("document read lock");
        let buffer = &binding.buffer;
        buffer.len()
    }

    pub fn is_read_only(&self) -> bool {
        self.document.read().expect("document read lock").is_read_only()
    }

    pub fn set_read_only(&mut self, read_only: bool) {
        self.document.write().expect("document write lock").set_read_only(read_only);
    }

    pub fn toggle_read_only(&mut self) -> bool {
        self.document.write().expect("document write lock").toggle_read_only()
    }

    pub fn find_line_index(offset: usize, line_starts: &LineMap) -> usize {
        LayoutEngine::find_line_index(offset, line_starts)
    }

    pub fn find_line_index_in_slice(offset: usize, line_starts: &[usize]) -> usize {
        LayoutEngine::find_line_index_in_slice(offset, line_starts)
    }

    pub fn prev_data_line(idx: usize, line_starts: &LineMap, folded_regions: &std::collections::BTreeMap<usize, usize>) -> Option<usize> {
        LayoutEngine::prev_data_line(idx, line_starts, folded_regions)
    }

    pub fn next_data_line(idx: usize, line_starts: &LineMap, total_size: usize, folded_regions: &std::collections::BTreeMap<usize, usize>) -> Option<usize> {
        LayoutEngine::next_data_line(idx, line_starts, total_size, folded_regions)
    }

    pub fn value_at_cursor(&self) -> Option<u8> {
        let binding = self.document.read().expect("document read lock");
        let buffer = &binding.buffer;
        buffer.data().get(self.cursor.offset).copied()
    }

    pub fn read_bytes_at_cursor(&self, count: usize) -> Vec<u8> {
        let binding = self.document.read().expect("document read lock");
        binding.read_contiguous_bytes(self.cursor.offset, count).to_vec()
    }

    pub fn set_encoding(&mut self, encoding: Encoding) {
        if self.options.encoding != encoding {
            self.options.encoding = encoding;
            self.search_state.on_encoding_changed();
        }
    }

    pub fn set_radix(&mut self, radix: DisplayRadix) {
        self.options.radix = radix;
    }

    pub fn set_group_size(&mut self, group_size: ByteGroupSize) {
        self.options.group_size = group_size;
        self.cursor.set_group_size(group_size, self.total_size());
    }

    pub fn set_is_big_endian(&mut self, is_big_endian: bool) {
        self.options.is_big_endian = is_big_endian;
    }

    pub fn toggle_byte_order(&mut self) {
        self.options.is_big_endian = !self.options.is_big_endian;
    }

    pub fn selection_range(&self) -> Option<Range<usize>> {
        self.cursor.selection_range(self.total_size())
    }

    pub fn selection(&self) -> Selection {
        self.cursor.selection(self.total_size())
    }

    pub fn has_selection(&self) -> bool {
        self.cursor.has_selection(self.total_size())
    }

    pub fn set_selection(&mut self, anchor: usize, active: usize) {
        self.cursor.set_selection(anchor, active, self.total_size());
    }

    pub fn set_selection_range(&mut self, range: Range<usize>) {
        self.cursor.set_selection_range(range, self.total_size());
    }

    pub fn clear_selection(&mut self) {
        self.cursor.clear_selection(self.total_size());
    }

    pub fn insert_cursor_offset(&self) -> usize {
        self.cursor.insert_cursor_offset(self.total_size())
    }

    pub fn selected_range_or_cursor(&self) -> Option<Range<usize>> {
        self.cursor.selected_range_or_cursor(self.total_size())
    }

    pub fn edit_range(&self) -> Option<Range<usize>> {
        self.cursor.edit_range(self.total_size())
    }

    pub fn set_cursor_offset(&mut self, offset: usize) {
        self.cursor.set_cursor_offset(offset, self.total_size());
    }

    pub fn set_cursor_offset_exact(&mut self, offset: usize) {
        self.cursor.set_cursor_offset_exact(offset, self.total_size());
    }

    pub(crate) fn cursor_state(&self) -> CursorState {
        self.cursor.cursor_state()
    }

    pub(crate) fn restore_cursor_state(&mut self, state: CursorState) {
        self.cursor.restore_cursor_state(state, self.total_size());
    }

    pub fn adjust_after_edit(&mut self, start: usize, old_len: usize, new_len: usize) {
        self.cursor.adjust_after_edit(start, old_len, new_len, self.total_size());
        self.layout.invalidate();
    }

    pub fn move_left(&mut self) {
        self.cursor.move_left(self.total_size());
    }

    pub fn move_left_for_insert(&mut self) {
        self.cursor.move_left_for_insert(self.total_size());
    }

    pub fn move_right_for_insert(&mut self) {
        self.cursor.move_right_for_insert(self.total_size());
    }

    pub fn move_right(&mut self) {
        self.cursor.move_right(self.total_size());
    }

    pub fn move_up(&mut self) {
        let line_map = self.line_starts();
        let folded = self.computed_folded_regions();
        self.cursor.move_up(&line_map, &folded, self.total_size());
    }

    pub fn move_down(&mut self) {
        let line_map = self.line_starts();
        let folded = self.computed_folded_regions();
        self.cursor.move_down(&line_map, &folded, self.total_size());
    }

    pub fn move_down_for_insert(&mut self) {
        let line_map = self.line_starts();
        let folded = self.computed_folded_regions();
        self.cursor.move_down_for_insert(&line_map, &folded, self.total_size());
    }

    pub fn select_left(&mut self) {
        self.cursor.select_left(self.total_size());
    }

    pub fn select_left_for_insert(&mut self) {
        self.cursor.select_left_for_insert(self.total_size());
    }

    pub fn select_right(&mut self) {
        self.cursor.select_right(self.total_size());
    }

    pub fn select_right_for_insert(&mut self) {
        self.cursor.select_right_for_insert(self.total_size());
    }

    pub fn select_up_for_insert(&mut self) {
        let line_map = self.line_starts();
        let folded = self.computed_folded_regions();
        self.cursor.select_up_for_insert(&line_map, &folded, self.total_size());
    }

    pub fn select_up(&mut self) {
        let line_map = self.line_starts();
        let folded = self.computed_folded_regions();
        self.cursor.select_up(&line_map, &folded, self.total_size());
    }

    pub fn select_down_for_insert(&mut self) {
        let line_map = self.line_starts();
        let folded = self.computed_folded_regions();
        self.cursor.select_down_for_insert(&line_map, &folded, self.total_size());
    }

    pub fn select_down(&mut self) {
        let line_map = self.line_starts();
        let folded = self.computed_folded_regions();
        self.cursor.select_down(&line_map, &folded, self.total_size());
    }

    pub fn select_all(&mut self) {
        self.cursor.select_all(self.total_size());
    }

    pub fn go_to_beginning(&mut self) {
        self.cursor.go_to_beginning(self.total_size());
    }

    pub fn go_to_end(&mut self) {
        self.cursor.go_to_end(self.total_size());
    }

    pub fn go_to_offset(&mut self, offset: usize, extend_selection: bool) {
        let target = if self.total_size() == 0 {
            0
        } else {
            offset.min(self.total_size().saturating_sub(1))
        };
        self.auto_unfold_if_needed(target);
        self.cursor.go_to_offset(offset, extend_selection, self.total_size());
    }

    pub fn page_up(&mut self, visible_rows: usize) {
        let line_map = self.line_starts();
        self.cursor.page_up(visible_rows, &line_map, self.total_size());
    }

    pub fn page_down(&mut self, visible_rows: usize) {
        let line_map = self.line_starts();
        self.cursor.page_down(visible_rows, &line_map, self.total_size());
    }

    pub fn home(&mut self) {
        self.cursor.home(self.total_size());
    }

    pub fn end(&mut self) {
        self.cursor.end(self.total_size());
    }

    pub fn select_page_up(&mut self, visible_rows: usize) {
        let line_map = self.line_starts();
        self.cursor.select_page_up(visible_rows, &line_map, self.total_size());
    }

    pub fn select_page_down(&mut self, visible_rows: usize) {
        let line_map = self.line_starts();
        self.cursor.select_page_down(visible_rows, &line_map, self.total_size());
    }

    pub fn select_home_for_insert(&mut self) {
        self.cursor.select_home_for_insert(self.total_size());
    }

    pub fn select_home(&mut self) {
        self.cursor.select_home(self.total_size());
    }

    pub fn select_end(&mut self) {
        self.cursor.select_end(self.total_size());
    }

    pub fn select_end_for_insert(&mut self) {
        self.cursor.select_end_for_insert(self.total_size());
    }

    pub fn start_drag(&mut self, byte_pos: usize) {
        self.cursor.start_drag(byte_pos, self.total_size());
    }

    pub fn continue_drag(&mut self, anchor_pos: usize, byte_pos: usize) {
        self.cursor.continue_drag(anchor_pos, byte_pos, self.total_size());
    }

    pub fn line_starts(&self) -> LineMap {
        let doc = self.document.read().expect("document read lock");
        self.layout.line_starts(
            &doc,
            self.structure.show_inline_structure_view,
            self.structure.is_parsing,
            &self.structure.collapsed_struct_ids,
        )
    }

    pub fn has_custom_layout(&self) -> bool {
        let doc = self.document.read().expect("document read lock");
        LayoutEngine::has_custom_layout(&doc, self.structure.show_inline_structure_view, self.structure.is_parsing)
    }

    pub fn has_custom_layout_doc(&self, doc: &Document) -> bool {
        LayoutEngine::has_custom_layout(doc, self.structure.show_inline_structure_view, self.structure.is_parsing)
    }

    pub fn is_parsing_structure(&self) -> bool {
        self.structure.is_parsing
    }

    pub fn is_finalizing_structure(&self) -> bool {
        self.structure.is_finalizing
    }

    pub fn show_inline_structure_view(&self) -> bool {
        self.structure.show_inline_structure_view
    }

    pub fn parse_progress_offset(&self) -> usize {
        self.structure.progress_offset
    }

    pub fn parse_total_size(&self) -> usize {
        self.structure.total_size
    }

    pub fn parse_generation(&self) -> usize {
        self.structure.generation
    }

    pub fn structure_parse_async(&self) -> bool {
        self.structure.is_async
    }

    pub fn structure_reparse_requested(&self) -> bool {
        self.structure.reparse_requested
    }

    pub fn collapsed_struct_ids(&self) -> &std::collections::HashSet<String> {
        &self.structure.collapsed_struct_ids
    }

    pub fn collapsed_struct_ids_mut(&mut self) -> &mut std::collections::HashSet<String> {
        &mut self.structure.collapsed_struct_ids
    }

    pub fn bookmarks_snapshot(&self) -> Vec<BookmarkItem> {
        self.document.read().expect("document read lock").metadata.bookmarks.clone()
    }

    pub fn bookmark_by_id(&self, id: &str) -> Option<BookmarkItem> {
        self.document
            .read()
            .expect("document read lock")
            .metadata
            .bookmarks
            .iter()
            .find(|b| b.id == id)
            .cloned()
    }

    pub fn parse_result(&self) -> Option<Arc<ParseResult>> {
        self.document.read().expect("document read lock").metadata.parse_result.clone()
    }

    pub fn ksy_definition(&self) -> Option<Arc<crate::core::structure::KsyDefinition>> {
        self.document.read().expect("document read lock").metadata.ksy_definition.clone()
    }

    pub fn add_bookmark(&mut self, item: BookmarkItem) -> String {
        if item.size == 0 {
            return String::new();
        }
        let total = self.total_size();
        let clamped_offset = item.offset.min(total);
        let clamped_size = item.size.min(total.saturating_sub(clamped_offset));
        if clamped_size == 0 {
            return String::new();
        }

        let mut item = item;
        item.offset = clamped_offset;
        item.size = clamped_size;

        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        let bookmarks = &mut doc.metadata.bookmarks;

        // If an existing bookmark has the exact same range, update its color and/or comment
        if let Some(existing) = bookmarks.iter_mut().find(|h| h.offset == item.offset && h.size == item.size) {
            existing.color = item.color;
            if !item.comment.is_empty() {
                existing.comment = item.comment;
            }
            return existing.id.clone();
        }

        // Ensure ID is non-empty and unique within this editor
        if item.id.is_empty() || bookmarks.iter().any(|h| h.id == item.id) {
            item.id = generate_bookmark_id();
        }

        let id = item.id.clone();
        bookmarks.push(item);
        bookmarks.sort_by_key(|h| (h.offset, h.size));
        id
    }

    pub fn add_custom_bookmark(&mut self, range: Range<usize>, color: RgbaColor) {
        if range.is_empty() {
            return;
        }
        let total_len = self.total_size();
        let clamped_start = range.start.min(total_len);
        let clamped_end = range.end.min(total_len);
        if clamped_start >= clamped_end {
            return;
        }
        let new_range = clamped_start..clamped_end;
        let hl_color = BookmarkColor::from_rgba(color);

        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        let bookmarks = &mut doc.metadata.bookmarks;
        let mut updated = Vec::new();
        for h in bookmarks.drain(..) {
            let h_range = h.range();
            if h_range.end <= new_range.start || h_range.start >= new_range.end {
                updated.push(h);
            } else {
                if h_range.start < new_range.start {
                    let mut left = h.clone();
                    left.id = generate_bookmark_id();
                    left.size = new_range.start - h_range.start;
                    updated.push(left);
                }
                if h_range.end > new_range.end {
                    let mut right = h.clone();
                    right.id = generate_bookmark_id();
                    right.offset = new_range.end;
                    right.size = h_range.end - new_range.end;
                    updated.push(right);
                }
            }
        }
        updated.push(BookmarkItem::new(new_range.start, new_range.len(), hl_color, ""));
        updated.sort_by_key(|h| (h.offset, h.size));
        *bookmarks = updated;
    }

    pub fn update_bookmark_comment(&mut self, id: &str, comment: impl Into<String>) -> bool {
        let mut doc = self.document.write().expect("document write lock");
        if let Some(item) = doc.metadata.bookmarks.iter_mut().find(|h| h.id == id) {
            item.comment = comment.into();
            true
        } else {
            false
        }
    }

    pub fn update_bookmark_color(&mut self, id: &str, color: BookmarkColor) -> bool {
        let mut doc = self.document.write().expect("document write lock");
        if let Some(item) = doc.metadata.bookmarks.iter_mut().find(|h| h.id == id) {
            item.color = color;
            true
        } else {
            false
        }
    }

    pub fn update_bookmark_range(&mut self, id: &str, offset: usize, size: usize) -> bool {
        if size == 0 {
            return false;
        }
        let total = self.total_size();
        let clamped_offset = offset.min(total);
        let clamped_size = size.min(total.saturating_sub(clamped_offset));
        if clamped_size == 0 {
            return false;
        }

        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        let bookmarks = &mut doc.metadata.bookmarks;
        if let Some(item) = bookmarks.iter_mut().find(|h| h.id == id) {
            item.offset = clamped_offset;
            item.size = clamped_size;
            bookmarks.sort_by_key(|h| (h.offset, h.size));
            true
        } else {
            false
        }
    }

    pub fn remove_bookmark_by_id(&mut self, id: &str) -> bool {
        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        let bookmarks = &mut doc.metadata.bookmarks;
        let initial_len = bookmarks.len();
        bookmarks.retain(|h| h.id != id);
        bookmarks.len() < initial_len
    }

    pub fn remove_bookmark_by_index(&mut self, index: usize) -> Option<BookmarkItem> {
        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        let bookmarks = &mut doc.metadata.bookmarks;
        if index < bookmarks.len() { Some(bookmarks.remove(index)) } else { None }
    }

    pub fn clear_custom_bookmark(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        let bookmarks = &mut doc.metadata.bookmarks;
        let mut updated = Vec::new();
        for h in bookmarks.drain(..) {
            let h_range = h.range();
            if h_range.end <= range.start || h_range.start >= range.end {
                updated.push(h);
            } else {
                if h_range.start < range.start {
                    let mut left = h.clone();
                    left.id = generate_bookmark_id();
                    left.size = range.start - h_range.start;
                    updated.push(left);
                }
                if h_range.end > range.end {
                    let mut right = h.clone();
                    right.id = generate_bookmark_id();
                    right.offset = range.end;
                    right.size = h_range.end - range.end;
                    updated.push(right);
                }
            }
        }
        *bookmarks = updated;
        bookmarks.sort_by_key(|h| (h.offset, h.size));
    }

    pub fn clear_all_custom_bookmarks(&mut self) {
        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        doc.metadata.bookmarks.clear();
    }

    pub fn custom_bookmarks_for_rendering(&self) -> Vec<(Range<usize>, RgbaColor)> {
        self.document
            .read()
            .expect("document read lock")
            .metadata
            .bookmarks
            .iter()
            .map(|h| (h.range(), h.rgba_color()))
            .collect()
    }

    pub fn export_bookmarks_to_file(&self, path: &Path) -> anyhow::Result<()> {
        let doc = self.document.read().expect("document read lock");
        let doc_path = doc.path().to_path_buf();
        BookmarkFile::save_to_path(path, &doc.metadata.bookmarks, Some(&doc_path))
    }

    pub fn import_bookmarks_from_file(&mut self, path: &Path) -> anyhow::Result<usize> {
        let loaded = BookmarkFile::load_from_path(path)?;
        let count = loaded.len();
        for item in loaded {
            self.add_bookmark(item);
        }
        Ok(count)
    }

    pub fn replace_range(&mut self, range: Range<usize>, replacement: Vec<u8>) -> bool {
        let total = self.total_size();
        let start = range.start.min(total);
        let end = range.end.min(total).max(start);
        let old_len = end - start;
        let new_total = total - old_len + replacement.len();
        let cursor_after = if replacement.is_empty() {
            start.saturating_sub(1)
        } else {
            let next = start.saturating_add(replacement.len());
            if next < new_total { next } else { new_total.saturating_sub(1) }
        };
        self.replace_range_with_cursor(start..end, replacement, cursor_after)
    }

    /// Replaces a range and explicitly chooses the resulting cursor offset.
    pub fn replace_range_with_cursor(&mut self, range: Range<usize>, replacement: Vec<u8>, cursor_after: usize) -> bool {
        let total = self.total_size();
        let start = range.start.min(total);
        let end = range.end.min(total).max(start);
        let old = self.document.read().expect("document read lock").buffer.get_range(start, end - start).to_vec();
        if old == replacement {
            return false;
        }

        let success = self.execute_command(Box::new(ReplaceRangeCommand::new(start, old, replacement)));
        if success {
            self.set_cursor_offset_exact(cursor_after);
        }
        success
    }

    /// Inserts bytes at `position` and advances the cursor after them.
    pub fn insert_bytes(&mut self, position: usize, bytes: Vec<u8>) -> bool {
        let position = position.min(self.total_size());
        let cursor_after = position.saturating_add(bytes.len());
        self.replace_range_with_cursor(position..position, bytes, cursor_after)
    }

    /// Replaces the byte at `position` without changing the buffer length.
    pub fn replace_byte(&mut self, position: usize, byte: u8) -> bool {
        let total = self.total_size();
        if position >= total {
            return false;
        }
        let cursor_after = if position + 1 < total { position + 1 } else { position };
        self.replace_range_with_cursor(position..position + 1, vec![byte], cursor_after)
    }

    /// Deletes the selected bytes, or the byte at the cursor when there is no
    /// selection.
    pub fn delete_forward(&mut self) -> bool {
        let Some(range) = self.edit_range() else {
            return false;
        };
        let has_selection = self.has_selection();
        let cursor_after = if has_selection {
            range.start
        } else {
            range.start.min(self.total_size().saturating_sub(1))
        };
        self.replace_range_with_cursor(range, Vec::new(), cursor_after)
    }

    /// Deletes the selected bytes, or the byte immediately before the cursor
    /// when there is no selection.
    pub fn delete_backward(&mut self) -> bool {
        let total = self.total_size();
        if total == 0 {
            return false;
        }

        let has_selection = self.has_selection();
        let range = if has_selection {
            self.selection_range().expect("a non-empty selection has a range")
        } else if self.cursor.offset > 0 {
            self.cursor.offset - 1..self.cursor.offset
        } else {
            return false;
        };
        // Backspace removes the byte immediately before the caret. The caret
        // should remain at the deletion start, which is one position left of
        // its original location—not one additional position to the left.
        let cursor_after = range.start;
        self.replace_range_with_cursor(range, Vec::new(), cursor_after)
    }

    pub fn search_pattern(&self) -> Option<Vec<crate::core::search::PatternByte>> {
        self.search_state.search_pattern(self.options.encoding)
    }

    pub fn set_search_query(&mut self, query: String) {
        self.search_state.set_query(query);
    }

    pub fn set_search_query_and_mode(&mut self, query: String, mode: crate::core::search::SearchMode) {
        self.search_state.set_query_and_mode(query, mode);
    }

    pub fn set_search_results(&mut self, results: Vec<usize>, generation: usize, is_full: bool) {
        self.search_state.set_results(results, generation, is_full);
    }

    pub fn clear_search(&mut self) {
        self.search_state.clear();
    }

    /// Finds and navigates to the next occurrence starting after the current cursor offset,
    /// wrapping around if necessary. Updates cursor offset and returns the match offset.
    pub fn find_and_navigate_next(&mut self) -> Option<usize> {
        let pattern = self.search_pattern()?;
        let next_offset = {
            let doc = self.document.read().ok()?;
            let data = doc.buffer.data();
            let from_offset = self.cursor.offset.saturating_add(1);
            crate::core::search::find_next_occurrence(data, &pattern, from_offset)?
        };
        self.auto_unfold_if_needed(next_offset);
        self.cursor.offset = next_offset;
        Some(next_offset)
    }

    /// Finds and navigates to the previous occurrence starting before the current cursor offset,
    /// wrapping around if necessary. Updates cursor offset and returns the match offset.
    pub fn find_and_navigate_prev(&mut self) -> Option<usize> {
        let pattern = self.search_pattern()?;
        let prev_offset = {
            let doc = self.document.read().ok()?;
            let data = doc.buffer.data();
            crate::core::search::find_prev_occurrence(data, &pattern, self.cursor.offset)?
        };
        self.auto_unfold_if_needed(prev_offset);
        self.cursor.offset = prev_offset;
        Some(prev_offset)
    }

    pub fn next_search_result(&mut self) -> Option<usize> {
        if let Some(offset) = self.search_state.next_result_offset() {
            self.auto_unfold_if_needed(offset);
            self.cursor.offset = offset;
            Some(offset)
        } else {
            self.find_and_navigate_next()
        }
    }

    pub fn add_custom_break(&mut self, offset: usize) {
        let total_size = self.total_size();
        if offset < total_size {
            // カスタム改行を追加する前に、現在の行レイアウトを取得する。
            // 追加後は同じオフセットを含む「結合済みメガ行」が分割されるため、
            // その行に属していた custom_joins は到達不能になり無効化される。
            let line_starts = self.line_starts();
            let current_line_idx = Self::find_line_index(offset, &line_starts);
            let line_start = line_starts.get(current_line_idx).unwrap_or(0);
            let line_end = if current_line_idx + 1 < line_starts.len() {
                line_starts.get(current_line_idx + 1).unwrap_or(total_size)
            } else {
                total_size
            };
            let line_length = line_end.saturating_sub(line_start);

            {
                let mut doc = self.document.write().expect("document write lock");
                doc.bump_layout_version();
                let meta = &mut doc.metadata;
                let custom_breaks = &mut meta.custom_breaks;
                let custom_joins = &mut meta.custom_joins;

                // offset より後ろの custom_joins を削除する。
                // offset より前の join（例: オフセット18で分割する際の join@16）は
                // 第1部分 [line_start..offset] を1行に保つために必要なので残す。
                if offset < line_end {
                    let joins_to_remove: Vec<usize> = custom_joins.range((offset + 1)..line_end).copied().collect();
                    for j in joins_to_remove {
                        custom_joins.remove(&j);
                    }
                }

                custom_breaks.insert(offset);
                // Custom break と custom join が同じ位置にある場合、join を解除
                custom_joins.remove(&offset);

                // メガ行（1行が BYTES_PER_ROW を超える）を分割した場合、
                // 第2部分 [offset..line_end] が1行として維持されるよう再結合する。
                // offset から BYTES_PER_ROW ずつ進むステップを custom_joins に追加し、
                // line_end が offset+k*BYTES_PER_ROW と一致しない場合は line_end にも
                // custom_break を追加して行末を明示する。
                if line_length > BYTES_PER_ROW && offset != line_start {
                    let mut step = offset + BYTES_PER_ROW;
                    while step < line_end {
                        custom_joins.insert(step);
                        step += BYTES_PER_ROW;
                    }
                    // line_end が offset から BYTES_PER_ROW の倍数で到達できない場合、
                    // アルゴリズムが line_end をまたいでしまうため、明示的に break を追加する
                    if line_end < total_size && !(line_end - offset).is_multiple_of(BYTES_PER_ROW) && !custom_breaks.contains(&line_end) {
                        custom_breaks.insert(line_end);
                    }
                }
            }

            self.layout.invalidate();
        }
    }

    pub fn remove_custom_break(&mut self, offset: usize) {
        let mut doc = self.document.write().expect("document write lock");
        if doc.metadata.custom_breaks.remove(&offset) {
            doc.bump_layout_version();
            self.layout.invalidate();
        }
    }

    pub fn has_custom_break(&self, offset: usize) -> bool {
        self.document.read().expect("document read lock").metadata.custom_breaks.contains(&offset)
    }

    pub fn custom_breaks_count(&self) -> usize {
        self.document.read().expect("document read lock").metadata.custom_breaks.len()
    }

    pub fn custom_breaks_snapshot(&self) -> BTreeSet<usize> {
        self.document.read().expect("document read lock").metadata.custom_breaks.clone()
    }

    pub fn has_custom_join(&self, offset: usize) -> bool {
        self.document.read().expect("document read lock").metadata.custom_joins.contains(&offset)
    }

    pub fn empty_lines_at(&self, offset: usize) -> usize {
        self.document
            .read()
            .expect("document read lock")
            .metadata
            .empty_lines
            .get(&offset)
            .copied()
            .unwrap_or(0)
    }

    pub fn toggle_custom_break(&mut self, offset: usize) {
        let contains = self.has_custom_break(offset);
        if contains {
            self.remove_custom_break(offset);
        } else {
            self.add_custom_break(offset);
        }
    }

    pub fn add_empty_line(&mut self, offset: usize) {
        if offset <= self.total_size() {
            let mut doc = self.document.write().expect("document write lock");
            doc.bump_layout_version();
            *doc.metadata.empty_lines.entry(offset).or_insert(0) += 1;
            self.layout.invalidate();
        }
    }

    pub fn remove_empty_line(&mut self, offset: usize) -> bool {
        let mut doc = self.document.write().expect("document write lock");
        let empty_lines = &mut doc.metadata.empty_lines;
        if let Some(count) = empty_lines.get_mut(&offset) {
            if *count > 1 {
                *count -= 1;
            } else {
                empty_lines.remove(&offset);
            }
            doc.bump_layout_version();
            self.layout.invalidate();
            true
        } else {
            false
        }
    }

    /// カーソルの現在行と次の行を結合する。
    /// 範囲選択中（複数バイト選択時）は、その選択範囲全体が1行になるように結合する。
    /// 選択がない場合は、現在行と次の行を結合する。
    pub fn join_line(&mut self) {
        if let Some(range) = self.selection_range() {
            self.join_range(range);
            return;
        }

        let line_starts = self.line_starts();
        let current_line_idx = Self::find_line_index(self.cursor.offset, &line_starts);

        // 次の行がなければ何もしない
        if current_line_idx + 1 >= line_starts.len() {
            return;
        }

        let next_line_start = line_starts.get(current_line_idx + 1).expect("valid next line start");

        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        let meta = &mut doc.metadata;
        let custom_breaks = &mut meta.custom_breaks;
        let custom_joins = &mut meta.custom_joins;

        if custom_breaks.contains(&next_line_start) {
            // Custom Break による改行なら、その break を削除
            custom_breaks.remove(&next_line_start);
            self.layout.invalidate();
        } else if next_line_start != line_starts.get(current_line_idx).unwrap_or(0) {
            // 自然境界（16バイト境界 or カスタム改行後の次行など）を join として記録
            // next_line_start が現在行と同オフセット（空行の重複）でない場合のみ
            custom_joins.insert(next_line_start);
            self.layout.invalidate();
        }
    }

    /// 指定した範囲 [s..e) を1行に結合する。
    pub fn join_range(&mut self, range: Range<usize>) {
        let total = self.total_size();
        let s = range.start.min(total);
        let e = range.end.min(total);

        if s >= e {
            return;
        }

        let line_starts = self.line_starts();
        let current_line_idx = Self::find_line_index(s, &line_starts);
        let line_start_of_s = line_starts.get(current_line_idx).unwrap_or(0);

        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        let meta = &mut doc.metadata;
        let custom_breaks = &mut meta.custom_breaks;
        let custom_joins = &mut meta.custom_joins;
        let empty_lines = &mut meta.empty_lines;

        // 1. s が行の先頭でなければ、s に custom_break を追加して s から始まるようにする
        if s > 0 && s != line_start_of_s {
            custom_breaks.insert(s);
        }
        custom_joins.remove(&s);

        // 2. e がファイル末尾でなく、e で改行する必要がある場合は e に custom_break を追加する
        if e < total {
            custom_breaks.insert(e);
        }
        custom_joins.remove(&e);

        // 3. (s..e) 内の custom_breaks, custom_joins, empty_lines をすべて削除する
        let breaks_to_remove: Vec<usize> = custom_breaks.range((s + 1)..e).copied().collect();
        for b in breaks_to_remove {
            custom_breaks.remove(&b);
        }
        let joins_to_remove: Vec<usize> = custom_joins.range((s + 1)..e).copied().collect();
        for j in joins_to_remove {
            custom_joins.remove(&j);
        }
        let empty_lines_to_remove: Vec<usize> = empty_lines.range((s + 1)..e).map(|(&k, _)| k).collect();
        for el in empty_lines_to_remove {
            empty_lines.remove(&el);
        }

        // 4. s から BYTES_PER_ROW ずつ進むステップを custom_joins に追加し、1行に結合する
        let mut step = s + BYTES_PER_ROW;
        while step < e {
            custom_joins.insert(step);
            step += BYTES_PER_ROW;
        }

        self.layout.invalidate();
    }

    /// 全ての Custom Break と Join をクリアし、デフォルトの16バイト表示に戻す。
    pub fn clear_all_custom_breaks(&mut self) {
        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        doc.metadata.custom_breaks.clear();
        doc.metadata.custom_joins.clear();
        doc.metadata.empty_lines.clear();
        self.layout.invalidate();
    }

    /// Returns the active folded intervals [start, end) computed from hidden bookmarks or unbookmarked gaps.
    /// Overlapping and adjacent intervals are merged into disjoint union ranges.
    pub fn computed_folded_regions(&self) -> std::collections::BTreeMap<usize, usize> {
        if let Ok(doc) = self.document.read() {
            doc.computed_folded_regions()
        } else {
            std::collections::BTreeMap::new()
        }
    }

    /// Returns summary details for a folded region starting at `offset`.
    pub fn fold_bookmark_summary_at(&self, offset: usize) -> Option<FoldedBookmarkSummary> {
        self.document.read().ok()?.fold_bookmark_summary_at(offset)
    }

    pub fn is_bookmark_color_hidden(&self, color: BookmarkColor) -> bool {
        let doc = self.document.read().expect("document read lock");
        let meta = &doc.metadata;
        if meta.hidden_bookmark_colors.contains(&color) {
            return true;
        }
        if meta.hidden_bookmark_ids.is_empty() {
            return false;
        }
        let mut count_total = 0;
        let mut count_hidden = 0;
        for b in meta.bookmarks.iter() {
            if b.color == color {
                count_total += 1;
                if meta.hidden_bookmark_ids.contains(&b.id) {
                    count_hidden += 1;
                }
            }
        }
        count_total > 0 && count_total == count_hidden
    }

    pub fn is_bookmark_id_hidden(&self, id: &str) -> bool {
        self.document.read().expect("document read lock").metadata.hidden_bookmark_ids.contains(id)
    }

    pub fn is_bookmark_item_hidden(&self, item: &BookmarkItem) -> bool {
        let doc = self.document.read().expect("document read lock");
        let meta = &doc.metadata;
        meta.hidden_bookmark_colors.contains(&item.color) || meta.hidden_bookmark_ids.contains(&item.id)
    }

    pub fn toggle_bookmark_color(&mut self, color: BookmarkColor) {
        if self.is_bookmark_color_hidden(color) {
            self.show_bookmark_color(color);
        } else {
            self.hide_bookmark_color(color);
        }
    }

    pub fn show_bookmark_color(&mut self, color: BookmarkColor) {
        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        let meta = &mut doc.metadata;
        meta.hidden_bookmark_colors.remove(&color);
        for b in &meta.bookmarks {
            if b.color == color {
                meta.hidden_bookmark_ids.remove(&b.id);
            }
        }
        self.layout.invalidate();
    }

    pub fn hide_bookmark_color(&mut self, color: BookmarkColor) {
        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        let meta = &mut doc.metadata;
        meta.hidden_bookmark_colors.insert(color);
        for b in &meta.bookmarks {
            if b.color == color {
                meta.hidden_bookmark_ids.remove(&b.id);
            }
        }
        self.layout.invalidate();
    }

    pub fn show_only_bookmark_color(&mut self, target_color: BookmarkColor) {
        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        let meta = &mut doc.metadata;
        let all_colors: Vec<BookmarkColor> = meta.bookmarks.iter().map(|b| b.color).collect();
        meta.hidden_bookmark_colors.clear();
        for c in all_colors {
            if c != target_color {
                meta.hidden_bookmark_colors.insert(c);
            }
        }
        meta.hidden_bookmark_ids.clear();
        self.layout.invalidate();
    }

    pub fn show_all_bookmarks(&mut self) {
        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        doc.metadata.hidden_bookmark_colors.clear();
        doc.metadata.hidden_bookmark_ids.clear();
        self.layout.invalidate();
    }

    pub fn hide_all_bookmarks(&mut self) {
        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        let meta = &mut doc.metadata;
        let all_colors: Vec<BookmarkColor> = meta.bookmarks.iter().map(|b| b.color).collect();
        for c in all_colors {
            meta.hidden_bookmark_colors.insert(c);
        }
        meta.hidden_bookmark_ids.clear();
        self.layout.invalidate();
    }

    pub fn toggle_bookmark_item_visibility(&mut self, id: &str) {
        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        let meta = &mut doc.metadata;
        let target_item = meta.bookmarks.iter().find(|b| b.id == id).cloned();

        if let Some(target) = target_item {
            if meta.hidden_bookmark_colors.contains(&target.color) {
                meta.hidden_bookmark_colors.remove(&target.color);
                for other_bm in &meta.bookmarks {
                    if other_bm.color == target.color && other_bm.id != id {
                        meta.hidden_bookmark_ids.insert(other_bm.id.clone());
                    }
                }
                meta.hidden_bookmark_ids.remove(id);
            } else if meta.hidden_bookmark_ids.contains(id) {
                meta.hidden_bookmark_ids.remove(id);
            } else {
                meta.hidden_bookmark_ids.insert(id.to_string());
            }
        } else if meta.hidden_bookmark_ids.contains(id) {
            meta.hidden_bookmark_ids.remove(id);
        } else {
            meta.hidden_bookmark_ids.insert(id.to_string());
        }
        self.layout.invalidate();
    }

    pub fn unfold_bookmark_at(&mut self, offset: usize) -> bool {
        let folded = self.computed_folded_regions();
        let found = folded.iter().find(|&(&start, &end)| offset >= start && offset < end);
        if let Some((&start, &end)) = found {
            let mut doc = self.document.write().expect("document write lock");
            let meta = &mut doc.metadata;
            let mut colors_to_decompose = std::collections::HashSet::new();
            let mut ids_to_unhide = Vec::new();

            for it in &meta.bookmarks {
                if it.offset < end && it.offset.saturating_add(it.size) > start {
                    colors_to_decompose.insert(it.color);
                    ids_to_unhide.push(it.id.clone());
                }
            }

            let mut changed = false;
            for &color in &colors_to_decompose {
                if meta.hidden_bookmark_colors.contains(&color) {
                    meta.hidden_bookmark_colors.remove(&color);
                    for other_bm in &meta.bookmarks {
                        if other_bm.color == color {
                            let other_start = other_bm.offset;
                            let other_end = other_bm.offset.saturating_add(other_bm.size);
                            if !(other_start < end && other_end > start) {
                                meta.hidden_bookmark_ids.insert(other_bm.id.clone());
                            }
                        }
                    }
                    changed = true;
                }
            }

            for id in ids_to_unhide {
                if meta.hidden_bookmark_ids.remove(&id) {
                    changed = true;
                }
            }

            if meta.hide_unbookmarked && colors_to_decompose.is_empty() {
                meta.hide_unbookmarked = false;
                changed = true;
            }

            if changed {
                doc.bump_layout_version();
                self.layout.invalidate();
                return true;
            }
            false
        } else {
            false
        }
    }

    pub fn is_hide_unbookmarked(&self) -> bool {
        self.document.read().expect("document read lock").metadata.hide_unbookmarked
    }

    pub fn toggle_hide_unbookmarked(&mut self) {
        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        doc.metadata.hide_unbookmarked = !doc.metadata.hide_unbookmarked;
        self.layout.invalidate();
    }

    pub fn set_hide_unbookmarked(&mut self, hide: bool) {
        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        doc.metadata.hide_unbookmarked = hide;
        self.layout.invalidate();
    }

    pub fn auto_unfold_if_needed(&mut self, target_offset: usize) -> bool {
        self.unfold_bookmark_at(target_offset)
    }

    pub fn is_folded(&self, offset: usize) -> bool {
        let regions = self.computed_folded_regions();
        for (&s, &e) in regions.iter() {
            if offset >= s && offset < e {
                return true;
            }
        }
        false
    }

    pub fn fold_end_at(&self, start: usize) -> Option<usize> {
        let regions = self.computed_folded_regions();
        regions.get(&start).copied()
    }

    pub fn fold_containing(&self, offset: usize) -> Option<(usize, usize)> {
        let regions = self.computed_folded_regions();
        for (&s, &e) in regions.iter() {
            if offset >= s && offset < e {
                return Some((s, e));
            }
        }
        None
    }

    /// Returns the base address of the document.
    pub fn base_address(&self) -> usize {
        self.document.read().expect("document read lock").base_address()
    }

    /// Converts a buffer offset to its physical memory address.
    pub fn offset_to_address(&self, offset: usize) -> usize {
        self.document.read().expect("document read lock").offset_to_address(offset)
    }

    /// Returns the physical memory address of the current cursor position.
    pub fn cursor_address(&self) -> usize {
        self.offset_to_address(self.cursor.offset)
    }

    /// Converts a physical memory address to a buffer offset.
    pub fn address_to_offset(&self, address: usize) -> Option<usize> {
        self.document.read().expect("document read lock").address_to_offset(address)
    }

    /// Returns true if the document has multiple discontinuous memory segments with address gaps.
    pub fn has_address_gaps(&self) -> bool {
        self.document.read().expect("document read lock").address_map.has_gaps()
    }

    pub fn toggle_struct_collapsed(&mut self, struct_id: &str) {
        self.structure.toggle_collapsed(struct_id);
        self.layout.invalidate();
    }

    pub fn toggle_inline_structure_view(&mut self) {
        self.structure.toggle_inline_view();
        self.layout.invalidate();
    }

    pub fn custom_layout_count(&self) -> usize {
        let doc = self.document.read().expect("document read lock");
        let meta = &doc.metadata;
        meta.custom_breaks.len() + meta.custom_joins.len() + meta.empty_lines.values().sum::<usize>() + doc.computed_folded_regions().len()
    }

    pub fn prev_search_result(&mut self) -> Option<usize> {
        if let Some(offset) = self.search_state.prev_result_offset() {
            self.auto_unfold_if_needed(offset);
            self.cursor.offset = offset;
            Some(offset)
        } else {
            self.find_and_navigate_prev()
        }
    }

    pub fn current_search_result(&self) -> Option<usize> {
        self.search_state.current_result()
    }

    pub fn execute_command(&mut self, command: Box<dyn Command>) -> bool {
        if self.is_read_only() {
            return false;
        }
        let delta = self.document.write().expect("document write lock").execute_command(command);
        if let Some(delta) = delta {
            self.adjust_after_edit(delta.offset, delta.old_len, delta.new_len);
            self.document_changed();
            true
        } else {
            false
        }
    }

    pub fn undo(&mut self) -> bool {
        if self.is_read_only() {
            return false;
        }
        let delta = self.document.write().expect("document write lock").undo();
        if let Some(delta) = delta {
            self.adjust_after_edit(delta.offset, delta.old_len, delta.new_len);
            self.set_cursor_offset_exact(delta.offset);
            self.document_changed();
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if self.is_read_only() {
            return false;
        }
        let delta = self.document.write().expect("document write lock").redo();
        if let Some(delta) = delta {
            self.adjust_after_edit(delta.offset, delta.old_len, delta.new_len);
            self.set_cursor_offset_exact(delta.offset.saturating_add(delta.new_len));
            self.document_changed();
            true
        } else {
            false
        }
    }

    /// Returns whether this editor has an undoable command.
    pub fn can_undo(&self) -> bool {
        self.document.read().expect("document read lock").can_undo()
    }

    /// Returns whether this editor has a redoable command.
    pub fn can_redo(&self) -> bool {
        self.document.read().expect("document read lock").can_redo()
    }

    pub fn set_kaitai_definition(&mut self, ksy: Arc<crate::core::structure::KsyDefinition>) {
        self.cancel_structure_parsing();
        self.structure.is_async = false;
        self.structure.reparse_requested = false;
        {
            let mut doc = self.document.write().expect("document write lock");
            doc.bump_layout_version();
            doc.metadata.ksy_definition = Some(ksy);
        }
        self.reparse_structure();
    }

    pub fn set_ksy_definition(&mut self, ksy: Arc<crate::core::structure::KsyDefinition>) {
        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        doc.metadata.ksy_definition = Some(ksy);
    }

    pub fn set_parse_result(&mut self, result: ParseResult) {
        self.structure.progress_offset = result.total_parsed_bytes;
        self.structure.is_finalizing = false;
        {
            let mut doc = self.document.write().expect("document write lock");
            doc.bump_layout_version();
            doc.metadata.parse_result = Some(Arc::new(result));
        }
        self.layout.invalidate();
    }

    /// Starts a new partial structure result that can receive parse batches.
    pub fn begin_partial_parse_result(&mut self, definition_id: String) {
        let old = {
            let mut doc = self.document.write().expect("document write lock");
            doc.bump_layout_version();
            doc.metadata.parse_result.replace(Arc::new(ParseResult::empty(definition_id)))
        };
        if let Some(old_res) = old {
            crate::core::dealloc::discard_in_background(old_res);
        }
        self.structure.is_finalizing = false;
        self.layout.invalidate();
    }

    /// Appends a batch of parsed root fields without cloning earlier batches.
    pub fn append_parse_fields(&mut self, definition_id: String, fields: Vec<ParsedField>, offset: usize, total_size: usize) {
        let chunk: Arc<[ParsedField]> = Arc::from(fields.into_boxed_slice());
        self.append_parse_chunks(definition_id, vec![chunk], offset, total_size);
    }

    /// Appends shared parse chunks without cloning their fields.
    pub fn append_parse_chunks(&mut self, definition_id: String, chunks: Vec<Arc<[ParsedField]>>, offset: usize, total_size: usize) {
        self.structure.progress_offset = offset;
        self.structure.total_size = total_size;
        if chunks.iter().all(|chunk| chunk.is_empty()) {
            return;
        }

        let next = if let Some(current) = self.parse_result() {
            Arc::new(current.append_shared_chunks_without_index(&chunks, offset))
        } else {
            Arc::new(ParseResult::empty(definition_id).append_shared_chunks_without_index(&chunks, offset))
        };
        let mut doc = self.document.write().expect("document write lock");
        doc.bump_layout_version();
        doc.metadata.parse_result = Some(next);
    }

    pub fn set_parse_result_arc(&mut self, result: Arc<ParseResult>) {
        self.structure.progress_offset = result.total_parsed_bytes;
        self.structure.is_finalizing = false;
        let old = {
            let mut doc = self.document.write().expect("document write lock");
            doc.bump_layout_version();
            doc.metadata.parse_result.replace(result)
        };
        if let Some(old_res) = old {
            crate::core::dealloc::discard_in_background(old_res);
        }
        self.layout.invalidate();
    }

    pub fn update_parse_progress(&mut self, offset: usize, total_size: usize, intermediate_result: Option<ParseResult>) {
        self.structure.progress_offset = offset;
        self.structure.total_size = total_size;
        if let Some(res) = intermediate_result {
            let mut doc = self.document.write().expect("document write lock");
            doc.bump_layout_version();
            doc.metadata.parse_result = Some(Arc::new(res));
            self.layout.invalidate();
        }
    }

    pub fn invalidate_line_map(&self) {
        self.layout.invalidate();
    }

    pub fn reparse_structure(&mut self) {
        if let Some(ksy) = self.ksy_definition() {
            let (buffer, ksy_clone) = {
                let buffer_lock = self.document.read().expect("document read lock");
                (buffer_lock.buffer.clone(), (*ksy).clone())
            };
            let mut stream = crate::core::structure::KaitaiStream::new(buffer.data());
            let interpreter = crate::core::structure::KaitaiInterpreter::new(ksy_clone);
            let result = interpreter.parse(&mut stream);
            self.set_parse_result(result);
        }
    }

    /// Returns the current deferred edit request without consuming it.
    ///
    /// The UI uses the generation as a debounce token. A request remains
    /// pending until [`Self::take_structure_reparse_request`] starts it, so a
    /// newer edit can invalidate an older timer without racing the parser.
    pub fn pending_structure_reparse(&self) -> Option<(Arc<crate::core::structure::KsyDefinition>, usize)> {
        self.structure.pending_reparse(self.ksy_definition().as_ref())
    }

    /// Takes a deferred edit request if it still belongs to `generation`.
    pub fn take_structure_reparse_request(&mut self, generation: usize) -> Option<Arc<crate::core::structure::KsyDefinition>> {
        self.structure.take_reparse_request(generation, self.ksy_definition().as_ref())
    }

    fn document_changed(&mut self) {
        self.layout.invalidate();
        self.search_state.on_document_changed();

        if self.structure.is_async && self.ksy_definition().is_some() {
            self.cancel_structure_parsing();
            self.structure.reparse_requested = true;
            self.structure.is_parsing = true;
            self.structure.is_finalizing = false;
            self.structure.progress_offset = 0;
            self.structure.total_size = self.total_size();

            if let Some(ksy) = self.ksy_definition() {
                self.begin_partial_parse_result(ksy.meta.id.clone());
            }
        } else {
            self.reparse_structure();
        }
    }

    pub fn cancel_structure_parsing(&mut self) {
        self.structure.cancel();
    }

    pub fn clear_structure_definition(&mut self) {
        self.cancel_structure_parsing();
        self.structure.is_async = false;
        self.structure.reparse_requested = false;
        let (old_definition, old) = {
            let mut doc = self.document.write().expect("document write lock");
            doc.bump_layout_version();
            (doc.metadata.ksy_definition.take(), doc.metadata.parse_result.take())
        };
        if old_definition.is_some() || old.is_some() {
            crate::core::dealloc::discard_in_background((old_definition, old));
        }
        self.structure.reset_progress();
        self.layout.invalidate();
    }
}

#[cfg(test)]
mod tests;
