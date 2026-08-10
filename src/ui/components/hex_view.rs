use crate::actions::{
    AddCustomBreak, ClearAllCustomBreaks, ClearAllHighlights, ClearHighlight, Copy, CopyAsBase64, CopyAsBinary, CopyAsCppArray, CopyAsEscapedString,
    CopyAsHexDump, CopyAsHexSpaces, CopyAsHexStream, CopyAsJsonArray, CopyAsPrintableText, CopyAsRustArray, ExportHighlights, HighlightBlue, HighlightCyan,
    HighlightGreen, HighlightOrange, HighlightPink, HighlightPurple, HighlightRed, HighlightYellow, ImportHighlights, JoinLine, RemoveCustomBreakBackward,
    RemoveCustomBreakForward, SearchNext, SearchPrev, SelectAll as AppSelectAll, SetByteOrderBigEndian, SetByteOrderLittleEndian, SetGroupSize1, SetGroupSize2,
    SetGroupSize4, SetGroupSize8, SetRadixBin, SetRadixDec, SetRadixHex, SetRadixOct, ShowHighlightsTab, ToggleByteOrder, ToggleSearch,
};
use crate::core::document::Document;
use crate::core::editor::Editor;
use crate::core::encoding::Encoding;
use crate::core::format::{CopyFormat, format_bytes};
use crate::core::radix::{ByteGroupSize, DisplayRadix, digit_count, format_group, is_group_zero};
use crate::core::structure::ParseResult;
use crate::ui::style::StyleExt as _;
use gpui::prelude::*;
use gpui::*;
use gpui_component::menu::ContextMenuExt;
use gpui_component::{ActiveTheme, StyledExt, h_flex};
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
pub const SECTION_GAP: f32 = 16.0;

pub const ADDRESS_WIDTH: f32 = 80.0;
pub const DESC_WIDTH: f32 = 240.0;
pub const COMMENT_WIDTH: f32 = 300.0;

#[inline]
pub fn item_metrics(radix: DisplayRadix, group_size: ByteGroupSize, font_size: Pixels) -> (f32, f32) {
    let digits = digit_count(radix, group_size);
    let char_w = f32::from(font_size) * 0.61;
    let item_w = (char_w * digits as f32 + 6.0).ceil().max(20.0);
    let item_gap = 4.0;
    (item_w, item_gap)
}

#[inline]
pub fn calculate_data_col_width(radix: DisplayRadix, group_size: ByteGroupSize, max_bytes_per_row: usize, font_size: Pixels) -> f32 {
    let (item_w, item_gap) = item_metrics(radix, group_size, font_size);
    let items_in_row = max_bytes_per_row.div_ceil(group_size.byte_count()).max(1);
    items_in_row as f32 * (item_w + item_gap)
}

#[inline]
fn format_offset_08(offset: usize) -> SharedString {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut buf = [b'0'; 8];
    let mut val = offset;
    for i in (0..8).rev() {
        buf[i] = DIGITS[val & 0xf];
        val >>= 4;
    }
    SharedString::from(std::str::from_utf8(&buf).expect("valid ascii utf8").to_string())
}

#[inline]
fn paint_border_box(window: &mut Window, bounds: Bounds<Pixels>, border_width: Pixels, color: Hsla) {
    let top = Bounds::new(bounds.origin, size(bounds.size.width, border_width));
    let bottom = Bounds::new(
        point(bounds.origin.x, bounds.origin.y + bounds.size.height - border_width),
        size(bounds.size.width, border_width),
    );
    let left = Bounds::new(bounds.origin, size(border_width, bounds.size.height));
    let right = Bounds::new(
        point(bounds.origin.x + bounds.size.width - border_width, bounds.origin.y),
        size(border_width, bounds.size.height),
    );
    window.paint_quad(gpui::fill(top, color));
    window.paint_quad(gpui::fill(bottom, color));
    window.paint_quad(gpui::fill(left, color));
    window.paint_quad(gpui::fill(right, color));
}

fn row_highlights(highlights: &[(Range<usize>, Hsla)], max_len: usize, offset: usize, next_offset: usize) -> &[(Range<usize>, Hsla)] {
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
        KeyBinding::new("cmd-c", Copy, Some(CONTEXT)),
        KeyBinding::new("ctrl-c", Copy, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-c", CopyAsHexDump, Some(CONTEXT)),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizingColumn {
    Address,
    Hex,
    Description,
    Comment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollAxisLock {
    Horizontal,
    Vertical,
}

pub struct HexView {
    editor: Entity<Editor>,
    focus_handle: FocusHandle,
    uniform_scroll_handle: UniformListScrollHandle,
    scroll_offset: usize,
    pub hex_scroll_x: f32,
    pub desc_scroll_x: f32,
    pub comment_scroll_x: f32,
    scroll_lock_axis: Option<ScrollAxisLock>,
    last_scroll_time: Option<std::time::Instant>,
    scroll_lock_top_row: usize,
    is_selecting: bool,
    bounds: std::cell::Cell<Option<Bounds<Pixels>>>,
    list_bounds: std::cell::Cell<Option<Bounds<Pixels>>>,
    visible_range: std::cell::Cell<Option<(usize, usize)>>,
    visible_row_info: std::cell::Cell<Option<(usize, Pixels, Pixels)>>,
    highlights: Arc<Vec<(Range<usize>, Hsla)>>,
    max_highlight_len: usize,
    show_offset: bool,
    show_header: bool,
    show_ascii: bool,
    encoding: Encoding,
    radix: DisplayRadix,
    group_size: ByteGroupSize,
    is_big_endian: bool,
    font_family_prop: SharedString,
    font_size_prop: Pixels,
    pub address_col_width: f32,
    pub hex_col_width: f32,
    pub desc_col_width: f32,
    pub comment_col_width: f32,
    resizing_column: Option<(ResizingColumn, f32, f32)>,
    _editor_subscription: Subscription,
}

impl EventEmitter<HexViewEvent> for HexView {}

#[allow(dead_code)]
impl HexView {
    pub fn new(editor: Entity<Editor>, cx: &mut Context<Self>) -> Self {
        let (radix, group_size, is_big_endian, encoding) = {
            let ed = editor.read(cx);
            (ed.radix, ed.group_size, ed.is_big_endian, ed.encoding)
        };
        let font_size_prop = px(14.0);
        let hex_col_width = calculate_data_col_width(radix, group_size, 16, font_size_prop);

        let _editor_subscription = cx.observe(&editor, |this, editor_entity, cx| {
            let ed = editor_entity.read(cx);
            let new_encoding = ed.encoding;
            let new_radix = ed.radix;
            let new_group_size = ed.group_size;
            let new_endian = ed.is_big_endian;

            if this.encoding != new_encoding {
                this.encoding = new_encoding;
            }
            if this.radix != new_radix || this.group_size != new_group_size || this.is_big_endian != new_endian {
                this.radix = new_radix;
                this.group_size = new_group_size;
                this.is_big_endian = new_endian;
                let max_bytes = ed.line_starts().max_bytes_per_row();
                this.hex_col_width = calculate_data_col_width(new_radix, new_group_size, max_bytes, this.font_size_prop);
            }
            this.clamp_scroll_offsets(cx);
            this.ensure_cursor_visible(cx);
            cx.notify();
        });

        Self {
            editor,
            focus_handle: cx.focus_handle(),
            uniform_scroll_handle: UniformListScrollHandle::new(),
            scroll_offset: 0,
            hex_scroll_x: 0.0,
            desc_scroll_x: 0.0,
            comment_scroll_x: 0.0,
            scroll_lock_axis: None,
            last_scroll_time: None,
            scroll_lock_top_row: 0,
            is_selecting: false,
            bounds: std::cell::Cell::new(None),
            list_bounds: std::cell::Cell::new(None),
            visible_range: std::cell::Cell::new(None),
            visible_row_info: std::cell::Cell::new(None),
            highlights: Arc::new(Vec::new()),
            max_highlight_len: 0,
            show_offset: true,
            show_header: true,
            show_ascii: true,
            encoding,
            radix,
            group_size,
            is_big_endian,
            font_family_prop: "Zed Sans Mono".into(),
            font_size_prop,
            address_col_width: ADDRESS_WIDTH,
            hex_col_width,
            desc_col_width: DESC_WIDTH,
            comment_col_width: COMMENT_WIDTH,
            resizing_column: None,
            _editor_subscription,
        }
    }

    pub fn max_hex_scroll(&self, cx: &App) -> f32 {
        let editor = self.editor.read(cx);
        let max_bytes = editor.line_starts().max_bytes_per_row();
        let total_data_width = calculate_data_col_width(self.radix, self.group_size, max_bytes, self.font_size_prop);
        (total_data_width - self.hex_col_width).max(0.0)
    }

    pub fn max_desc_scroll(&self, cx: &App) -> f32 {
        let editor = self.editor.read(cx);
        if let Some(ref parse_res) = editor.parse_result {
            let char_w = f32::from(self.font_size_prop) * 0.61;
            let mut max_w: f32 = 0.0;
            for container in &parse_res.index.container_structs {
                let text = &container.id;
                let char_count: f32 = text.chars().map(|c| if c.is_ascii() { 1.0 } else { 1.8 }).sum();
                let w = char_count * char_w + 40.0;
                if w > max_w {
                    max_w = w;
                }
            }
            for field in &parse_res.index.leaf_fields {
                let expr = field.format_expression();
                let char_count: f32 = expr.chars().map(|c| if c.is_ascii() { 1.0 } else { 1.8 }).sum();
                let w = char_count * char_w + 50.0;
                if w > max_w {
                    max_w = w;
                }
            }
            let total_max = (max_w + 32.0).max(self.desc_col_width);
            (total_max - self.desc_col_width).max(0.0)
        } else {
            0.0
        }
    }

    pub fn max_comment_scroll(&self, cx: &App) -> f32 {
        let editor = self.editor.read(cx);
        let line_starts = editor.line_starts();
        let char_w = f32::from(self.font_size_prop) * 0.61;
        let dot_size = 8.0;
        let dot_margin_right = 5.0;
        let item_spacing = 14.0;

        use std::collections::HashMap;
        let mut row_widths: HashMap<usize, f32> = HashMap::new();

        for h in &editor.highlights {
            let trimmed = h.comment.trim();
            if trimmed.is_empty() {
                continue;
            }
            let h_start_row = Editor::find_line_index(h.offset, &line_starts);
            let char_count: f32 = trimmed.chars().map(|c| if c.is_ascii() { 1.0 } else { 1.8 }).sum();
            let item_w = dot_size + dot_margin_right + (char_count * char_w) + item_spacing;
            *row_widths.entry(h_start_row).or_insert(8.0) += item_w;
        }

        let max_content_w = row_widths.values().copied().fold(0.0f32, f32::max);
        let max_w = (max_content_w + 32.0).max(self.comment_col_width);
        (max_w - self.comment_col_width).max(0.0)
    }

    pub fn clamp_scroll_offsets(&mut self, cx: &App) {
        let max_hex = self.max_hex_scroll(cx);
        self.hex_scroll_x = self.hex_scroll_x.clamp(0.0, max_hex);

        let max_desc = self.max_desc_scroll(cx);
        self.desc_scroll_x = self.desc_scroll_x.clamp(0.0, max_desc);

        let max_comment = self.max_comment_scroll(cx);
        self.comment_scroll_x = self.comment_scroll_x.clamp(0.0, max_comment);
    }

    pub fn auto_fit_column(&mut self, col: ResizingColumn, cx: &mut Context<Self>) {
        match col {
            ResizingColumn::Address => {
                self.address_col_width = ADDRESS_WIDTH;
            }
            ResizingColumn::Hex => {
                let editor = self.editor.read(cx);
                let max_bytes = editor.line_starts().max_bytes_per_row();
                self.hex_col_width = calculate_data_col_width(self.radix, self.group_size, max_bytes, self.font_size_prop);
                self.hex_scroll_x = 0.0;
            }
            ResizingColumn::Description => {
                let editor = self.editor.read(cx);
                if let Some(ref parse_res) = editor.parse_result {
                    let char_w = f32::from(self.font_size_prop) * 0.61;
                    let mut max_w: f32 = 0.0;
                    for container in &parse_res.index.container_structs {
                        let text = &container.id;
                        let char_count: f32 = text.chars().map(|c| if c.is_ascii() { 1.0 } else { 1.8 }).sum();
                        let w = char_count * char_w + 40.0;
                        if w > max_w {
                            max_w = w;
                        }
                    }
                    for field in &parse_res.index.leaf_fields {
                        let expr = field.format_expression();
                        let char_count: f32 = expr.chars().map(|c| if c.is_ascii() { 1.0 } else { 1.8 }).sum();
                        let w = char_count * char_w + 50.0;
                        if w > max_w {
                            max_w = w;
                        }
                    }
                    self.desc_col_width = max_w.max(DESC_WIDTH);
                    self.desc_scroll_x = 0.0;
                } else {
                    self.desc_col_width = DESC_WIDTH;
                    self.desc_scroll_x = 0.0;
                }
            }
            ResizingColumn::Comment => {
                let editor = self.editor.read(cx);
                let line_starts = editor.line_starts();
                let char_w = f32::from(self.font_size_prop) * 0.61;
                let dot_size = 8.0;
                let dot_margin_right = 5.0;
                let item_spacing = 14.0;

                use std::collections::HashMap;
                let mut row_widths: HashMap<usize, f32> = HashMap::new();

                for h in &editor.highlights {
                    let trimmed = h.comment.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let h_start_row = Editor::find_line_index(h.offset, &line_starts);
                    let char_count: f32 = trimmed.chars().map(|c| if c.is_ascii() { 1.0 } else { 1.8 }).sum();
                    let item_w = dot_size + dot_margin_right + (char_count * char_w) + item_spacing;
                    *row_widths.entry(h_start_row).or_insert(8.0) += item_w;
                }

                let max_content_w = row_widths.values().copied().fold(0.0f32, f32::max);
                self.comment_col_width = if max_content_w > 0.0 {
                    (max_content_w + 24.0).max(COMMENT_WIDTH)
                } else {
                    COMMENT_WIDTH
                };
                self.comment_scroll_x = 0.0;
            }
        }
        self.clamp_scroll_offsets(cx);
        cx.notify();
    }

    fn on_scroll_wheel(&mut self, event: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let now = std::time::Instant::now();
        let pixel_delta = event.delta.pixel_delta(px(ROW_HEIGHT));
        let mut delta_x = f32::from(pixel_delta.x);
        let delta_y = f32::from(pixel_delta.y);

        if delta_x == 0.0 && event.modifiers.shift && delta_y != 0.0 {
            delta_x = delta_y;
        }

        // 120ms 以上イベント間隔が空いたらジェスチャー終了と判定してリセット
        if let Some(last_time) = self.last_scroll_time
            && now.duration_since(last_time).as_millis() > 120
        {
            self.scroll_lock_axis = None;
        }
        self.last_scroll_time = Some(now);

        let abs_x = delta_x.abs();
        let abs_y = delta_y.abs();

        // 軸ロックの決定（初動の移動量で方向をロック）
        if self.scroll_lock_axis.is_none() && (abs_x > 0.5 || abs_y > 0.5) {
            if abs_x > abs_y * 1.1 {
                self.scroll_lock_axis = Some(ScrollAxisLock::Horizontal);
                self.scroll_lock_top_row = self.current_scroll_top_row();
            } else if abs_y > abs_x * 1.1 {
                self.scroll_lock_axis = Some(ScrollAxisLock::Vertical);
            }
        }

        // 縦スクロールロック中の場合は横スクロールを行わずスルー
        if self.scroll_lock_axis == Some(ScrollAxisLock::Vertical) {
            return;
        }

        let is_horizontal = self.scroll_lock_axis == Some(ScrollAxisLock::Horizontal) || abs_x > abs_y;

        if is_horizontal && abs_x > 0.01 {
            // 横スクロールロック中は縦スクロール位置を固定して縦揺れを防止
            if self.scroll_lock_axis == Some(ScrollAxisLock::Horizontal) {
                let lock_row = self.scroll_lock_top_row;
                self.uniform_scroll_handle.scroll_to_item(lock_row, ScrollStrategy::Top);
            }

            let bounds = if let Some(b) = self.bounds.get() {
                b
            } else {
                return;
            };

            let is_struct_mode = {
                let editor = self.editor.read(cx);
                editor.show_inline_structure_view && editor.parse_result.is_some()
            };
            let max_bytes_per_row = self.editor.read(cx).line_starts().max_bytes_per_row();

            let offset_w = if is_struct_mode {
                self.address_col_width
            } else if self.show_offset {
                OFFSET_WIDTH
            } else {
                0.0
            };
            let gap = SECTION_GAP;
            let base_x = f32::from(bounds.left()) + 8.0;

            let hex_start_x = base_x + offset_w + gap;
            let hex_end_x = hex_start_x + self.hex_col_width;

            let (desc_start_x, desc_end_x, comment_start_x, comment_end_x) = if is_struct_mode {
                let d_start = hex_end_x + gap;
                let d_end = d_start + self.desc_col_width;
                let c_start = d_end + gap;
                let c_end = c_start + self.comment_col_width;
                (d_start, d_end, c_start, c_end)
            } else {
                let ascii_w = if self.show_ascii { max_bytes_per_row as f32 * 10.0 } else { 0.0 };
                let c_start = if self.show_ascii { hex_end_x + gap + ascii_w + gap } else { hex_end_x + gap };
                let c_end = c_start + self.comment_col_width;
                (0.0, 0.0, c_start, c_end)
            };

            let mouse_x = f32::from(event.position.x);

            if mouse_x >= hex_start_x && mouse_x <= hex_end_x + (gap / 2.0) {
                let max_hex = self.max_hex_scroll(cx);
                let new_scroll = (self.hex_scroll_x - delta_x).clamp(0.0, max_hex);
                if (new_scroll - self.hex_scroll_x).abs() > 0.01 {
                    self.hex_scroll_x = new_scroll;
                    cx.notify();
                }
            } else if is_struct_mode && mouse_x >= desc_start_x && mouse_x <= desc_end_x + (gap / 2.0) {
                let max_desc = self.max_desc_scroll(cx);
                let new_scroll = (self.desc_scroll_x - delta_x).clamp(0.0, max_desc);
                if (new_scroll - self.desc_scroll_x).abs() > 0.01 {
                    self.desc_scroll_x = new_scroll;
                    cx.notify();
                }
            } else if mouse_x >= comment_start_x && mouse_x <= comment_end_x + (gap / 2.0) {
                let max_comment = self.max_comment_scroll(cx);
                let new_scroll = (self.comment_scroll_x - delta_x).clamp(0.0, max_comment);
                if (new_scroll - self.comment_scroll_x).abs() > 0.01 {
                    self.comment_scroll_x = new_scroll;
                    cx.notify();
                }
            }
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

    pub fn set_highlights_arc(&mut self, highlights: Arc<Vec<(Range<usize>, Hsla)>>, cx: &mut Context<Self>) {
        self.max_highlight_len = highlights.iter().map(|(r, _)| r.end.saturating_sub(r.start)).max().unwrap_or(0);
        self.highlights = highlights;
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
        if let Some((top_row, _)) = self.visible_range.get() {
            top_row
        } else {
            self.scroll_offset
        }
    }

    pub fn viewport_byte_range(&self, cx: &App) -> (usize, usize) {
        let editor = self.editor.read(cx);
        let line_starts = editor.line_starts();
        let current_top = self.current_scroll_top_row();
        let start_byte = line_starts.get(current_top).unwrap_or(0);
        let end_row = (current_top + 30).min(line_starts.len());
        let end_byte = if end_row < line_starts.len() {
            line_starts.get(end_row).expect("valid line start at end_row")
        } else {
            editor.total_size()
        };
        (start_byte, end_byte)
    }

    pub fn scroll_to_row(&mut self, row: usize, cx: &mut Context<Self>) {
        let total_rows = self.editor.read(cx).line_starts().len();
        let max_offset = total_rows.saturating_sub(1);
        let new_offset = row.min(max_offset);

        self.scroll_offset = new_offset;
        self.uniform_scroll_handle.scroll_to_item(new_offset, ScrollStrategy::Top);
        cx.notify();
        cx.emit(HexViewEvent::Scrolled(self.scroll_offset));
    }

    pub fn scroll_to_bottom_row(&mut self, row: usize, cx: &mut Context<Self>) {
        let total_rows = self.editor.read(cx).line_starts().len();
        let max_offset = total_rows.saturating_sub(1);
        let new_offset = row.min(max_offset);

        self.scroll_offset = new_offset;
        self.uniform_scroll_handle.scroll_to_item(new_offset, ScrollStrategy::Bottom);
        cx.notify();
        cx.emit(HexViewEvent::Scrolled(self.scroll_offset));
    }

    fn ensure_cursor_visible(&mut self, cx: &mut Context<Self>) {
        let editor = self.editor.read(cx);
        let cursor_offset = editor.cursor_offset;
        let line_starts = editor.line_starts();
        let cursor_row = Editor::find_line_index(cursor_offset, &line_starts);

        if let Some((top_row, bottom_row)) = self.visible_range.get() {
            if cursor_row < top_row {
                self.scroll_to_row(cursor_row, cx);
            } else if cursor_row >= bottom_row {
                self.scroll_to_bottom_row(cursor_row, cx);
            }
        } else {
            self.scroll_to_row(cursor_row, cx);
        }

        // Horizontal visibility in Hex column
        let line_offset = line_starts.get(cursor_row).unwrap_or(0);
        let byte_in_line = cursor_offset.saturating_sub(line_offset);
        let group_bytes = self.group_size.byte_count();
        let (item_width, item_gap) = item_metrics(self.radix, self.group_size, self.font_size_prop);
        let item_step = item_width + item_gap;
        let item_idx = byte_in_line / group_bytes;
        let item_left = item_idx as f32 * item_step;
        let item_right = item_left + item_width;

        let max_hex = self.max_hex_scroll(cx);
        if item_left < self.hex_scroll_x {
            self.hex_scroll_x = item_left.clamp(0.0, max_hex);
        } else if item_right > self.hex_scroll_x + self.hex_col_width {
            self.hex_scroll_x = (item_right - self.hex_col_width + item_gap).clamp(0.0, max_hex);
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

    fn copy_formatted(&self, format: CopyFormat, cx: &mut Context<Self>) {
        let formatted = {
            let editor = self.editor.read(cx);
            let doc = editor.document.read().expect("document read lock");
            let total = doc.buffer.len();
            if total == 0 {
                String::new()
            } else {
                let (start_offset, slice) = if let Some(range) = editor.selection_range() {
                    if !range.is_empty() {
                        (range.start, doc.buffer.get_range(range.start, range.len()))
                    } else {
                        let off = editor.cursor_offset.min(total.saturating_sub(1));
                        (off, doc.buffer.get_range(off, 1))
                    }
                } else {
                    let off = editor.cursor_offset.min(total.saturating_sub(1));
                    (off, doc.buffer.get_range(off, 1))
                };
                format_bytes(slice, start_offset, format)
            }
        };

        cx.write_to_clipboard(gpui::ClipboardItem::new_string(formatted));
    }

    fn copy(&mut self, _: &Copy, _window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::HexStream, cx);
    }

    fn copy_as_hexdump(&mut self, _: &CopyAsHexDump, _window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::HexDump, cx);
    }

    fn copy_as_cpp_array(&mut self, _: &CopyAsCppArray, _window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::CppArray, cx);
    }

    fn copy_as_hex_stream(&mut self, _: &CopyAsHexStream, _window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::HexStream, cx);
    }

    fn copy_as_hex_spaces(&mut self, _: &CopyAsHexSpaces, _window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::HexWithSpaces, cx);
    }

    fn copy_as_printable_text(&mut self, _: &CopyAsPrintableText, _window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::PrintableText, cx);
    }

    fn copy_as_base64(&mut self, _: &CopyAsBase64, _window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::Base64, cx);
    }

    fn copy_as_escaped_string(&mut self, _: &CopyAsEscapedString, _window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::EscapedString, cx);
    }

    fn copy_as_binary(&mut self, _: &CopyAsBinary, _window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::Binary, cx);
    }

    fn copy_as_rust_array(&mut self, _: &CopyAsRustArray, _window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::RustArray, cx);
    }

    fn copy_as_json_array(&mut self, _: &CopyAsJsonArray, _window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::JsonArray, cx);
    }

    fn apply_highlight(&mut self, color: Option<Hsla>, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            if let Some(range) = editor.selected_range_or_cursor() {
                if let Some(color) = color {
                    editor.add_custom_highlight(range, color);
                } else {
                    editor.clear_custom_highlight(range);
                }
                cx.notify();
            }
        });
    }

    fn highlight_red(&mut self, _: &HighlightRed, _window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(Some(hsla(0.0, 0.75, 0.55, 0.35)), cx);
    }

    fn highlight_orange(&mut self, _: &HighlightOrange, _window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(Some(hsla(30.0 / 360.0, 0.85, 0.55, 0.35)), cx);
    }

    fn highlight_yellow(&mut self, _: &HighlightYellow, _window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(Some(hsla(50.0 / 360.0, 0.85, 0.50, 0.35)), cx);
    }

    fn highlight_green(&mut self, _: &HighlightGreen, _window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(Some(hsla(120.0 / 360.0, 0.65, 0.45, 0.35)), cx);
    }

    fn highlight_cyan(&mut self, _: &HighlightCyan, _window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(Some(hsla(180.0 / 360.0, 0.70, 0.45, 0.35)), cx);
    }

    fn highlight_blue(&mut self, _: &HighlightBlue, _window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(Some(hsla(215.0 / 360.0, 0.75, 0.55, 0.35)), cx);
    }

    fn highlight_purple(&mut self, _: &HighlightPurple, _window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(Some(hsla(280.0 / 360.0, 0.70, 0.55, 0.35)), cx);
    }

    fn highlight_pink(&mut self, _: &HighlightPink, _window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(Some(hsla(330.0 / 360.0, 0.75, 0.55, 0.35)), cx);
    }

    fn clear_highlight(&mut self, _: &ClearHighlight, _window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(None, cx);
    }

    fn clear_all_highlights(&mut self, _: &ClearAllHighlights, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.editor.read(cx).highlights.len();
        if count == 0 {
            return;
        }

        let prompt = window.prompt(
            gpui::PromptLevel::Warning,
            "Clear all highlights?",
            Some(&format!(
                "Are you sure you want to clear all {} highlight{} and comments? This action cannot be undone.",
                count,
                if count == 1 { "" } else { "s" }
            )),
            &["Clear All", "Cancel"],
            cx,
        );

        let editor = self.editor.clone();
        cx.spawn_in(window, async move |_this, window| {
            if let Ok(0) = prompt.await {
                window
                    .update(|_, cx| {
                        editor.update(cx, |editor, cx| {
                            editor.clear_all_custom_highlights();
                            cx.notify();
                        });
                    })
                    .ok();
            }
        })
        .detach();
    }

    fn set_radix_hex(&mut self, _: &SetRadixHex, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |ed, cx| {
            ed.set_radix(DisplayRadix::Hexadecimal);
            cx.notify();
        });
    }

    fn set_radix_dec(&mut self, _: &SetRadixDec, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |ed, cx| {
            ed.set_radix(DisplayRadix::Decimal);
            cx.notify();
        });
    }

    fn set_radix_oct(&mut self, _: &SetRadixOct, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |ed, cx| {
            ed.set_radix(DisplayRadix::Octal);
            cx.notify();
        });
    }

    fn set_radix_bin(&mut self, _: &SetRadixBin, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |ed, cx| {
            ed.set_radix(DisplayRadix::Binary);
            cx.notify();
        });
    }

    fn set_group_size_1(&mut self, _: &SetGroupSize1, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |ed, cx| {
            ed.set_group_size(ByteGroupSize::One);
            cx.notify();
        });
    }

    fn set_group_size_2(&mut self, _: &SetGroupSize2, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |ed, cx| {
            ed.set_group_size(ByteGroupSize::Two);
            cx.notify();
        });
    }

    fn set_group_size_4(&mut self, _: &SetGroupSize4, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |ed, cx| {
            ed.set_group_size(ByteGroupSize::Four);
            cx.notify();
        });
    }

    fn set_group_size_8(&mut self, _: &SetGroupSize8, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |ed, cx| {
            ed.set_group_size(ByteGroupSize::Eight);
            cx.notify();
        });
    }

    fn set_byte_order_le(&mut self, _: &SetByteOrderLittleEndian, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |ed, cx| {
            ed.set_is_big_endian(false);
            cx.notify();
        });
    }

    fn set_byte_order_be(&mut self, _: &SetByteOrderBigEndian, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |ed, cx| {
            ed.set_is_big_endian(true);
            cx.notify();
        });
    }

    fn toggle_byte_order(&mut self, _: &ToggleByteOrder, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |ed, cx| {
            ed.toggle_byte_order();
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

        let parse_result = editor.parse_result.as_ref();
        let is_struct_mode = editor.show_inline_structure_view && parse_result.is_some();
        let _max_bytes_per_row = line_starts.max_bytes_per_row();

        let offset_width = if is_struct_mode {
            self.address_col_width + SECTION_GAP
        } else if self.show_offset {
            OFFSET_WIDTH + SECTION_GAP
        } else {
            0.0
        };

        let base_x = f32::from(sample_left) + 8.0;
        let hex_start_x = base_x + offset_width;
        let hex_end_x = hex_start_x + self.hex_col_width;
        let desc_start_x = hex_end_x + SECTION_GAP;
        let rel_x = f32::from(point.x);

        if is_struct_mode && rel_x >= desc_start_x {
            if let Some(parse_res) = parse_result {
                let leaf_fields = parse_res.find_leaf_fields_starting_at(line_offset, chunk_len);
                if let Some(first) = leaf_fields.first() {
                    return Some(first.offset);
                }
            }
            return Some(line_offset);
        }

        let group_bytes = self.group_size.byte_count();
        let (item_width, item_gap) = item_metrics(self.radix, self.group_size, self.font_size_prop);
        let item_step = item_width + item_gap;

        let byte_offset_in_row = if !is_struct_mode && self.show_ascii && rel_x >= hex_end_x + SECTION_GAP {
            let ascii_x = (rel_x - (hex_end_x + SECTION_GAP)).max(0.0);
            (ascii_x / 10.0) as usize
        } else {
            let col_x = (rel_x - hex_start_x + self.hex_scroll_x).max(0.0);
            let item_idx = (col_x / item_step) as usize;
            let within_item_x = col_x - item_idx as f32 * item_step;

            let mut chunk_idx = 0;
            let mut curr_item = 0;
            let mut target_item_info = None;

            while chunk_idx < chunk_len {
                let item_start_offset = line_offset + chunk_idx;
                let start_slot = item_start_offset % group_bytes;
                let max_in_group = group_bytes - start_slot;
                let item_slice_len = (chunk_len - chunk_idx).min(max_in_group);

                if curr_item == item_idx {
                    target_item_info = Some((chunk_idx, start_slot, item_slice_len));
                    break;
                }

                chunk_idx += item_slice_len;
                curr_item += 1;
                if chunk_idx < chunk_len {
                    let next_start_slot = (line_offset + chunk_idx) % group_bytes;
                    let next_max = group_bytes - next_start_slot;
                    target_item_info = Some((chunk_idx, next_start_slot, (chunk_len - chunk_idx).min(next_max)));
                }
            }

            let (item_chunk_start, start_slot, item_slice_len) = target_item_info.unwrap_or((0, line_offset % group_bytes, chunk_len.min(group_bytes)));
            let slot_w = item_width / group_bytes as f32;
            let slot_idx = ((within_item_x / slot_w) as usize).min(group_bytes.saturating_sub(1));
            let byte_in_item = if slot_idx < start_slot {
                0
            } else {
                (slot_idx - start_slot).min(item_slice_len.saturating_sub(1))
            };
            item_chunk_start + byte_in_item
        };

        let byte_idx = byte_offset_in_row.min(chunk_len.saturating_sub(1));
        Some(line_offset + byte_idx)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_hex_row(
        row_idx: usize,
        top_visible_row: usize,
        doc: &Document,
        line_starts: &crate::core::editor::LineMap,
        parse_result: Option<Arc<ParseResult>>,
        collapsed_structs: Option<Arc<std::collections::HashSet<String>>>,
        _max_bytes_per_row: usize,
        encoding: Encoding,
        radix: DisplayRadix,
        group_size: ByteGroupSize,
        is_big_endian: bool,
        cursor_offset: usize,
        min_sel: usize,
        max_sel: usize,
        highlights: &Arc<Vec<(Range<usize>, Hsla)>>,
        highlight_items: &Arc<Vec<crate::core::highlight::HighlightItem>>,
        max_highlight_len: usize,
        show_offset: bool,
        show_ascii: bool,
        is_focused: bool,
        hex_scroll_x: f32,
        desc_scroll_x: f32,
        comment_scroll_x: f32,
        address_col_width: f32,
        hex_col_width: f32,
        desc_col_width: f32,
        comment_col_width: f32,
        _font_family: &SharedString,
        font_size: Pixels,
        view: Entity<Self>,
        _focus_handle: &FocusHandle,
    ) -> AnyElement {
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

        let is_struct_mode = parse_result.is_some();
        let visible_row_view = view.clone();
        let collapsed_structs_arc = collapsed_structs.clone();
        let highlights_arc = highlights.clone();
        let highlight_items_arc = highlight_items.clone();
        let line_starts_clone = line_starts.clone();

        div()
            .id(row_idx)
            .w_full()
            .h(px(ROW_HEIGHT))
            .child(canvas(
                move |bounds, _window, cx| {
                    visible_row_view.update(cx, |this, _cx| {
                        this.visible_row_info.set(Some((row_idx, bounds.top(), bounds.left())));
                    });
                },
                move |bounds, _prepaint, window, cx| {
                    let active_row_highlights = row_highlights(&highlights_arc, max_highlight_len, offset, next_offset);
                    let (selection_bg, cursor_bg, muted_color, fg_color, accent_fg_color, border_color, _sidebar_bg, bg_color_theme) = {
                        let theme = cx.theme();
                        (
                            if is_focused { theme.selection } else { theme.muted_foreground.opacity(0.3) },
                            theme.accent,
                            theme.muted_foreground,
                            theme.foreground,
                            theme.accent_foreground,
                            theme.border,
                            theme.sidebar,
                            theme.background,
                        )
                    };
                    let line_height = px(ROW_HEIGHT);
                    let font = window.text_style().font();

                    // 1. Draw Left Columns (Address OR Offset)
                    let (offset_w, gap) = if is_struct_mode {
                        let addr_str = format_offset_08(offset);
                        let run = gpui::TextRun {
                            len: addr_str.len(),
                            font: font.clone(),
                            color: muted_color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let shaped = window.text_system().shape_line(addr_str, font_size, &[run], None);
                        let addr_pos = point(bounds.left() + px(8.0), bounds.top() + px(2.0));
                        let _ = shaped.paint(addr_pos, line_height, window, cx);
                        (address_col_width, SECTION_GAP)
                    } else {
                        if show_offset {
                            let offset_str = format_offset_08(offset);
                            let run = gpui::TextRun {
                                len: offset_str.len(),
                                font: font.clone(),
                                color: muted_color,
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            };
                            let shaped = window.text_system().shape_line(offset_str, font_size, &[run], None);
                            let offset_pos = point(bounds.left() + px(8.0), bounds.top() + px(2.0));
                            let _ = shaped.paint(offset_pos, line_height, window, cx);
                        }
                        (if show_offset { OFFSET_WIDTH } else { 0.0 }, SECTION_GAP)
                    };

                    let base_x = bounds.left() + px(8.0);
                    let hex_start_x = base_x + px(offset_w + gap);
                    let hex_end_x = hex_start_x + px(hex_col_width);
                    let (comment_start_x, ascii_width) = if is_struct_mode {
                        let desc_start_x = hex_end_x + px(gap);
                        let comment_start_x = desc_start_x + px(desc_col_width + gap);
                        (comment_start_x, 0.0)
                    } else {
                        let ascii_w = if show_ascii { _max_bytes_per_row as f32 * 10.0 } else { 0.0 };
                        let comment_start_x = if show_ascii {
                            hex_end_x + px(gap + ascii_w + gap)
                        } else {
                            hex_end_x + px(gap)
                        };
                        (comment_start_x, ascii_w)
                    };

                    // Vertical Column Divider Borders (matching header splitters exactly)
                    let border_line_color = border_color.opacity(0.4);
                    if is_struct_mode || show_offset {
                        let div1_x = base_x + px(offset_w + (gap / 2.0));
                        window.paint_quad(gpui::fill(
                            Bounds::new(point(div1_x, bounds.top()), size(px(1.0), px(ROW_HEIGHT))),
                            border_line_color,
                        ));
                    }
                    let div2_x = hex_start_x + px(hex_col_width + (gap / 2.0));
                    window.paint_quad(gpui::fill(
                        Bounds::new(point(div2_x, bounds.top()), size(px(1.0), px(ROW_HEIGHT))),
                        border_line_color,
                    ));
                    if is_struct_mode {
                        let desc_start_x = hex_end_x + px(gap);
                        let div3_x = desc_start_x + px(desc_col_width + (gap / 2.0));
                        let div4_x = comment_start_x + px(comment_col_width + (gap / 2.0));
                        window.paint_quad(gpui::fill(
                            Bounds::new(point(div3_x, bounds.top()), size(px(1.0), px(ROW_HEIGHT))),
                            border_line_color,
                        ));
                        window.paint_quad(gpui::fill(
                            Bounds::new(point(div4_x, bounds.top()), size(px(1.0), px(ROW_HEIGHT))),
                            border_line_color,
                        ));
                    } else {
                        if show_ascii {
                            let div3_x = hex_end_x + px(gap + ascii_width + (gap / 2.0));
                            window.paint_quad(gpui::fill(
                                Bounds::new(point(div3_x, bounds.top()), size(px(1.0), px(ROW_HEIGHT))),
                                border_line_color,
                            ));
                        }
                        let div4_x = comment_start_x + px(comment_col_width + (gap / 2.0));
                        window.paint_quad(gpui::fill(
                            Bounds::new(point(div4_x, bounds.top()), size(px(1.0), px(ROW_HEIGHT))),
                            border_line_color,
                        ));
                    }

                    // 2. Background Quads Pass for Data Items (with clipping mask)
                    let hex_mask_bounds = Bounds::new(point(hex_start_x, bounds.top()), size(px(hex_col_width), px(ROW_HEIGHT)));
                    let group_bytes = group_size.byte_count();
                    let (item_width, item_gap) = item_metrics(radix, group_size, font_size);
                    let item_step = item_width + item_gap;

                    window.with_content_mask(Some(gpui::ContentMask { bounds: hex_mask_bounds }), |window| {
                        let mut chunk_idx = 0;
                        let mut item_idx = 0;
                        while chunk_idx < chunk.len() {
                            let item_start_offset = offset + chunk_idx;
                            let start_slot = item_start_offset % group_bytes;
                            let max_in_group = group_bytes - start_slot;
                            let item_slice_len = (chunk.len() - chunk_idx).min(max_in_group);
                            let item_end_offset = item_start_offset + item_slice_len;

                            let is_cursor = cursor_offset >= item_start_offset && cursor_offset < item_end_offset;
                            let is_selected = if min_sel <= max_sel {
                                let sel_start = min_sel;
                                let sel_end = max_sel;
                                item_start_offset <= sel_end && item_end_offset > sel_start
                            } else {
                                false
                            };

                            let mut bg_color = if is_selected { selection_bg } else { hsla(0.0, 0.0, 0.0, 0.0) };
                            let mut current_hl_color = None;

                            if !active_row_highlights.is_empty() {
                                let mut smallest_len = usize::MAX;
                                for (range, color) in active_row_highlights.iter() {
                                    if range.start < item_end_offset && range.end > item_start_offset {
                                        let len = range.end.saturating_sub(range.start);
                                        if len <= smallest_len {
                                            smallest_len = len;
                                            bg_color = *color;
                                            current_hl_color = Some(*color);
                                        }
                                    }
                                }
                            }

                            let next_start_offset = item_end_offset;
                            let next_is_selected =
                                is_selected && min_sel <= max_sel && next_start_offset <= max_sel && (chunk_idx + item_slice_len < chunk.len());
                            let next_has_same_highlight = if let Some(cur_col) = current_hl_color {
                                if chunk_idx + item_slice_len < chunk.len() {
                                    let mut next_col = None;
                                    let mut smallest_len = usize::MAX;
                                    for (range, color) in active_row_highlights.iter() {
                                        if range.start < next_start_offset + 1 && range.end > next_start_offset {
                                            let len = range.end.saturating_sub(range.start);
                                            if len <= smallest_len {
                                                smallest_len = len;
                                                next_col = Some(*color);
                                            }
                                        }
                                    }
                                    next_col == Some(cur_col)
                                } else {
                                    false
                                }
                            } else {
                                false
                            };

                            let fill_width = if next_is_selected || next_has_same_highlight { item_step } else { item_width };
                            let item_draw_x = hex_start_x - px(hex_scroll_x) + px(item_idx as f32 * item_step);

                            let item_fill_bounds = Bounds::new(point(item_draw_x, bounds.top() + px(1.0)), size(px(fill_width), px(ROW_HEIGHT - 2.0)));

                            let item_box_bounds = Bounds::new(point(item_draw_x, bounds.top() + px(1.0)), size(px(item_width), px(ROW_HEIGHT - 2.0)));

                            if bg_color.a > 0.0 {
                                window.paint_quad(gpui::fill(item_fill_bounds, bg_color));
                            }

                            if is_cursor {
                                let cursor_border_color = if is_focused { cursor_bg } else { muted_color.opacity(0.6) };
                                paint_border_box(window, item_box_bounds, px(1.5), cursor_border_color);
                            }

                            chunk_idx += item_slice_len;
                            item_idx += 1;
                        }

                        // 3. Text Pass for Data Items
                        let mut chunk_idx = 0;
                        let mut item_idx = 0;
                        while chunk_idx < chunk.len() {
                            let item_start_offset = offset + chunk_idx;
                            let start_slot = item_start_offset % group_bytes;
                            let max_in_group = group_bytes - start_slot;
                            let item_slice_len = (chunk.len() - chunk_idx).min(max_in_group);
                            let item_slice = &chunk[chunk_idx..chunk_idx + item_slice_len];
                            let item_end_offset = item_start_offset + item_slice_len;

                            let is_cursor = cursor_offset >= item_start_offset && cursor_offset < item_end_offset;
                            let is_zero = is_group_zero(item_slice);

                            let text_color = if is_cursor && is_focused {
                                fg_color
                            } else if is_zero {
                                muted_color.opacity(0.5)
                            } else {
                                fg_color
                            };

                            let item_str = SharedString::from(format_group(item_slice, start_slot, radix, group_size, is_big_endian));
                            let run = gpui::TextRun {
                                len: item_str.len(),
                                font: font.clone(),
                                color: text_color,
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            };
                            let shaped_item = window.text_system().shape_line(item_str, font_size, &[run], None);
                            let item_pos = point(hex_start_x - px(hex_scroll_x) + px(item_idx as f32 * item_step + 3.0), bounds.top() + px(2.0));
                            let _ = shaped_item.paint(item_pos, line_height, window, cx);

                            chunk_idx += item_slice_len;
                            item_idx += 1;
                        }

                        // Left-edge subtle gradient fade when hex_scroll_x > 1.0
                        if hex_scroll_x > 1.0 {
                            let bg = bg_color_theme;
                            for step in 0..5 {
                                let x = hex_start_x + px(step as f32 * 3.2);
                                let alpha = 1.0 - (step as f32 / 5.0);
                                window.paint_quad(gpui::fill(
                                    Bounds::new(point(x, bounds.top()), size(px(3.4), px(ROW_HEIGHT))),
                                    bg.opacity(alpha * 0.95),
                                ));
                            }
                        }

                        // Right-edge subtle gradient fade when row data overflows hex_col_width
                        let total_row_data_width = item_idx as f32 * item_step;
                        if hex_scroll_x + hex_col_width < total_row_data_width - 1.0 {
                            let fade_w = 22.0;
                            let fade_start = hex_end_x - px(fade_w);
                            let bg = bg_color_theme;
                            for step in 0..6 {
                                let x = fade_start + px(step as f32 * 3.6);
                                let alpha = (step + 1) as f32 / 7.0;
                                window.paint_quad(gpui::fill(
                                    Bounds::new(point(x, bounds.top()), size(px(3.8), px(ROW_HEIGHT))),
                                    bg.opacity(alpha * 0.95),
                                ));
                            }
                        }
                    });

                    // 3. ASCII Column (when not in structure definition mode and ASCII view is enabled)
                    if !is_struct_mode && show_ascii {
                        let ascii_start_x = hex_end_x + px(gap);
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

                        for (j, _) in chunk.iter().enumerate() {
                            let byte_pos = offset + j;
                            let is_cursor = byte_pos == cursor_offset;
                            let is_selected = byte_pos >= min_sel && byte_pos <= max_sel;

                            let mut bg_color = if is_selected { selection_bg } else { hsla(0.0, 0.0, 0.0, 0.0) };

                            if !active_row_highlights.is_empty() {
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

                            let ascii_item_bounds = Bounds::new(
                                point(ascii_start_x + px(j as f32 * 10.0), bounds.top() + px(1.0)),
                                size(px(10.0), px(ROW_HEIGHT - 2.0)),
                            );

                            if bg_color.a > 0.0 {
                                window.paint_quad(gpui::fill(ascii_item_bounds, bg_color));
                            }

                            if is_cursor {
                                let cursor_border_color = if is_focused { cursor_bg } else { muted_color.opacity(0.6) };
                                paint_border_box(window, ascii_item_bounds, px(1.5), cursor_border_color);
                            }
                        }

                        for (j, opt) in char_map.into_iter().enumerate() {
                            if let Some((c, _)) = opt {
                                let is_control = (c as u32) < 0x20 || (c as u32) == 0x7f;
                                let text_color = if is_control || c == '·' { muted_color.opacity(0.4) } else { fg_color };

                                let char_str = SharedString::from(c.to_string());
                                let run = gpui::TextRun {
                                    len: char_str.len(),
                                    font: font.clone(),
                                    color: text_color,
                                    background_color: None,
                                    underline: None,
                                    strikethrough: None,
                                };
                                let shaped_ascii = window.text_system().shape_line(char_str, font_size, &[run], None);
                                let ascii_pos = point(ascii_start_x + px(j as f32 * 10.0 + 1.0), bounds.top() + px(2.0));
                                let _ = shaped_ascii.paint(ascii_pos, line_height, window, cx);
                            }
                        }
                    }

                    // 4. Description Column (when structure definition is present)
                    if let Some(ref parse_res) = parse_result {
                        let desc_start_x = hex_end_x + px(SECTION_GAP);
                        let desc_end_x = desc_start_x + px(desc_col_width);

                        let active_ranges = parse_res.find_active_struct_ranges(offset, chunk_len);
                        let container_structs = parse_res.find_container_structs_starting_at(offset, chunk_len);
                        let leaf_fields = parse_res.find_leaf_fields_starting_at(offset, chunk_len);

                        let is_collapsed = container_structs
                            .first()
                            .map(|c| collapsed_structs_arc.as_ref().map(|s| s.contains(&c.id)).unwrap_or(false))
                            .unwrap_or(false);

                        let struct_depth = active_ranges.len().saturating_sub(1);
                        let indent_level = if !container_structs.is_empty() {
                            active_ranges
                                .iter()
                                .find(|r| container_structs.first().map(|c| c.id == r.3).unwrap_or(false))
                                .map(|r| r.2)
                                .unwrap_or(struct_depth)
                        } else {
                            active_ranges.len()
                        };
                        let indent_px = indent_level as f32 * 14.0;

                        let mut desc_parts = Vec::new();
                        if let Some(container) = container_structs.first() {
                            let icon = if is_collapsed { "▶" } else { "▼" };
                            if is_collapsed {
                                desc_parts.push(format!("{} {} ({} bytes)", icon, container.id, container.size));
                            } else {
                                desc_parts.push(format!("{} {}", icon, container.id));
                            }
                        }
                        if !is_collapsed {
                            for f in &leaf_fields {
                                desc_parts.push(f.format_expression());
                            }
                        }

                        if !desc_parts.is_empty() {
                            let expr_shared = SharedString::from(desc_parts.join("  "));
                            let text_color = if !container_structs.is_empty() { accent_fg_color } else { fg_color };
                            let run = gpui::TextRun {
                                len: expr_shared.len(),
                                font: font.clone(),
                                color: text_color,
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            };
                            let shaped_expr = window.text_system().shape_line(expr_shared, font_size, &[run], None);
                            let desc_mask_bounds = Bounds::new(point(desc_start_x, bounds.top()), size(px(desc_col_width), px(ROW_HEIGHT)));
                            let desc_text_width = f32::from(shaped_expr.width) + indent_px + 8.0;

                            window.with_content_mask(Some(gpui::ContentMask { bounds: desc_mask_bounds }), |window| {
                                let _ = shaped_expr.paint(
                                    point(desc_start_x - px(desc_scroll_x) + px(indent_px), bounds.top() + px(2.0)),
                                    line_height,
                                    window,
                                    cx,
                                );

                                // Left fade
                                if desc_scroll_x > 1.0 {
                                    let bg = bg_color_theme;
                                    for step in 0..5 {
                                        let x = desc_start_x + px(step as f32 * 3.2);
                                        let alpha = 1.0 - (step as f32 / 5.0);
                                        window.paint_quad(gpui::fill(
                                            Bounds::new(point(x, bounds.top()), size(px(3.4), px(ROW_HEIGHT))),
                                            bg.opacity(alpha * 0.95),
                                        ));
                                    }
                                }

                                // Right fade
                                if desc_scroll_x + desc_col_width < desc_text_width - 1.0 {
                                    let fade_w = 20.0;
                                    let fade_start = desc_end_x - px(fade_w);
                                    let bg = bg_color_theme;
                                    for step in 0..5 {
                                        let x = fade_start + px(step as f32 * 4.0);
                                        let alpha = (step + 1) as f32 / 6.0;
                                        window.paint_quad(gpui::fill(
                                            Bounds::new(point(x, bounds.top()), size(px(4.2), px(ROW_HEIGHT))),
                                            bg.opacity(alpha * 0.95),
                                        ));
                                    }
                                }
                            });
                        }
                    }

                    // 5. Highlight Comments Column
                    // Only display comment once per highlight in the visible range:
                    // - On the highlight's starting row if visible (>= top_visible_row)
                    // - Or on top_visible_row if the highlight started before top_visible_row but extends into it
                    let row_highlight_comments: Vec<(gpui::Hsla, SharedString)> = highlight_items_arc
                        .iter()
                        .filter(|h| {
                            if h.comment.trim().is_empty() {
                                return false;
                            }
                            let h_start_row = Editor::find_line_index(h.offset, &line_starts_clone);
                            let h_end_offset = h.offset.saturating_add(h.size);
                            let h_last_byte = h_end_offset.saturating_sub(1).max(h.offset);
                            let h_end_row = Editor::find_line_index(h_last_byte, &line_starts_clone);
                            let display_row = h_start_row.max(top_visible_row);
                            row_idx == display_row && row_idx <= h_end_row
                        })
                        .map(|h| (h.color.to_badge_hsla(), SharedString::from(h.comment.trim().to_string())))
                        .collect();

                    if !row_highlight_comments.is_empty() {
                        let comment_mask_bounds = Bounds::new(point(comment_start_x, bounds.top()), size(px(comment_col_width), px(ROW_HEIGHT)));
                        let comment_end_x = comment_start_x + px(comment_col_width);

                        let dot_size = 8.0;
                        let dot_radius = 4.0;
                        let dot_margin_right = 5.0;
                        let item_spacing = 14.0;

                        let mut shaped_items = Vec::new();
                        let mut total_content_width = 4.0;

                        for (badge_color, comment_shared) in &row_highlight_comments {
                            let run = gpui::TextRun {
                                len: comment_shared.len(),
                                font: font.clone(),
                                color: muted_color,
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            };
                            let shaped_comment = window.text_system().shape_line(comment_shared.clone(), font_size, &[run], None);
                            let text_w = f32::from(shaped_comment.width);
                            shaped_items.push((*badge_color, shaped_comment, text_w));
                            total_content_width += dot_size + dot_margin_right + text_w + item_spacing;
                        }
                        let comment_text_width = total_content_width;

                        window.with_content_mask(Some(gpui::ContentMask { bounds: comment_mask_bounds }), |window| {
                            let mut cur_x = comment_start_x - px(comment_scroll_x) + px(4.0);
                            let dot_y = bounds.top() + px((ROW_HEIGHT - dot_size) / 2.0);

                            for (badge_color, shaped_comment, text_w) in shaped_items {
                                // Highlight colored circle dot
                                let dot_bounds = Bounds::new(point(cur_x, dot_y), size(px(dot_size), px(dot_size)));
                                let mut dot_quad = gpui::fill(dot_bounds, badge_color);
                                dot_quad.corner_radii = gpui::Corners {
                                    top_left: px(dot_radius),
                                    top_right: px(dot_radius),
                                    bottom_left: px(dot_radius),
                                    bottom_right: px(dot_radius),
                                };
                                window.paint_quad(dot_quad);

                                // Comment text
                                let text_x = cur_x + px(dot_size + dot_margin_right);
                                let _ = shaped_comment.paint(point(text_x, bounds.top() + px(2.0)), line_height, window, cx);

                                cur_x = text_x + px(text_w + item_spacing);
                            }

                            // Left fade
                            if comment_scroll_x > 1.0 {
                                let bg = bg_color_theme;
                                for step in 0..5 {
                                    let x = comment_start_x + px(step as f32 * 3.2);
                                    let alpha = 1.0 - (step as f32 / 5.0);
                                    window.paint_quad(gpui::fill(
                                        Bounds::new(point(x, bounds.top()), size(px(3.4), px(ROW_HEIGHT))),
                                        bg.opacity(alpha * 0.95),
                                    ));
                                }
                            }

                            // Right fade
                            if comment_scroll_x + comment_col_width < comment_text_width - 1.0 {
                                let fade_w = 20.0;
                                let fade_start = comment_end_x - px(fade_w);
                                let bg = bg_color_theme;
                                for step in 0..5 {
                                    let x = fade_start + px(step as f32 * 4.0);
                                    let alpha = (step + 1) as f32 / 6.0;
                                    window.paint_quad(gpui::fill(
                                        Bounds::new(point(x, bounds.top()), size(px(4.2), px(ROW_HEIGHT))),
                                        bg.opacity(alpha * 0.95),
                                    ));
                                }
                            }
                        });
                    }
                },
            ))
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
        let font_size = self.font_size_prop;

        let (total_rows, max_bytes_per_row, is_struct_mode) = {
            let editor = self.editor.read(cx);
            let line_starts = editor.line_starts();
            (
                line_starts.len().max(1),
                line_starts.max_bytes_per_row(),
                editor.show_inline_structure_view && editor.parse_result.is_some(),
            )
        };

        let total_width = {
            let mut w = 8.0;
            if is_struct_mode {
                w += self.address_col_width + SECTION_GAP;
            } else if self.show_offset {
                w += OFFSET_WIDTH + SECTION_GAP;
            }
            w += self.hex_col_width + SECTION_GAP;
            if is_struct_mode {
                w += self.desc_col_width + SECTION_GAP;
            } else if self.show_ascii {
                w += (max_bytes_per_row as f32 * 10.0) + SECTION_GAP;
            }
            w += self.comment_col_width + SECTION_GAP;
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

        let (item_width, item_gap) = item_metrics(self.radix, self.group_size, font_size);
        let item_step = item_width + item_gap;
        let group_bytes = self.group_size.byte_count();
        let items_in_row = max_bytes_per_row.div_ceil(group_bytes).max(1);
        let total_data_width = items_in_row as f32 * item_step;
        let max_hex_scroll = (total_data_width - self.hex_col_width).max(0.0);
        let is_hex_clipped_left = self.hex_scroll_x > 1.0;
        let is_hex_clipped_right = self.hex_scroll_x < max_hex_scroll - 1.0;

        let header = if self.show_header {
            let mut hex_cols = Vec::with_capacity(items_in_row);
            for i in 0..items_in_row {
                let byte_offset = i * group_bytes;
                let label = SharedString::from(format!("+{:X}", byte_offset));
                hex_cols.push(
                    div()
                        .w(px(item_width))
                        .mr(px(item_gap))
                        .flex_none()
                        .text_center()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(label),
                );
            }

            let comment_header_el = |width: f32, theme: &gpui_component::Theme| {
                h_flex()
                    .w(px(width + SECTION_GAP))
                    .child(div().w(px(width)).text_xs().text_color(theme.muted_foreground).child("Comment").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                            if event.click_count >= 2 {
                                this.auto_fit_column(ResizingColumn::Comment, cx);
                            }
                        }),
                    ))
                    .child(
                        div()
                            .w(px(SECTION_GAP))
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor(gpui::CursorStyle::ResizeLeftRight)
                            .hover(|s| s.bg(theme.accent.opacity(0.2)))
                            .child(div().w(px(1.0)).h(px(16.0)).bg(theme.border))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                    if event.click_count >= 2 {
                                        this.resizing_column = None;
                                        this.auto_fit_column(ResizingColumn::Comment, cx);
                                    } else {
                                        this.resizing_column = Some((ResizingColumn::Comment, event.position.x.into(), this.comment_col_width));
                                        cx.notify();
                                    }
                                }),
                            ),
                    )
            };

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
                .child(if is_struct_mode {
                    h_flex()
                        .w(px(self.address_col_width + SECTION_GAP))
                        .child(
                            div()
                                .w(px(self.address_col_width))
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("Address")
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                        if event.click_count >= 2 {
                                            this.auto_fit_column(ResizingColumn::Address, cx);
                                        }
                                    }),
                                ),
                        )
                        .child(
                            div()
                                .w(px(SECTION_GAP))
                                .h_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor(gpui::CursorStyle::ResizeLeftRight)
                                .hover(|s| s.bg(theme.accent.opacity(0.2)))
                                .child(div().w(px(1.0)).h(px(16.0)).bg(theme.border))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                        if event.click_count >= 2 {
                                            this.resizing_column = None;
                                            this.auto_fit_column(ResizingColumn::Address, cx);
                                        } else {
                                            this.resizing_column = Some((ResizingColumn::Address, event.position.x.into(), this.address_col_width));
                                            cx.notify();
                                        }
                                    }),
                                ),
                        )
                        .into_any_element()
                } else if self.show_offset {
                    h_flex()
                        .w(px(OFFSET_WIDTH + SECTION_GAP))
                        .child(div().w(px(OFFSET_WIDTH)).text_xs().text_color(theme.muted_foreground).child("Offset"))
                        .child(
                            div()
                                .w(px(SECTION_GAP))
                                .h_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(div().w(px(1.0)).h(px(16.0)).bg(theme.border)),
                        )
                        .into_any_element()
                } else {
                    div().into_any_element()
                })
                .child(
                    h_flex()
                        .w(px(self.hex_col_width + SECTION_GAP))
                        .child(
                            div()
                                .w(px(self.hex_col_width))
                                .overflow_hidden()
                                .relative()
                                .child(h_flex().ml(px(-self.hex_scroll_x)).children(hex_cols))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                        if event.click_count >= 2 {
                                            this.auto_fit_column(ResizingColumn::Hex, cx);
                                        }
                                    }),
                                )
                                .when(is_hex_clipped_left, |el| {
                                    el.child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .left_0()
                                            .bottom_0()
                                            .w(px(18.0))
                                            .flex()
                                            .items_center()
                                            .justify_start()
                                            .pl_1()
                                            .bg(theme.sidebar.opacity(0.85))
                                            .text_xs()
                                            .font_semibold()
                                            .text_color(theme.muted_foreground)
                                            .child("…"),
                                    )
                                })
                                .when(is_hex_clipped_right, |el| {
                                    el.child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .right_0()
                                            .bottom_0()
                                            .w(px(18.0))
                                            .flex()
                                            .items_center()
                                            .justify_end()
                                            .pr_1()
                                            .bg(theme.sidebar.opacity(0.85))
                                            .text_xs()
                                            .font_semibold()
                                            .text_color(theme.muted_foreground)
                                            .child("…"),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .w(px(SECTION_GAP))
                                .h_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor(gpui::CursorStyle::ResizeLeftRight)
                                .hover(|s| s.bg(theme.accent.opacity(0.2)))
                                .child(div().w(px(1.0)).h(px(16.0)).bg(theme.border))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                        if event.click_count >= 2 {
                                            this.resizing_column = None;
                                            this.auto_fit_column(ResizingColumn::Hex, cx);
                                        } else {
                                            this.resizing_column = Some((ResizingColumn::Hex, event.position.x.into(), this.hex_col_width));
                                            cx.notify();
                                        }
                                    }),
                                ),
                        ),
                )
                .child(if is_struct_mode {
                    h_flex()
                        .child(
                            h_flex()
                                .w(px(self.desc_col_width + SECTION_GAP))
                                .child(
                                    div()
                                        .w(px(self.desc_col_width))
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child("Description")
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                                if event.click_count >= 2 {
                                                    this.auto_fit_column(ResizingColumn::Description, cx);
                                                }
                                            }),
                                        ),
                                )
                                .child(
                                    div()
                                        .w(px(SECTION_GAP))
                                        .h_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor(gpui::CursorStyle::ResizeLeftRight)
                                        .hover(|s| s.bg(theme.accent.opacity(0.2)))
                                        .child(div().w(px(1.0)).h(px(16.0)).bg(theme.border))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                                if event.click_count >= 2 {
                                                    this.resizing_column = None;
                                                    this.auto_fit_column(ResizingColumn::Description, cx);
                                                } else {
                                                    this.resizing_column = Some((ResizingColumn::Description, event.position.x.into(), this.desc_col_width));
                                                    cx.notify();
                                                }
                                            }),
                                        ),
                                ),
                        )
                        .child(comment_header_el(self.comment_col_width, theme))
                        .into_any_element()
                } else if self.show_ascii {
                    let label = match self.encoding {
                        Encoding::Ascii => "ASCII",
                        Encoding::Utf8 => "UTF-8",
                        Encoding::Utf16Le => "UTF-16 LE",
                        Encoding::Utf16Be => "UTF-16 BE",
                    };
                    h_flex()
                        .child(
                            h_flex()
                                .w(px((max_bytes_per_row as f32 * 10.0) + SECTION_GAP))
                                .child(
                                    div()
                                        .w(px(max_bytes_per_row as f32 * 10.0))
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .w(px(SECTION_GAP))
                                        .h_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(div().w(px(1.0)).h(px(16.0)).bg(theme.border)),
                                ),
                        )
                        .child(comment_header_el(self.comment_col_width, theme))
                        .into_any_element()
                } else {
                    comment_header_el(self.comment_col_width, theme).into_any_element()
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
        let list_bounds_view = view.clone();

        container
            .track_focus(&self.focus_handle(cx))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .on_action(cx.listener(Self::move_left))
            .on_action(cx.listener(Self::move_right))
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(|this, _: &AppSelectAll, window, cx| {
                this.select_all(&SelectAll, window, cx);
            }))
            .on_action(cx.listener(Self::set_radix_hex))
            .on_action(cx.listener(Self::set_radix_dec))
            .on_action(cx.listener(Self::set_radix_oct))
            .on_action(cx.listener(Self::set_radix_bin))
            .on_action(cx.listener(Self::set_group_size_1))
            .on_action(cx.listener(Self::set_group_size_2))
            .on_action(cx.listener(Self::set_group_size_4))
            .on_action(cx.listener(Self::set_group_size_8))
            .on_action(cx.listener(Self::set_byte_order_le))
            .on_action(cx.listener(Self::set_byte_order_be))
            .on_action(cx.listener(Self::toggle_byte_order))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::copy_as_hexdump))
            .on_action(cx.listener(Self::copy_as_cpp_array))
            .on_action(cx.listener(Self::copy_as_hex_stream))
            .on_action(cx.listener(Self::copy_as_hex_spaces))
            .on_action(cx.listener(Self::copy_as_printable_text))
            .on_action(cx.listener(Self::copy_as_base64))
            .on_action(cx.listener(Self::copy_as_escaped_string))
            .on_action(cx.listener(Self::copy_as_binary))
            .on_action(cx.listener(Self::copy_as_rust_array))
            .on_action(cx.listener(Self::copy_as_json_array))
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
            .on_action(cx.listener(Self::highlight_red))
            .on_action(cx.listener(Self::highlight_orange))
            .on_action(cx.listener(Self::highlight_yellow))
            .on_action(cx.listener(Self::highlight_green))
            .on_action(cx.listener(Self::highlight_cyan))
            .on_action(cx.listener(Self::highlight_blue))
            .on_action(cx.listener(Self::highlight_purple))
            .on_action(cx.listener(Self::highlight_pink))
            .on_action(cx.listener(Self::clear_highlight))
            .on_action(cx.listener(Self::clear_all_highlights))
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
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.focus_handle.focus(window);
                    if let Some(target_pos) = this.offset_from_point(event.position, cx) {
                        this.editor.update(cx, |editor, cx| {
                            let in_selection = if let (Some(s), Some(e)) = (editor.selection_start, editor.selection_end) {
                                let min = s.min(e);
                                let max = s.max(e);
                                target_pos >= min && target_pos <= max
                            } else {
                                false
                            };
                            if !in_selection {
                                editor.set_cursor_offset(target_pos);
                                cx.notify();
                            }
                        });
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                if let Some((col, start_x, start_w)) = this.resizing_column {
                    let current_x: f32 = event.position.x.into();
                    let delta = current_x - start_x;
                    match col {
                        ResizingColumn::Address => {
                            this.address_col_width = (start_w + delta).max(60.0);
                        }
                        ResizingColumn::Hex => {
                            this.hex_col_width = (start_w + delta).max(100.0);
                        }
                        ResizingColumn::Description => {
                            this.desc_col_width = (start_w + delta).max(80.0);
                        }
                        ResizingColumn::Comment => {
                            this.comment_col_width = (start_w + delta).max(80.0);
                        }
                    }
                    this.clamp_scroll_offsets(cx);
                    cx.notify();
                    return;
                }
                if this.is_selecting
                    && let Some(target_pos) = this.offset_from_point(event.position, cx)
                {
                    this.editor.update(cx, |editor, cx| {
                        let prev_end = editor.selection_end;
                        if prev_end != Some(target_pos) {
                            editor.continue_drag(target_pos);
                            cx.notify();
                        }
                    });
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                    if this.resizing_column.is_some() {
                        this.resizing_column = None;
                        cx.notify();
                    }
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
                    .flex_1()
                    .w_full()
                    .relative()
                    .overflow_hidden()
                    .child(
                        canvas(
                            move |bounds, _window, cx| {
                                list_bounds_view.update(cx, |this, _cx| {
                                    this.list_bounds.set(Some(bounds));
                                });
                            },
                            |_bounds, _prepaint, _window, _cx| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .child(
                        uniform_list(
                            if is_struct_mode { "hex-view-list-struct" } else { "hex-view-list-std" },
                            total_rows,
                            move |range, _window, cx| {
                                let top_row = range.start;
                                let bottom_row = range.end.saturating_sub(2);
                                view.update(cx, |this, _cx| {
                                    this.visible_range.set(Some((top_row, bottom_row)));
                                });
                                let view_read = view.read(cx);
                                let hex_scroll_x = view_read.hex_scroll_x;
                                let desc_scroll_x = view_read.desc_scroll_x;
                                let comment_scroll_x = view_read.comment_scroll_x;
                                let address_col_width = view_read.address_col_width;
                                let hex_col_width = view_read.hex_col_width;
                                let desc_col_width = view_read.desc_col_width;
                                let comment_col_width = view_read.comment_col_width;
                                let editor = view_read.editor.read(cx);
                                let parse_result = editor.parse_result.clone();
                                let collapsed_structs = Arc::new(editor.collapsed_struct_ids.clone());
                                let highlight_items = Arc::new(editor.highlights.clone());
                                let doc = editor.document.read().expect("document read lock");
                                let line_starts = editor.line_starts();
                                let cursor_offset = editor.cursor_offset;
                                let radix = editor.radix;
                                let group_size = editor.group_size;
                                let is_big_endian = editor.is_big_endian;
                                let (min_sel, max_sel) = if let (Some(s), Some(e)) = (editor.selection_start, editor.selection_end) {
                                    if s <= e { (s, e) } else { (e, s) }
                                } else {
                                    (usize::MAX, usize::MIN)
                                };

                                range
                                    .map(|row_idx| {
                                        Self::render_hex_row(
                                            row_idx,
                                            top_row,
                                            &doc,
                                            &line_starts,
                                            parse_result.clone(),
                                            Some(collapsed_structs.clone()),
                                            max_bytes_per_row,
                                            encoding,
                                            radix,
                                            group_size,
                                            is_big_endian,
                                            cursor_offset,
                                            min_sel,
                                            max_sel,
                                            &highlights,
                                            &highlight_items,
                                            max_highlight_len,
                                            show_offset,
                                            show_ascii,
                                            is_focused,
                                            hex_scroll_x,
                                            desc_scroll_x,
                                            comment_scroll_x,
                                            address_col_width,
                                            hex_col_width,
                                            desc_col_width,
                                            comment_col_width,
                                            &font_family,
                                            font_size,
                                            view.clone(),
                                            &focus_handle,
                                        )
                                    })
                                    .collect::<Vec<_>>()
                            },
                        )
                        .track_scroll(self.uniform_scroll_handle.clone())
                        .size_full(),
                    )
                    .child(
                        div().absolute().top_0().right_0().bottom_0().w_3().child(
                            Scrollbar::vertical(&self.uniform_scroll_handle)
                                .axis(ScrollbarAxis::Vertical)
                                .scroll_size(size(px(total_width), px(total_height))),
                        ),
                    ),
            )
            .context_menu({
                let focus_handle = self.focus_handle.clone();
                move |menu, window, cx| {
                    menu.action_context(focus_handle.clone())
                        .submenu("Radix", window, cx, move |menu, _window, _cx| {
                            menu.menu("Hexadecimal (16)", Box::new(SetRadixHex))
                                .menu("Decimal (10)", Box::new(SetRadixDec))
                                .menu("Octal (8)", Box::new(SetRadixOct))
                                .menu("Binary (2)", Box::new(SetRadixBin))
                        })
                        .submenu("Grouping", window, cx, move |menu, _window, _cx| {
                            menu.menu("1 Byte (8-bit)", Box::new(SetGroupSize1))
                                .menu("2 Bytes (16-bit)", Box::new(SetGroupSize2))
                                .menu("4 Bytes (32-bit)", Box::new(SetGroupSize4))
                                .menu("8 Bytes (64-bit)", Box::new(SetGroupSize8))
                        })
                        .submenu("Byte Order", window, cx, move |menu, _window, _cx| {
                            menu.menu("Little Endian", Box::new(SetByteOrderLittleEndian))
                                .menu("Big Endian", Box::new(SetByteOrderBigEndian))
                        })
                        .separator()
                        .menu("Copy", Box::new(Copy))
                        .submenu("Copy As", window, cx, move |menu, _window, _cx| {
                            menu.menu("as Hex Dump", Box::new(CopyAsHexDump))
                                .menu("as C++ Array", Box::new(CopyAsCppArray))
                                .menu("as Hex Stream", Box::new(CopyAsHexStream))
                                .menu("as Hex with Spaces", Box::new(CopyAsHexSpaces))
                                .menu("as Printable Text", Box::new(CopyAsPrintableText))
                                .menu("as Base64", Box::new(CopyAsBase64))
                                .menu("as Escaped String", Box::new(CopyAsEscapedString))
                                .menu("as Binary", Box::new(CopyAsBinary))
                                .menu("as Rust Array", Box::new(CopyAsRustArray))
                                .menu("as JSON Array", Box::new(CopyAsJsonArray))
                        })
                        .separator()
                        .submenu("Highlight", window, cx, move |menu, _window, _cx| {
                            menu.menu("Red", Box::new(HighlightRed))
                                .menu("Orange", Box::new(HighlightOrange))
                                .menu("Yellow", Box::new(HighlightYellow))
                                .menu("Green", Box::new(HighlightGreen))
                                .menu("Cyan", Box::new(HighlightCyan))
                                .menu("Blue", Box::new(HighlightBlue))
                                .menu("Purple", Box::new(HighlightPurple))
                                .menu("Pink", Box::new(HighlightPink))
                                .separator()
                                .menu("Clear Highlight", Box::new(ClearHighlight))
                                .menu("Clear All Highlights", Box::new(ClearAllHighlights))
                                .separator()
                                .menu("Show Highlights Panel", Box::new(ShowHighlightsTab))
                                .menu("Export Highlights...", Box::new(ExportHighlights))
                                .menu("Import Highlights...", Box::new(ImportHighlights))
                        })
                        .separator()
                        .menu("Select All", Box::new(SelectAll))
                        .separator()
                        .menu("Break Line", Box::new(AddCustomBreak))
                        .menu("Join Lines", Box::new(JoinLine))
                        .menu("Reset Layout", Box::new(ClearAllCustomBreaks))
                }
            })
    }
}
