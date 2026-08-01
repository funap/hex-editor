use crate::actions::{
    AddCustomBreak, ClearAllCustomBreaks, JoinLine, RemoveCustomBreakBackward, RemoveCustomBreakForward, SearchNext, SearchPrev, ToggleSearch,
};
use crate::core::document::Document;
use crate::core::editor::Editor;
use crate::core::encoding::Encoding;
use crate::ui::style::StyleExt as _;
use gpui::prelude::*;
use gpui::*;
use gpui::{ScrollWheelEvent, WeakEntity};
use gpui_component::scroll::*;
use gpui_component::{ActiveTheme, PixelsExt};
use std::cmp;
use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::Arc;
use std::sync::RwLock;

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
pub const HEADER_HEIGHT: f32 = 32.0;
pub const ROW_HEIGHT: f32 = 24.0;
pub const OFFSET_WIDTH: f32 = 80.0;
pub const HEX_BYTE_WIDTH: f32 = 22.0;
pub const HEX_GAP: f32 = 4.0;
pub const SECTION_GAP: f32 = 16.0;

const HEX_STR_LUT: [&str; 256] = [
    "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "0a", "0b", "0c", "0d", "0e", "0f", "10", "11", "12", "13", "14", "15", "16", "17", "18", "19",
    "1a", "1b", "1c", "1d", "1e", "1f", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "2a", "2b", "2c", "2d", "2e", "2f", "30", "31", "32", "33",
    "34", "35", "36", "37", "38", "39", "3a", "3b", "3c", "3d", "3e", "3f", "40", "41", "42", "43", "44", "45", "46", "47", "48", "49", "4a", "4b", "4c", "4d",
    "4e", "4f", "50", "51", "52", "53", "54", "55", "56", "57", "58", "59", "5a", "5b", "5c", "5d", "5e", "5f", "60", "61", "62", "63", "64", "65", "66", "67",
    "68", "69", "6a", "6b", "6c", "6d", "6e", "6f", "70", "71", "72", "73", "74", "75", "76", "77", "78", "79", "7a", "7b", "7c", "7d", "7e", "7f", "80", "81",
    "82", "83", "84", "85", "86", "87", "88", "89", "8a", "8b", "8c", "8d", "8e", "8f", "90", "91", "92", "93", "94", "95", "96", "97", "98", "99", "9a", "9b",
    "9c", "9d", "9e", "9f", "a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9", "aa", "ab", "ac", "ad", "ae", "af", "b0", "b1", "b2", "b3", "b4", "b5",
    "b6", "b7", "b8", "b9", "ba", "bb", "bc", "bd", "be", "bf", "c0", "c1", "c2", "c3", "c4", "c5", "c6", "c7", "c8", "c9", "ca", "cb", "cc", "cd", "ce", "cf",
    "d0", "d1", "d2", "d3", "d4", "d5", "d6", "d7", "d8", "d9", "da", "db", "dc", "dd", "de", "df", "e0", "e1", "e2", "e3", "e4", "e5", "e6", "e7", "e8", "e9",
    "ea", "eb", "ec", "ed", "ee", "ef", "f0", "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "fa", "fb", "fc", "fd", "fe", "ff",
];

const ASCII_STR_LUT: [&str; 128] = [
    ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".", ".",
    ".", " ", "!", "\"", "#", "$", "%", "&", "'", "(", ")", "*", "+", ",", "-", ".", "/", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", ":", ";", "<", "=",
    ">", "?", "@", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "[", "\\",
    "]", "^", "_", "`", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", "{",
    "|", "}", "~", ".",
];

const HEADER_COL_LUT: [&str; 32] = [
    "+0", "+1", "+2", "+3", "+4", "+5", "+6", "+7", "+8", "+9", "+A", "+B", "+C", "+D", "+E", "+F", "+10", "+11", "+12", "+13", "+14", "+15", "+16", "+17",
    "+18", "+19", "+1A", "+1B", "+1C", "+1D", "+1E", "+1F",
];

const HEX_DIGITS_LOWER: &[u8; 16] = b"0123456789abcdef";

#[inline]
fn format_offset_08(offset: usize) -> SharedString {
    let mut buf = [b'0'; 8];
    let mut v = offset;
    for i in (0..8).rev() {
        buf[i] = HEX_DIGITS_LOWER[v & 0xF];
        v >>= 4;
    }
    SharedString::from(String::from_utf8(buf.to_vec()).unwrap())
}

pub const OFFSET_X_START: f32 = 4.0;
pub const SELECTION_PADDING: f32 = 2.0;
// pub const FONT_FAMILY: &str = "Menlo"; // Removed in favor of global Appearance

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
        // Vi-like search commands (only when HexView is focused)
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

#[allow(dead_code)]
pub struct HexView {
    editor: Entity<Editor>,
    focus_handle: FocusHandle,
    is_dragging: bool,
    last_bounds: std::cell::Cell<Option<Bounds<Pixels>>>,
    last_ascii_layout: std::cell::Cell<Option<(Pixels, Pixels)>>, // (ascii_start_x, ascii_char_width)
    scroll_offset: usize,
    scroll_remainder: f32,
    scroll_handle: ScrollHandle,
    horizontal_scroll_handle: ScrollHandle,
    scroll_offset_x: Pixels,
    highlights: Arc<Vec<(Range<usize>, Hsla)>>,
    max_highlight_len: usize,
    show_offset: bool,
    show_header: bool,
    show_ascii: bool,
    font_family_prop: SharedString,
    font_size_prop: Pixels,
    _editor_subscription: Subscription,
}

impl EventEmitter<HexViewEvent> for HexView {}

#[allow(dead_code)]
impl HexView {
    pub fn new(editor: Entity<Editor>, cx: &mut Context<Self>) -> Self {
        let _editor_subscription = cx.observe(&editor, |this, _, cx| {
            this.ensure_cursor_visible(cx);
            cx.notify();
        });

        Self {
            editor,
            focus_handle: cx.focus_handle(),
            is_dragging: false,
            last_bounds: std::cell::Cell::new(None),
            last_ascii_layout: std::cell::Cell::new(None),
            scroll_offset: 0,
            scroll_remainder: 0.0,
            scroll_handle: ScrollHandle::new(),
            horizontal_scroll_handle: ScrollHandle::new(),
            scroll_offset_x: px(0.),
            highlights: Arc::new(Vec::new()),
            max_highlight_len: 0,
            show_offset: true,
            show_header: true,
            show_ascii: true,
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

    pub fn set_highlights(&mut self, mut highlights: Vec<(Range<usize>, Hsla)>, cx: &mut Context<Self>) {
        highlights.sort_by_key(|(range, _)| range.start);
        self.max_highlight_len = highlights.iter().map(|(r, _)| r.end.saturating_sub(r.start)).max().unwrap_or(0);
        self.highlights = Arc::new(highlights);
        cx.notify();
    }

    pub fn set_highlight_ranges(&mut self, ranges: Vec<Range<usize>>, cx: &mut Context<Self>) {
        let theme = cx.theme();
        let highlight_color = theme.accent;
        let mut highlights: Vec<_> = ranges.into_iter().map(|range| (range, highlight_color)).collect();
        highlights.sort_by_key(|(range, _)| range.start);
        self.max_highlight_len = highlights.iter().map(|(r, _)| r.end.saturating_sub(r.start)).max().unwrap_or(0);
        self.highlights = Arc::new(highlights);
        cx.notify();
    }

    pub fn scroll_to_byte(&mut self, byte_offset: usize, cx: &mut Context<Self>) {
        let line_starts = self.editor.read(cx).line_starts();
        let row = Editor::find_line_index(byte_offset, &line_starts);
        self.scroll_to_row(row, cx);
    }

    /// Returns the byte range of the current viewport (visible area).
    /// Returns (start_byte, end_byte) where end_byte is exclusive.
    pub fn viewport_byte_range(&self, cx: &App) -> (usize, usize) {
        let editor = self.editor.read(cx);
        let line_starts = editor.line_starts();
        let start_byte = line_starts.get(self.scroll_offset).unwrap_or(0);
        let visible_rows = self.get_visible_rows();
        let end_row = (self.scroll_offset + visible_rows).min(line_starts.len());
        let end_byte = if end_row < line_starts.len() {
            line_starts.get(end_row).unwrap()
        } else {
            editor.total_size()
        };
        (start_byte, end_byte)
    }

    pub fn scroll_to_row(&mut self, row: usize, cx: &mut Context<Self>) {
        let row_height = px(ROW_HEIGHT);
        let total_rows = self.editor.read(cx).line_starts().len();
        let max_offset = total_rows.saturating_sub(1);
        let new_offset = row.min(max_offset);

        if self.scroll_offset == new_offset {
            return;
        }

        self.scroll_offset = new_offset;
        self.scroll_handle.set_offset(point(px(0.), -(self.scroll_offset as f32 * row_height)));
        cx.notify();
        cx.emit(HexViewEvent::Scrolled(self.scroll_offset));
    }

    fn ensure_cursor_visible(&mut self, cx: &mut Context<Self>) {
        let bounds = match self.last_bounds.get() {
            Some(b) => b,
            None => return,
        };

        let header_height = px(HEADER_HEIGHT);
        let row_height = px(ROW_HEIGHT);
        let visible_height = bounds.size.height - header_height;
        let visible_rows = (visible_height / row_height).floor() as usize;

        let editor = self.editor.read(cx);
        let cursor_offset = editor.cursor_offset;
        let line_starts = editor.line_starts();
        let cursor_row = Editor::find_line_index(cursor_offset, &line_starts);

        if cursor_row < self.scroll_offset {
            self.scroll_offset = cursor_row;
        } else if cursor_row >= self.scroll_offset + visible_rows {
            self.scroll_offset = cursor_row.saturating_sub(visible_rows - 1);
        }
        self.scroll_handle.set_offset(point(px(0.), -(self.scroll_offset as f32 * row_height)));
        cx.emit(HexViewEvent::Scrolled(self.scroll_offset));
    }

    fn byte_pos_from_point(&self, point: Point<Pixels>, cx: &App) -> Option<usize> {
        let bounds = self.last_bounds.get()?;
        let header_height = if self.show_header { px(HEADER_HEIGHT) } else { px(0.) };
        let row_height = px(ROW_HEIGHT);
        let offset_width = if self.show_offset { px(OFFSET_WIDTH) } else { px(0.) };
        let hex_start_x = bounds.left() - self.scroll_offset_x + px(OFFSET_X_START) + offset_width + px(SECTION_GAP);

        let hex_byte_width = px(HEX_BYTE_WIDTH);
        let hex_gap = px(HEX_GAP);

        if point.y < bounds.top() + header_height {
            return None;
        }

        let y_offset = point.y - bounds.top() - header_height;
        let visible_row = (y_offset / row_height).floor() as usize;

        // Add scroll offset to get the actual row in the buffer
        let row_idx = visible_row + self.scroll_offset;
        let editor = self.editor.read(cx);
        let line_starts = editor.line_starts();

        if row_idx >= line_starts.len() {
            return None;
        }

        let row_start = line_starts.get(row_idx).unwrap();
        let row_end = if row_idx + 1 < line_starts.len() {
            line_starts.get(row_idx + 1).unwrap()
        } else {
            editor.total_size()
        };
        let row_len = row_end - row_start;

        let x_offset = point.x - hex_start_x;
        if x_offset >= px(0.) {
            let byte_in_row = (x_offset / (hex_byte_width + hex_gap)).floor() as usize;
            if byte_in_row < row_len {
                let byte_pos = row_start + byte_in_row;
                if byte_pos < editor.total_size() {
                    return Some(byte_pos);
                }
            }
        }

        if let Some((ascii_start_x, ascii_char_width)) = self.last_ascii_layout.get() {
            let ascii_x_offset = point.x - ascii_start_x;
            if ascii_x_offset >= px(0.) && ascii_char_width > px(0.) {
                let byte_in_row = (ascii_x_offset / ascii_char_width).floor() as usize;
                if byte_in_row < row_len {
                    let byte_pos = row_start + byte_in_row;
                    if byte_pos < editor.total_size() {
                        return Some(byte_pos);
                    }
                }
            }
        }

        None
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        cx.focus_self(window);
        if let Some(byte_pos) = self.byte_pos_from_point(event.position, cx) {
            self.is_dragging = true;
            self.editor.update(cx, |editor, cx| {
                editor.start_drag(byte_pos);
                cx.notify();
            });
            cx.emit(HexViewEvent::SelectionChanged {
                start: Some(byte_pos),
                end: Some(byte_pos),
            });
        }
    }

    fn byte_pos_from_point_clamped(&self, point: Point<Pixels>, cx: &App) -> Option<usize> {
        let bounds = self.last_bounds.get()?;
        let header_height = if self.show_header { px(HEADER_HEIGHT) } else { px(0.) };
        let row_height = px(ROW_HEIGHT);
        let offset_width = if self.show_offset { px(OFFSET_WIDTH) } else { px(0.) };
        let hex_start_x = bounds.left() - self.scroll_offset_x + px(OFFSET_X_START) + offset_width + px(SECTION_GAP);

        let hex_byte_width = px(HEX_BYTE_WIDTH);
        let hex_gap = px(HEX_GAP);

        let y_offset = point.y - bounds.top() - header_height;
        let visible_row = (y_offset / row_height).floor() as i32;

        // Allow selecting above/below visible area
        let row_idx = (visible_row + self.scroll_offset as i32).max(0) as usize;
        let editor = self.editor.read(cx);
        let line_starts = editor.line_starts();

        if line_starts.is_empty() {
            return Some(0);
        }

        let row_idx = row_idx.min(line_starts.len() - 1);
        let row_start = line_starts.get(row_idx).unwrap();
        let row_end = if row_idx + 1 < line_starts.len() {
            line_starts.get(row_idx + 1).unwrap()
        } else {
            editor.total_size()
        };
        let row_len = row_end - row_start;

        let x_offset = point.x - hex_start_x;
        let byte_in_row = if let Some((ascii_start_x, ascii_char_width)) = self.last_ascii_layout.get() {
            if point.x > ascii_start_x - px(SECTION_GAP) / 2.0 && ascii_char_width > px(0.) {
                let ascii_x_offset = point.x - ascii_start_x;
                (ascii_x_offset / ascii_char_width).floor() as i32
            } else {
                (x_offset / (hex_byte_width + hex_gap)).floor() as i32
            }
        } else {
            (x_offset / (hex_byte_width + hex_gap)).floor() as i32
        };

        let byte_in_row = if row_len > 0 {
            byte_in_row.max(0).min((row_len - 1) as i32) as usize
        } else {
            0
        };

        let byte_pos = row_start + byte_in_row;
        Some(byte_pos.min(editor.total_size().saturating_sub(1)))
    }

    const SCROLL_TRIGGER_MARGIN: f32 = 32.0;

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // Sync scroll handle to scroll offset if changed by scrollbar drag
        let row_height = px(ROW_HEIGHT);
        let handle_y = self.scroll_handle.offset().y;
        let handle_row = ((-handle_y).max(px(0.)) / row_height).round() as usize;
        let total_rows = self.editor.read(cx).line_starts().len();
        let visible_rows = self.get_visible_rows();
        let max_scroll_row = total_rows.saturating_sub(visible_rows).max(0);
        if handle_row != self.scroll_offset {
            self.scroll_offset = handle_row.min(max_scroll_row);
            cx.notify();
            cx.emit(HexViewEvent::Scrolled(self.scroll_offset));
        }

        // Sync horizontal scroll handle
        let handle_x = self.horizontal_scroll_handle.offset().x;
        let synced_offset_x = -handle_x;
        if self.scroll_offset_x != synced_offset_x {
            self.scroll_offset_x = synced_offset_x;
            cx.notify();
        }

        if self.is_dragging {
            if let Some(bounds) = self.last_bounds.get() {
                let header_height = if self.show_header { px(HEADER_HEIGHT) } else { px(0.) };
                let top_edge = bounds.top() + header_height;

                let bottom_edge = bounds.bottom();
                let margin = px(Self::SCROLL_TRIGGER_MARGIN);

                // Auto-scroll if dragging near/outside bounds
                if event.position.y < top_edge + margin {
                    if self.scroll_offset > 0 {
                        self.scroll_offset -= 1;
                        self.scroll_handle.set_offset(point(px(0.), -(self.scroll_offset as f32 * row_height)));
                        cx.notify();
                        cx.emit(HexViewEvent::Scrolled(self.scroll_offset));
                    }
                } else if event.position.y > bottom_edge - margin {
                    let visible_rows = ((bounds.size.height - header_height) / row_height).floor() as usize;
                    let max_scroll = total_rows.saturating_sub(visible_rows);
                    if self.scroll_offset < max_scroll {
                        self.scroll_offset += 1;
                        self.scroll_handle.set_offset(point(px(0.), -(self.scroll_offset as f32 * row_height)));
                        cx.notify();
                        cx.emit(HexViewEvent::Scrolled(self.scroll_offset));
                    }
                }
            }

            if let Some(byte_pos) = self.byte_pos_from_point_clamped(event.position, cx) {
                self.editor.update(cx, |editor, _| {
                    editor.continue_drag(byte_pos);
                });
                cx.notify();
                let editor = self.editor.read(cx);
                cx.emit(HexViewEvent::SelectionChanged {
                    start: editor.selection_start,
                    end: editor.selection_end,
                });
            }
        }
    }

    fn on_mouse_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.is_dragging = false;

        // Sync scroll handle on mouse up as well
        let row_height = px(ROW_HEIGHT);
        let handle_y = self.scroll_handle.offset().y;
        let handle_row = ((-handle_y).max(px(0.)) / row_height).round() as usize;
        let total_rows = self.editor.read(cx).line_starts().len();
        let visible_rows = self.get_visible_rows();
        let max_scroll_row = total_rows.saturating_sub(visible_rows).max(0);
        if handle_row != self.scroll_offset {
            self.scroll_offset = handle_row.min(max_scroll_row);
            cx.notify();
            cx.emit(HexViewEvent::Scrolled(self.scroll_offset));
        } else {
            cx.notify();
        }

        // Sync horizontal scroll handle on mouse up as well
        let handle_x = self.horizontal_scroll_handle.offset().x;
        let synced_offset_x = -handle_x;
        if self.scroll_offset_x != synced_offset_x {
            self.scroll_offset_x = synced_offset_x;
            cx.notify();
        }
    }

    fn on_scroll_wheel(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let row_height = px(ROW_HEIGHT);
        let total_rows = self.editor.read(cx).line_starts().len();
        let visible_rows = self.get_visible_rows();
        let max_offset = total_rows.saturating_sub(visible_rows).max(0) as i32;

        let delta_y_pixels = event.delta.pixel_delta(row_height).y.as_f32();
        let total_delta = delta_y_pixels + self.scroll_remainder;
        let delta_rows = (total_delta / row_height.as_f32()) as i32;
        self.scroll_remainder = total_delta - (delta_rows as f32 * row_height.as_f32());

        let new_scroll_offset = self.scroll_offset as i32 - delta_rows;

        self.scroll_offset = cmp::max(0, cmp::min(new_scroll_offset, max_offset)) as usize;
        self.scroll_handle.set_offset(point(px(0.), -(self.scroll_offset as f32 * row_height)));

        // Handle horizontal scrolling via wheel
        let delta_x_pixels = event.delta.pixel_delta(row_height).x.as_f32();
        if delta_x_pixels != 0.0 {
            let bounds_width = self.last_bounds.get().map(|b| b.size.width).unwrap_or(px(0.));
            let line_starts = self.editor.read(cx).line_starts();
            let max_bytes_per_row = line_starts.max_bytes_per_row();
            let offset_width = if self.show_offset { px(OFFSET_WIDTH) } else { px(0.) };
            let hex_byte_width = px(HEX_BYTE_WIDTH);
            let hex_gap = px(HEX_GAP);
            let section_gap = px(SECTION_GAP);

            let text_style = TextStyle {
                font_family: self.font_family_prop.clone(),
                font_size: gpui::AbsoluteLength::Pixels(self.font_size_prop),
                ..window.text_style()
            };
            let font = text_style.font();
            let font_size = self.font_size_prop;
            let ascii_run = TextRun {
                len: 5,
                font: font.clone(),
                color: cx.theme().foreground.into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let ascii = window.text_system().shape_line("ASCII".into(), font_size, &[ascii_run], None);
            let ascii_char_width = if ascii.len() > 0 { ascii.width / 5.0 } else { px(10.0) };

            let mut total_width = px(OFFSET_X_START) + offset_width + section_gap + (hex_byte_width + hex_gap) * max_bytes_per_row as f32;

            if self.show_ascii {
                total_width += section_gap + ascii_char_width * max_bytes_per_row as f32 + section_gap;
            }

            let max_scroll_x = (total_width - bounds_width).max(px(0.));
            let new_scroll_offset_x = (self.scroll_offset_x.as_f32() - delta_x_pixels).max(0.0);
            self.scroll_offset_x = px(new_scroll_offset_x).min(max_scroll_x);
            self.horizontal_scroll_handle.set_offset(point(-self.scroll_offset_x, px(0.)));
        }

        cx.notify();
        cx.emit(HexViewEvent::Scrolled(self.scroll_offset));
    }

    fn move_left(&mut self, _: &MoveLeft, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, _| {
            editor.move_left();
        });
        self.ensure_cursor_visible(cx);
        cx.notify();
    }

    fn move_right(&mut self, _: &MoveRight, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, _| {
            editor.move_right();
        });
        self.ensure_cursor_visible(cx);
        cx.notify();
    }

    fn move_up(&mut self, _: &MoveUp, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, _| {
            editor.move_up();
        });
        self.ensure_cursor_visible(cx);
        cx.notify();
    }

    fn move_down(&mut self, _: &MoveDown, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, _| {
            editor.move_down();
        });
        self.ensure_cursor_visible(cx);
        cx.notify();
    }

    fn select_left(&mut self, _: &SelectLeft, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, _| {
            editor.select_left();
        });
        self.ensure_cursor_visible(cx);
        cx.notify();
    }

    fn select_right(&mut self, _: &SelectRight, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, _| {
            editor.select_right();
        });
        self.ensure_cursor_visible(cx);
        cx.notify();
    }

    fn select_up(&mut self, _: &SelectUp, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, _| {
            editor.select_up();
        });
        self.ensure_cursor_visible(cx);
        cx.notify();
    }

    fn select_down(&mut self, _: &SelectDown, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, _| {
            editor.select_down();
        });
        self.ensure_cursor_visible(cx);
        cx.notify();
    }

    fn select_all(&mut self, _: &SelectAll, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, _| editor.select_all());
        cx.notify();
    }

    fn get_visible_rows(&self) -> usize {
        if let Some(bounds) = self.last_bounds.get() {
            let header_height = px(HEADER_HEIGHT);
            let row_height = px(ROW_HEIGHT);
            let visible_height = bounds.size.height - header_height;
            (visible_height / row_height).floor() as usize
        } else {
            10 // Default fallback
        }
    }

    fn page_up(&mut self, _: &PageUp, _window: &mut Window, cx: &mut Context<Self>) {
        let visible_rows = self.get_visible_rows();
        self.editor.update(cx, |editor, _| {
            editor.page_up(visible_rows);
        });
        self.ensure_cursor_visible(cx);
        cx.notify();
        let cursor_offset = self.editor.read(cx).cursor_offset;
        cx.emit(HexViewEvent::CursorMoved(cursor_offset));
    }

    fn page_down(&mut self, _: &PageDown, _window: &mut Window, cx: &mut Context<Self>) {
        let visible_rows = self.get_visible_rows();
        self.editor.update(cx, |editor, _| {
            editor.page_down(visible_rows);
        });
        self.ensure_cursor_visible(cx);
        cx.notify();
        let cursor_offset = self.editor.read(cx).cursor_offset;
        cx.emit(HexViewEvent::CursorMoved(cursor_offset));
    }

    fn home(&mut self, _: &Home, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, _| {
            editor.home();
        });
        self.ensure_cursor_visible(cx);
        cx.notify();
        let cursor_offset = self.editor.read(cx).cursor_offset;
        cx.emit(HexViewEvent::CursorMoved(cursor_offset));
    }

    fn end(&mut self, _: &End, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, _| {
            editor.end();
        });
        self.ensure_cursor_visible(cx);
        cx.notify();
        let cursor_offset = self.editor.read(cx).cursor_offset;
        cx.emit(HexViewEvent::CursorMoved(cursor_offset));
    }

    fn select_page_up(&mut self, _: &SelectPageUp, _window: &mut Window, cx: &mut Context<Self>) {
        let visible_rows = self.get_visible_rows();
        self.editor.update(cx, |editor, _| {
            editor.select_page_up(visible_rows);
        });
        self.ensure_cursor_visible(cx);
        cx.notify();
        let editor = self.editor.read(cx);
        cx.emit(HexViewEvent::SelectionChanged {
            start: editor.selection_start,
            end: editor.selection_end,
        });
    }

    fn select_page_down(&mut self, _: &SelectPageDown, _window: &mut Window, cx: &mut Context<Self>) {
        let visible_rows = self.get_visible_rows();
        self.editor.update(cx, |editor, _| {
            editor.select_page_down(visible_rows);
        });
        self.ensure_cursor_visible(cx);
        cx.notify();
        let editor = self.editor.read(cx);
        cx.emit(HexViewEvent::SelectionChanged {
            start: editor.selection_start,
            end: editor.selection_end,
        });
    }

    fn select_home(&mut self, _: &SelectHome, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, _| {
            editor.select_home();
        });
        self.ensure_cursor_visible(cx);
        cx.notify();
        let editor = self.editor.read(cx);
        cx.emit(HexViewEvent::SelectionChanged {
            start: editor.selection_start,
            end: editor.selection_end,
        });
    }

    fn select_end(&mut self, _: &SelectEnd, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, _| {
            editor.select_end();
        });
        self.ensure_cursor_visible(cx);
        cx.notify();
        let editor = self.editor.read(cx);
        cx.emit(HexViewEvent::SelectionChanged {
            start: editor.selection_start,
            end: editor.selection_end,
        });
    }

    fn trigger_search(&mut self, _: &TriggerSearch, window: &mut Window, cx: &mut Context<Self>) {
        window.dispatch_action(ToggleSearch.boxed_clone(), cx);
    }

    fn trigger_search_next(&mut self, _: &TriggerSearchNext, window: &mut Window, cx: &mut Context<Self>) {
        window.dispatch_action(SearchNext.boxed_clone(), cx);
    }

    fn trigger_search_prev(&mut self, _: &TriggerSearchPrev, window: &mut Window, cx: &mut Context<Self>) {
        window.dispatch_action(SearchPrev.boxed_clone(), cx);
    }

    fn add_custom_break(&mut self, _: &AddCustomBreak, _window: &mut Window, cx: &mut Context<Self>) {
        let cursor_offset = self.editor.read(cx).cursor_offset;
        self.editor.update(cx, |editor, _| {
            let line_starts = editor.line_starts();
            let current_line_idx = Editor::find_line_index(cursor_offset, &line_starts);
            let current_line_start = line_starts.get(current_line_idx).unwrap_or(0);

            if cursor_offset == current_line_start {
                editor.add_empty_line(cursor_offset);
            } else {
                editor.toggle_custom_break(cursor_offset);
            }
        });
        cx.notify();
    }

    fn remove_custom_break_backward(&mut self, _: &RemoveCustomBreakBackward, _window: &mut Window, cx: &mut Context<Self>) {
        let cursor_offset = self.editor.read(cx).cursor_offset;
        self.editor.update(cx, |editor, _| {
            if !editor.remove_empty_line(cursor_offset) {
                if editor.custom_breaks.contains(&cursor_offset) {
                    editor.remove_custom_break(cursor_offset);
                }
            }
        });
        cx.notify();
    }

    fn remove_custom_break_forward(&mut self, _: &RemoveCustomBreakForward, _window: &mut Window, cx: &mut Context<Self>) {
        let cursor_offset = self.editor.read(cx).cursor_offset;
        self.editor.update(cx, |editor, _| {
            let line_starts = editor.line_starts();
            let current_line_idx = Editor::find_line_index(cursor_offset, &line_starts);
            let current_line_end = if current_line_idx + 1 < line_starts.len() {
                line_starts.get(current_line_idx + 1).unwrap()
            } else {
                editor.total_size()
            };

            if editor.custom_breaks.contains(&current_line_end) {
                editor.remove_custom_break(current_line_end);
            }
        });
        cx.notify();
    }

    fn join_line(&mut self, _: &JoinLine, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, _| {
            editor.join_line();
        });
        cx.notify();
    }

    fn clear_all_custom_breaks(&mut self, _: &ClearAllCustomBreaks, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, _| {
            editor.clear_all_custom_breaks();
        });
        cx.notify();
    }
}

impl Focusable for HexView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for HexView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.editor.read(cx);
        let document = editor.document.clone();
        let line_starts = editor.line_starts();
        let total_rows = line_starts.len().max(1);

        // Sync scroll offset from scroll handle (e.g. if changed by scrollbar drag)
        let row_height = px(ROW_HEIGHT);
        let handle_y = self.scroll_handle.offset().y;
        let handle_row = ((-handle_y).max(px(0.)) / row_height).round() as usize;
        let visible_rows = self.get_visible_rows();
        let max_scroll_row = total_rows.saturating_sub(visible_rows).max(0);
        let synced_offset = handle_row.min(max_scroll_row);
        if self.scroll_offset != synced_offset {
            self.scroll_offset = synced_offset;
            cx.emit(HexViewEvent::Scrolled(self.scroll_offset));
        }

        // Sync horizontal scroll offset
        let handle_x = self.horizontal_scroll_handle.offset().x;
        let synced_offset_x = -handle_x;
        if self.scroll_offset_x != synced_offset_x {
            self.scroll_offset_x = synced_offset_x;
        }

        let visible_rows = self.get_visible_rows();
        let extra_scroll_rows = visible_rows.saturating_sub(1);
        let header_height = if self.show_header { px(HEADER_HEIGHT) } else { px(0.) };
        let total_height = header_height + row_height * (total_rows + extra_scroll_rows) as f32;

        let (selection_start, selection_end, cursor_offset) = {
            let editor = self.editor.read(cx);
            (editor.selection_start, editor.selection_end, editor.cursor_offset)
        };

        // Compute ascii char width to calculate total content width
        let text_style = TextStyle {
            font_family: self.font_family_prop.clone(),
            font_size: gpui::AbsoluteLength::Pixels(self.font_size_prop),
            ..window.text_style()
        };
        let font = text_style.font();
        let font_size = self.font_size_prop;
        let ascii_run = TextRun {
            len: 5,
            font: font.clone(),
            color: cx.theme().foreground.into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let ascii = window.text_system().shape_line("ASCII".into(), font_size, &[ascii_run], None);
        let ascii_char_width = if ascii.len() > 0 { ascii.width / 5.0 } else { px(10.0) };

        let offset_width = if self.show_offset { px(OFFSET_WIDTH) } else { px(0.) };
        let max_bytes_per_row = line_starts.max_bytes_per_row();
        let hex_byte_width = px(HEX_BYTE_WIDTH);
        let hex_gap = px(HEX_GAP);
        let section_gap = px(SECTION_GAP);

        let mut total_width = px(OFFSET_X_START) + offset_width + section_gap + (hex_byte_width + hex_gap) * max_bytes_per_row as f32;

        if self.show_ascii {
            total_width += section_gap + ascii_char_width * max_bytes_per_row as f32 + section_gap;
        }

        let is_focused = self.focus_handle.is_focused(window);
        let theme = cx.theme();

        let container = div()
            .flex()
            .flex_col()
            .bg(theme.background)
            .font_family(self.font_family_prop.clone())
            .size_full()
            .key_context(CONTEXT);

        let container = container.focus_indicator(is_focused, theme);

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
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child({
                let editor = self.editor.read(cx);
                let custom_breaks = editor.custom_breaks.clone();

                HexViewElement {
                    view: cx.entity().downgrade(),
                    document,
                    line_starts,
                    selection_start,
                    selection_end,
                    cursor_offset,
                    scroll_offset: self.scroll_offset,
                    scroll_offset_x: self.scroll_offset_x,
                    focus_handle: self.focus_handle.clone(),
                    highlights: self.highlights.clone(),
                    max_highlight_len: self.max_highlight_len,
                    show_offset: self.show_offset,
                    show_header: self.show_header,
                    show_ascii: self.show_ascii,
                    font_family: self.font_family_prop.clone(),
                    font_size: self.font_size_prop,
                    custom_breaks,
                    max_bytes_per_row,
                    encoding: editor.encoding,
                }
            })
            .child(
                div().absolute().top_0().right_0().bottom_0().w_4().child(
                    Scrollbar::vertical(&self.scroll_handle)
                        .axis(ScrollbarAxis::Vertical)
                        .scroll_size(size(px(0.), total_height)),
                ),
            )
            .child(
                div().absolute().left_0().right_0().bottom_0().h_4().child(
                    Scrollbar::horizontal(&self.horizontal_scroll_handle)
                        .axis(ScrollbarAxis::Horizontal)
                        .scroll_size(size(total_width, px(0.))),
                ),
            )
    }
}

struct HexViewElement {
    view: WeakEntity<HexView>,
    document: Arc<RwLock<Document>>,
    line_starts: crate::core::editor::LineMap,
    selection_start: Option<usize>,
    selection_end: Option<usize>,
    cursor_offset: usize,
    scroll_offset: usize,
    scroll_offset_x: Pixels,
    focus_handle: FocusHandle,
    highlights: Arc<Vec<(Range<usize>, Hsla)>>,
    max_highlight_len: usize,
    show_offset: bool,
    show_header: bool,
    show_ascii: bool,
    font_family: SharedString,
    font_size: Pixels,
    custom_breaks: BTreeSet<usize>,
    max_bytes_per_row: usize,
    encoding: Encoding,
}

struct PrepaintState {
    data_lines: Vec<DataLine>,
    selection_quads: Vec<PaintQuad>,
    break_indicator_quads: Vec<PaintQuad>,
    cursor_quads: Vec<PaintQuad>,
    header: HeaderParts,
    max_bytes_per_row: usize,
    ascii_char_width: Pixels,
}

struct HeaderParts {
    offset: ShapedLine,
    hex_bytes: Vec<ShapedLine>,
    ascii: ShapedLine,
}

struct DataLine {
    offset_line: ShapedLine,
    hex_lines: Vec<ShapedLine>,
    ascii_chars: Vec<(ShapedLine, usize, usize)>,
}

impl IntoElement for HexViewElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for HexViewElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // Ensure at least one line is shown, even for empty buffer
        let line_count = self.line_starts.len().max(1);
        let header_height = if self.show_header { px(HEADER_HEIGHT) } else { px(0.) };

        let row_height = px(ROW_HEIGHT);
        let total_height = header_height + row_height * line_count as f32;

        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = total_height.into();

        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let document = self.document.read().unwrap();
        let buffer = &document.buffer;
        let selection_start = self.selection_start;
        let selection_end = self.selection_end;
        let highlights = &self.highlights;
        let text_style = TextStyle {
            font_family: self.font_family.clone(),
            font_size: gpui::AbsoluteLength::Pixels(self.font_size),
            ..window.text_style()
        };
        let font_size = self.font_size;

        let theme = cx.theme();
        let offset_color = theme.muted_foreground;
        let hex_byte_color = theme.foreground;
        let hex_null_color = theme.muted_foreground;
        let ascii_printable_color = theme.foreground;
        let ascii_non_printable_color = theme.muted_foreground;
        let is_focused = self.focus_handle.is_focused(window);
        let selection_bg_color = if is_focused { theme.selection } else { theme.accent.opacity(0.5) };

        // Ensure at least one line is shown, even for empty buffer
        let line_starts = &self.line_starts;
        let line_count = line_starts.len().max(1);
        let header_height = if self.show_header { px(HEADER_HEIGHT) } else { px(0.) };

        let row_height = px(ROW_HEIGHT);

        let scroll_offset = self.scroll_offset;
        let visible_height = bounds.size.height - header_height;
        let visible_rows = (visible_height / row_height).ceil() as usize + 1;
        let start_row = scroll_offset;
        let end_row = (scroll_offset + visible_rows).min(line_count);

        let mut data_lines = Vec::new();
        let mut selection_quads = Vec::new();
        let mut break_indicator_quads = Vec::new();

        let offset_width = if self.show_offset { px(OFFSET_WIDTH) } else { px(0.) };
        let hex_start_x = bounds.left() - self.scroll_offset_x + px(OFFSET_X_START) + offset_width + px(SECTION_GAP);

        let hex_byte_width = px(HEX_BYTE_WIDTH);
        let hex_gap = px(HEX_GAP);

        let ascii_start_x = hex_start_x + (hex_byte_width + hex_gap) * self.max_bytes_per_row as f32 + px(SECTION_GAP);
        let ascii_char_width = {
            let font = text_style.font();
            let ascii_run = TextRun {
                len: 5,
                font: font.clone(),
                color: theme.foreground.into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let ascii = window.text_system().shape_line("ASCII".into(), font_size, &[ascii_run], None);
            if ascii.len() > 0 { ascii.width / 5.0 } else { px(10.0) }
        };

        let (min_sel, max_sel) = if let (Some(start), Some(end)) = (selection_start, selection_end) {
            if start <= end { (start, end) } else { (end, start) }
        } else {
            (usize::MAX, usize::MIN)
        };

        // Binary search to find overlapping highlights for the visible viewport
        let viewport_start = line_starts.get(start_row).unwrap_or(0);
        let viewport_end = if end_row < line_starts.len() {
            line_starts.get(end_row).unwrap_or(buffer.len())
        } else {
            buffer.len()
        };

        // 1. Find upper bound using binary search (highlights starting after viewport_end can't overlap)
        let upper_bound = match highlights.binary_search_by_key(&viewport_end, |(r, _)| r.start) {
            Ok(idx) => idx,
            Err(idx) => idx,
        };

        // 2. Find lower bound using binary search (highlights ending before viewport_start can't overlap)
        let search_start = viewport_start.saturating_sub(self.max_highlight_len);
        let lower_bound = match highlights.binary_search_by_key(&search_start, |(r, _)| r.start) {
            Ok(idx) => idx,
            Err(idx) => idx,
        };

        let visible_highlights = &highlights[lower_bound..upper_bound];

        for i in start_row..end_row {
            let offset = line_starts.get(i).unwrap();
            let next_offset = if i + 1 < line_starts.len() {
                line_starts.get(i + 1).unwrap()
            } else {
                buffer.len()
            };
            let chunk_len = next_offset - offset;
            let chunk = buffer.get_range(offset, chunk_len);
            let row_index = i - start_row;
            let y_pos = bounds.top() + header_height + row_height * row_index as f32;

            // Draw highlights
            for (range, color) in visible_highlights {
                let range_start = range.start;
                let range_end = range.end;

                // Check if this line has any overlap with the highlight range
                let line_start = offset;
                let line_end = next_offset; // Exclusive end for the line

                if line_start < range_end && line_end > range_start {
                    let start_in_line = cmp::max(line_start, range_start) - line_start;
                    let end_in_line = cmp::min(line_end, range_end) - line_start;

                    let x_start = hex_start_x + (hex_byte_width + hex_gap) * start_in_line as f32;
                    let x_end = hex_start_x + (hex_byte_width + hex_gap) * end_in_line as f32 - hex_gap;
                    // Adjust width to cover the gap if it's a continuous range within the line
                    let width = x_end - x_start;

                    selection_quads.push(fill(
                        Bounds::new(
                            point(x_start - px(SELECTION_PADDING), y_pos),
                            size(width + 2.0 * px(SELECTION_PADDING), row_height),
                        ),
                        *color,
                    ));

                    if self.show_ascii {
                        let ascii_x_start = ascii_start_x + ascii_char_width * start_in_line as f32;
                        let ascii_x_end = ascii_start_x + ascii_char_width * end_in_line as f32;
                        let ascii_width = ascii_x_end - ascii_x_start;

                        if ascii_width > px(0.) {
                            selection_quads.push(fill(Bounds::new(point(ascii_x_start, y_pos), size(ascii_width, row_height)), *color));
                        }
                    }
                }
            }

            // Draw continuous selection background for this line
            let line_start = offset;
            let line_end = next_offset.saturating_sub(1);
            if line_start <= max_sel && (next_offset > min_sel || (line_start == 0 && buffer.is_empty())) {
                let start_in_line = cmp::max(line_start, min_sel).saturating_sub(line_start);
                let end_in_line = cmp::min(line_end, max_sel).saturating_sub(line_start);

                let x_start = hex_start_x + (hex_byte_width + hex_gap) * start_in_line as f32;
                let x_end = hex_start_x + (hex_byte_width + hex_gap) * end_in_line as f32 + hex_byte_width;
                let width = x_end - x_start;

                selection_quads.push(fill(
                    Bounds::new(
                        point(x_start - px(SELECTION_PADDING), y_pos),
                        size(width + 2.0 * px(SELECTION_PADDING), row_height),
                    ),
                    selection_bg_color,
                ));

                if self.show_ascii {
                    let ascii_x_start = ascii_start_x + ascii_char_width * start_in_line as f32;
                    let ascii_x_end = ascii_start_x + ascii_char_width * (end_in_line + 1) as f32;
                    let mut ascii_width = ascii_x_end - ascii_x_start;
                    if buffer.is_empty() {
                        ascii_width = ascii_char_width;
                    }

                    if ascii_width > px(0.) {
                        selection_quads.push(fill(
                            Bounds::new(point(ascii_x_start, y_pos), size(ascii_width, row_height)),
                            selection_bg_color,
                        ));
                    }
                }
            }

            let offset_str = format_offset_08(offset);
            let offset_run = TextRun {
                len: offset_str.len(),
                font: text_style.font(),
                color: offset_color.into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let offset_line = if self.show_offset {
                window.text_system().shape_line(offset_str, font_size, &[offset_run], None)
            } else {
                window.text_system().shape_line("".into(), font_size, &[], None)
            };

            let mut hex_lines = Vec::new();
            for (_byte_idx, byte) in chunk.iter().enumerate() {
                let color = if *byte == 0 { hex_null_color } else { hex_byte_color };

                let hex_str = HEX_STR_LUT[*byte as usize];
                let hex_run = TextRun {
                    len: 2,
                    font: text_style.font(),
                    color: color.into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let hex_line = window.text_system().shape_line(hex_str.into(), font_size, &[hex_run], None);
                hex_lines.push(hex_line);
            }

            let mut ascii_chars = Vec::new();

            if self.show_ascii && !chunk.is_empty() {
                for (byte_idx, _) in chunk.iter().enumerate() {
                    let global_offset = offset + byte_idx;

                    if self.encoding.is_continuation_byte(buffer.data(), global_offset) {
                        continue;
                    }

                    let (char_str, color, len): (SharedString, _, _) = if let Some((c, char_len)) = self.encoding.decode_char_at(buffer.data(), global_offset) {
                        let str_val = if (c as u32) < 128 {
                            SharedString::from(ASCII_STR_LUT[c as usize])
                        } else {
                            SharedString::from(c.to_string())
                        };
                        (str_val, ascii_printable_color, char_len)
                    } else {
                        (SharedString::from("."), ascii_non_printable_color, 1)
                    };

                    let ascii_run = TextRun {
                        len: char_str.len(),
                        font: text_style.font(),
                        color: color.into(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };

                    let shaped = window.text_system().shape_line(char_str, font_size, &[ascii_run], None);
                    ascii_chars.push((shaped, byte_idx, len));
                }
            }

            data_lines.push(DataLine {
                offset_line,
                hex_lines,
                ascii_chars,
            });

            // Custom Break インジケーター: 行の先頭が custom_breaks に含まれる場合、左端にマーカーを描画
            if self.custom_breaks.contains(&offset) {
                let indicator_x = bounds.left() + px(1.);
                let indicator_width = px(3.);
                break_indicator_quads.push(fill(
                    Bounds::new(point(indicator_x, y_pos), size(indicator_width, row_height)),
                    theme.yellow.opacity(0.8),
                ));
            }
        }

        // ascii_start_x and ascii_char_width already computed above

        let mut cursor_quads = Vec::new();
        {
            let cursor_offset = self.cursor_offset;
            let focus_handle = self.focus_handle.clone();

            // Show cursor (colored when focused, muted when not focused)
            let is_focused = focus_handle.is_focused(window);
            let cursor_row = Editor::find_line_index(cursor_offset, &line_starts);
            let byte_in_row = cursor_offset - line_starts.get(cursor_row).unwrap();

            if cursor_row >= start_row && cursor_row < end_row {
                let visible_cursor_row = cursor_row - start_row;
                let y_pos = bounds.top() + header_height + row_height * visible_cursor_row as f32;
                let cursor_x = hex_start_x + (hex_byte_width + hex_gap) * byte_in_row as f32;

                let cursor_color = if is_focused { theme.selection } else { theme.accent.opacity(0.5) };

                cursor_quads.push(fill(
                    Bounds::new(
                        point(cursor_x - px(SELECTION_PADDING), y_pos),
                        size(hex_byte_width + 2.0 * px(SELECTION_PADDING), row_height),
                    ),
                    cursor_color,
                ));

                if self.show_ascii {
                    let ascii_cursor_x = ascii_start_x + ascii_char_width * byte_in_row as f32;
                    cursor_quads.push(fill(
                        Bounds::new(point(ascii_cursor_x, y_pos), size(ascii_char_width, row_height)),
                        cursor_color,
                    ));
                }
            }
        }

        let header = {
            let header_color = theme.foreground;
            let font = text_style.font();

            let offset_run = TextRun {
                len: 6,
                font: font.clone(),
                color: header_color.into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let offset = if self.show_offset {
                window.text_system().shape_line("Offset".into(), font_size, &[offset_run], None)
            } else {
                window.text_system().shape_line("".into(), font_size, &[], None)
            };

            let mut hex_bytes = Vec::new();
            let header_cols = self.max_bytes_per_row;
            for i in 0..header_cols {
                let s: SharedString = if i < HEADER_COL_LUT.len() {
                    SharedString::from(HEADER_COL_LUT[i])
                } else {
                    SharedString::from(format!("+{:X}", i))
                };
                let run = TextRun {
                    len: s.len(),
                    font: font.clone(),
                    color: header_color.into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                hex_bytes.push(window.text_system().shape_line(s, font_size, &[run], None));
            }

            let encoding_name = match self.encoding {
                crate::core::encoding::Encoding::Ascii => "ASCII",
                crate::core::encoding::Encoding::Utf8 => "UTF-8",
                crate::core::encoding::Encoding::Utf16Le => "UTF-16 LE",
                crate::core::encoding::Encoding::Utf16Be => "UTF-16 BE",
            };
            let ascii_run = TextRun {
                len: encoding_name.len(),
                font: font.clone(),
                color: header_color.into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let ascii = if self.show_ascii {
                window.text_system().shape_line(encoding_name.into(), font_size, &[ascii_run], None)
            } else {
                window.text_system().shape_line("".into(), font_size, &[], None)
            };

            HeaderParts { offset, hex_bytes, ascii }
        };

        PrepaintState {
            data_lines,
            selection_quads,
            break_indicator_quads,
            cursor_quads,
            header,
            max_bytes_per_row: self.max_bytes_per_row,
            ascii_char_width,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let header_height = if self.show_header { px(HEADER_HEIGHT) } else { px(0.) };
        let row_height = px(ROW_HEIGHT);
        let offset_width = if self.show_offset { px(OFFSET_WIDTH) } else { px(0.) };

        let hex_start_x = bounds.left() - self.scroll_offset_x + px(OFFSET_X_START) + offset_width + px(SECTION_GAP);
        let hex_byte_width = px(HEX_BYTE_WIDTH);
        let hex_gap = px(HEX_GAP);
        let ascii_start_x = hex_start_x + (hex_byte_width + hex_gap) * prepaint.max_bytes_per_row as f32 + px(SECTION_GAP);

        let theme = cx.theme();
        let bg_color = theme.background;
        let border_color = theme.border;

        window.paint_quad(fill(bounds, bg_color));

        window.paint_quad(fill(
            Bounds::new(
                point(bounds.left() - self.scroll_offset_x + px(OFFSET_X_START), bounds.top() + header_height - px(1.)),
                size(bounds.size.width - px(OFFSET_X_START), px(1.)),
            ),
            border_color,
        ));

        // Paint header
        if self.show_header {
            let header_y = bounds.top();
            if self.show_offset {
                prepaint
                    .header
                    .offset
                    .paint(
                        point(bounds.left() - self.scroll_offset_x + px(OFFSET_X_START), header_y),
                        header_height,
                        window,
                        cx,
                    )
                    .ok();
            }

            for (i, hex_header) in prepaint.header.hex_bytes.iter().enumerate() {
                let x_pos = hex_start_x + (hex_byte_width + hex_gap) * i as f32;
                hex_header.paint(point(x_pos, header_y), header_height, window, cx).ok();
            }

            if self.show_ascii {
                prepaint.header.ascii.paint(point(ascii_start_x, header_y), header_height, window, cx).ok();
            }
        }

        for selection_quad in prepaint.selection_quads.drain(..) {
            window.paint_quad(selection_quad);
        }

        // Custom Break インジケーターを描画
        for indicator_quad in prepaint.break_indicator_quads.drain(..) {
            window.paint_quad(indicator_quad);
        }

        // Draw cursor block behind the text
        for cursor_quad in prepaint.cursor_quads.drain(..) {
            window.paint_quad(cursor_quad);
        }

        for (i, data_line) in prepaint.data_lines.iter().enumerate() {
            let y_pos = bounds.top() + header_height + row_height * i as f32;

            if self.show_offset {
                data_line
                    .offset_line
                    .paint(point(bounds.left() - self.scroll_offset_x + px(OFFSET_X_START), y_pos), row_height, window, cx)
                    .ok();
            }

            for (byte_idx, hex_line) in data_line.hex_lines.iter().enumerate() {
                let x_pos = hex_start_x + (hex_byte_width + hex_gap) * byte_idx as f32;
                hex_line.paint(point(x_pos, y_pos), row_height, window, cx).ok();
            }

            if self.show_ascii {
                let ascii_char_width = prepaint.ascii_char_width;
                for (shaped, byte_idx, _len) in &data_line.ascii_chars {
                    let x_start = ascii_start_x + ascii_char_width * (*byte_idx as f32);
                    shaped.paint(point(x_start, y_pos), row_height, window, cx).ok();
                }
            }
        }

        let ascii_char_width = prepaint.ascii_char_width;
        if let Some(view) = self.view.upgrade() {
            let view = view.read(cx);
            view.last_bounds.set(Some(bounds));
            view.last_ascii_layout.set(Some((ascii_start_x, ascii_char_width)));
        }
    }
}
