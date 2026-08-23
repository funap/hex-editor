#![allow(dead_code)]

use crate::core::bookmark::{BookmarkColor, BookmarkFile, BookmarkItem, generate_bookmark_id};
use crate::core::command::{Command, CursorState, ReplaceRangeCommand};
use crate::core::document::Document;
use crate::core::encoding::Encoding;
use crate::core::radix::{ByteGroupSize, DisplayRadix};
use crate::core::selection::Selection;
use crate::core::structure::{ParseResult, ParsedField};
use gpui::Hsla;
use std::cell::RefCell;
use std::cmp;
use std::collections::BTreeSet;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::sync::RwLock;

pub use crate::core::layout::{BYTES_PER_ROW, LayoutSegment, LineMap, SegmentKind, SparseLineMap};

#[derive(Default, Clone)]
pub struct SearchState {
    pub query: String,
    pub mode: crate::core::search::SearchMode,
    pub results: Vec<usize>,
    pub current_result_index: Option<usize>,
    pub is_full_search_complete: bool,
    pub generation: usize,
}

/// Represents the editor.
pub struct Editor {
    // Shared document containing buffer and history
    pub document: Arc<RwLock<Document>>,
    pub cursor_offset: usize,
    selection: Selection,
    pub search_state: SearchState,
    pub custom_breaks: Arc<RwLock<BTreeSet<usize>>>,
    /// 16バイト自然境界のうち、改行を抑制すべき位置を記録する。
    /// join_line() で追加され、行を16バイト超に結合するために使用。
    pub custom_joins: Arc<RwLock<BTreeSet<usize>>>,
    /// 特定のオフセットに挿入された空行の数を記録する。
    pub empty_lines: Arc<RwLock<std::collections::BTreeMap<usize, usize>>>,
    pub encoding: Encoding,
    pub radix: DisplayRadix,
    pub group_size: ByteGroupSize,
    pub is_big_endian: bool,
    pub ksy_definition: Arc<RwLock<Option<Arc<crate::core::structure::KsyDefinition>>>>,
    pub parse_result: Arc<RwLock<Option<Arc<ParseResult>>>>,
    pub is_parsing_structure: bool,
    /// True after byte parsing reaches the end and display indexes are being finalized.
    pub is_finalizing_structure: bool,
    pub parse_progress_offset: usize,
    pub parse_total_size: usize,
    pub parse_generation: usize,
    pub parse_cancel_token: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// Enables background reparsing after document edits.
    ///
    /// This is enabled by the UI entry point that owns the parser task. It
    /// remains disabled for the synchronous core API used by deterministic
    /// tests and non-UI callers.
    pub structure_parse_async: bool,
    /// Set after an edit until the UI starts the debounced background parse.
    pub structure_reparse_requested: bool,
    pub collapsed_struct_ids: std::collections::HashSet<String>,
    pub show_inline_structure_view: bool,
    pub bookmarks: Arc<RwLock<Vec<BookmarkItem>>>,
    cached_line_map: RefCell<Option<LineMap>>,
}

impl Editor {
    pub fn new(document: Arc<RwLock<Document>>) -> Self {
        let (bookmarks, ksy_definition, parse_result, custom_breaks, custom_joins, empty_lines) = {
            let doc = document.read().expect("document read lock");
            (
                doc.bookmarks.clone(),
                doc.ksy_definition.clone(),
                doc.parse_result.clone(),
                doc.custom_breaks.clone(),
                doc.custom_joins.clone(),
                doc.empty_lines.clone(),
            )
        };

        Self {
            document,
            cursor_offset: 0,
            selection: Selection::collapsed(0),
            search_state: SearchState::default(),
            custom_breaks,
            custom_joins,
            empty_lines,
            encoding: Encoding::default(),
            radix: DisplayRadix::default(),
            group_size: ByteGroupSize::default(),
            is_big_endian: false,
            ksy_definition,
            parse_result,
            is_parsing_structure: false,
            is_finalizing_structure: false,
            parse_progress_offset: 0,
            parse_total_size: 0,
            parse_generation: 0,
            parse_cancel_token: None,
            structure_parse_async: false,
            structure_reparse_requested: false,
            collapsed_struct_ids: std::collections::HashSet::new(),
            show_inline_structure_view: true,
            bookmarks,
            cached_line_map: RefCell::new(None),
        }
    }

    pub fn total_size(&self) -> usize {
        self.document.read().expect("document read lock").buffer.len()
    }

    /// Returns whether this editor's document currently rejects edits.
    pub fn is_read_only(&self) -> bool {
        self.document.read().expect("document read lock").is_read_only()
    }

    /// line_starts の中から、指定オフセットが属するデータ行（空行でない行）のインデックスを返す。
    /// 空行（重複エントリ）がある場合、最後の重複（データ行）を返す。
    pub fn find_line_index(offset: usize, line_starts: &LineMap) -> usize {
        match line_starts.binary_search(&offset) {
            Ok(mut idx) => {
                // 重複がある場合、最後の重複（データ行）に移動
                while idx + 1 < line_starts.len() && line_starts.get(idx + 1) == Some(offset) {
                    idx += 1;
                }
                idx
            }
            Err(idx) => idx.saturating_sub(1),
        }
    }

    /// 上方向の次のデータ行（空行をスキップ）のインデックスを返す。
    fn prev_data_line(idx: usize, line_starts: &LineMap) -> Option<usize> {
        let mut i = idx.checked_sub(1)?;
        if line_starts.is_empty() {
            return None;
        }
        // 行の長さを確認して空行をスキップ
        loop {
            let line_start = line_starts.get(i)?;
            let line_end = if i + 1 < line_starts.len() {
                line_starts.get(i + 1)?
            } else {
                return Some(i);
            };
            if line_end > line_start {
                return Some(i);
            }
            if i == 0 {
                return None;
            }
            i -= 1;
        }
    }

    /// 下方向の次のデータ行（空行をスキップ）のインデックスを返す。
    fn next_data_line(idx: usize, line_starts: &LineMap, total_size: usize) -> Option<usize> {
        let mut i = idx + 1;
        while i < line_starts.len() {
            let line_start = line_starts.get(i)?;
            let line_end = if i + 1 < line_starts.len() { line_starts.get(i + 1)? } else { total_size };
            if line_end > line_start {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn value_at_cursor(&self) -> Option<u8> {
        let binding = self.document.read().expect("document read lock");
        let buffer = &binding.buffer;
        buffer.data().get(self.cursor_offset).copied()
    }

    pub fn read_bytes_at_cursor(&self, count: usize) -> Vec<u8> {
        let binding = self.document.read().expect("document read lock");
        let buffer = &binding.buffer;
        let data = buffer.data();
        if self.cursor_offset < data.len() {
            let end = std::cmp::min(self.cursor_offset + count, data.len());
            data[self.cursor_offset..end].to_vec()
        } else {
            Vec::new()
        }
    }

    pub fn set_encoding(&mut self, encoding: Encoding) {
        self.encoding = encoding;
    }

    pub fn set_radix(&mut self, radix: DisplayRadix) {
        self.radix = radix;
    }

    pub fn set_group_size(&mut self, group_size: ByteGroupSize) {
        let has_selection = self.has_selection();
        self.group_size = group_size;
        let step = group_size.byte_count();
        let total = self.total_size();
        self.cursor_offset = if self.cursor_offset >= total {
            total
        } else {
            (self.cursor_offset / step) * step
        };
        let align_boundary = |offset: usize| if offset >= total { total } else { (offset / step) * step };
        self.selection = if has_selection {
            Selection::new(align_boundary(self.selection.anchor()), align_boundary(self.selection.active()))
        } else {
            Selection::collapsed(self.cursor_offset.min(total))
        };
    }

    pub fn set_is_big_endian(&mut self, is_big_endian: bool) {
        self.is_big_endian = is_big_endian;
    }

    pub fn toggle_byte_order(&mut self) {
        self.is_big_endian = !self.is_big_endian;
    }

    /// Returns the selected half-open byte range.
    pub fn selection_range(&self) -> Option<Range<usize>> {
        let total = self.total_size();
        let selection = self.selection.clamped(total);
        let range = selection.range()?;
        (range.start < total).then_some(range.start..range.end.min(total))
    }

    /// Returns the current selection, including a collapsed selection that
    /// only stores the caret/anchor for a possible Shift-selection.
    pub fn selection(&self) -> Selection {
        self.selection.clamped(self.total_size())
    }

    /// Returns whether at least one byte is selected.
    pub fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    /// Replaces the selection with two half-open buffer boundaries.
    pub fn set_selection(&mut self, anchor: usize, active: usize) {
        let total = self.total_size();
        self.selection = Selection::new(anchor, active).clamped(total);
    }

    /// Selects `range` and places the overwrite cursor at its first byte.
    pub fn set_selection_range(&mut self, range: Range<usize>) {
        let total = self.total_size();
        let start = range.start.min(total);
        let end = range.end.min(total).max(start);
        self.selection = Selection::new(start, end);
        self.cursor_offset = start.min(total.saturating_sub(1));
    }

    /// Clears the selected bytes while preserving the current caret position.
    pub fn clear_selection(&mut self) {
        self.selection = Selection::collapsed(self.cursor_offset.min(self.total_size()));
    }

    /// Returns the insertion-boundary offset where a text-style caret should
    /// be painted for the current selection.
    ///
    /// The active boundary is the caret position while a selection is active;
    /// without a selection, the editor's current cursor position is used.
    pub fn insert_cursor_offset(&self) -> usize {
        let total = self.total_size();
        self.selection_range()
            .map_or(self.cursor_offset.min(total), |_| self.selection.active().min(total))
    }

    fn selection_right_boundary(&self) -> usize {
        let total = self.total_size();
        let Some(range) = self.selection_range() else {
            return self.cursor_offset.min(total);
        };
        range.end.min(total)
    }

    pub fn selected_range_or_cursor(&self) -> Option<Range<usize>> {
        let total = self.total_size();
        if total == 0 {
            return None;
        }
        let group_bytes = self.group_size.byte_count();
        if let Some(range) = self.selection_range() {
            let s = ((range.start / group_bytes) * group_bytes).min(total);
            let e = range.end.div_ceil(group_bytes).saturating_mul(group_bytes).min(total);
            if s < e {
                return Some(s..e);
            }
        }
        let cur = self.cursor_offset.min(total.saturating_sub(1));
        let group_start = (cur / group_bytes) * group_bytes;
        let group_end = (group_start + group_bytes).min(total);
        Some(group_start..group_end)
    }

    /// Returns the exact byte range affected by an edit.
    ///
    /// Returns the exact half-open byte range affected by an edit.
    pub fn edit_range(&self) -> Option<Range<usize>> {
        let total = self.total_size();
        if total == 0 {
            return None;
        }

        if let Some(range) = self.selection_range() {
            return Some(range);
        }

        if self.cursor_offset >= total {
            return None;
        }
        let start = self.cursor_offset.min(total.saturating_sub(1));
        Some(start..start + 1)
    }

    pub fn bookmarks_snapshot(&self) -> Vec<BookmarkItem> {
        self.bookmarks.read().expect("bookmarks read lock").clone()
    }

    pub fn parse_result(&self) -> Option<Arc<ParseResult>> {
        self.parse_result.read().expect("parse_result read lock").clone()
    }

    pub fn ksy_definition(&self) -> Option<Arc<crate::core::structure::KsyDefinition>> {
        self.ksy_definition.read().expect("ksy_definition read lock").clone()
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

        let mut bookmarks = self.bookmarks.write().expect("bookmarks write lock");

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

    pub fn add_custom_bookmark(&mut self, range: Range<usize>, color: Hsla) {
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
        let hl_color = BookmarkColor::from_hsla(color);

        let mut bookmarks = self.bookmarks.write().expect("bookmarks write lock");
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
        let mut bookmarks = self.bookmarks.write().expect("bookmarks write lock");
        if let Some(item) = bookmarks.iter_mut().find(|h| h.id == id) {
            item.comment = comment.into();
            true
        } else {
            false
        }
    }

    pub fn update_bookmark_color(&mut self, id: &str, color: BookmarkColor) -> bool {
        let mut bookmarks = self.bookmarks.write().expect("bookmarks write lock");
        if let Some(item) = bookmarks.iter_mut().find(|h| h.id == id) {
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

        let mut bookmarks = self.bookmarks.write().expect("bookmarks write lock");
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
        let mut bookmarks = self.bookmarks.write().expect("bookmarks write lock");
        let initial_len = bookmarks.len();
        bookmarks.retain(|h| h.id != id);
        bookmarks.len() < initial_len
    }

    pub fn remove_bookmark_by_index(&mut self, index: usize) -> Option<BookmarkItem> {
        let mut bookmarks = self.bookmarks.write().expect("bookmarks write lock");
        if index < bookmarks.len() { Some(bookmarks.remove(index)) } else { None }
    }

    pub fn clear_custom_bookmark(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let mut bookmarks = self.bookmarks.write().expect("bookmarks write lock");
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
        self.bookmarks.write().expect("bookmarks write lock").clear();
    }

    pub fn custom_bookmarks_for_rendering(&self) -> Vec<(Range<usize>, Hsla)> {
        self.bookmarks
            .read()
            .expect("bookmarks read lock")
            .iter()
            .map(|h| (h.range(), h.hsla_color()))
            .collect()
    }

    pub fn export_bookmarks_to_file(&self, path: &Path) -> anyhow::Result<()> {
        let doc_path = self.document.read().ok().map(|d| d.path().to_path_buf());
        BookmarkFile::save_to_path(path, &self.bookmarks.read().expect("bookmarks read lock"), doc_path.as_deref())
    }

    pub fn import_bookmarks_from_file(&mut self, path: &Path) -> anyhow::Result<usize> {
        let loaded = BookmarkFile::load_from_path(path)?;
        let count = loaded.len();
        for item in loaded {
            self.add_bookmark(item);
        }
        Ok(count)
    }

    pub fn set_cursor_offset(&mut self, offset: usize) {
        let buffer_len = self.total_size();
        let step = self.group_size.byte_count();
        self.cursor_offset = if offset >= buffer_len {
            buffer_len
        } else {
            let aligned = (offset / step) * step;
            aligned.min(buffer_len.saturating_sub(1))
        };
        self.clear_selection();
    }

    /// Sets the cursor to an exact byte without applying the current display
    /// group alignment. This is used when a user clicks an individual byte in
    /// a multi-byte display group.
    pub fn set_cursor_offset_exact(&mut self, offset: usize) {
        let buffer_len = self.total_size();
        self.cursor_offset = offset.min(buffer_len);
        self.clear_selection();
    }

    pub(crate) fn cursor_state(&self) -> CursorState {
        CursorState {
            cursor_offset: self.cursor_offset,
            selection: self.selection,
        }
    }

    pub(crate) fn restore_cursor_state(&mut self, state: CursorState) {
        let total = self.total_size();
        self.cursor_offset = state.cursor_offset.min(total);
        self.selection = state.selection.clamped(total);
    }

    pub(crate) fn adjust_after_edit(&mut self, start: usize, old_len: usize, new_len: usize) {
        let old_end = start.saturating_add(old_len);
        let shift = |offset: usize| {
            if old_len == 0 {
                if offset >= start { offset.saturating_add(new_len) } else { offset }
            } else if offset <= start {
                offset
            } else if offset >= old_end {
                if new_len >= old_len {
                    offset.saturating_add(new_len - old_len)
                } else {
                    offset.saturating_sub(old_len - new_len)
                }
            } else {
                start.saturating_add(new_len)
            }
        };

        for breaks in [&self.custom_breaks, &self.custom_joins] {
            let mut values = breaks.write().expect("custom layout write lock");
            let shifted = values.iter().copied().map(shift).collect::<BTreeSet<_>>();
            *values = shifted;
        }
        {
            let mut lines = self.empty_lines.write().expect("empty_lines write lock");
            let shifted = lines.iter().map(|(&offset, &count)| (shift(offset), count)).collect();
            *lines = shifted;
        }
        {
            let mut bookmarks = self.bookmarks.write().expect("bookmarks write lock");
            for item in bookmarks.iter_mut() {
                let item_start = item.offset;
                let item_end = item.offset.saturating_add(item.size);
                if old_len == 0 {
                    if item_start >= start {
                        item.offset = item_start.saturating_add(new_len);
                    } else if item_end > start {
                        item.size = item.size.saturating_add(new_len);
                    }
                    continue;
                }

                if item_end <= start {
                    continue;
                }
                if item_start >= old_end {
                    item.offset = shift(item_start);
                    continue;
                }

                let prefix = item_end.min(start).saturating_sub(item_start);
                let suffix = item_end.saturating_sub(old_end.max(item_start));
                item.offset = item_start.min(start);
                item.size = prefix.saturating_add(new_len).saturating_add(suffix);
            }
            bookmarks.retain(|item| item.size > 0);
            bookmarks.sort_by_key(|item| (item.offset, item.size));
        }
    }

    /// Replaces `range` with `replacement` and places the cursor at the next
    /// byte after the replacement.
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

        let before = self.cursor_state();
        let after = CursorState {
            cursor_offset: cursor_after,
            selection: Selection::collapsed(cursor_after),
        };
        self.execute_command(Box::new(ReplaceRangeCommand::new(start, old, replacement, before, after)))
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
        } else if self.cursor_offset > 0 {
            self.cursor_offset - 1..self.cursor_offset
        } else {
            return false;
        };
        // Backspace removes the byte immediately before the caret. The caret
        // should remain at the deletion start, which is one position left of
        // its original location—not one additional position to the left.
        let cursor_after = range.start;
        self.replace_range_with_cursor(range, Vec::new(), cursor_after)
    }

    fn previous_group_boundary(offset: usize, total: usize, step: usize) -> usize {
        if offset == 0 {
            return 0;
        }

        if offset >= total && total > 0 {
            ((total - 1) / step) * step
        } else {
            (offset / step).saturating_sub(1) * step
        }
    }

    fn next_group_boundary(offset: usize, total: usize, step: usize) -> usize {
        if offset >= total {
            return total;
        }

        ((offset / step) + 1).saturating_mul(step).min(total)
    }

    pub fn move_left(&mut self) {
        if let Some(range) = self.selection_range() {
            let start = range.start;
            self.cursor_offset = start.min(self.total_size());
            self.clear_selection();
            return;
        }

        let step = self.group_size.byte_count();
        let total = self.total_size();
        if self.cursor_offset > 0 {
            self.cursor_offset = Self::previous_group_boundary(self.cursor_offset, total, step);
            self.clear_selection();
        }
    }

    /// Moves right by one display group and allows the insertion caret to stop
    /// at the end-of-file boundary.
    pub fn move_right_for_insert(&mut self) {
        if self.has_selection() {
            self.cursor_offset = self.selection_right_boundary();
            self.clear_selection();
            return;
        }

        let step = self.group_size.byte_count();
        let total = self.total_size();
        if self.cursor_offset >= total {
            return;
        }

        self.cursor_offset = Self::next_group_boundary(self.cursor_offset, total, step);
        self.clear_selection();
    }

    pub fn move_right(&mut self) {
        if let Some(range) = self.selection_range() {
            self.cursor_offset = range.end.saturating_sub(1).min(self.total_size().saturating_sub(1));
            self.clear_selection();
            return;
        }

        let step = self.group_size.byte_count();
        let buffer_len = self.total_size();
        let max_offset = buffer_len.saturating_sub(1);
        let next = Self::next_group_boundary(self.cursor_offset, buffer_len, step);
        if next <= max_offset {
            self.cursor_offset = next;
            self.clear_selection();
        }
    }

    /// Calculates the target vertical offset when moving down one data row from `offset`.
    /// When `is_insert_mode` is true, clamps to line-end insertion boundaries (`0..=len`);
    /// otherwise clamps to byte cells (`0..len-1`).
    fn calculate_down_offset(&self, offset: usize, is_insert_mode: bool) -> usize {
        let step = self.group_size.byte_count();
        let total_size = self.total_size();
        let line_starts = self.line_starts();
        let current_line_idx = Self::find_line_index(offset, &line_starts);

        if let Some(next_idx) = Self::next_data_line(current_line_idx, &line_starts, total_size) {
            let current_line_start = line_starts.get(current_line_idx).expect("valid current line start");
            let offset_in_line = offset - current_line_start;
            let next_line_start = line_starts.get(next_idx).expect("valid next line start");
            let next_line_end = if next_idx + 1 < line_starts.len() {
                line_starts.get(next_idx + 1).expect("valid next line end")
            } else {
                total_size
            };
            let next_line_len = next_line_end - next_line_start;

            if is_insert_mode {
                let col = (cmp::min(offset_in_line, next_line_len) / step) * step;
                (next_line_start + col).min(next_line_end)
            } else if next_line_len > 0 {
                let target_offset = next_line_start + cmp::min(offset_in_line, next_line_len - 1);
                let aligned_offset = (target_offset / step) * step;
                aligned_offset.min(next_line_end.saturating_sub(1))
            } else {
                next_line_start
            }
        } else if is_insert_mode {
            total_size
        } else {
            let max_offset = total_size.saturating_sub(1);
            (max_offset / step) * step
        }
    }

    /// Calculates the target vertical offset when moving up one data row from `offset`.
    /// When `is_insert_mode` is true, clamps to line-end insertion boundaries (`0..=len`);
    /// otherwise clamps to byte cells (`0..len-1`).
    fn calculate_up_offset(&self, offset: usize, is_insert_mode: bool) -> usize {
        let step = self.group_size.byte_count();
        let line_starts = self.line_starts();
        let current_line_idx = Self::find_line_index(offset, &line_starts);

        if let Some(prev_idx) = Self::prev_data_line(current_line_idx, &line_starts) {
            let current_line_start = line_starts.get(current_line_idx).expect("valid current line start");
            let offset_in_line = offset - current_line_start;
            let prev_line_start = line_starts.get(prev_idx).expect("valid prev line start");
            let prev_line_end = line_starts.get(prev_idx + 1).expect("valid prev line end");
            let prev_line_len = prev_line_end - prev_line_start;

            if is_insert_mode {
                let col = (cmp::min(offset_in_line, prev_line_len) / step) * step;
                (prev_line_start + col).min(prev_line_end)
            } else {
                let target_offset = prev_line_start + cmp::min(offset_in_line, prev_line_len.saturating_sub(1));
                let aligned_offset = (target_offset / step) * step;
                aligned_offset.min(prev_line_end.saturating_sub(1))
            }
        } else {
            0
        }
    }

    pub fn move_up(&mut self) {
        if let Some(range) = self.selection_range() {
            let start = range.start;
            self.cursor_offset = start.min(self.total_size().saturating_sub(1));
            self.clear_selection();
            return;
        }

        self.cursor_offset = self.calculate_up_offset(self.cursor_offset, false);
        self.clear_selection();
    }

    pub fn move_down(&mut self) {
        if let Some(range) = self.selection_range() {
            self.cursor_offset = range.end.saturating_sub(1).min(self.total_size().saturating_sub(1));
            self.clear_selection();
            return;
        }

        self.cursor_offset = self.calculate_down_offset(self.cursor_offset, false);
        self.clear_selection();
    }

    /// Moves down while allowing a collapsed selection to end at EOF.
    pub fn move_down_for_insert(&mut self) {
        if self.has_selection() {
            self.cursor_offset = self.selection_right_boundary();
            self.clear_selection();
            return;
        }

        if self.cursor_offset >= self.total_size() {
            return;
        }

        self.move_down();
    }

    pub fn select_left(&mut self) {
        let step = self.group_size.byte_count();
        if self.cursor_offset > 0 {
            let target = (self.cursor_offset / step).saturating_sub(1) * step;
            let anchor = if self.has_selection() { self.selection.anchor() } else { self.cursor_offset };
            self.cursor_offset = target;
            self.selection = Selection::new(anchor, target).clamped(self.total_size());
        }
    }

    /// Extends or contracts the Insert Mode selection by one display group to
    /// the left.
    pub fn select_left_for_insert(&mut self) {
        let total = self.total_size();
        let step = self.group_size.byte_count();
        let caret = if self.has_selection() {
            self.selection.active()
        } else {
            self.cursor_offset.min(total)
        };
        if caret == 0 {
            return;
        }

        let active = Self::previous_group_boundary(caret, total, step);
        let anchor = if self.has_selection() { self.selection.anchor() } else { caret };
        self.selection = Selection::new(anchor, active);
        self.cursor_offset = active;
    }

    pub fn select_right(&mut self) {
        let step = self.group_size.byte_count();
        let buffer_len = self.total_size();
        let next = ((self.cursor_offset / step).saturating_add(1)).saturating_mul(step).min(buffer_len);
        if self.cursor_offset < next {
            let anchor = if self.has_selection() { self.selection.anchor() } else { self.cursor_offset };
            self.cursor_offset = next;
            self.selection = Selection::new(anchor, next);
        }
    }

    /// Extends or contracts the Insert Mode selection by one display group to
    /// the right.
    pub fn select_right_for_insert(&mut self) {
        let total = self.total_size();
        let step = self.group_size.byte_count();
        let caret = if self.has_selection() {
            self.selection.active()
        } else {
            self.cursor_offset.min(total)
        };
        if caret >= total {
            return;
        }

        let active = Self::next_group_boundary(caret, total, step);
        let anchor = if self.has_selection() { self.selection.anchor() } else { caret };
        self.selection = Selection::new(anchor, active);
        self.cursor_offset = active;
    }

    /// Extends or contracts the Insert Mode selection upward by one data row.
    pub fn select_up_for_insert(&mut self) {
        let total_size = self.total_size();
        let caret = if self.has_selection() {
            self.selection.active().min(total_size)
        } else {
            self.cursor_offset.min(total_size)
        };
        let anchor = if self.has_selection() { self.selection.anchor() } else { caret };

        let active = self.calculate_up_offset(caret, true);
        self.cursor_offset = active;
        self.selection = Selection::new(anchor, active);
    }

    pub fn select_up(&mut self) {
        let anchor = if self.has_selection() { self.selection.anchor() } else { self.cursor_offset };

        self.cursor_offset = self.calculate_up_offset(self.cursor_offset, false);
        self.selection = Selection::new(anchor, self.cursor_offset);
    }

    /// Extends or contracts the Insert Mode selection downward by one data row.
    pub fn select_down_for_insert(&mut self) {
        let total_size = self.total_size();
        let caret = if self.has_selection() {
            self.selection.active().min(total_size)
        } else {
            self.cursor_offset.min(total_size)
        };
        let anchor = if self.has_selection() { self.selection.anchor() } else { caret };

        let active = self.calculate_down_offset(caret, true);
        self.cursor_offset = active;
        self.selection = Selection::new(anchor, active);
    }

    pub fn select_down(&mut self) {
        let total_size = self.total_size();
        let anchor = if self.has_selection() { self.selection.anchor() } else { self.cursor_offset };
        self.cursor_offset = self.calculate_down_offset(self.cursor_offset, false);
        self.selection = Selection::new(anchor, self.cursor_offset.min(total_size));
    }

    pub fn select_all(&mut self) {
        let buffer_len = self.total_size();
        self.selection = Selection::new(0, buffer_len);
        self.cursor_offset = buffer_len;
    }

    pub fn go_to_beginning(&mut self) {
        self.cursor_offset = 0;
        self.clear_selection();
    }

    pub fn go_to_end(&mut self) {
        let step = self.group_size.byte_count();
        let max_offset = self.total_size().saturating_sub(1);
        self.cursor_offset = (max_offset / step) * step;
        self.clear_selection();
    }

    pub fn page_up(&mut self, visible_rows: usize) {
        let step = self.group_size.byte_count();
        let line_starts = self.line_starts();
        let current_line_idx = Self::find_line_index(self.cursor_offset, &line_starts);

        let target_line_idx = current_line_idx.saturating_sub(visible_rows);
        let current_line_start = line_starts.get(current_line_idx).expect("valid current line start");
        let offset_in_line = self.cursor_offset - current_line_start;

        let target_line_start = line_starts.get(target_line_idx).expect("valid target line start");
        let target_line_end = if target_line_idx + 1 < line_starts.len() {
            line_starts.get(target_line_idx + 1).expect("valid target line end")
        } else {
            self.total_size()
        };
        let target_line_len = target_line_end - target_line_start;

        let target_offset = target_line_start + cmp::min(offset_in_line, target_line_len.saturating_sub(1));
        let aligned_offset = (target_offset / step) * step;
        self.cursor_offset = aligned_offset.min(target_line_end.saturating_sub(1));
        self.clear_selection();
    }

    pub fn page_down(&mut self, visible_rows: usize) {
        let step = self.group_size.byte_count();
        let line_starts = self.line_starts();
        let current_line_idx = Self::find_line_index(self.cursor_offset, &line_starts);

        let target_line_idx = cmp::min(current_line_idx + visible_rows, line_starts.len() - 1);
        let current_line_start = line_starts.get(current_line_idx).expect("valid current line start");
        let offset_in_line = self.cursor_offset - current_line_start;

        let target_line_start = line_starts.get(target_line_idx).expect("valid target line start");
        let target_line_end = if target_line_idx + 1 < line_starts.len() {
            line_starts.get(target_line_idx + 1).expect("valid target line end")
        } else {
            self.total_size()
        };
        let target_line_len = target_line_end - target_line_start;

        if target_line_idx == line_starts.len() - 1 && target_line_len == 0 {
            let max_offset = self.total_size().saturating_sub(1);
            self.cursor_offset = (max_offset / step) * step;
        } else {
            let target_offset = target_line_start + cmp::min(offset_in_line, target_line_len.saturating_sub(1));
            let aligned_offset = (target_offset / step) * step;
            self.cursor_offset = aligned_offset.min(target_line_end.saturating_sub(1));
        }
        self.clear_selection();
    }

    pub fn home(&mut self) {
        self.cursor_offset = 0;
        self.clear_selection();
    }

    pub fn end(&mut self) {
        let step = self.group_size.byte_count();
        let buffer_len = self.total_size();
        let max_offset = buffer_len.saturating_sub(1);
        self.cursor_offset = (max_offset / step) * step;
        self.clear_selection();
    }

    pub fn select_page_up(&mut self, visible_rows: usize) {
        let step = self.group_size.byte_count();
        let line_starts = self.line_starts();
        let current_line_idx = Self::find_line_index(self.cursor_offset, &line_starts);
        let anchor = if self.has_selection() { self.selection.anchor() } else { self.cursor_offset };

        let target_line_idx = current_line_idx.saturating_sub(visible_rows);
        let current_line_start = line_starts.get(current_line_idx).expect("valid current line start");
        let offset_in_line = self.cursor_offset - current_line_start;

        let target_line_start = line_starts.get(target_line_idx).expect("valid target line start");
        let target_line_end = if target_line_idx + 1 < line_starts.len() {
            line_starts.get(target_line_idx + 1).expect("valid target line end")
        } else {
            self.total_size()
        };
        let target_line_len = target_line_end - target_line_start;

        let target_offset = target_line_start + cmp::min(offset_in_line, target_line_len.saturating_sub(1));
        let aligned_offset = (target_offset / step) * step;
        self.cursor_offset = aligned_offset.min(target_line_end.saturating_sub(1));
        self.selection = Selection::new(anchor, self.cursor_offset);
    }

    pub fn select_page_down(&mut self, visible_rows: usize) {
        let step = self.group_size.byte_count();
        let line_starts = self.line_starts();
        let current_line_idx = Self::find_line_index(self.cursor_offset, &line_starts);
        let anchor = if self.has_selection() { self.selection.anchor() } else { self.cursor_offset };

        let target_line_idx = cmp::min(current_line_idx + visible_rows, line_starts.len() - 1);
        let current_line_start = line_starts.get(current_line_idx).expect("valid current line start");
        let offset_in_line = self.cursor_offset - current_line_start;

        let target_line_start = line_starts.get(target_line_idx).expect("valid target line start");
        let target_line_end = if target_line_idx + 1 < line_starts.len() {
            line_starts.get(target_line_idx + 1).expect("valid target line end")
        } else {
            self.total_size()
        };
        let target_line_len = target_line_end - target_line_start;

        if target_line_idx == line_starts.len() - 1 && target_line_len == 0 {
            let max_offset = self.total_size().saturating_sub(1);
            self.cursor_offset = (max_offset / step) * step;
        } else {
            let target_offset = target_line_start + cmp::min(offset_in_line, target_line_len.saturating_sub(1));
            let aligned_offset = (target_offset / step) * step;
            self.cursor_offset = aligned_offset.min(target_line_end.saturating_sub(1));
        }
        self.selection = Selection::new(anchor, self.cursor_offset.min(self.total_size()));
    }

    /// Extends the selection to the beginning of the buffer for Insert Mode.
    pub fn select_home_for_insert(&mut self) {
        let total_size = self.total_size();
        let anchor = if self.has_selection() {
            self.selection.anchor()
        } else {
            self.cursor_offset.min(total_size)
        };
        self.cursor_offset = 0;
        self.selection = Selection::new(anchor, 0);
    }

    pub fn select_home(&mut self) {
        let anchor = if self.has_selection() { self.selection.anchor() } else { self.cursor_offset };
        self.cursor_offset = 0;
        self.selection = Selection::new(anchor, 0);
    }

    pub fn select_end(&mut self) {
        let step = self.group_size.byte_count();
        let buffer_len = self.total_size();
        let anchor = if self.has_selection() { self.selection.anchor() } else { self.cursor_offset };
        let max_offset = buffer_len.saturating_sub(1);
        self.cursor_offset = (max_offset / step) * step;
        self.selection = Selection::new(anchor, self.cursor_offset.saturating_add(1).min(buffer_len));
    }

    /// Extends the selection to the EOF insertion boundary.
    pub fn select_end_for_insert(&mut self) {
        let buffer_len = self.total_size();
        let anchor = if self.has_selection() {
            self.selection.anchor()
        } else {
            self.cursor_offset.min(buffer_len)
        };
        self.cursor_offset = buffer_len;
        self.selection = Selection::new(anchor, buffer_len);
    }

    pub fn start_drag(&mut self, byte_pos: usize) {
        let step = self.group_size.byte_count();
        let aligned = (byte_pos / step) * step;
        self.cursor_offset = aligned;
        self.selection = Selection::collapsed(aligned);
    }

    pub fn continue_drag(&mut self, anchor_pos: usize, byte_pos: usize) {
        let step = self.group_size.byte_count();
        let total = self.total_size();
        let aligned_anchor = (anchor_pos / step) * step;
        let cursor_offset = if byte_pos >= total { total } else { (byte_pos / step) * step };
        self.cursor_offset = cursor_offset;

        let (anchor, active) = if cursor_offset >= aligned_anchor {
            (aligned_anchor.min(total), cursor_offset.saturating_add(step).min(total))
        } else {
            (aligned_anchor.saturating_add(step).min(total), cursor_offset.min(total))
        };
        self.selection = Selection::new(anchor, active);
    }

    pub fn search_pattern(&self) -> Option<Vec<crate::core::search::PatternByte>> {
        if self.search_state.query.is_empty() {
            return None;
        }
        match self.search_state.mode {
            crate::core::search::SearchMode::Text => Some(
                self.search_state
                    .query
                    .as_bytes()
                    .iter()
                    .map(|&b| crate::core::search::PatternByte::new_exact(b))
                    .collect(),
            ),
            crate::core::search::SearchMode::Hex => crate::core::search::parse_hex_pattern(&self.search_state.query),
        }
    }

    pub fn set_search_query(&mut self, query: String) {
        if self.search_state.query != query {
            self.search_state.query = query;
            self.search_state.results.clear();
            self.search_state.current_result_index = None;
            self.search_state.is_full_search_complete = false;
            self.search_state.generation += 1;
        }
    }

    pub fn set_search_query_and_mode(&mut self, query: String, mode: crate::core::search::SearchMode) {
        if self.search_state.query != query || self.search_state.mode != mode {
            self.search_state.query = query;
            self.search_state.mode = mode;
            self.search_state.results.clear();
            self.search_state.current_result_index = None;
            self.search_state.is_full_search_complete = false;
            self.search_state.generation += 1;
        }
    }

    pub fn set_search_results(&mut self, results: Vec<usize>, generation: usize, is_full: bool) {
        if generation < self.search_state.generation {
            return;
        }
        if generation > self.search_state.generation {
            self.search_state.generation = generation;
        }
        if self.search_state.is_full_search_complete && !is_full {
            return;
        }
        self.search_state.results = results;
        if is_full {
            self.search_state.is_full_search_complete = true;
        }
        if !self.search_state.results.is_empty() && self.search_state.current_result_index.is_none() {
            self.search_state.current_result_index = Some(0);
        }
    }

    pub fn clear_search(&mut self) {
        self.search_state.query.clear();
        self.search_state.results.clear();
        self.search_state.current_result_index = None;
        self.search_state.is_full_search_complete = false;
        self.search_state.generation += 1;
    }

    /// Finds and navigates to the next occurrence starting after the current cursor offset,
    /// wrapping around if necessary. Updates cursor offset and returns the match offset.
    pub fn find_and_navigate_next(&mut self) -> Option<usize> {
        let pattern = self.search_pattern()?;
        let doc = self.document.read().ok()?;
        let data = doc.buffer.data();
        let from_offset = self.cursor_offset.saturating_add(1);
        let next_offset = crate::core::search::find_next_occurrence(data, &pattern, from_offset)?;
        self.cursor_offset = next_offset;
        Some(next_offset)
    }

    /// Finds and navigates to the previous occurrence starting before the current cursor offset,
    /// wrapping around if necessary. Updates cursor offset and returns the match offset.
    pub fn find_and_navigate_prev(&mut self) -> Option<usize> {
        let pattern = self.search_pattern()?;
        let doc = self.document.read().ok()?;
        let data = doc.buffer.data();
        let prev_offset = crate::core::search::find_prev_occurrence(data, &pattern, self.cursor_offset)?;
        self.cursor_offset = prev_offset;
        Some(prev_offset)
    }

    pub fn next_search_result(&mut self) -> Option<usize> {
        if !self.search_state.results.is_empty() {
            let next_index = if let Some(index) = self.search_state.current_result_index {
                (index + 1) % self.search_state.results.len()
            } else {
                0
            };
            self.search_state.current_result_index = Some(next_index);
            let offset = self.search_state.results[next_index];
            self.cursor_offset = offset;
            Some(offset)
        } else {
            self.find_and_navigate_next()
        }
    }

    pub fn line_starts(&self) -> LineMap {
        if let Some(cached) = self.cached_line_map.borrow().as_ref() {
            return cached.clone();
        }

        // The parser prepares the default expanded structure layout before it
        // publishes the 100% result. Reuse it directly on the UI thread; the
        // dynamic builder below remains the compatibility path for custom
        // joins/breaks and collapsed structures.
        if self.show_inline_structure_view
            && !self.is_parsing_structure
            && self.collapsed_struct_ids.is_empty()
            && self.custom_breaks.read().expect("custom_breaks read lock").is_empty()
            && self.custom_joins.read().expect("custom_joins read lock").is_empty()
            && self.empty_lines.read().expect("empty_lines read lock").is_empty()
            && let Some(parse_res) = self.parse_result()
            && let Some(line_map) = &parse_res.structure_line_map
        {
            let map = (**line_map).clone();
            *self.cached_line_map.borrow_mut() = Some(map.clone());
            return map;
        }

        let map = if !self.has_custom_layout() {
            LineMap::Standard { total_size: self.total_size() }
        } else {
            let total_size = self.total_size();
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

                let custom_breaks_guard = self.custom_breaks.read().expect("custom_breaks read lock");
                let custom_joins_guard = self.custom_joins.read().expect("custom_joins read lock");
                let empty_lines_guard = self.empty_lines.read().expect("empty_lines read lock");
                let mut empty_line_counts = empty_lines_guard.clone();

                let mut layout_events: Vec<usize> = Vec::new();
                layout_events.extend(custom_breaks_guard.iter().copied());
                layout_events.extend(custom_joins_guard.iter().copied());
                layout_events.extend(empty_line_counts.keys().copied());
                if self.show_inline_structure_view
                    && !self.is_parsing_structure
                    && let Some(parse_res) = self.parse_result()
                {
                    parse_res.collect_field_breaks(&mut layout_events, &self.collapsed_struct_ids);
                    parse_res.collect_structure_header_lines(&mut empty_line_counts, &self.collapsed_struct_ids);
                }
                layout_events.extend(empty_line_counts.keys().copied());
                layout_events.sort_unstable();
                layout_events.dedup();

                let mut break_events: Vec<usize> = Vec::new();
                break_events.extend(custom_breaks_guard.iter().copied());
                break_events.extend(empty_line_counts.keys().copied());
                if self.show_inline_structure_view
                    && !self.is_parsing_structure
                    && let Some(parse_res) = self.parse_result()
                {
                    parse_res.collect_field_breaks(&mut break_events, &self.collapsed_struct_ids);
                }
                break_events.sort_unstable();
                break_events.dedup();

                let mut event_idx = 0;
                let mut break_idx = 0;

                while current < total_size {
                    // Find next event > current
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
                            // We can fit one or more standard lines of BYTES_PER_ROW
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
                            // No more events, and we have at least one full standard line remaining
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
                    // We must generate a Custom segment using localized layout logic.
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

                        // Process empty lines at current
                        if let Some(&count) = empty_line_counts.get(&current) {
                            for _ in 0..count {
                                starts.push(current);
                            }
                        }

                        starts.push(current);

                        // Find next event break after current (includes structure field breaks, custom breaks, etc.) in O(1) amortized
                        while break_idx < break_events.len() && break_events[break_idx] <= current {
                            break_idx += 1;
                        }
                        let next_event_break = break_events.get(break_idx).copied();

                        // Advance in BYTES_PER_ROW increments, skipping joined boundaries
                        let mut next_pos = current + BYTES_PER_ROW;
                        while custom_joins_guard.contains(&next_pos) && next_pos < total_size {
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

            // Quick final pass to compute max_bytes_per_row and total_lines
            let mut max_bytes_per_row = BYTES_PER_ROW;
            let mut total_lines = 0;
            for i in 0..segments.len() {
                let seg = &segments[i];
                total_lines += seg.line_count;
                match &seg.kind {
                    SegmentKind::Standard => {
                        if i + 1 == segments.len() {
                            let last_line_start = seg.start_offset + (seg.line_count - 1) * BYTES_PER_ROW;
                            let last_line_len = total_size - last_line_start;
                            max_bytes_per_row = max_bytes_per_row.max(last_line_len);
                        }
                    }
                    SegmentKind::Custom { starts } => {
                        let next_start_offset = if i + 1 < segments.len() { segments[i + 1].start_offset } else { total_size };
                        for j in 0..seg.line_count {
                            let end = if j + 1 < seg.line_count { starts[j + 1] } else { next_start_offset };
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
        };

        *self.cached_line_map.borrow_mut() = Some(map.clone());
        map
    }

    pub fn add_custom_break(&mut self, offset: usize) {
        if offset < self.total_size() {
            // カスタム改行を追加する前に、現在の行レイアウトを取得する。
            // 追加後は同じオフセットを含む「結合済みメガ行」が分割されるため、
            // その行に属していた custom_joins は到達不能になり無効化される。
            let line_starts = self.line_starts();
            let current_line_idx = Self::find_line_index(offset, &line_starts);
            let line_start = line_starts.get(current_line_idx).unwrap_or(0);
            let line_end = if current_line_idx + 1 < line_starts.len() {
                line_starts.get(current_line_idx + 1).unwrap_or(self.total_size())
            } else {
                self.total_size()
            };
            let line_length = line_end.saturating_sub(line_start);

            {
                let mut custom_breaks = self.custom_breaks.write().expect("custom_breaks write lock");
                let mut custom_joins = self.custom_joins.write().expect("custom_joins write lock");

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
                    if line_end < self.total_size() && !(line_end - offset).is_multiple_of(BYTES_PER_ROW) && !custom_breaks.contains(&line_end) {
                        custom_breaks.insert(line_end);
                    }
                }
            }

            self.cached_line_map.replace(None);
        }
    }

    pub fn remove_custom_break(&mut self, offset: usize) {
        if self.custom_breaks.write().expect("custom_breaks write lock").remove(&offset) {
            self.cached_line_map.replace(None);
        }
    }

    pub fn toggle_custom_break(&mut self, offset: usize) {
        let contains = self.custom_breaks.read().expect("custom_breaks read lock").contains(&offset);
        if contains {
            self.remove_custom_break(offset);
        } else {
            self.add_custom_break(offset);
        }
    }

    pub fn add_empty_line(&mut self, offset: usize) {
        if offset <= self.total_size() {
            *self.empty_lines.write().expect("empty_lines write lock").entry(offset).or_insert(0) += 1;
            self.cached_line_map.replace(None);
        }
    }

    pub fn remove_empty_line(&mut self, offset: usize) -> bool {
        let mut empty_lines = self.empty_lines.write().expect("empty_lines write lock");
        if let Some(count) = empty_lines.get_mut(&offset) {
            if *count > 1 {
                *count -= 1;
            } else {
                empty_lines.remove(&offset);
            }
            drop(empty_lines);
            self.cached_line_map.replace(None);
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
        let current_line_idx = Self::find_line_index(self.cursor_offset, &line_starts);

        // 次の行がなければ何もしない
        if current_line_idx + 1 >= line_starts.len() {
            return;
        }

        let next_line_start = line_starts.get(current_line_idx + 1).expect("valid next line start");

        let mut custom_breaks = self.custom_breaks.write().expect("custom_breaks write lock");
        let mut custom_joins = self.custom_joins.write().expect("custom_joins write lock");

        if custom_breaks.contains(&next_line_start) {
            // Custom Break による改行なら、その break を削除
            custom_breaks.remove(&next_line_start);
            drop(custom_breaks);
            drop(custom_joins);
            self.cached_line_map.replace(None);
        } else if next_line_start != line_starts.get(current_line_idx).unwrap_or(0) {
            // 自然境界（16バイト境界 or カスタム改行後の次行など）を join として記録
            // next_line_start が現在行と同オフセット（空行の重複）でない場合のみ
            custom_joins.insert(next_line_start);
            drop(custom_breaks);
            drop(custom_joins);
            self.cached_line_map.replace(None);
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

        let mut custom_breaks = self.custom_breaks.write().expect("custom_breaks write lock");
        let mut custom_joins = self.custom_joins.write().expect("custom_joins write lock");
        let mut empty_lines = self.empty_lines.write().expect("empty_lines write lock");

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

        drop(custom_breaks);
        drop(custom_joins);
        drop(empty_lines);
        self.cached_line_map.replace(None);
    }

    /// 全ての Custom Break と Join をクリアし、デフォルトの16バイト表示に戻す。
    pub fn clear_all_custom_breaks(&mut self) {
        self.custom_breaks.write().expect("custom_breaks write lock").clear();
        self.custom_joins.write().expect("custom_joins write lock").clear();
        self.empty_lines.write().expect("empty_lines write lock").clear();
        self.cached_line_map.replace(None);
    }

    pub fn has_custom_layout(&self) -> bool {
        !self.custom_breaks.read().expect("custom_breaks read lock").is_empty()
            || !self.custom_joins.read().expect("custom_joins read lock").is_empty()
            || !self.empty_lines.read().expect("empty_lines read lock").is_empty()
            || (self.show_inline_structure_view && !self.is_parsing_structure && self.parse_result.read().expect("parse_result read lock").is_some())
    }

    pub fn toggle_struct_collapsed(&mut self, struct_id: &str) {
        if self.collapsed_struct_ids.contains(struct_id) {
            self.collapsed_struct_ids.remove(struct_id);
        } else {
            self.collapsed_struct_ids.insert(struct_id.to_string());
        }
        self.cached_line_map.replace(None);
    }

    pub fn toggle_inline_structure_view(&mut self) {
        self.show_inline_structure_view = !self.show_inline_structure_view;
        self.cached_line_map.replace(None);
    }

    pub fn custom_layout_count(&self) -> usize {
        self.custom_breaks.read().expect("custom_breaks read lock").len()
            + self.custom_joins.read().expect("custom_joins read lock").len()
            + self.empty_lines.read().expect("empty_lines read lock").values().sum::<usize>()
    }

    pub fn prev_search_result(&mut self) -> Option<usize> {
        if !self.search_state.results.is_empty() {
            let prev_index = if let Some(index) = self.search_state.current_result_index {
                if index == 0 { self.search_state.results.len() - 1 } else { index - 1 }
            } else {
                self.search_state.results.len() - 1
            };

            self.search_state.current_result_index = Some(prev_index);
            let offset = self.search_state.results[prev_index];
            self.cursor_offset = offset;
            Some(offset)
        } else {
            self.find_and_navigate_prev()
        }
    }

    pub fn current_search_result(&self) -> Option<usize> {
        if let Some(i) = self.search_state.current_result_index {
            self.search_state.results.get(i).copied()
        } else {
            None
        }
    }

    pub fn execute_command(&mut self, mut command: Box<dyn Command>) -> bool {
        if self.is_read_only() {
            return false;
        }
        command.execute(self);
        if command.is_noop() {
            return false;
        }
        self.document.write().expect("document write lock").history.push(command);
        self.document_changed();
        true
    }

    pub fn undo(&mut self) -> bool {
        if self.is_read_only() {
            return false;
        }

        // Need to acquire a write lock on the document to access history
        // And also we need to pop from history, then call command.undo(self)
        // command.undo might need to access document.buf, which is in the same lock if we are not careful
        // The current History implementation stores Box<dyn Command>, which is fine.
        // But if I hold the lock while calling command.undo(self), and command.undo tries to lock document again... deadlock.

        let command = {
            let mut doc = self.document.write().expect("document write lock");
            doc.history.pop_undo()
        };

        if let Some(mut cmd) = command {
            cmd.undo(self);

            // Re-acquire lock to push redo
            self.document.write().expect("document write lock").history.push_redo(cmd);
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

        let command = {
            let mut doc = self.document.write().expect("document write lock");
            doc.history.pop_redo()
        };

        if let Some(mut cmd) = command {
            cmd.execute(self);

            if cmd.is_noop() {
                return false;
            }

            // Re-acquire lock to push undo
            self.document.write().expect("document write lock").history.push_undo(cmd);
            self.document_changed();
            true
        } else {
            false
        }
    }

    /// Returns whether this editor has an undoable command.
    pub fn can_undo(&self) -> bool {
        self.document.read().expect("document read lock").history.can_undo()
    }

    /// Returns whether this editor has a redoable command.
    pub fn can_redo(&self) -> bool {
        self.document.read().expect("document read lock").history.can_redo()
    }

    pub fn set_kaitai_definition(&mut self, ksy: Arc<crate::core::structure::KsyDefinition>) {
        self.cancel_structure_parsing();
        self.structure_parse_async = false;
        self.structure_reparse_requested = false;
        *self.ksy_definition.write().expect("ksy_definition write lock") = Some(ksy);
        self.reparse_structure();
    }

    pub fn set_parse_result(&mut self, result: ParseResult) {
        self.parse_progress_offset = result.total_parsed_bytes;
        self.is_finalizing_structure = false;
        *self.parse_result.write().expect("parse_result write lock") = Some(Arc::new(result));
        self.cached_line_map.replace(None);
    }

    /// Starts a new partial structure result that can receive parse batches.
    pub fn begin_partial_parse_result(&mut self, definition_id: String) {
        let old = self
            .parse_result
            .write()
            .expect("parse_result write lock")
            .replace(Arc::new(ParseResult::empty(definition_id)));
        if let Some(old_res) = old {
            std::thread::spawn(move || drop(old_res));
        }
        self.is_finalizing_structure = false;
        self.cached_line_map.replace(None);
    }

    /// Appends a batch of parsed root fields without cloning earlier batches.
    pub fn append_parse_fields(&mut self, definition_id: String, fields: Vec<ParsedField>, offset: usize, total_size: usize) {
        let chunk: Arc<[ParsedField]> = Arc::from(fields.into_boxed_slice());
        self.append_parse_chunks(definition_id, vec![chunk], offset, total_size);
    }

    /// Appends shared parse chunks without cloning their fields.
    pub fn append_parse_chunks(&mut self, definition_id: String, chunks: Vec<Arc<[ParsedField]>>, offset: usize, total_size: usize) {
        self.parse_progress_offset = offset;
        self.parse_total_size = total_size;
        if chunks.iter().all(|chunk| chunk.is_empty()) {
            return;
        }

        let next = if let Some(current) = self.parse_result() {
            Arc::new(current.append_shared_chunks_without_index(&chunks, offset))
        } else {
            Arc::new(ParseResult::empty(definition_id).append_shared_chunks_without_index(&chunks, offset))
        };
        *self.parse_result.write().expect("parse_result write lock") = Some(next);
    }

    pub fn set_parse_result_arc(&mut self, result: Arc<ParseResult>) {
        self.parse_progress_offset = result.total_parsed_bytes;
        self.is_finalizing_structure = false;
        let old = self.parse_result.write().expect("parse_result write lock").replace(result);
        if let Some(old_res) = old {
            std::thread::spawn(move || drop(old_res));
        }
        self.cached_line_map.replace(None);
    }

    pub fn update_parse_progress(&mut self, offset: usize, total_size: usize, intermediate_result: Option<ParseResult>) {
        self.parse_progress_offset = offset;
        self.parse_total_size = total_size;
        if let Some(res) = intermediate_result {
            *self.parse_result.write().expect("parse_result write lock") = Some(Arc::new(res));
            self.cached_line_map.replace(None);
        }
    }

    pub fn invalidate_line_map(&self) {
        self.cached_line_map.replace(None);
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
        self.structure_reparse_requested
            .then(|| self.ksy_definition().map(|ksy| (ksy, self.parse_generation)))
            .flatten()
    }

    /// Takes a deferred edit request if it still belongs to `generation`.
    pub fn take_structure_reparse_request(&mut self, generation: usize) -> Option<Arc<crate::core::structure::KsyDefinition>> {
        if !self.structure_reparse_requested || self.parse_generation != generation {
            return None;
        }

        self.structure_reparse_requested = false;
        self.ksy_definition()
    }

    fn document_changed(&mut self) {
        self.cached_line_map.replace(None);
        self.search_state.results.clear();
        self.search_state.current_result_index = None;
        self.search_state.is_full_search_complete = false;
        self.search_state.generation = self.search_state.generation.wrapping_add(1);

        if self.structure_parse_async && self.ksy_definition().is_some() {
            self.cancel_structure_parsing();
            self.structure_reparse_requested = true;
            self.is_parsing_structure = true;
            self.is_finalizing_structure = false;
            self.parse_progress_offset = 0;
            self.parse_total_size = self.total_size();

            if let Some(ksy) = self.ksy_definition() {
                self.begin_partial_parse_result(ksy.meta.id.clone());
            }
        } else {
            self.reparse_structure();
        }
    }

    pub fn cancel_structure_parsing(&mut self) {
        if let Some(token) = self.parse_cancel_token.take() {
            token.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        // Invalidate callbacks that are already queued in the parser's
        // mailbox, and cancel a debounce request that has not started yet.
        self.parse_generation = self.parse_generation.wrapping_add(1);
        self.structure_reparse_requested = false;
        self.is_parsing_structure = false;
        self.is_finalizing_structure = false;
    }

    pub fn clear_structure_definition(&mut self) {
        self.cancel_structure_parsing();
        self.structure_parse_async = false;
        self.structure_reparse_requested = false;
        let old_definition = self.ksy_definition.write().expect("ksy_definition write lock").take();
        let old = self.parse_result.write().expect("parse_result write lock").take();
        if old_definition.is_some() || old.is_some() {
            std::thread::spawn(move || {
                drop((old_definition, old));
            });
        }
        self.is_parsing_structure = false;
        self.is_finalizing_structure = false;
        self.parse_progress_offset = 0;
        self.parse_total_size = 0;
        self.cached_line_map.replace(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::command::InsertCharCommand;
    use std::sync::Arc;

    fn create_editor_with_content(content: &[u8]) -> Editor {
        let buffer = crate::core::buffer::Buffer::new(content.to_vec());
        let document = Arc::new(RwLock::new(Document::new(std::path::PathBuf::from("test"), buffer)));
        Editor::new(document)
    }

    #[test]
    fn test_initialization() {
        let editor = create_editor_with_content(b"Hello");
        assert_eq!(editor.total_size(), 5);
        assert_eq!(editor.cursor_offset, 0);
        assert!(!editor.has_selection());
    }

    #[test]
    fn test_cursor_movement() {
        let mut editor = create_editor_with_content(b"123");

        // Move right
        editor.move_right();
        assert_eq!(editor.cursor_offset, 1);

        // Move left
        editor.move_left();
        assert_eq!(editor.cursor_offset, 0);

        // Boundary checks
        editor.move_left();
        assert_eq!(editor.cursor_offset, 0);

        editor.end();
        assert_eq!(editor.cursor_offset, 2);
        editor.move_right();
        assert_eq!(editor.cursor_offset, 2);

        editor.go_to_beginning();
        assert_eq!(editor.cursor_offset, 0);
        editor.go_to_end();
        assert_eq!(editor.cursor_offset, 2);
    }

    #[test]
    fn test_selection() {
        let mut editor = create_editor_with_content(b"12345");

        // Select Right
        editor.select_right();
        assert_eq!(editor.selection(), Selection::new(0, 1));
        assert_eq!(editor.cursor_offset, 1);
        assert_eq!(editor.insert_cursor_offset(), 1);
        assert_eq!(editor.selected_range_or_cursor(), Some(0..1));
        assert_eq!(editor.edit_range(), Some(0..1));

        // Clear selection on move
        editor.move_right();
        assert!(!editor.has_selection());

        // Select All
        editor.select_all();
        assert_eq!(editor.selection(), Selection::new(0, 5));
        assert_eq!(editor.insert_cursor_offset(), 5);
        assert_eq!(editor.selected_range_or_cursor(), Some(0..5));

        editor.set_cursor_offset_exact(4);
        editor.select_end_for_insert();
        assert_eq!(editor.cursor_offset, 5);
        assert_eq!(editor.selection(), Selection::new(4, 5));
    }

    #[test]
    fn test_overwrite_selection_select_down_and_up() {
        let mut editor = create_editor_with_content(&[0u8; 48]);
        assert_eq!(editor.cursor_offset, 0);

        // Shift+Down from offset 0 selects exactly 16 bytes (one row) to cursor offset 16
        editor.select_down();
        assert_eq!(editor.cursor_offset, 16);
        assert_eq!(editor.selection(), Selection::new(0, 16));
        assert_eq!(editor.edit_range(), Some(0..16));

        // Shift+Down again selects 32 bytes (two rows) to cursor offset 32
        editor.select_down();
        assert_eq!(editor.cursor_offset, 32);
        assert_eq!(editor.selection(), Selection::new(0, 32));
        assert_eq!(editor.edit_range(), Some(0..32));

        // Shift+Up shrinks the selection back to 16 bytes
        editor.select_up();
        assert_eq!(editor.cursor_offset, 16);
        assert_eq!(editor.selection(), Selection::new(0, 16));
        assert_eq!(editor.edit_range(), Some(0..16));

        // Shift+Up collapses the selection
        editor.select_up();
        assert_eq!(editor.cursor_offset, 0);
        assert_eq!(editor.selection(), Selection::new(0, 0));
        assert_eq!(editor.edit_range(), Some(0..1));
    }

    #[test]
    fn test_overwrite_selection_select_left_and_right() {
        let mut editor = create_editor_with_content(&[0u8; 10]);
        editor.set_cursor_offset_exact(5);

        // Shift+Left from offset 5 selects 1 byte (4..5) with cursor at 4
        editor.select_left();
        assert_eq!(editor.cursor_offset, 4);
        assert_eq!(editor.selection(), Selection::new(5, 4));
        assert_eq!(editor.edit_range(), Some(4..5));

        // Shift+Right shrinks back
        editor.select_right();
        assert_eq!(editor.cursor_offset, 5);
        assert_eq!(editor.selection(), Selection::new(5, 5));
    }

    #[test]
    fn test_insert_selection_moves_by_one_display_group_and_collapses_at_anchor() {
        let mut editor = create_editor_with_content(b"12345");
        editor.set_cursor_offset_exact(2);

        editor.select_left_for_insert();
        assert_eq!(editor.edit_range(), Some(1..2));
        assert_eq!(editor.selection(), Selection::new(2, 1));
        assert_eq!(editor.cursor_offset, 1);
        assert_eq!(editor.insert_cursor_offset(), 1);

        editor.select_left_for_insert();
        assert_eq!(editor.edit_range(), Some(0..2));
        assert_eq!(editor.selection(), Selection::new(2, 0));
        assert_eq!(editor.cursor_offset, 0);

        editor.select_right_for_insert();
        assert_eq!(editor.edit_range(), Some(1..2));
        assert_eq!(editor.selection(), Selection::new(2, 1));
        assert_eq!(editor.cursor_offset, 1);

        editor.select_right_for_insert();
        assert!(!editor.has_selection());
        assert_eq!(editor.selection(), Selection::collapsed(2));
        assert_eq!(editor.cursor_offset, 2);
        assert_eq!(editor.edit_range(), Some(2..3));
    }

    #[test]
    fn test_insert_selection_moves_by_four_byte_groups() {
        let mut editor = create_editor_with_content(&[0u8; 10]);
        editor.set_group_size(ByteGroupSize::Four);
        editor.set_is_big_endian(true);

        editor.select_right_for_insert();
        assert_eq!(editor.selection(), Selection::new(0, 4));
        assert_eq!(editor.cursor_offset, 4);

        editor.select_right_for_insert();
        assert_eq!(editor.selection(), Selection::new(0, 8));
        assert_eq!(editor.cursor_offset, 8);

        // The final partial group ends at EOF instead of jumping past it.
        editor.select_right_for_insert();
        assert_eq!(editor.selection(), Selection::new(0, 10));
        assert_eq!(editor.cursor_offset, 10);

        editor.select_left_for_insert();
        assert_eq!(editor.selection(), Selection::new(0, 8));
        assert_eq!(editor.cursor_offset, 8);

        editor.select_left_for_insert();
        assert_eq!(editor.selection(), Selection::new(0, 4));
        assert_eq!(editor.cursor_offset, 4);

        editor.select_left_for_insert();
        assert_eq!(editor.selection(), Selection::new(0, 0));
        assert_eq!(editor.cursor_offset, 0);

        // A caret inside a group follows the same boundary as an unmodified
        // left-arrow move, and Shift+Right uses the matching next boundary.
        editor.set_cursor_offset_exact(5);
        editor.select_left_for_insert();
        assert_eq!(editor.selection(), Selection::new(5, 0));
        assert_eq!(editor.cursor_offset, 0);

        editor.select_right_for_insert();
        assert_eq!(editor.selection(), Selection::new(5, 4));
        assert_eq!(editor.cursor_offset, 4);
    }

    #[test]
    fn test_insert_selection_select_down_and_up_standard_lines() {
        let mut editor = create_editor_with_content(&[0u8; 48]);
        // Default 16-byte lines: [0..16], [16..32], [32..48]
        assert_eq!(editor.line_starts(), vec![0, 16, 32]);

        // Start at offset 0 (beginning of line 0)
        editor.set_cursor_offset_exact(0);
        editor.select_down_for_insert();
        assert_eq!(editor.selection(), Selection::new(0, 16));
        assert_eq!(editor.cursor_offset, 16);
        assert_eq!(editor.insert_cursor_offset(), 16);
        assert_eq!(editor.selection_range(), Some(0..16));

        // Select down again to line 2 (offset 32)
        editor.select_down_for_insert();
        assert_eq!(editor.selection(), Selection::new(0, 32));
        assert_eq!(editor.cursor_offset, 32);
        assert_eq!(editor.insert_cursor_offset(), 32);
        assert_eq!(editor.selection_range(), Some(0..32));

        // Select down at last line reaches EOF (48)
        editor.select_down_for_insert();
        assert_eq!(editor.selection(), Selection::new(0, 48));
        assert_eq!(editor.cursor_offset, 48);
        assert_eq!(editor.insert_cursor_offset(), 48);

        // Select up contracts selection back to line 2 (32)
        editor.select_up_for_insert();
        assert_eq!(editor.selection(), Selection::new(0, 32));
        assert_eq!(editor.cursor_offset, 32);

        // Select up contracts back to line 1 (16)
        editor.select_up_for_insert();
        assert_eq!(editor.selection(), Selection::new(0, 16));
        assert_eq!(editor.cursor_offset, 16);

        // Select up contracts back to anchor (0) -> collapsed
        editor.select_up_for_insert();
        assert_eq!(editor.selection(), Selection::new(0, 0));
        assert_eq!(editor.cursor_offset, 0);
        assert!(!editor.has_selection());

        // Select up at first line stays at 0
        editor.select_up_for_insert();
        assert_eq!(editor.selection(), Selection::new(0, 0));
        assert_eq!(editor.cursor_offset, 0);
    }

    #[test]
    fn test_insert_selection_select_down_with_join_line() {
        let mut editor = create_editor_with_content(&[0u8; 64]);
        // Join line 0 and line 1 -> line 0 is 32 bytes (0..32), line 1 is 16 bytes (32..48), line 2 is 16 bytes (48..64)
        editor.set_cursor_offset_exact(5);
        editor.join_line();
        assert_eq!(editor.line_starts(), vec![0, 32, 48]);

        // Start at offset 5 (column 5 of 32-byte line 0)
        editor.select_down_for_insert();
        // Cursor moves straight down to column 5 of line 1 (offset 32 + 5 = 37)
        assert_eq!(editor.selection(), Selection::new(5, 37));
        assert_eq!(editor.cursor_offset, 37);
        assert_eq!(editor.insert_cursor_offset(), 37);
        assert_eq!(editor.selection_range(), Some(5..37));

        // Start at offset 20 (column 20 of 32-byte line 0)
        editor.clear_selection();
        editor.set_cursor_offset_exact(20);
        editor.select_down_for_insert();
        // Line 1 is 16 bytes (32..48). Column 20 clamps to line 1 length 16 -> offset 32 + 16 = 48
        assert_eq!(editor.selection(), Selection::new(20, 48));
        assert_eq!(editor.cursor_offset, 48);
        assert_eq!(editor.insert_cursor_offset(), 48);
        assert_eq!(editor.selection_range(), Some(20..48));

        // When line 0 is 16 bytes and line 1 is joined to be 32 bytes (48 total bytes)
        let mut editor2 = create_editor_with_content(&[0u8; 48]);
        editor2.set_cursor_offset_exact(16);
        editor2.join_line(); // lines: [0..16], [16..48]
        assert_eq!(editor2.line_starts(), vec![0, 16]);

        // Caret at offset 10 in line 0 (16 bytes) moving down to joined line 1 (32 bytes)
        editor2.set_cursor_offset_exact(10);
        editor2.select_down_for_insert();
        // Moves straight down to column 10 of line 1 (16 + 10 = 26)
        assert_eq!(editor2.selection(), Selection::new(10, 26));
        assert_eq!(editor2.cursor_offset, 26);
        assert_eq!(editor2.insert_cursor_offset(), 26);
        assert_eq!(editor2.selection_range(), Some(10..26));

        // Select up from 26 moves back up to line 0 column 10 (0 + 10 = 10)
        editor2.select_up_for_insert();
        assert_eq!(editor2.selection(), Selection::new(10, 10));
        assert_eq!(editor2.cursor_offset, 10);
        assert!(!editor2.has_selection());
    }

    #[test]
    fn test_insert_selection_select_down_and_up_with_custom_breaks() {
        let mut editor = create_editor_with_content(&[0u8; 32]);
        editor.add_custom_break(10); // Lines: [0..10], [10..26], [26..32]
        assert_eq!(editor.line_starts(), vec![0, 10, 26]);

        editor.set_cursor_offset_exact(4);
        editor.select_down_for_insert();
        // Line 1 start is 10, offset_in_line is 4 -> active = 14
        assert_eq!(editor.selection(), Selection::new(4, 14));
        assert_eq!(editor.cursor_offset, 14);

        editor.select_down_for_insert();
        // Line 2 start is 26, offset_in_line is 4 -> active = 30
        assert_eq!(editor.selection(), Selection::new(4, 30));
        assert_eq!(editor.cursor_offset, 30);

        editor.select_up_for_insert();
        assert_eq!(editor.selection(), Selection::new(4, 14));
        assert_eq!(editor.cursor_offset, 14);

        editor.select_up_for_insert();
        assert_eq!(editor.selection(), Selection::new(4, 4));
        assert_eq!(editor.cursor_offset, 4);
        assert!(!editor.has_selection());
    }

    #[test]
    fn test_insert_selection_select_home() {
        let mut editor = create_editor_with_content(&[0u8; 32]);
        editor.set_cursor_offset_exact(18);
        editor.select_home_for_insert();
        assert_eq!(editor.selection(), Selection::new(18, 0));
        assert_eq!(editor.cursor_offset, 0);
        assert_eq!(editor.insert_cursor_offset(), 0);
        assert_eq!(editor.selection_range(), Some(0..18));
    }

    #[test]
    fn test_selection_caret_direction_and_collapse() {
        let mut editor = create_editor_with_content(b"12345");
        editor.set_selection(3, 1);
        editor.cursor_offset = 1;
        assert_eq!(editor.insert_cursor_offset(), 1);

        editor.move_right_for_insert();
        assert_eq!(editor.cursor_offset, 3);
        assert!(!editor.has_selection());

        editor.set_selection(1, 3);
        editor.cursor_offset = 3;
        editor.move_left();
        assert_eq!(editor.cursor_offset, 1);
        assert!(!editor.has_selection());
    }

    #[test]
    fn test_select_right_reaches_eof_and_drag_updates_caret() {
        let mut editor = create_editor_with_content(b"12345");
        editor.set_cursor_offset_exact(4);
        editor.select_right();
        assert_eq!(editor.selection(), Selection::new(4, 5));
        assert_eq!(editor.cursor_offset, 5);
        assert_eq!(editor.insert_cursor_offset(), 5);

        editor.start_drag(1);
        editor.continue_drag(1, 3);
        assert_eq!(editor.cursor_offset, 3);
        assert_eq!(editor.selection_range(), Some(1..4));
    }

    #[test]
    fn test_drag_selection_step_by_step_forward_and_backward() {
        let mut editor = create_editor_with_content(b"0123456789");

        // Forward dragging from offset 0
        editor.continue_drag(0, 0);
        assert_eq!(editor.selection_range(), Some(0..1));
        assert_eq!(editor.cursor_offset, 0);

        editor.continue_drag(0, 1);
        assert_eq!(editor.selection_range(), Some(0..2));
        assert_eq!(editor.cursor_offset, 1);

        editor.continue_drag(0, 2);
        assert_eq!(editor.selection_range(), Some(0..3));
        assert_eq!(editor.cursor_offset, 2);

        editor.continue_drag(0, 3);
        assert_eq!(editor.selection_range(), Some(0..4));
        assert_eq!(editor.cursor_offset, 3);

        // Backward dragging from offset 5
        editor.continue_drag(5, 5);
        assert_eq!(editor.selection_range(), Some(5..6));
        assert_eq!(editor.cursor_offset, 5);

        editor.continue_drag(5, 4);
        assert_eq!(editor.selection_range(), Some(4..6));
        assert_eq!(editor.cursor_offset, 4);

        editor.continue_drag(5, 3);
        assert_eq!(editor.selection_range(), Some(3..6));
        assert_eq!(editor.cursor_offset, 3);

        // Reversing direction from anchor 5 to offset 7
        editor.continue_drag(5, 7);
        assert_eq!(editor.selection_range(), Some(5..8));
        assert_eq!(editor.cursor_offset, 7);

        // Multi-byte group size (Two bytes)
        editor.set_group_size(crate::core::radix::ByteGroupSize::Two);
        editor.continue_drag(0, 0);
        assert_eq!(editor.selection_range(), Some(0..2));
        assert_eq!(editor.cursor_offset, 0);

        editor.continue_drag(0, 2);
        assert_eq!(editor.selection_range(), Some(0..4));
        assert_eq!(editor.cursor_offset, 2);

        editor.continue_drag(4, 2);
        assert_eq!(editor.selection_range(), Some(2..6));
        assert_eq!(editor.cursor_offset, 2);
    }

    #[test]
    fn test_selected_range_or_cursor_includes_selection_end() {
        let mut editor = create_editor_with_content(b"12345");

        editor.set_selection(1, 4);
        assert_eq!(editor.selected_range_or_cursor(), Some(1..4));

        editor.set_selection(4, 1);
        assert_eq!(editor.selected_range_or_cursor(), Some(1..4));

        editor.set_cursor_offset_exact(4);
        editor.set_selection(4, 4);
        assert!(!editor.has_selection());
        assert_eq!(editor.selected_range_or_cursor(), Some(4..5));
    }

    #[test]
    fn test_search_navigation() {
        let mut editor = create_editor_with_content(b"test match test");
        editor.search_state.results = vec![0, 11];

        // Ensure we handle no current index gracefully
        assert_eq!(editor.current_search_result(), None);

        // Next: 0 -> 11
        editor.next_search_result();
        assert_eq!(editor.current_search_result(), Some(0));
        assert_eq!(editor.cursor_offset, 0);

        editor.next_search_result();
        assert_eq!(editor.current_search_result(), Some(11));

        // Wrap around
        editor.next_search_result();
        assert_eq!(editor.current_search_result(), Some(0));

        // Prev
        editor.prev_search_result();
        assert_eq!(editor.current_search_result(), Some(11));
    }

    #[test]
    fn test_search_on_demand_navigation() {
        let mut editor = create_editor_with_content(b"test match test");
        editor.set_search_query_and_mode("test".to_string(), crate::core::search::SearchMode::Text);
        // results is empty because inline search does not scan the whole file
        assert!(editor.search_state.results.is_empty());

        // Next from offset 0 finds next match at offset 11
        editor.cursor_offset = 0;
        let next = editor.find_and_navigate_next();
        assert_eq!(next, Some(11));
        assert_eq!(editor.cursor_offset, 11);

        // Next from offset 11 wraps to offset 0
        let next = editor.find_and_navigate_next();
        assert_eq!(next, Some(0));
        assert_eq!(editor.cursor_offset, 0);

        // Prev from offset 0 wraps to offset 11
        let prev = editor.find_and_navigate_prev();
        assert_eq!(prev, Some(11));
        assert_eq!(editor.cursor_offset, 11);

        // Prev from offset 11 finds offset 0
        let prev = editor.find_and_navigate_prev();
        assert_eq!(prev, Some(0));
        assert_eq!(editor.cursor_offset, 0);
    }

    #[test]
    fn test_search_generation_and_race_condition() {
        let mut editor = create_editor_with_content(b"test match test");
        assert_eq!(editor.search_state.generation, 0);

        // 1. Verification of query changes incrementing generation
        editor.set_search_query("foo".to_string());
        assert_eq!(editor.search_state.generation, 1);

        editor.set_search_query("foo".to_string());
        assert_eq!(editor.search_state.generation, 1); // No change

        editor.set_search_query("bar".to_string());
        assert_eq!(editor.search_state.generation, 2);

        // 2. Discarding older queries (generation < current_generation)
        editor.set_search_results(vec![0], 1, true);
        assert!(editor.search_state.results.is_empty());

        // 3. Allowing same generation results
        editor.set_search_results(vec![1, 2], 2, true);
        assert_eq!(editor.search_state.results, vec![1, 2]);

        // 4. Overwriting or syncing generation if generation > current
        editor.set_search_results(vec![3, 4], 3, true);
        assert_eq!(editor.search_state.results, vec![3, 4]);
        assert_eq!(editor.search_state.generation, 3);
        assert!(editor.search_state.is_full_search_complete);

        // 5. Preventing partial viewport search results from overwriting full search results within the same generation
        editor.set_search_results(vec![3], 3, false); // partial results for same generation
        assert_eq!(editor.search_state.results, vec![3, 4]); // results remain full-search results

        // 6. Discarding all results and incrementing generation upon clear_search
        editor.clear_search();
        assert_eq!(editor.search_state.generation, 4);
        assert!(editor.search_state.results.is_empty());
        assert!(!editor.search_state.is_full_search_complete);

        // Try setting results with an older generation (3)
        editor.set_search_results(vec![5], 3, true);
        assert!(editor.search_state.results.is_empty());
    }

    #[test]
    fn test_shared_document() {
        let buffer = crate::core::buffer::Buffer::new(b"".to_vec());
        let document = Arc::new(RwLock::new(Document::new(std::path::PathBuf::from("test"), buffer)));
        let mut editor1 = Editor::new(document.clone());
        let mut editor2 = Editor::new(document.clone());

        // Insert in editor1
        let cmd1 = Box::new(InsertCharCommand::new(0, b'A'));
        editor1.execute_command(cmd1);

        // Verify editor2 sees change
        assert_eq!(editor2.total_size(), 1);

        // Undo in editor2
        editor2.undo();
        assert_eq!(editor1.total_size(), 0);
    }

    #[test]
    fn test_read_only_rejects_edits_and_history_changes() {
        let document = Arc::new(RwLock::new(Document::new_read_only(
            std::path::PathBuf::from("read-only.bin"),
            crate::core::buffer::Buffer::new(b"abc".to_vec()),
        )));
        let mut editor = Editor::new(document.clone());

        assert!(editor.is_read_only());
        assert!(!editor.replace_byte(0, b'X'));
        assert!(!editor.insert_bytes(1, vec![b'Y']));
        assert!(!editor.delete_forward());
        assert!(!editor.undo());
        assert!(!editor.redo());
        assert_eq!(document.read().unwrap().buffer.data(), b"abc");
        assert!(!document.read().unwrap().is_dirty());
    }

    #[test]
    fn test_shared_document_independent_cursors_and_offsets() {
        let data = (0..256).map(|i| i as u8).collect::<Vec<_>>();
        let buffer = crate::core::buffer::Buffer::new(data);
        let document = Arc::new(RwLock::new(Document::new(std::path::PathBuf::from("binary.bin"), buffer)));
        let mut editor1 = Editor::new(document.clone());
        let mut editor2 = Editor::new(document.clone());

        // Set different offsets and selections (simulating vertical/horizontal split panes)
        editor1.set_cursor_offset(0x00);
        editor1.set_selection(0x00, 0x10);

        editor2.set_cursor_offset(0x80);
        editor2.set_selection(0x80, 0xA0);

        assert_eq!(editor1.cursor_offset, 0x00);
        assert_eq!(editor1.selection(), Selection::new(0x00, 0x10));

        assert_eq!(editor2.cursor_offset, 0x80);
        assert_eq!(editor2.selection(), Selection::new(0x80, 0xA0));

        // Both editors access identical underlying bytes
        assert_eq!(editor1.total_size(), 256);
        assert_eq!(editor2.total_size(), 256);

        // Edit byte in editor1 at offset 0
        let cmd = Box::new(InsertCharCommand::new(0, 0xFF));
        editor1.execute_command(cmd);

        // Both editors see updated buffer size
        assert_eq!(editor1.total_size(), 257);
        assert_eq!(editor2.total_size(), 257);
    }

    #[test]
    fn test_undo_redo() {
        let mut editor = create_editor_with_content(b"");

        // Insert 'A'
        let cmd = Box::new(InsertCharCommand::new(0, b'A'));
        editor.execute_command(cmd);
        assert_eq!(editor.total_size(), 1);

        // Undo
        editor.undo();
        assert_eq!(editor.total_size(), 0);

        // Redo
        editor.redo();
        assert_eq!(editor.total_size(), 1);
    }

    #[test]
    fn test_replace_range_selection_and_undo_redo() {
        let mut editor = create_editor_with_content(b"abcdef");
        editor.cursor_offset = 1;
        editor.set_selection(4, 1);

        assert_eq!(editor.edit_range(), Some(1..4));
        assert!(editor.replace_range(editor.edit_range().expect("selection range"), b"XYZ".to_vec()));
        assert_eq!(editor.document.read().unwrap().buffer.data(), b"aXYZef");
        assert_eq!(editor.cursor_offset, 4);
        assert!(!editor.has_selection());

        assert!(editor.undo());
        assert_eq!(editor.document.read().unwrap().buffer.data(), b"abcdef");
        assert_eq!(editor.cursor_offset, 1);
        assert_eq!(editor.selection(), Selection::new(4, 1));

        assert!(editor.redo());
        assert_eq!(editor.document.read().unwrap().buffer.data(), b"aXYZef");
        assert_eq!(editor.cursor_offset, 4);
        assert!(!editor.has_selection());
    }

    #[test]
    fn test_insert_delete_and_selection_backspace_cursor() {
        let mut editor = create_editor_with_content(b"abcd");
        editor.set_cursor_offset(2);

        assert!(editor.insert_bytes(2, b"XY".to_vec()));
        assert_eq!(editor.document.read().unwrap().buffer.data(), b"abXYcd");
        assert_eq!(editor.cursor_offset, 4);

        assert!(editor.undo());
        assert_eq!(editor.document.read().unwrap().buffer.data(), b"abcd");
        assert_eq!(editor.cursor_offset, 2);

        editor.set_selection(1, 3);
        editor.cursor_offset = 2;
        assert!(editor.delete_backward());
        assert_eq!(editor.document.read().unwrap().buffer.data(), b"ad");
        assert_eq!(editor.cursor_offset, 1);
        assert!(!editor.has_selection());
    }

    #[test]
    fn test_backspace_moves_cursor_to_deletion_start() {
        let mut editor = create_editor_with_content(b"abcd");
        editor.set_cursor_offset_exact(2);

        assert!(editor.delete_backward());
        assert_eq!(editor.document.read().unwrap().buffer.data(), b"acd");
        assert_eq!(editor.cursor_offset, 1);
        assert!(!editor.has_selection());
    }

    #[test]
    fn test_insert_at_eof_keeps_the_insertion_cursor_at_eof() {
        let mut editor = create_editor_with_content(b"abcd");
        editor.set_cursor_offset_exact(editor.total_size());

        assert!(editor.insert_bytes(editor.cursor_offset, vec![b'X']));
        assert_eq!(editor.document.read().unwrap().buffer.data(), b"abcdX");
        assert_eq!(editor.cursor_offset, 5);
        assert!(editor.edit_range().is_none());

        editor.move_left();
        assert_eq!(editor.cursor_offset, 4);
        editor.move_right_for_insert();
        assert_eq!(editor.cursor_offset, 5);

        assert!(editor.undo());
        assert_eq!(editor.document.read().unwrap().buffer.data(), b"abcd");
        assert_eq!(editor.cursor_offset, 4);

        assert!(editor.redo());
        assert_eq!(editor.document.read().unwrap().buffer.data(), b"abcdX");
        assert_eq!(editor.cursor_offset, 5);
    }

    #[test]
    fn test_edit_adjusts_layout_and_bookmark_offsets() {
        let mut editor = create_editor_with_content(&[0; 32]);
        editor.add_custom_break(8);
        editor.add_custom_bookmark(4..12, gpui::hsla(0.0, 1.0, 0.5, 0.5));
        let bookmark_id = editor.bookmarks.read().unwrap()[0].id.clone();

        assert!(editor.insert_bytes(4, vec![1, 2]));
        assert!(editor.custom_breaks.read().unwrap().contains(&10));
        let bookmark_range = editor
            .bookmarks
            .read()
            .unwrap()
            .iter()
            .find(|item| item.id == bookmark_id)
            .map(|item| item.range())
            .expect("bookmark remains after insertion");
        assert_eq!(bookmark_range, 6..14);

        assert!(editor.undo());
        assert!(editor.custom_breaks.read().unwrap().contains(&8));
        let (bookmark_range, bookmark_color) = editor
            .bookmarks
            .read()
            .unwrap()
            .iter()
            .find(|item| item.id == bookmark_id)
            .map(|item| (item.range(), item.color))
            .expect("bookmark remains after undo");
        assert_eq!(bookmark_range, 4..12);
        assert_eq!(bookmark_color, crate::core::bookmark::BookmarkColor::Red);
    }

    #[test]
    fn test_dirty_state_survives_new_branch_after_undo() {
        let mut editor = create_editor_with_content(b"ab");

        assert!(editor.insert_bytes(1, vec![b'X']));
        editor.document.write().unwrap().mark_as_saved();
        assert!(!editor.document.read().unwrap().is_dirty());

        assert!(editor.undo());
        assert!(editor.document.read().unwrap().is_dirty());

        assert!(editor.insert_bytes(1, vec![b'Y']));
        assert!(editor.document.read().unwrap().is_dirty());
        assert!(!editor.can_redo());
    }

    #[test]
    fn test_line_starts_with_custom_breaks() {
        let mut editor = create_editor_with_content(&[0; 32]);
        // Default: 0, 16
        assert_eq!(editor.line_starts(), vec![0, 16]);

        // Add custom break at 10
        editor.add_custom_break(10);
        // Should be 0, 10, 26
        // current=0 -> push 0. next_custom=10, next_default=16. 10 < 16, so current=10.
        // current=10 -> push 10. next_custom=None, next_default=26. current=26.
        // current=26 -> push 26. next_custom=None, next_default=42. current=42 (>= 32, loop ends).
        assert_eq!(editor.line_starts(), vec![0, 10, 26]);

        // Add custom break at 5
        editor.add_custom_break(5);
        // 0, 5, 10, 26
        assert_eq!(editor.line_starts(), vec![0, 5, 10, 26]);
    }

    #[test]
    fn test_move_up_down_with_custom_breaks() {
        let mut editor = create_editor_with_content(&[0; 32]);
        editor.add_custom_break(10); // Lines: [0..10], [10..26], [26..32]

        editor.set_cursor_offset(5);
        editor.move_down();
        // Move from line 0 pos 5 to line 1 pos 5 (offset 10 + 5 = 15)
        assert_eq!(editor.cursor_offset, 15);

        editor.move_down();
        // Move from line 1 pos 5 to line 2 pos 5 (offset 26 + 5 = 31)
        assert_eq!(editor.cursor_offset, 31);

        editor.move_up();
        assert_eq!(editor.cursor_offset, 15);

        editor.move_up();
        assert_eq!(editor.cursor_offset, 5);

        // Test clamping to line length
        editor.set_cursor_offset(28); // Line 2, pos 2 (28-26)
        editor.move_up();
        // Line 1 is 16 bytes long. pos 2 is valid. 10 + 2 = 12.
        assert_eq!(editor.cursor_offset, 12);

        editor.set_cursor_offset(20); // Line 1, pos 10
        editor.move_down();
        // Line 2 is 6 bytes long. pos 10 is too far. Clamp to 5. 26 + 5 = 31.
        assert_eq!(editor.cursor_offset, 31);
    }

    #[test]
    fn test_join_line_creates_long_rows() {
        let mut editor = create_editor_with_content(&[0; 48]);
        // Default: 0, 16, 32
        assert_eq!(editor.line_starts(), vec![0, 16, 32]);

        // Join line 0 and line 1 (remove 16-byte boundary at offset 16)
        editor.set_cursor_offset(5); // On line 0
        editor.join_line();
        // Now offset 16 is in custom_joins, so line_starts should skip it
        // current=0 -> push 0. next_pos=16, but 16 is in joins, so next_pos=32. current=32.
        // current=32 -> push 32. next_pos=48, not in joins. current=48 (>= 48, loop ends).
        assert_eq!(editor.line_starts(), vec![0, 32]);
    }

    #[test]
    fn test_join_line_removes_custom_break() {
        let mut editor = create_editor_with_content(&[0; 32]);
        editor.add_custom_break(10);
        // Lines: [0..10], [10..26], [26..32]
        assert_eq!(editor.line_starts(), vec![0, 10, 26]);

        // Join line 0 with line 1 (removes custom break at 10)
        editor.set_cursor_offset(3);
        editor.join_line();
        // Custom break at 10 removed, back to default 16-byte lines
        assert_eq!(editor.line_starts(), vec![0, 16]);
    }

    #[test]
    fn test_join_line_multiple_joins() {
        let mut editor = create_editor_with_content(&[0; 64]);
        // Default: 0, 16, 32, 48
        assert_eq!(editor.line_starts(), vec![0, 16, 32, 48]);

        // Join all into one big line
        editor.set_cursor_offset(0);
        editor.join_line(); // joins 0+16 -> skip 16
        editor.join_line(); // joins 0+32 -> skip 32
        editor.join_line(); // joins 0+48 -> skip 48
        // All boundaries joined, single line
        assert_eq!(editor.line_starts(), vec![0]);
    }

    #[test]
    fn test_clear_all_custom_breaks() {
        let mut editor = create_editor_with_content(&[0; 48]);
        editor.add_custom_break(5);
        editor.add_custom_break(10);
        editor.set_cursor_offset(0);
        editor.join_line(); // join some lines

        assert!(editor.has_custom_layout());
        assert!(editor.custom_layout_count() > 0);

        editor.clear_all_custom_breaks();
        assert!(!editor.has_custom_layout());
        assert_eq!(editor.custom_layout_count(), 0);
        // Back to default
        assert_eq!(editor.line_starts(), vec![0, 16, 32]);
    }

    #[test]
    fn test_custom_break_overrides_join() {
        let mut editor = create_editor_with_content(&[0; 48]);
        // Join at 16
        editor.set_cursor_offset(0);
        editor.join_line();
        assert_eq!(editor.line_starts(), vec![0, 32]);

        // Adding a custom break at 16 should remove the join
        editor.add_custom_break(16);
        assert_eq!(editor.line_starts(), vec![0, 16, 32]);
    }

    #[test]
    fn test_sparse_line_map_large_offsets() {
        let buffer = crate::core::buffer::Buffer::new(vec![0; 10_000_000]);
        let document = Arc::new(RwLock::new(Document::new(std::path::PathBuf::from("test"), buffer)));
        let mut editor = Editor::new(document);

        let starts = editor.line_starts();
        assert!(matches!(starts, LineMap::Standard { .. }));
        assert_eq!(starts.len(), 10_000_000_usize.div_ceil(16));

        editor.add_custom_break(5_000_000);
        editor.add_custom_break(5_000_010);

        let starts = editor.line_starts();
        assert!(matches!(starts, LineMap::Sparse(_)));

        if let LineMap::Sparse(ref sparse) = starts {
            assert!(sparse.segments.len() <= 5);
        }

        assert_eq!(starts.get(0), Some(0));
        assert_eq!(starts.get(100), Some(1600));

        assert_eq!(starts.binary_search(&0), Ok(0));
        assert_eq!(starts.binary_search(&1600), Ok(100));

        let line_idx = Editor::find_line_index(5_000_000, &starts);
        assert_eq!(starts.get(line_idx), Some(5_000_000));
        assert_eq!(starts.get(line_idx + 1), Some(5_000_010));
    }

    #[test]
    fn test_double_empty_line() {
        // Enterを2回押すと empty_lines[offset] = 2 になる
        // 2行分の空行が正しく生成されることを確認するリグレッションテスト
        let mut editor = create_editor_with_content(&[0; 32]);
        // デフォルト: [0..16], [16..32]
        assert_eq!(editor.line_starts(), vec![0, 16]);

        // offset 16 に空行を1つ追加
        editor.add_empty_line(16);
        // [0..16], [空], [16..32] の3行
        assert_eq!(editor.line_starts(), vec![0, 16, 16]);
        assert_eq!(editor.line_starts().len(), 3);

        // offset 16 にさらに空行をもう1つ追加（2回目のEnter）
        editor.add_empty_line(16);
        // [0..16], [空1], [空2], [16..32] の4行
        // 修正前はここで3行しか返らずバグになっていた
        assert_eq!(editor.line_starts(), vec![0, 16, 16, 16]);
        assert_eq!(editor.line_starts().len(), 4);

        // offset 0 にも2回空行を追加
        editor.add_empty_line(0);
        editor.add_empty_line(0);
        // [空1@0], [空2@0], [0..16], [空1@16], [空2@16], [16..32] の6行
        assert_eq!(editor.line_starts(), vec![0, 0, 0, 16, 16, 16]);
        assert_eq!(editor.line_starts().len(), 6);
    }

    #[test]
    fn test_split_mega_line_preserves_end() {
        // delete×3 で 64バイトのメガ行を作り、オフセット5で改行したとき
        // [0..5] と [5..64] の2行になることを確認するリグレッションテスト
        let mut editor = create_editor_with_content(&[0; 64]);
        assert_eq!(editor.line_starts(), vec![0, 16, 32, 48]);

        // delete×3 → 64バイトのメガ行
        editor.set_cursor_offset(0);
        editor.join_line();
        editor.join_line();
        editor.join_line();
        assert_eq!(editor.line_starts(), vec![0]);

        // オフセット5で改行 → [0..5] と [5..64]
        editor.add_custom_break(5);
        assert_eq!(editor.line_starts(), vec![0, 5]);
        assert_eq!(editor.line_starts().len(), 2);

        // 48バイト行のケース（user報告のシナリオ）:
        // 別バッファ: 48バイト, delete×2 → 48バイト行, オフセット5で改行
        let mut editor2 = create_editor_with_content(&[0; 48]);
        assert_eq!(editor2.line_starts(), vec![0, 16, 32]);
        editor2.set_cursor_offset(0);
        editor2.join_line();
        editor2.join_line();
        assert_eq!(editor2.line_starts(), vec![0]); // 48バイト行

        editor2.add_custom_break(5);
        // [0..5] (5バイト) と [5..48] (43バイト)
        assert_eq!(editor2.line_starts(), vec![0, 5]);
        assert_eq!(editor2.line_starts().len(), 2);

        // 追加ケース: 32バイトのメガ行をオフセット7で分割
        let mut editor3 = create_editor_with_content(&[0; 32]);
        editor3.set_cursor_offset(0);
        editor3.join_line();
        assert_eq!(editor3.line_starts(), vec![0]); // 32バイト行
        editor3.add_custom_break(7);
        // [0..7] と [7..32]
        assert_eq!(editor3.line_starts(), vec![0, 7]);
    }

    #[test]
    fn test_split_mega_line_mid_join() {
        // delete で 32バイトのメガ行を作り、オフセット18（結合境界16の後）で改行したとき
        // [0..18] と [18..32] の2行になることを確認するリグレッションテスト
        // 修正前は join@16 が削除されて [0..16], [16..18], [18..32] の3行になっていた
        let mut editor = create_editor_with_content(&[0; 32]);
        assert_eq!(editor.line_starts(), vec![0, 16]);

        // delete → 32バイトのメガ行
        editor.set_cursor_offset(0);
        editor.join_line();
        assert_eq!(editor.line_starts(), vec![0]);

        // オフセット18で改行 → [0..18] と [18..32]
        editor.add_custom_break(18);
        assert_eq!(editor.line_starts(), vec![0, 18]);
        assert_eq!(editor.line_starts().len(), 2);

        // 64バイトのメガ行をオフセット18で分割
        let mut editor2 = create_editor_with_content(&[0; 64]);
        editor2.set_cursor_offset(0);
        editor2.join_line();
        editor2.join_line();
        editor2.join_line();
        assert_eq!(editor2.line_starts(), vec![0]); // 64バイトのメガ行

        editor2.add_custom_break(18);
        // [0..18] と [18..64]
        assert_eq!(editor2.line_starts(), vec![0, 18]);
        assert_eq!(editor2.line_starts().len(), 2);
    }

    #[test]
    fn test_editor_empty_lines_and_breaks() {
        use crate::core::buffer::Buffer;
        use std::path::PathBuf;
        let doc = Arc::new(RwLock::new(Document::new(PathBuf::from("test.bin"), Buffer::new(vec![0; 100]))));
        let mut editor = Editor::new(doc);

        editor.add_empty_line(10);
        assert_eq!(editor.empty_lines.read().unwrap().get(&10), Some(&1));

        editor.add_empty_line(10);
        assert_eq!(editor.empty_lines.read().unwrap().get(&10), Some(&2));

        assert!(editor.remove_empty_line(10));
        assert_eq!(editor.empty_lines.read().unwrap().get(&10), Some(&1));

        assert!(editor.remove_empty_line(10));
        assert_eq!(editor.empty_lines.read().unwrap().get(&10), None);

        editor.toggle_custom_break(20);
        assert!(editor.custom_breaks.read().unwrap().contains(&20));

        editor.toggle_custom_break(20);
        assert!(!editor.custom_breaks.read().unwrap().contains(&20));
    }

    #[test]
    fn test_custom_bookmarks() {
        use gpui::hsla;
        let mut editor = create_editor_with_content(b"01234567890123456789"); // 20 bytes
        let red = hsla(0.0, 1.0, 0.5, 0.5);
        let blue = hsla(0.6, 1.0, 0.5, 0.5);

        // Add red bookmark on 0..10
        editor.add_custom_bookmark(0..10, red);
        assert_eq!(editor.bookmarks.read().unwrap().len(), 1);
        assert_eq!(editor.bookmarks.read().unwrap()[0].range(), 0..10);
        assert_eq!(editor.bookmarks.read().unwrap()[0].color, BookmarkColor::Red);

        // Update comment
        let id = editor.bookmarks.read().unwrap()[0].id.clone();
        assert!(editor.update_bookmark_comment(&id, "Header block"));
        assert_eq!(editor.bookmarks.read().unwrap()[0].comment, "Header block");

        // Add blue bookmark on 5..15
        editor.add_custom_bookmark(5..15, blue);
        assert_eq!(editor.bookmarks.read().unwrap().len(), 2);
        assert_eq!(editor.bookmarks.read().unwrap()[0].range(), 0..5);
        assert_eq!(editor.bookmarks.read().unwrap()[1].range(), 5..15);
        assert_eq!(editor.bookmarks.read().unwrap()[1].color, BookmarkColor::Blue);

        // Clear sub-range 3..7
        editor.clear_custom_bookmark(3..7);
        assert_eq!(editor.bookmarks.read().unwrap().len(), 2);
        assert_eq!(editor.bookmarks.read().unwrap()[0].range(), 0..3);
        assert_eq!(editor.bookmarks.read().unwrap()[1].range(), 7..15);

        // Clear all
        editor.clear_all_custom_bookmarks();
        assert!(editor.bookmarks.read().unwrap().is_empty());
    }

    #[test]
    fn test_editor_bookmarks_crud_and_file_io() {
        let mut editor = create_editor_with_content(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"); // 36 bytes

        // Add bookmarks
        let item1 = BookmarkItem::new(0, 4, BookmarkColor::Red, "Magic bytes");
        let id1 = item1.id.clone();
        editor.add_bookmark(item1);

        let item2 = BookmarkItem::new(10, 8, BookmarkColor::Green, "Payload");
        let id2 = item2.id.clone();
        editor.add_bookmark(item2);

        assert_eq!(editor.bookmarks.read().unwrap().len(), 2);

        // Update comment
        assert!(editor.update_bookmark_comment(&id1, "ELF Magic"));
        assert_eq!(editor.bookmarks.read().unwrap()[0].comment, "ELF Magic");

        // Update color
        assert!(editor.update_bookmark_color(&id1, BookmarkColor::Cyan));
        assert_eq!(editor.bookmarks.read().unwrap()[0].color, BookmarkColor::Cyan);

        // Update range
        assert!(editor.update_bookmark_range(&id2, 12, 10));
        assert_eq!(editor.bookmarks.read().unwrap()[1].offset, 12);
        assert_eq!(editor.bookmarks.read().unwrap()[1].size, 10);

        // Test export and import
        let temp_file = std::env::temp_dir().join("editor_bookmarks_test.json");
        editor.export_bookmarks_to_file(&temp_file).unwrap();
        assert!(temp_file.exists());

        // Create new editor and import
        let mut editor2 = create_editor_with_content(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789");
        let count = editor2.import_bookmarks_from_file(&temp_file).unwrap();
        assert_eq!(count, 2);
        assert_eq!(editor2.bookmarks.read().unwrap().len(), 2);
        assert_eq!(editor2.bookmarks.read().unwrap()[0].comment, "ELF Magic");
        assert_eq!(editor2.bookmarks.read().unwrap()[0].color, BookmarkColor::Cyan);
        assert_eq!(editor2.bookmarks.read().unwrap()[1].offset, 12);
        assert_eq!(editor2.bookmarks.read().unwrap()[1].size, 10);

        // Remove by id
        assert!(editor.remove_bookmark_by_id(&id1));
        assert_eq!(editor.bookmarks.read().unwrap().len(), 1);
        assert_eq!(editor.bookmarks.read().unwrap()[0].id, id2);

        // Remove by index
        let removed = editor.remove_bookmark_by_index(0);
        assert!(removed.is_some());
        assert!(editor.bookmarks.read().unwrap().is_empty());

        let _ = std::fs::remove_file(temp_file);
    }

    #[test]
    fn test_import_and_add_bookmark_no_id_collision() {
        let mut editor = create_editor_with_content(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789");
        let item1 = BookmarkItem::new(0, 4, BookmarkColor::Red, "Magic bytes");
        editor.add_bookmark(item1);

        let temp_file = std::env::temp_dir().join("collision_test.json");
        editor.export_bookmarks_to_file(&temp_file).unwrap();

        let mut editor2 = create_editor_with_content(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789");
        editor2.import_bookmarks_from_file(&temp_file).unwrap();
        assert_eq!(editor2.bookmarks.read().unwrap().len(), 1);

        // Now add a new bookmark
        let new_item = BookmarkItem::new(10, 4, BookmarkColor::Yellow, "New bookmark");
        let added_id = editor2.add_bookmark(new_item);

        // Must have 2 distinct IDs
        assert_eq!(editor2.bookmarks.read().unwrap().len(), 2);
        assert_ne!(editor2.bookmarks.read().unwrap()[0].id, editor2.bookmarks.read().unwrap()[1].id);
        assert_eq!(editor2.bookmarks.read().unwrap()[1].id, added_id);

        // Editing one must not affect the other
        assert!(editor2.update_bookmark_comment(&added_id, "Updated new comment"));
        assert_eq!(editor2.bookmarks.read().unwrap()[0].comment, "Magic bytes");
        assert_eq!(editor2.bookmarks.read().unwrap()[1].comment, "Updated new comment");

        // Deleting the new one must leave the first intact
        assert!(editor2.remove_bookmark_by_id(&added_id));
        assert_eq!(editor2.bookmarks.read().unwrap().len(), 1);
        assert_eq!(editor2.bookmarks.read().unwrap()[0].comment, "Magic bytes");

        let _ = std::fs::remove_file(temp_file);
    }

    #[test]
    fn test_shared_bookmarks_across_split_editors() {
        use crate::core::buffer::Buffer;
        use std::path::PathBuf;
        let doc = Arc::new(RwLock::new(Document::new(PathBuf::from("shared.bin"), Buffer::new(vec![0xAA; 128]))));
        let mut editor1 = Editor::new(doc.clone());
        let editor2 = Editor::new(doc.clone());

        // Both start empty
        assert_eq!(editor1.bookmarks_snapshot().len(), 0);
        assert_eq!(editor2.bookmarks_snapshot().len(), 0);

        // Add bookmark in editor1
        let hl = BookmarkItem::new(0, 16, BookmarkColor::Red, "Header");
        let id = editor1.add_bookmark(hl);

        // editor2 immediately sees the bookmark from shared instance
        assert_eq!(editor2.bookmarks_snapshot().len(), 1);
        assert_eq!(editor2.bookmarks_snapshot()[0].comment, "Header");
        assert_eq!(editor2.bookmarks_snapshot()[0].color, BookmarkColor::Red);

        // Update comment in editor1
        assert!(editor1.update_bookmark_comment(&id, "Updated Header"));
        assert_eq!(editor2.bookmarks_snapshot()[0].comment, "Updated Header");

        // Clear in editor1
        editor1.clear_all_custom_bookmarks();
        assert_eq!(editor2.bookmarks_snapshot().len(), 0);
    }

    #[test]
    fn test_shared_custom_breaks_across_split_editors() {
        use crate::core::buffer::Buffer;
        use std::path::PathBuf;
        let doc = Arc::new(RwLock::new(Document::new(PathBuf::from("shared.bin"), Buffer::new(vec![0xBB; 128]))));
        let mut editor1 = Editor::new(doc.clone());
        let editor2 = Editor::new(doc.clone());

        editor1.add_custom_break(20);
        assert!(editor2.custom_breaks.read().unwrap().contains(&20));

        editor1.remove_custom_break(20);
        assert!(!editor2.custom_breaks.read().unwrap().contains(&20));
    }

    #[test]
    fn test_editor_radix_group_size_and_endian() {
        let mut editor = create_editor_with_content(b"Hello World 12345678");
        assert_eq!(editor.radix, DisplayRadix::Hexadecimal);
        assert_eq!(editor.group_size, ByteGroupSize::One);
        assert!(!editor.is_big_endian);

        editor.set_radix(DisplayRadix::Decimal);
        assert_eq!(editor.radix, DisplayRadix::Decimal);

        editor.set_group_size(ByteGroupSize::Four);
        assert_eq!(editor.group_size, ByteGroupSize::Four);

        editor.set_is_big_endian(true);
        assert!(editor.is_big_endian);

        editor.toggle_byte_order();
        assert!(!editor.is_big_endian);
    }

    #[test]
    fn test_editor_grouping_cursor_movement_and_selection() {
        let mut editor = create_editor_with_content(&[0u8; 64]);
        editor.set_group_size(ByteGroupSize::Four);
        assert_eq!(editor.cursor_offset, 0);

        // Move right by 4 bytes (1 group)
        editor.move_right();
        assert_eq!(editor.cursor_offset, 4);
        editor.move_right();
        assert_eq!(editor.cursor_offset, 8);

        // Move left by 4 bytes
        editor.move_left();
        assert_eq!(editor.cursor_offset, 4);

        // Selection right by 4 bytes
        editor.select_right();
        assert_eq!(editor.selection(), Selection::new(4, 8));
        assert_eq!(editor.cursor_offset, 8);
        assert_eq!(editor.selected_range_or_cursor(), Some(4..8));

        // Selection left
        editor.select_left();
        assert_eq!(editor.cursor_offset, 4);
        assert_eq!(editor.selected_range_or_cursor(), Some(4..8));

        // Group 2 bytes
        editor.set_group_size(ByteGroupSize::Two);
        editor.go_to_beginning();
        assert_eq!(editor.cursor_offset, 0);
        editor.move_right();
        assert_eq!(editor.cursor_offset, 2);
        assert_eq!(editor.selected_range_or_cursor(), Some(2..4));

        // Move down across 16-byte rows
        editor.move_down();
        assert_eq!(editor.cursor_offset, 18);
        editor.move_up();
        assert_eq!(editor.cursor_offset, 2);
    }

    #[test]
    fn test_join_line_with_selection_multiple_lines() {
        let mut editor = create_editor_with_content(&[0u8; 64]);
        assert_eq!(editor.line_starts().len(), 4); // 0, 16, 32, 48

        // Select 0..32 (first 2 lines: bytes 0..=31)
        editor.set_selection(0, 32);

        editor.join_line();

        let line_starts = editor.line_starts();
        assert_eq!(line_starts.len(), 3);
        assert_eq!(line_starts.get(0), Some(0));
        assert_eq!(line_starts.get(1), Some(32));
        assert_eq!(line_starts.get(2), Some(48));
    }

    #[test]
    fn test_join_line_with_selection_arbitrary_sub_range() {
        let mut editor = create_editor_with_content(&[0u8; 100]);
        // Initially 16-byte chunks: 0, 16, 32, 48, 64, 80, 96

        // Select bytes 10..=49 (range 10..50, 40 bytes)
        editor.set_selection(10, 50);

        editor.join_line();

        let line_starts = editor.line_starts();
        assert_eq!(line_starts.get(0), Some(0)); // 0..10
        assert_eq!(line_starts.get(1), Some(10)); // 10..50 (joined line!)
        assert_eq!(line_starts.get(2), Some(50)); // 50..66
    }

    #[test]
    fn test_join_line_with_selection_cleans_custom_breaks() {
        let mut editor = create_editor_with_content(&[0u8; 32]);
        editor.add_custom_break(4);
        editor.add_custom_break(8);
        editor.add_custom_break(12);

        // Select bytes 0..=15 (0..16)
        editor.set_selection(0, 16);

        editor.join_line();

        let line_starts = editor.line_starts();
        assert_eq!(line_starts.get(0), Some(0)); // 0..16
        assert_eq!(line_starts.get(1), Some(16)); // 16..32
    }
}
