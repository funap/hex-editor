use crate::actions::{
    AddCustomBreak, ClearAllCustomBreaks, JoinLine, RemoveCustomBreakBackward, RemoveCustomBreakForward, SearchNext, SearchPrev, ToggleSearch,
};
use crate::core::document::Document;
use crate::core::editor::Editor;
use crate::core::encoding::Encoding;
use crate::ui::style::StyleExt as _;
use gpui::prelude::*;
use gpui::*;
use gpui_component::{ActiveTheme, h_flex};
use std::ops::Range;
use std::sync::Arc;

#[allow(dead_code)]
pub enum HexViewEvent {
    Scrolled(usize),
    SelectionChanged { start: Option<usize>, end: Option<usize> },
    CursorMoved(usize),
}

actions!(
    hex_view,
    [
        MoveLeft,
        MoveRight,
        MoveUp,
        MoveDown,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectAll,
        PageUp,
        PageDown,
        Home,
        End,
        SelectPageUp,
        SelectPageDown,
        SelectHome,
        SelectEnd,
        TriggerSearch,
        TriggerSearchNext,
        TriggerSearchPrev
    ]
);

const CONTEXT: &str = "HexView";

// HexView layout constants
pub const HEADER_HEIGHT: f32 = 28.0;
pub const ROW_HEIGHT: f32 = 22.0;
pub const OFFSET_WIDTH: f32 = 80.0;
pub const HEX_BYTE_WIDTH: f32 = 22.0;
pub const HEX_GAP: f32 = 4.0;
pub const SECTION_GAP: f32 = 16.0;

#[inline]
fn format_offset_08(offset: usize) -> SharedString {
    static DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut buf = [b'0'; 8];
    let mut val = offset;
    for i in (0..8).rev() {
        buf[i] = DIGITS[val & 0xf];
        val >>= 4;
    }
    SharedString::from(std::str::from_utf8(&buf).unwrap().to_string())
}

static HEX_STR_TABLE: [&str; 256] = [
    "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "0a", "0b", "0c", "0d", "0e", "0f",
    "10", "11", "12", "13", "14", "15", "16", "17", "18", "19", "1a", "1b", "1c", "1d", "1e", "1f",
    "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "2a", "2b", "2c", "2d", "2e", "2f",
    "30", "31", "32", "33", "34", "35", "36", "37", "38", "39", "3a", "3b", "3c", "3d", "3e", "3f",
    "40", "41", "42", "43", "44", "45", "46", "47", "48", "49", "4a", "4b", "4c", "4d", "4e", "4f",
    "50", "51", "52", "53", "54", "55", "56", "57", "58", "59", "5a", "5b", "5c", "5d", "5e", "5f",
    "60", "61", "62", "63", "64", "65", "66", "67", "68", "69", "6a", "6b", "6c", "6d", "6e", "6f",
    "70", "71", "72", "73", "74", "75", "76", "77", "78", "79", "7a", "7b", "7c", "7d", "7e", "7f",
    "80", "81", "82", "83", "84", "85", "86", "87", "88", "89", "8a", "8b", "8c", "8d", "8e", "8f",
    "90", "91", "92", "93", "94", "95", "96", "97", "98", "99", "9a", "9b", "9c", "9d", "9e", "9f",
    "a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9", "aa", "ab", "ac", "ad", "ae", "af",
    "b0", "b1", "b2", "b3", "b4", "b5", "b6", "b7", "b8", "b9", "ba", "bb", "bc", "bd", "be", "bf",
    "c0", "c1", "c2", "c3", "c4", "c5", "c6", "c7", "c8", "c9", "ca", "cb", "cc", "cd", "ce", "cf",
    "d0", "d1", "d2", "d3", "d4", "d5", "d6", "d7", "d8", "d9", "da", "db", "dc", "dd", "de", "df",
    "e0", "e1", "e2", "e3", "e4", "e5", "e6", "e7", "e8", "e9", "ea", "eb", "ec", "ed", "ee", "ef",
    "f0", "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "fa", "fb", "fc", "fd", "fe", "ff",
];


static HEADER_HEX_LABELS: [&str; 16] = [
    "+0", "+1", "+2", "+3", "+4", "+5", "+6", "+7",
    "+8", "+9", "+A", "+B", "+C", "+D", "+E", "+F",
];

fn row_highlights<'a>(
    highlights: &'a [(Range<usize>, Hsla)],
    max_len: usize,
    offset: usize,
    next_offset: usize,
) -> &'a [(Range<usize>, Hsla)] {
    if highlights.is_empty() {
        return &[];
    }
    let start_search = offset.saturating_sub(max_len);
    let search_start = highlights.partition_point(|(r, _)| r.start < start_search);
    let search_end = highlights.partition_point(|(r, _)| r.start < next_offset);
    &highlights[search_start..search_end]
}

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("left", MoveLeft, Some(CONTEXT)),
        KeyBinding::new("right", MoveRight, Some(CONTEXT)),
        KeyBinding::new("up", MoveUp, Some(CONTEXT)),
        KeyBinding::new("down", MoveDown, Some(CONTEXT)),
        KeyBinding::new("shift-left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("shift-right", SelectRight, Some(CONTEXT)),
        KeyBinding::new("shift-up", SelectUp, Some(CONTEXT)),
        KeyBinding::new("shift-down", SelectDown, Some(CONTEXT)),
        KeyBinding::new("cmd-a", SelectAll, Some(CONTEXT)),
        KeyBinding::new("pageup", PageUp, Some(CONTEXT)),
        KeyBinding::new("pagedown", PageDown, Some(CONTEXT)),
        KeyBinding::new("home", Home, Some(CONTEXT)),
        KeyBinding::new("end", End, Some(CONTEXT)),
        KeyBinding::new("shift-pageup", SelectPageUp, Some(CONTEXT)),
        KeyBinding::new("shift-pagedown", SelectPageDown, Some(CONTEXT)),
        KeyBinding::new("shift-home", SelectHome, Some(CONTEXT)),
        KeyBinding::new("shift-end", SelectEnd, Some(CONTEXT)),
        // Vi-like navigation
        KeyBinding::new("h", MoveLeft, Some(CONTEXT)),
        KeyBinding::new("l", MoveRight, Some(CONTEXT)),
        KeyBinding::new("k", MoveUp, Some(CONTEXT)),
        KeyBinding::new("j", MoveDown, Some(CONTEXT)),
        KeyBinding::new("shift-h", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("shift-l", SelectRight, Some(CONTEXT)),
        KeyBinding::new("shift-k", SelectUp, Some(CONTEXT)),
        KeyBinding::new("shift-j", SelectDown, Some(CONTEXT)),
        // Vi-like search commands
        KeyBinding::new("/", TriggerSearch, Some(CONTEXT)),
        KeyBinding::new("n", TriggerSearchNext, Some(CONTEXT)),
        KeyBinding::new("shift-n", TriggerSearchPrev, Some(CONTEXT)),
        KeyBinding::new("ctrl-f", ToggleSearch, Some(CONTEXT)),
        KeyBinding::new("cmd-f", ToggleSearch, Some(CONTEXT)),
        KeyBinding::new("f3", SearchNext, Some(CONTEXT)),
        KeyBinding::new("ctrl-g", SearchNext, Some(CONTEXT)),
        KeyBinding::new("cmd-g", SearchNext, Some(CONTEXT)),
        KeyBinding::new("shift-f3", SearchPrev, Some(CONTEXT)),
        KeyBinding::new("ctrl-shift-g", SearchPrev, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-g", SearchPrev, Some(CONTEXT)),
        KeyBinding::new("enter", AddCustomBreak, Some(CONTEXT)),
        KeyBinding::new("shift-j", JoinLine, Some(CONTEXT)),
        KeyBinding::new("backspace", RemoveCustomBreakBackward, Some(CONTEXT)),
        KeyBinding::new("delete", RemoveCustomBreakForward, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-backspace", ClearAllCustomBreaks, Some(CONTEXT)),
    ]);
}

use gpui_component::scroll::{Scrollbar, ScrollbarAxis};

pub struct HexView {
    editor: Entity<Editor>,
    focus_handle: FocusHandle,
    uniform_scroll_handle: UniformListScrollHandle,
    scroll_offset: usize,
    is_selecting: bool,
    bounds: std::cell::Cell<Option<Bounds<Pixels>>>,
    visible_row_info: std::cell::Cell<Option<(usize, Pixels, Pixels)>>,
    highlights: Arc<Vec<(Range<usize>, Hsla)>>,
    max_highlight_len: usize,
    show_offset: bool,
    show_header: bool,
    show_ascii: bool,
    encoding: Encoding,
    font_family_prop: SharedString,
    font_size_prop: Pixels,
    _editor_subscription: Subscription,
}

impl EventEmitter<HexViewEvent> for HexView {}

#[allow(dead_code)]
impl HexView {
    pub fn new(editor: Entity<Editor>, cx: &mut Context<Self>) -> Self {
        let _editor_subscription = cx.observe(&editor, |this, editor_entity, cx| {
            let new_encoding = editor_entity.read(cx).encoding;
            if this.encoding != new_encoding {
                this.encoding = new_encoding;
            }
            this.ensure_cursor_visible(cx);
            cx.notify();
        });

        Self {
            editor,
            focus_handle: cx.focus_handle(),
            uniform_scroll_handle: UniformListScrollHandle::new(),
            scroll_offset: 0,
            is_selecting: false,
            bounds: std::cell::Cell::new(None),
            visible_row_info: std::cell::Cell::new(None),
            highlights: Arc::new(Vec::new()),
            max_highlight_len: 0,
            show_offset: true,
            show_header: true,
            show_ascii: true,
            encoding: Encoding::Ascii,
            font_family_prop: "Zed Sans Mono".into(),
            font_size_prop: px(14.0),
            _editor_subscription,
        }
    }

    pub fn font_family(mut self, font_family: impl Into<SharedString>) -> Self {
        self.font_family_prop = font_family.into();
        self
    }

    pub fn font_size(mut self, font_size: impl Into<Pixels>) -> Self {
        self.font_size_prop = font_size.into();
        self
    }

    pub fn set_font_family(&mut self, font_family: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.font_family_prop = font_family.into();
        cx.notify();
    }

    pub fn set_font_size(&mut self, font_size: impl Into<Pixels>, cx: &mut Context<Self>) {
        self.font_size_prop = font_size.into();
        cx.notify();
    }

    pub fn with_offset(mut self, show: bool) -> Self {
        self.show_offset = show;
        self
    }

    pub fn with_header(mut self, show: bool) -> Self {
        self.show_header = show;
        self
    }

    pub fn with_ascii(mut self, show: bool) -> Self {
        self.show_ascii = show;
        self
    }

    pub fn set_show_offset(&mut self, show: bool, cx: &mut Context<Self>) {
        self.show_offset = show;
        cx.notify();
    }

    pub fn set_show_header(&mut self, show: bool, cx: &mut Context<Self>) {
        self.show_header = show;
        cx.notify();
    }

    pub fn set_show_ascii(&mut self, show: bool, cx: &mut Context<Self>) {
        self.show_ascii = show;
        cx.notify();
    }

    pub fn set_encoding(&mut self, encoding: Encoding, cx: &mut Context<Self>) {
        self.encoding = encoding;
        cx.notify();
    }

    pub fn encoding(&self) -> Encoding {
        self.encoding
    }


    pub fn set_highlights(&mut self, mut highlights: Vec<(Range<usize>, Hsla)>, cx: &mut Context<Self>) {
        highlights.sort_by_key(|(range, _)| range.start);
        self.max_highlight_len = highlights.iter().map(|(r, _)| r.end.saturating_sub(r.start)).max().unwrap_or(0);
        self.highlights = Arc::new(highlights);
        cx.notify();
    }

    pub fn set_highlight_ranges(&mut self, ranges: Vec<Range<usize>>, cx: &mut Context<Self>) {
        let highlight_color = cx.theme().accent;
        let highlights: Vec<_> = ranges.into_iter().map(|range| (range, highlight_color)).collect();
        self.set_highlights(highlights, cx);
    }

    pub fn scroll_to_byte(&mut self, byte_offset: usize, cx: &mut Context<Self>) {
        let line_starts = self.editor.read(cx).line_starts();
        let row = Editor::find_line_index(byte_offset, &line_starts);
        self.scroll_to_row(row, cx);
    }

    pub fn current_scroll_top_row(&self) -> usize {
        if let Some((sample_row_idx, sample_top, _)) = self.visible_row_info.get() {
            let root_top = self.bounds.get().map(|b| b.top()).unwrap_or(px(0.0));
            let header_h = if self.show_header { HEADER_HEIGHT } else { 0.0 };
            let list_top = root_top + px(header_h);

            let row_0_top = sample_top - px(sample_row_idx as f32 * ROW_HEIGHT);
            (f32::from((list_top - row_0_top).max(px(0.0))) / ROW_HEIGHT) as usize
        } else {
            0
        }
    }

    pub fn viewport_byte_range(&self, cx: &App) -> (usize, usize) {
        let editor = self.editor.read(cx);
        let line_starts = editor.line_starts();
        let current_top = self.current_scroll_top_row();
        let start_byte = line_starts.get(current_top).unwrap_or(0);
        let end_row = (current_top + 30).min(line_starts.len());
        let end_byte = if end_row < line_starts.len() {
            line_starts.get(end_row).unwrap()
        } else {
            editor.total_size()
        };
        (start_byte, end_byte)
    }

    pub fn scroll_to_row(&mut self, row: usize, cx: &mut Context<Self>) {
        let total_rows = self.editor.read(cx).line_starts().len();
        let max_offset = total_rows.saturating_sub(1);
        let new_offset = row.min(max_offset);

        let current_top = self.current_scroll_top_row();
        if current_top == new_offset {
            return;
        }

        self.scroll_offset = new_offset;
        self.uniform_scroll_handle.scroll_to_item(new_offset, ScrollStrategy::Top);
        cx.notify();
        cx.emit(HexViewEvent::Scrolled(self.scroll_offset));
    }

    fn ensure_cursor_visible(&mut self, cx: &mut Context<Self>) {
        let editor = self.editor.read(cx);
        let cursor_offset = editor.cursor_offset;
        let line_starts = editor.line_starts();
        let cursor_row = Editor::find_line_index(cursor_offset, &line_starts);

        let viewport_height = self.bounds.get().map(|b| f32::from(b.size.height)).unwrap_or(500.0);
        let header_h = if self.show_header { HEADER_HEIGHT } else { 0.0 };
        let visible_rows = (((viewport_height - header_h) / ROW_HEIGHT).floor() as usize).max(1);

        let top_row = self.current_scroll_top_row();
        if cursor_row < top_row {
            self.scroll_to_row(cursor_row, cx);
        } else if cursor_row >= top_row + visible_rows {
            let target_row = cursor_row.saturating_sub(visible_rows.saturating_sub(1));
            self.scroll_to_row(target_row, cx);
        }
    }

    fn exec_move(&mut self, window: &mut Window, cx: &mut Context<Self>, f: impl FnOnce(&mut Editor)) {
        cx.focus_self(window);
        self.editor.update(cx, |editor, cx| {
            f(editor);
            cx.notify();
        });
        let cursor_offset = self.editor.read(cx).cursor_offset;
        cx.emit(HexViewEvent::CursorMoved(cursor_offset));
    }

    fn exec_select(&mut self, window: &mut Window, cx: &mut Context<Self>, f: impl FnOnce(&mut Editor)) {
        cx.focus_self(window);
        self.editor.update(cx, |editor, cx| {
            f(editor);
            cx.notify();
        });
        let (start, end) = {
            let editor = self.editor.read(cx);
            (editor.selection_start, editor.selection_end)
        };
        cx.emit(HexViewEvent::SelectionChanged { start, end });
    }

    fn move_left(&mut self, _: &MoveLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.exec_move(window, cx, |e| e.move_left());
    }

    fn move_right(&mut self, _: &MoveRight, window: &mut Window, cx: &mut Context<Self>) {
        self.exec_move(window, cx, |e| e.move_right());
    }

    fn move_up(&mut self, _: &MoveUp, window: &mut Window, cx: &mut Context<Self>) {
        self.exec_move(window, cx, |e| e.move_up());
    }

    fn move_down(&mut self, _: &MoveDown, window: &mut Window, cx: &mut Context<Self>) {
        self.exec_move(window, cx, |e| e.move_down());
    }

    fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.exec_select(window, cx, |e| e.select_left());
    }

    fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.exec_select(window, cx, |e| e.select_right());
    }

    fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        self.exec_select(window, cx, |e| e.select_up());
    }

    fn select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        self.exec_select(window, cx, |e| e.select_down());
    }

    fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        self.exec_select(window, cx, |e| e.select_all());
    }

    fn page_up(&mut self, _: &PageUp, window: &mut Window, cx: &mut Context<Self>) {
        self.exec_move(window, cx, |e| {
            for _ in 0..10 {
                e.move_up();
            }
        });
    }

    fn page_down(&mut self, _: &PageDown, window: &mut Window, cx: &mut Context<Self>) {
        self.exec_move(window, cx, |e| {
            for _ in 0..10 {
                e.move_down();
            }
        });
    }

    fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        self.exec_move(window, cx, |e| e.set_cursor_offset(0));
    }

    fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        self.exec_move(window, cx, |e| {
            let total = e.total_size();
            e.set_cursor_offset(total.saturating_sub(1));
        });
    }

    fn select_page_up(&mut self, _: &SelectPageUp, window: &mut Window, cx: &mut Context<Self>) {
        self.exec_select(window, cx, |e| {
            for _ in 0..10 {
                e.select_up();
            }
        });
    }

    fn select_page_down(&mut self, _: &SelectPageDown, window: &mut Window, cx: &mut Context<Self>) {
        self.exec_select(window, cx, |e| {
            for _ in 0..10 {
                e.select_down();
            }
        });
    }

    fn select_home(&mut self, _: &SelectHome, window: &mut Window, cx: &mut Context<Self>) {
        self.exec_select(window, cx, |e| {
            let curr = e.cursor_offset;
            if e.selection_start.is_none() {
                e.selection_start = Some(curr);
            }
            e.selection_end = Some(0);
            e.cursor_offset = 0;
        });
    }

    fn select_end(&mut self, _: &SelectEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.exec_select(window, cx, |e| {
            let total = e.total_size();
            let end_pos = total.saturating_sub(1);
            let curr = e.cursor_offset;
            if e.selection_start.is_none() {
                e.selection_start = Some(curr);
            }
            e.selection_end = Some(end_pos);
            e.cursor_offset = end_pos;
        });
    }

    fn trigger_search(&mut self, _: &TriggerSearch, _window: &mut Window, cx: &mut Context<Self>) {
        cx.dispatch_action(&ToggleSearch);
    }

    fn add_custom_break(&mut self, _: &AddCustomBreak, window: &mut Window, cx: &mut Context<Self>) {
        cx.focus_self(window);
        self.editor.update(cx, |editor, cx| {
            let offset = editor.cursor_offset;
            if offset > 0 {
                editor.add_custom_break(offset);
            }
            cx.notify();
        });
    }

    fn remove_custom_break_backward(&mut self, _: &RemoveCustomBreakBackward, window: &mut Window, cx: &mut Context<Self>) {
        cx.focus_self(window);
        self.editor.update(cx, |editor, cx| {
            let offset = editor.cursor_offset;
            if offset > 0 && editor.custom_breaks.contains(&(offset - 1)) {
                editor.remove_custom_break(offset - 1);
            }
            cx.notify();
        });
    }

    fn remove_custom_break_forward(&mut self, _: &RemoveCustomBreakForward, window: &mut Window, cx: &mut Context<Self>) {
        cx.focus_self(window);
        self.editor.update(cx, |editor, cx| {
            let offset = editor.cursor_offset;
            if editor.custom_breaks.contains(&offset) {
                editor.remove_custom_break(offset);
            }
            cx.notify();
        });
    }

    fn join_line(&mut self, _: &JoinLine, window: &mut Window, cx: &mut Context<Self>) {
        cx.focus_self(window);
        self.editor.update(cx, |editor, cx| {
            editor.join_line();
            cx.notify();
        });
    }

    fn clear_all_custom_breaks(&mut self, _: &ClearAllCustomBreaks, window: &mut Window, cx: &mut Context<Self>) {
        cx.focus_self(window);
        self.editor.update(cx, |editor, cx| {
            editor.clear_all_custom_breaks();
            cx.notify();
        });
    }

    fn offset_from_point(&self, point: Point<Pixels>, cx: &App) -> Option<usize> {
        let root_bounds = self.bounds.get()?;
        let header_h = if self.show_header { HEADER_HEIGHT } else { 0.0 };

        if point.x < root_bounds.left() || point.y < root_bounds.top() + px(header_h) {
            return None;
        }

        let (sample_row_idx, sample_top, sample_left) = self.visible_row_info.get()?;

        let editor = self.editor.read(cx);
        let doc = editor.document.read().ok()?;
        let buffer_len = doc.buffer.len();
        if buffer_len == 0 {
            return Some(0);
        }
        let line_starts = editor.line_starts();
        if line_starts.is_empty() {
            return Some(0);
        }

        let row_0_top = sample_top - px(sample_row_idx as f32 * ROW_HEIGHT);
        let rel_y = f32::from((point.y - row_0_top).max(px(0.0)));
        let row_idx = (rel_y / ROW_HEIGHT) as usize;

        let row_idx = row_idx.min(line_starts.len().saturating_sub(1));
        let line_offset = line_starts.get(row_idx)?;

        let next_offset = if row_idx + 1 < line_starts.len() {
            line_starts.get(row_idx + 1).unwrap_or(buffer_len)
        } else {
            buffer_len
        };
        let chunk_len = next_offset.saturating_sub(line_offset);
        if chunk_len == 0 {
            return Some(line_offset);
        }

        let max_bytes_per_row = line_starts.max_bytes_per_row();
        let offset_width = if self.show_offset { OFFSET_WIDTH } else { 0.0 };
        let hex_start_x = f32::from(sample_left) + offset_width + 8.0;
        let hex_end_x = hex_start_x + (max_bytes_per_row as f32 * (HEX_BYTE_WIDTH + HEX_GAP));
        let ascii_start_x = hex_end_x + SECTION_GAP;
        let rel_x = f32::from(point.x);

        let col_idx = if self.show_ascii && rel_x >= ascii_start_x {
            let ascii_x = (rel_x - ascii_start_x).max(0.0);
            (ascii_x / 10.0) as usize
        } else {
            let col_x = (rel_x - hex_start_x).max(0.0);
            (col_x / (HEX_BYTE_WIDTH + HEX_GAP)) as usize
        };

        let byte_idx = col_idx.min(chunk_len.saturating_sub(1));
        Some(line_offset + byte_idx)
    }

    fn render_hex_row(
        row_idx: usize,
        doc: &Document,
        line_starts: &crate::core::editor::LineMap,
        max_bytes_per_row: usize,
        encoding: Encoding,
        cursor_offset: usize,
        min_sel: usize,
        max_sel: usize,
        highlights: &Arc<Vec<(Range<usize>, Hsla)>>,
        max_highlight_len: usize,
        show_offset: bool,
        show_ascii: bool,
        is_focused: bool,
        _font_family: &SharedString,
        view: Entity<Self>,
        _focus_handle: &FocusHandle,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let offset = match line_starts.get(row_idx) {
            Some(o) => o,
            None => return div().into_any_element(),
        };
        let next_offset = if row_idx + 1 < line_starts.len() {
            line_starts.get(row_idx + 1).unwrap_or(doc.buffer.len())
        } else {
            doc.buffer.len()
        };

        let chunk_len = next_offset - offset;
        let chunk = doc.buffer.get_range(offset, chunk_len).to_vec();
        let offset_str = format_offset_08(offset);

        let selection_bg = if is_focused { theme.selection } else { theme.muted_foreground.opacity(0.3) };
        let cursor_bg = theme.accent;
        let muted_color = theme.muted_foreground;
        let fg_color = theme.foreground;
        let accent_fg_color = theme.accent_foreground;

        let active_row_highlights = row_highlights(highlights, max_highlight_len, offset, next_offset).to_vec();

        let visible_row_view = view.clone();
        div()
            .id(row_idx)
            .w_full()
            .h(px(ROW_HEIGHT))
            .child(
                canvas(
                    move |bounds, _window, cx| {
                        visible_row_view.update(cx, |this, _cx| {
                            this.visible_row_info.set(Some((row_idx, bounds.top(), bounds.left())));
                        });
                    },
                    move |bounds, _prepaint, window, cx| {
                        let line_height = px(ROW_HEIGHT);
                        let font_size = px(13.0);
                        let font = window.text_style().font();

                        // 1. Draw Offset Column
                        if show_offset {
                            let run = gpui::TextRun {
                                len: offset_str.len(),
                                font: font.clone(),
                                color: muted_color,
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            };
                            let shaped = window.text_system().shape_line(offset_str.clone(), font_size, &[run], None);
                            let offset_pos = point(bounds.left() + px(8.0), bounds.top() + px(2.0));
                            let _ = shaped.paint(offset_pos, line_height, window, cx);
                        }

                        let offset_w = if show_offset { OFFSET_WIDTH } else { 0.0 };
                        let hex_start_x = bounds.left() + px(offset_w + 8.0);
                        let ascii_start_x = hex_start_x + px(max_bytes_per_row as f32 * (HEX_BYTE_WIDTH + HEX_GAP) + SECTION_GAP);

                        // 2. Background Quads Pass
                        for (j, &byte_val) in chunk.iter().enumerate() {
                            let byte_pos = offset + j;
                            let is_cursor = byte_pos == cursor_offset;
                            let is_selected = byte_pos >= min_sel && byte_pos <= max_sel;

                            let mut bg_color = if is_cursor {
                                cursor_bg
                            } else if is_selected {
                                selection_bg
                            } else {
                                hsla(0.0, 0.0, 0.0, 0.0)
                            };

                            if !is_cursor && !is_selected && !active_row_highlights.is_empty() {
                                let mut smallest_len = usize::MAX;
                                for (range, color) in active_row_highlights.iter() {
                                    if range.contains(&byte_pos) {
                                        let len = range.end.saturating_sub(range.start);
                                        if len <= smallest_len {
                                            smallest_len = len;
                                            bg_color = *color;
                                        }
                                    }
                                }
                            }

                            if bg_color.a > 0.0 {
                                let hex_bg_bounds = Bounds::new(
                                    point(hex_start_x + px(j as f32 * (HEX_BYTE_WIDTH + HEX_GAP)), bounds.top()),
                                    size(px(HEX_BYTE_WIDTH + HEX_GAP), px(ROW_HEIGHT)),
                                );
                                window.paint_quad(gpui::fill(hex_bg_bounds, bg_color));

                                if show_ascii {
                                    let ascii_bg_bounds = Bounds::new(
                                        point(ascii_start_x + px(j as f32 * 10.0), bounds.top()),
                                        size(px(10.0), px(ROW_HEIGHT)),
                                    );
                                    window.paint_quad(gpui::fill(ascii_bg_bounds, bg_color));
                                }
                            }

                            let _ = byte_val;
                        }

                        // 3. Text Pass
                        // Build a decoded char map for ASCII column: chunk index -> Option<(char, span_len)>
                        // For multi-byte encodings, each start byte carries the char and span;
                        // continuation bytes carry None so we skip them.
                        let char_map: Vec<Option<(char, usize)>> = {
                            let mut map = vec![None; chunk.len()];
                            let mut j = 0;
                            while j < chunk.len() {
                                if let Some((c, byte_len)) = encoding.decode_char_at(&chunk, j) {
                                    map[j] = Some((c, byte_len));
                                    j += byte_len.max(1);
                                } else {
                                    j += 1;
                                }
                            }
                            map
                        };

                        for (j, &byte) in chunk.iter().enumerate() {
                            let byte_pos = offset + j;
                            let is_cursor = byte_pos == cursor_offset;

                            let text_color = if is_cursor {
                                accent_fg_color
                            } else if byte == 0 {
                                muted_color.opacity(0.5)
                            } else {
                                fg_color
                            };

                            let hex_str = SharedString::from(HEX_STR_TABLE[byte as usize]);
                            let run = gpui::TextRun {
                                len: hex_str.len(),
                                font: font.clone(),
                                color: text_color,
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            };
                            let shaped_hex = window.text_system().shape_line(hex_str, font_size, &[run], None);
                            let hex_pos = point(hex_start_x + px(j as f32 * (HEX_BYTE_WIDTH + HEX_GAP) + 2.0), bounds.top() + px(2.0));
                            let _ = shaped_hex.paint(hex_pos, line_height, window, cx);

                            if show_ascii {
                                if let Some((ch, _span)) = char_map[j] {
                                    let s = SharedString::from(ch.to_string());
                                    let run = gpui::TextRun {
                                        len: s.len(),
                                        font: font.clone(),
                                        color: text_color,
                                        background_color: None,
                                        underline: None,
                                        strikethrough: None,
                                    };
                                    let shaped_ascii = window.text_system().shape_line(s, font_size, &[run], None);
                                    let ascii_pos = point(ascii_start_x + px(j as f32 * 10.0 + 1.0), bounds.top() + px(2.0));
                                    let _ = shaped_ascii.paint(ascii_pos, line_height, window, cx);
                                }
                                // continuation bytes: no character drawn (already covered by the start byte's char)
                            }
                        }
                    },
                ),
            )
            .into_any_element()
    }
}

impl Focusable for HexView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for HexView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity().clone();
        let is_focused = self.focus_handle.is_focused(window);
        let theme = cx.theme();
        let font_family = self.font_family_prop.clone();

        let (total_rows, max_bytes_per_row) = {
            let editor = self.editor.read(cx);
            let line_starts = editor.line_starts();
            (line_starts.len().max(1), line_starts.max_bytes_per_row())
        };

        let total_width = {
            let mut w = 8.0;
            if self.show_offset {
                w += OFFSET_WIDTH;
            }
            w += max_bytes_per_row as f32 * (HEX_BYTE_WIDTH + HEX_GAP);
            if self.show_ascii {
                w += SECTION_GAP + (max_bytes_per_row as f32 * 10.0);
            }
            w + 16.0
        };

        let total_height = total_rows as f32 * ROW_HEIGHT;

        let container = div()
            .flex()
            .flex_col()
            .bg(theme.background)
            .font_family(font_family.clone())
            .size_full()
            .key_context(CONTEXT);

        let container = container.focus_indicator(is_focused, theme);

        let header = if self.show_header {
            let mut hex_cols = Vec::with_capacity(max_bytes_per_row);
            for i in 0..max_bytes_per_row {
                let label = if i < 16 {
                    SharedString::from(HEADER_HEX_LABELS[i])
                } else {
                    SharedString::from(format!("+{:X}", i))
                };
                hex_cols.push(
                    div()
                        .w(px(HEX_BYTE_WIDTH + HEX_GAP))
                        .text_center()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(label),
                );
            }

            div()
                .flex()
                .flex_row()
                .items_center()
                .h(px(HEADER_HEIGHT))
                .bg(theme.sidebar)
                .border_b_1()
                .border_color(theme.border)
                .font_family(font_family.clone())
                .px_2()
                .child(if self.show_offset {
                    div().w(px(OFFSET_WIDTH)).text_xs().text_color(theme.muted_foreground).child("Offset")
                } else {
                    div()
                })
                .child(h_flex().children(hex_cols))
                .child(if self.show_ascii {
                    let label = match self.encoding {
                        Encoding::Ascii => "ASCII",
                        Encoding::Utf8 => "UTF-8",
                        Encoding::Utf16Le => "UTF-16 LE",
                        Encoding::Utf16Be => "UTF-16 BE",
                    };
                    div().ml(px(SECTION_GAP)).text_xs().text_color(theme.muted_foreground).child(label)
                } else {
                    div()
                })
                .into_any_element()
        } else {
            div().into_any_element()
        };

        let focus_handle = self.focus_handle.clone();
        let show_offset = self.show_offset;
        let show_ascii = self.show_ascii;
        let encoding = self.encoding;
        let highlights = self.highlights.clone();
        let max_highlight_len = self.max_highlight_len;

        let bounds_view = view.clone();

        container
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::move_left))
            .on_action(cx.listener(Self::move_right))
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::page_up))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_page_up))
            .on_action(cx.listener(Self::select_page_down))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::trigger_search))
            .on_action(cx.listener(Self::add_custom_break))
            .on_action(cx.listener(Self::remove_custom_break_backward))
            .on_action(cx.listener(Self::remove_custom_break_forward))
            .on_action(cx.listener(Self::join_line))
            .on_action(cx.listener(Self::clear_all_custom_breaks))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.focus_handle.focus(window);
                    if let Some(target_pos) = this.offset_from_point(event.position, cx) {
                        this.is_selecting = true;
                        this.editor.update(cx, |editor, cx| {
                            if event.modifiers.shift {
                                if editor.selection_start.is_none() {
                                    editor.selection_start = Some(editor.cursor_offset);
                                }
                                editor.selection_end = Some(target_pos);
                            } else {
                                editor.start_drag(target_pos);
                            }
                            cx.notify();
                        });
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                if this.is_selecting {
                    if let Some(target_pos) = this.offset_from_point(event.position, cx) {
                        this.editor.update(cx, |editor, cx| {
                            let prev_end = editor.selection_end;
                            if prev_end != Some(target_pos) {
                                editor.continue_drag(target_pos);
                                cx.notify();
                            }
                        });
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                    if this.is_selecting {
                        this.is_selecting = false;
                        let (start, end, cursor_offset) = {
                            let ed = this.editor.read(cx);
                            (ed.selection_start, ed.selection_end, ed.cursor_offset)
                        };
                        cx.emit(HexViewEvent::SelectionChanged { start, end });
                        cx.emit(HexViewEvent::CursorMoved(cursor_offset));
                    }
                }),
            )
            .child(
                canvas(
                    move |bounds, _window, cx| {
                        bounds_view.update(cx, |this, _cx| {
                            this.bounds.set(Some(bounds));
                        });
                    },
                    |_bounds, _prepaint, _window, _cx| {},
                )
                .absolute()
                .inset_0(),
            )
            .child(header)
            .child(
                div()
                    .relative()
                    .size_full()
                    .child(
                        uniform_list("hex-view-list", total_rows, move |range, _window, cx| {
                            let view_read = view.read(cx);
                            let editor = view_read.editor.read(cx);
                            let doc = editor.document.read().unwrap();
                            let line_starts = editor.line_starts();
                            let cursor_offset = editor.cursor_offset;
                            let (min_sel, max_sel) = if let (Some(s), Some(e)) = (editor.selection_start, editor.selection_end) {
                                if s <= e { (s, e) } else { (e, s) }
                            } else {
                                (usize::MAX, usize::MIN)
                            };

                            range
                                .map(|row_idx| {
                                    Self::render_hex_row(
                                        row_idx,
                                        &doc,
                                        &line_starts,
                                        max_bytes_per_row,
                                        encoding,
                                        cursor_offset,
                                        min_sel,
                                        max_sel,
                                        &highlights,
                                        max_highlight_len,
                                        show_offset,
                                        show_ascii,
                                        is_focused,
                                        &font_family,
                                        view.clone(),
                                        &focus_handle,
                                        cx,
                                    )
                                })
                                .collect::<Vec<_>>()
                        })
                        .track_scroll(self.uniform_scroll_handle.clone())
                        .size_full(),
                    )
                    .child(
                        div().absolute().top_0().right_0().bottom_0().w_3().child(
                            Scrollbar::vertical(&self.uniform_scroll_handle)
                                .axis(ScrollbarAxis::Vertical)
                                .scroll_size(size(px(total_width), px(total_height))),
                        ),
                    )
                    .child(
                        div().absolute().bottom_0().left_0().right_0().h_3().child(
                            Scrollbar::horizontal(&self.uniform_scroll_handle)
                                .axis(ScrollbarAxis::Horizontal)
                                .scroll_size(size(px(total_width), px(total_height))),
                        ),
                    ),
            )
    }
}
