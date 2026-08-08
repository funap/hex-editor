#![allow(dead_code)]

use crate::core::command::Command;
use crate::core::document::Document;
use crate::core::encoding::Encoding;
use crate::core::structure::ParseResult;
use std::cell::RefCell;
use std::cmp;
use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::Arc;
use std::sync::RwLock;

pub use crate::core::layout::{BYTES_PER_ROW, LayoutSegment, LineMap, SegmentKind, SparseLineMap};

#[derive(Default, Clone)]
pub struct SearchState {
    pub query: String,
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
    pub selection_start: Option<usize>,
    pub selection_end: Option<usize>,
    pub search_state: SearchState,
    pub custom_breaks: BTreeSet<usize>,
    /// 16バイト自然境界のうち、改行を抑制すべき位置を記録する。
    /// join_line() で追加され、行を16バイト超に結合するために使用。
    pub custom_joins: BTreeSet<usize>,
    /// 特定のオフセットに挿入された空行の数を記録する。
    pub empty_lines: std::collections::BTreeMap<usize, usize>,
    pub encoding: Encoding,
    pub ksy_definition: Option<Arc<crate::core::structure::KsyDefinition>>,
    pub parse_result: Option<Arc<ParseResult>>,
    pub is_parsing_structure: bool,
    pub parse_progress_offset: usize,
    pub parse_total_size: usize,
    pub parse_generation: usize,
    pub parse_cancel_token: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub collapsed_struct_ids: std::collections::HashSet<String>,
    pub show_inline_structure_view: bool,
    cached_line_map: RefCell<Option<LineMap>>,
}

impl Editor {
    pub fn new(document: Arc<RwLock<Document>>) -> Self {
        Self {
            document,
            cursor_offset: 0,
            selection_start: None,
            selection_end: None,
            search_state: SearchState::default(),
            custom_breaks: BTreeSet::new(),
            custom_joins: BTreeSet::new(),
            empty_lines: std::collections::BTreeMap::new(),
            encoding: Encoding::default(),
            ksy_definition: None,
            parse_result: None,
            is_parsing_structure: false,
            parse_progress_offset: 0,
            parse_total_size: 0,
            parse_generation: 0,
            parse_cancel_token: None,
            collapsed_struct_ids: std::collections::HashSet::new(),
            show_inline_structure_view: true,
            cached_line_map: RefCell::new(None),
        }
    }

    pub fn total_size(&self) -> usize {
        self.document.read().expect("document read lock").buffer.len()
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

    pub fn selection_range(&self) -> Option<Range<usize>> {
        if let (Some(start), Some(end)) = (self.selection_start, self.selection_end) {
            let min = cmp::min(start, end);
            let max = cmp::max(start, end);
            Some(min..max)
        } else {
            None
        }
    }

    pub fn set_cursor_offset(&mut self, offset: usize) {
        let buffer_len = self.total_size();
        self.selection_start = None;
        self.selection_end = None;
        self.cursor_offset = offset.min(buffer_len);
    }

    pub fn move_left(&mut self) {
        if self.cursor_offset > 0 {
            self.cursor_offset -= 1;
            self.selection_start = None;
            self.selection_end = None;
        }
    }

    pub fn move_right(&mut self) {
        let buffer_len = self.total_size();
        if self.cursor_offset < buffer_len {
            self.cursor_offset += 1;
            self.selection_start = None;
            self.selection_end = None;
        }
    }

    pub fn move_up(&mut self) {
        let line_starts = self.line_starts();
        let current_line_idx = Self::find_line_index(self.cursor_offset, &line_starts);

        if let Some(prev_idx) = Self::prev_data_line(current_line_idx, &line_starts) {
            let current_line_start = line_starts.get(current_line_idx).expect("valid current line start");
            let offset_in_line = self.cursor_offset - current_line_start;
            let prev_line_start = line_starts.get(prev_idx).expect("valid prev line start");
            let prev_line_end = line_starts.get(prev_idx + 1).expect("valid prev line end");
            let prev_line_len = prev_line_end - prev_line_start;

            self.cursor_offset = prev_line_start + cmp::min(offset_in_line, prev_line_len.saturating_sub(1));
            self.selection_start = None;
            self.selection_end = None;
        }
    }

    pub fn move_down(&mut self) {
        let line_starts = self.line_starts();
        let total_size = self.total_size();
        let current_line_idx = Self::find_line_index(self.cursor_offset, &line_starts);

        if let Some(next_idx) = Self::next_data_line(current_line_idx, &line_starts, total_size) {
            let current_line_start = line_starts.get(current_line_idx).expect("valid current line start");
            let offset_in_line = self.cursor_offset - current_line_start;
            let next_line_start = line_starts.get(next_idx).expect("valid next line start");
            let next_line_end = if next_idx + 1 < line_starts.len() {
                line_starts.get(next_idx + 1).expect("valid next line end")
            } else {
                total_size
            };
            let next_line_len = next_line_end - next_line_start;

            if next_line_len > 0 {
                self.cursor_offset = next_line_start + cmp::min(offset_in_line, next_line_len - 1);
            } else {
                self.cursor_offset = next_line_start;
            }
            self.selection_start = None;
            self.selection_end = None;
        } else {
            self.cursor_offset = total_size;
            self.selection_start = None;
            self.selection_end = None;
        }
    }

    pub fn select_left(&mut self) {
        if self.cursor_offset > 0 {
            if self.selection_start.is_none() {
                self.selection_start = Some(self.cursor_offset);
            }
            self.cursor_offset -= 1;
            self.selection_end = Some(self.cursor_offset);
        }
    }

    pub fn select_right(&mut self) {
        let buffer_len = self.total_size();
        if self.cursor_offset < buffer_len {
            if self.selection_start.is_none() {
                self.selection_start = Some(self.cursor_offset);
            }
            self.cursor_offset += 1;
            self.selection_end = Some(self.cursor_offset);
        }
    }

    pub fn select_up(&mut self) {
        let line_starts = self.line_starts();
        let current_line_idx = Self::find_line_index(self.cursor_offset, &line_starts);

        if let Some(prev_idx) = Self::prev_data_line(current_line_idx, &line_starts) {
            if self.selection_start.is_none() {
                self.selection_start = Some(self.cursor_offset);
            }
            let current_line_start = line_starts.get(current_line_idx).expect("valid current line start");
            let offset_in_line = self.cursor_offset - current_line_start;
            let prev_line_start = line_starts.get(prev_idx).expect("valid prev line start");
            let prev_line_end = line_starts.get(prev_idx + 1).expect("valid prev line end");
            let prev_line_len = prev_line_end - prev_line_start;

            self.cursor_offset = prev_line_start + cmp::min(offset_in_line, prev_line_len.saturating_sub(1));
            self.selection_end = Some(self.cursor_offset);
        }
    }

    pub fn select_down(&mut self) {
        let line_starts = self.line_starts();
        let total_size = self.total_size();
        let current_line_idx = Self::find_line_index(self.cursor_offset, &line_starts);

        if self.selection_start.is_none() {
            self.selection_start = Some(self.cursor_offset);
        }

        if let Some(next_idx) = Self::next_data_line(current_line_idx, &line_starts, total_size) {
            let current_line_start = line_starts.get(current_line_idx).expect("valid current line start");
            let offset_in_line = self.cursor_offset - current_line_start;
            let next_line_start = line_starts.get(next_idx).expect("valid next line start");
            let next_line_end = if next_idx + 1 < line_starts.len() {
                line_starts.get(next_idx + 1).expect("valid next line end")
            } else {
                total_size
            };
            let next_line_len = next_line_end - next_line_start;

            if next_line_len > 0 {
                self.cursor_offset = next_line_start + cmp::min(offset_in_line, next_line_len - 1);
            } else {
                self.cursor_offset = next_line_start;
            }
            self.selection_end = Some(self.cursor_offset);
        } else {
            self.cursor_offset = total_size;
            self.selection_end = Some(self.cursor_offset);
        }
    }

    pub fn select_all(&mut self) {
        let buffer_len = self.total_size();
        self.selection_start = Some(0);
        self.selection_end = Some(buffer_len);
        self.cursor_offset = buffer_len;
    }

    pub fn go_to_beginning(&mut self) {
        self.cursor_offset = 0;
        self.selection_start = None;
        self.selection_end = None;
    }

    pub fn go_to_end(&mut self) {
        self.cursor_offset = self.total_size();
        self.selection_start = None;
        self.selection_end = None;
    }

    pub fn page_up(&mut self, visible_rows: usize) {
        let line_starts = self.line_starts();
        let current_line_idx = Self::find_line_index(self.cursor_offset, &line_starts);

        self.selection_start = None;
        self.selection_end = None;

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

        self.cursor_offset = target_line_start + cmp::min(offset_in_line, target_line_len.saturating_sub(1));
    }

    pub fn page_down(&mut self, visible_rows: usize) {
        let line_starts = self.line_starts();
        let current_line_idx = Self::find_line_index(self.cursor_offset, &line_starts);

        self.selection_start = None;
        self.selection_end = None;

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
            self.cursor_offset = self.total_size();
        } else {
            self.cursor_offset = target_line_start + cmp::min(offset_in_line, target_line_len.saturating_sub(1));
        }
    }

    pub fn home(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
        self.cursor_offset = 0;
    }

    pub fn end(&mut self) {
        let buffer_len = self.total_size();
        self.selection_start = None;
        self.selection_end = None;
        self.cursor_offset = buffer_len;
    }

    pub fn select_page_up(&mut self, visible_rows: usize) {
        let line_starts = self.line_starts();
        let current_line_idx = Self::find_line_index(self.cursor_offset, &line_starts);

        if self.selection_start.is_none() {
            self.selection_start = Some(self.cursor_offset);
        }

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

        self.cursor_offset = target_line_start + cmp::min(offset_in_line, target_line_len.saturating_sub(1));
        self.selection_end = Some(self.cursor_offset);
    }

    pub fn select_page_down(&mut self, visible_rows: usize) {
        let line_starts = self.line_starts();
        let current_line_idx = Self::find_line_index(self.cursor_offset, &line_starts);

        if self.selection_start.is_none() {
            self.selection_start = Some(self.cursor_offset);
        }

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
            self.cursor_offset = self.total_size();
        } else {
            self.cursor_offset = target_line_start + cmp::min(offset_in_line, target_line_len.saturating_sub(1));
        }
        self.selection_end = Some(self.cursor_offset);
    }

    pub fn select_home(&mut self) {
        if self.selection_start.is_none() {
            self.selection_start = Some(self.cursor_offset);
        }
        self.cursor_offset = 0;
        self.selection_end = Some(self.cursor_offset);
    }

    pub fn select_end(&mut self) {
        let buffer_len = self.total_size();
        if self.selection_start.is_none() {
            self.selection_start = Some(self.cursor_offset);
        }
        self.cursor_offset = buffer_len;
        self.selection_end = Some(self.cursor_offset);
    }

    pub fn start_drag(&mut self, byte_pos: usize) {
        self.cursor_offset = byte_pos;
        self.selection_start = Some(byte_pos);
        self.selection_end = Some(byte_pos);
    }

    pub fn continue_drag(&mut self, byte_pos: usize) {
        self.selection_end = Some(byte_pos);
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

    pub fn next_search_result(&mut self) -> Option<usize> {
        if self.search_state.results.is_empty() {
            return None;
        }

        let next_index = if let Some(index) = self.search_state.current_result_index {
            (index + 1) % self.search_state.results.len()
        } else {
            0
        };

        self.search_state.current_result_index = Some(next_index);
        Some(self.search_state.results[next_index])
    }

    pub fn line_starts(&self) -> LineMap {
        if let Some(cached) = self.cached_line_map.borrow().as_ref() {
            return cached.clone();
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

                let mut layout_events: Vec<usize> = Vec::new();
                layout_events.extend(self.custom_breaks.iter().copied());
                layout_events.extend(self.custom_joins.iter().copied());
                layout_events.extend(self.empty_lines.keys().copied());
                if self.show_inline_structure_view
                    && let Some(parse_res) = &self.parse_result
                {
                    parse_res.collect_field_breaks(&mut layout_events, &self.collapsed_struct_ids);
                }
                layout_events.sort_unstable();
                layout_events.dedup();

                let mut break_events: Vec<usize> = Vec::new();
                break_events.extend(self.custom_breaks.iter().copied());
                break_events.extend(self.empty_lines.keys().copied());
                if self.show_inline_structure_view
                    && let Some(parse_res) = &self.parse_result
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
                                None => true,
                            };

                            if can_transition {
                                break;
                            }
                        }

                        // Process empty lines at current
                        if let Some(&count) = self.empty_lines.get(&current) {
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
                        while self.custom_joins.contains(&next_pos) && next_pos < total_size {
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

            // offset より後ろの custom_joins を削除する。
            // offset より前の join（例: オフセット18で分割する際の join@16）は
            // 第1部分 [line_start..offset] を1行に保つために必要なので残す。
            if offset < line_end {
                let joins_to_remove: Vec<usize> = self.custom_joins.range((offset + 1)..line_end).copied().collect();
                for j in joins_to_remove {
                    self.custom_joins.remove(&j);
                }
            }

            self.custom_breaks.insert(offset);
            // Custom break と custom join が同じ位置にある場合、join を解除
            self.custom_joins.remove(&offset);

            // メガ行（1行が BYTES_PER_ROW を超える）を分割した場合、
            // 第2部分 [offset..line_end] が1行として維持されるよう再結合する。
            // offset から BYTES_PER_ROW ずつ進むステップを custom_joins に追加し、
            // line_end が offset+k*BYTES_PER_ROW と一致しない場合は line_end にも
            // custom_break を追加して行末を明示する。
            if line_length > BYTES_PER_ROW && offset != line_start {
                let mut step = offset + BYTES_PER_ROW;
                while step < line_end {
                    self.custom_joins.insert(step);
                    step += BYTES_PER_ROW;
                }
                // line_end が offset から BYTES_PER_ROW の倍数で到達できない場合、
                // アルゴリズムが line_end をまたいでしまうため、明示的に break を追加する
                if line_end < self.total_size() && !(line_end - offset).is_multiple_of(BYTES_PER_ROW) && !self.custom_breaks.contains(&line_end) {
                    self.custom_breaks.insert(line_end);
                }
            }

            self.cached_line_map.replace(None);
        }
    }

    pub fn remove_custom_break(&mut self, offset: usize) {
        if self.custom_breaks.remove(&offset) {
            self.cached_line_map.replace(None);
        }
    }

    pub fn toggle_custom_break(&mut self, offset: usize) {
        if self.custom_breaks.contains(&offset) {
            self.remove_custom_break(offset);
        } else {
            self.add_custom_break(offset);
        }
    }

    pub fn add_empty_line(&mut self, offset: usize) {
        if offset <= self.total_size() {
            *self.empty_lines.entry(offset).or_insert(0) += 1;
            self.cached_line_map.replace(None);
        }
    }

    pub fn remove_empty_line(&mut self, offset: usize) -> bool {
        if let Some(count) = self.empty_lines.get_mut(&offset) {
            if *count > 1 {
                *count -= 1;
            } else {
                self.empty_lines.remove(&offset);
            }
            self.cached_line_map.replace(None);
            true
        } else {
            false
        }
    }

    /// カーソルの現在行と次の行を結合する。
    /// 次の行の開始位置がCustom Breakなら削除し、
    /// 16バイト自然境界ならcustom_joinsに追加して改行を抑制する。
    pub fn join_line(&mut self) {
        let line_starts = self.line_starts();
        let current_line_idx = Self::find_line_index(self.cursor_offset, &line_starts);

        // 次の行がなければ何もしない
        if current_line_idx + 1 >= line_starts.len() {
            return;
        }

        let next_line_start = line_starts.get(current_line_idx + 1).expect("valid next line start");

        if self.custom_breaks.contains(&next_line_start) {
            // Custom Break による改行なら、その break を削除
            self.custom_breaks.remove(&next_line_start);
            self.cached_line_map.replace(None);
        } else if next_line_start != line_starts.get(current_line_idx).unwrap_or(0) {
            // 自然境界（16バイト境界 or カスタム改行後の次行など）を join として記録
            // next_line_start が現在行と同オフセット（空行の重複）でない場合のみ
            self.custom_joins.insert(next_line_start);
            self.cached_line_map.replace(None);
        }
    }

    /// 全ての Custom Break と Join をクリアし、デフォルトの16バイト表示に戻す。
    pub fn clear_all_custom_breaks(&mut self) {
        self.custom_breaks.clear();
        self.custom_joins.clear();
        self.empty_lines.clear();
        self.cached_line_map.replace(None);
    }

    pub fn has_custom_layout(&self) -> bool {
        !self.custom_breaks.is_empty()
            || !self.custom_joins.is_empty()
            || !self.empty_lines.is_empty()
            || (self.show_inline_structure_view && self.parse_result.is_some())
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
        self.custom_breaks.len() + self.custom_joins.len() + self.empty_lines.values().sum::<usize>()
    }

    pub fn prev_search_result(&mut self) -> Option<usize> {
        if self.search_state.results.is_empty() {
            return None;
        }

        let prev_index = if let Some(index) = self.search_state.current_result_index {
            if index == 0 { self.search_state.results.len() - 1 } else { index - 1 }
        } else {
            self.search_state.results.len() - 1
        };

        self.search_state.current_result_index = Some(prev_index);
        Some(self.search_state.results[prev_index])
    }

    pub fn current_search_result(&self) -> Option<usize> {
        self.search_state.current_result_index.and_then(|i| self.search_state.results.get(i).copied())
    }

    pub fn execute_command(&mut self, mut command: Box<dyn Command>) {
        command.execute(self);
        self.document.write().expect("document write lock").history.push(command);
        self.cached_line_map.replace(None);
        self.reparse_structure();
    }

    pub fn undo(&mut self) {
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
            self.cached_line_map.replace(None);
            self.reparse_structure();
        }
    }

    pub fn redo(&mut self) {
        let command = {
            let mut doc = self.document.write().expect("document write lock");
            doc.history.pop_redo()
        };

        if let Some(mut cmd) = command {
            cmd.execute(self);

            // Re-acquire lock to push undo
            self.document.write().expect("document write lock").history.push_undo(cmd);
            self.cached_line_map.replace(None);
            self.reparse_structure();
        }
    }

    pub fn set_kaitai_definition(&mut self, ksy: Arc<crate::core::structure::KsyDefinition>) {
        self.ksy_definition = Some(ksy);
        self.reparse_structure();
    }

    pub fn set_parse_result(&mut self, result: ParseResult) {
        self.parse_progress_offset = result.total_parsed_bytes;
        self.parse_result = Some(Arc::new(result));
        self.cached_line_map.replace(None);
    }

    pub fn set_parse_result_arc(&mut self, result: Arc<ParseResult>) {
        self.parse_progress_offset = result.total_parsed_bytes;
        let old = self.parse_result.replace(result);
        if let Some(old_res) = old {
            std::thread::spawn(move || drop(old_res));
        }
        self.cached_line_map.replace(None);
    }

    pub fn update_parse_progress(&mut self, offset: usize, total_size: usize, intermediate_result: Option<ParseResult>) {
        self.parse_progress_offset = offset;
        self.parse_total_size = total_size;
        if let Some(res) = intermediate_result {
            self.parse_result = Some(Arc::new(res));
            self.cached_line_map.replace(None);
        }
    }

    pub fn invalidate_line_map(&self) {
        self.cached_line_map.replace(None);
    }

    pub fn reparse_structure(&mut self) {
        if let Some(ksy) = &self.ksy_definition {
            let (bytes, ksy_clone) = {
                let buffer_lock = self.document.read().expect("document read lock");
                (buffer_lock.buffer.data().to_vec(), (**ksy).clone())
            };
            let mut stream = crate::core::structure::KaitaiStream::new(&bytes);
            let interpreter = crate::core::structure::KaitaiInterpreter::new(ksy_clone);
            let result = interpreter.parse(&mut stream);
            self.set_parse_result(result);
        }
    }

    pub fn cancel_structure_parsing(&mut self) {
        if let Some(token) = self.parse_cancel_token.take() {
            token.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        self.is_parsing_structure = false;
    }

    pub fn clear_structure_definition(&mut self) {
        self.cancel_structure_parsing();
        self.ksy_definition = None;
        self.parse_result = None;
        self.is_parsing_structure = false;
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
        assert!(editor.selection_start.is_none());
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
        assert_eq!(editor.cursor_offset, 3);
        editor.move_right();
        assert_eq!(editor.cursor_offset, 3);
    }

    #[test]
    fn test_selection() {
        let mut editor = create_editor_with_content(b"12345");

        // Select Right
        editor.select_right();
        assert_eq!(editor.selection_start, Some(0));
        assert_eq!(editor.selection_end, Some(1));
        assert_eq!(editor.cursor_offset, 1);

        // Clear selection on move
        editor.move_right();
        assert!(editor.selection_start.is_none());
        assert!(editor.selection_end.is_none());

        // Select All
        editor.select_all();
        assert_eq!(editor.selection_start, Some(0));
        assert_eq!(editor.selection_end, Some(5)); // Corrected expectation
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
        assert_eq!(editor.empty_lines.get(&10), Some(&1));

        editor.add_empty_line(10);
        assert_eq!(editor.empty_lines.get(&10), Some(&2));

        assert!(editor.remove_empty_line(10));
        assert_eq!(editor.empty_lines.get(&10), Some(&1));

        assert!(editor.remove_empty_line(10));
        assert_eq!(editor.empty_lines.get(&10), None);

        editor.toggle_custom_break(20);
        assert!(editor.custom_breaks.contains(&20));

        editor.toggle_custom_break(20);
        assert!(!editor.custom_breaks.contains(&20));
    }
}
