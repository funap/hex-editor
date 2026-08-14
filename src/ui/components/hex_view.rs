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
use crate::core::radix::{ByteGroupSize, DisplayRadix, format_group, is_group_zero};
use crate::core::structure::ParseResult;
use crate::ui::style::StyleExt as _;
use gpui::prelude::*;
use gpui::*;
use gpui_component::menu::ContextMenuExt;
use gpui_component::scroll::Scrollbar;
use gpui_component::{ActiveTheme, StyledExt, h_flex};
use std::ops::Range;
use std::sync::Arc;

#[allow(dead_code)]
pub enum HexViewEvent {
    Scrolled(usize),
    HorizontalScrolled { target: HorizontalScrollTarget, progress: f32 },
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
const VERTICAL_SCROLLBAR_WIDTH: f32 = 12.0;
const HORIZONTAL_SCROLLBAR_HEIGHT: f32 = 12.0;
const ASCII_CELL_WIDTH: f32 = 10.0;
const CURSOR_BORDER_WIDTH: f32 = 1.0;
const CURSOR_PADDING_X: f32 = 0.0;
const CURSOR_PADDING_Y: f32 = 0.0;
const MIN_ASCII_COLUMN_WIDTH: f32 = 80.0;
const AUTO_FIT_SCAN_BYTES: usize = 64 * 1024;
const AUTO_FIT_MAX_ITEMS: usize = 16 * 1024;
const AUTO_FIT_MAX_TEXT_CHARS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollColumn {
    Hex,
    Ascii,
    Description,
    Comment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorizontalScrollTarget {
    View,
    Column(ScrollColumn),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ColumnLayout {
    start: f32,
    width: f32,
    inner_max: f32,
}

impl ColumnLayout {
    fn end(self) -> f32 {
        self.start + self.width
    }
}

fn ascii_byte_index_from_world_x(world_x: f32, column: ColumnLayout, scroll_x: f32) -> usize {
    let content_x = (world_x - column.start + scroll_x).max(0.0);
    (content_x / ASCII_CELL_WIDTH) as usize
}

fn build_ascii_char_map(encoding: Encoding, buffer: &[u8], row_offset: usize, row_len: usize) -> Vec<Option<(char, usize)>> {
    let mut map = vec![None; row_len];
    let row_end = row_offset + row_len;
    let mut local_offset = 0;

    while local_offset < row_len {
        let absolute_offset = row_offset + local_offset;
        if let Some((character, byte_len)) = encoding.decode_char_at(buffer, absolute_offset) {
            let byte_len = byte_len.max(1);
            let display_offset = if encoding == Encoding::Utf8 && absolute_offset.saturating_add(byte_len) > row_end {
                row_len - 1
            } else {
                local_offset
            };
            map[display_offset] = Some((character, byte_len));
            local_offset += byte_len;
        } else {
            local_offset += 1;
        }
    }

    map
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct HexViewLayout {
    fixed_width: f32,
    content_width: f32,
    viewport_width: f32,
    outer_max: f32,
    hex: ColumnLayout,
    ascii: Option<ColumnLayout>,
    description: Option<ColumnLayout>,
    comment: ColumnLayout,
}

impl HexViewLayout {
    fn column(self, target: ScrollColumn) -> Option<ColumnLayout> {
        match target {
            ScrollColumn::Hex => Some(self.hex),
            ScrollColumn::Ascii => self.ascii,
            ScrollColumn::Description => self.description,
            ScrollColumn::Comment => Some(self.comment),
        }
    }

    fn max_offset(self, target: HorizontalScrollTarget) -> f32 {
        match target {
            HorizontalScrollTarget::View => self.outer_max,
            HorizontalScrollTarget::Column(column) => self.column(column).map(|column| column.inner_max).unwrap_or(0.0),
        }
    }

    fn progress(self, target: HorizontalScrollTarget, offset: f32) -> f32 {
        let max_offset = self.max_offset(target);
        if max_offset > 0.0 { (offset / max_offset).clamp(0.0, 1.0) } else { 0.0 }
    }

    fn column_at(self, relative_x: f32, outer_scroll_x: f32) -> Option<ScrollColumn> {
        if relative_x < self.fixed_width {
            return None;
        }
        let world_x = relative_x + outer_scroll_x;

        if world_x >= self.hex.start && world_x <= self.hex.end() {
            return Some(ScrollColumn::Hex);
        }
        if let Some(column) = self.ascii
            && world_x >= column.start
            && world_x <= column.end()
        {
            return Some(ScrollColumn::Ascii);
        }
        if let Some(column) = self.description
            && world_x >= column.start
            && world_x <= column.end()
        {
            return Some(ScrollColumn::Description);
        }
        if world_x >= self.comment.start && world_x <= self.comment.end() {
            return Some(ScrollColumn::Comment);
        }
        None
    }
}

#[inline]
fn can_chain_to_outer(target: HorizontalScrollTarget, residual_delta: f32) -> bool {
    !matches!(target, HorizontalScrollTarget::View) && residual_delta.abs() > 0.01
}

fn make_hex_view_layout(
    bounds_width: f32,
    is_struct_mode: bool,
    show_ascii: bool,
    ascii_col_width: f32,
    ascii_inner_max: f32,
    fixed_column_width: f32,
    hex_col_width: f32,
    desc_col_width: f32,
    comment_col_width: f32,
    hex_inner_max: f32,
    desc_inner_max: f32,
    comment_inner_max: f32,
    section_gap: f32,
    content_padding: f32,
    scrollbar_width: f32,
) -> HexViewLayout {
    let fixed_width = fixed_column_width + section_gap;

    let hex = ColumnLayout {
        start: fixed_width,
        width: hex_col_width,
        inner_max: hex_inner_max.max(0.0),
    };

    let (ascii, description, comment_start) = if is_struct_mode {
        let description_start = hex.end() + section_gap;
        let description = ColumnLayout {
            start: description_start,
            width: desc_col_width,
            inner_max: desc_inner_max.max(0.0),
        };
        (None, Some(description), description.end() + section_gap)
    } else {
        let ascii = if show_ascii {
            let ascii_start = hex.end() + section_gap;
            Some(ColumnLayout {
                start: ascii_start,
                width: ascii_col_width,
                inner_max: ascii_inner_max.max(0.0),
            })
        } else {
            None
        };
        let comment_start = ascii.map(|column| column.end() + section_gap).unwrap_or_else(|| hex.end() + section_gap);
        (ascii, None, comment_start)
    };

    let comment = ColumnLayout {
        start: comment_start,
        width: comment_col_width,
        inner_max: comment_inner_max.max(0.0),
    };
    let content_width = (comment.end() - fixed_width).max(0.0);
    let viewport_width = (bounds_width - content_padding - scrollbar_width - fixed_width).max(0.0);

    HexViewLayout {
        fixed_width,
        content_width,
        viewport_width,
        outer_max: (content_width - viewport_width).max(0.0),
        hex,
        ascii,
        description,
        comment,
    }
}

#[cfg(test)]
mod horizontal_scroll_tests {
    use super::{
        AUTO_FIT_SCAN_BYTES, Encoding, HexViewLayout, HorizontalScrollTarget, ScrollColumn, ascii_byte_index_from_world_x, bounded_auto_fit_range,
        build_ascii_char_map, can_chain_to_outer, make_hex_view_layout,
    };

    fn layout(width: f32, is_struct_mode: bool, show_offset: bool, show_ascii: bool) -> HexViewLayout {
        make_hex_view_layout(
            width,
            is_struct_mode,
            show_ascii,
            160.0,
            0.0,
            if is_struct_mode || show_offset { 80.0 } else { 0.0 },
            200.0,
            240.0,
            300.0,
            120.0,
            80.0,
            60.0,
            16.0,
            8.0,
            12.0,
        )
    }

    #[test]
    fn outer_scroll_keeps_inner_hex_overflow_independent() {
        let layout = layout(600.0, false, true, true);

        assert_eq!(layout.fixed_width, 96.0);
        assert_eq!(layout.hex.start, layout.fixed_width);
        assert_eq!(layout.hex.inner_max, 120.0);
        assert!(layout.outer_max > 0.0);
        assert!(layout.outer_max < layout.content_width);
    }

    #[test]
    fn no_offset_mode_keeps_the_content_gap() {
        let layout = layout(800.0, false, false, true);

        assert_eq!(layout.fixed_width, 16.0);
        assert_eq!(layout.hex.start, 16.0);
    }

    #[test]
    fn column_hit_testing_accounts_for_outer_scroll() {
        let layout = layout(600.0, false, true, true);
        let outer_scroll_x = 40.0;

        let hex_x = layout.fixed_width + 10.0;
        assert_eq!(layout.column_at(hex_x, outer_scroll_x), Some(ScrollColumn::Hex));

        let ascii_x = layout.ascii.expect("ASCII column").start - outer_scroll_x + 10.0;
        assert_eq!(layout.column_at(ascii_x, outer_scroll_x), Some(ScrollColumn::Ascii));

        let fixed_x = layout.fixed_width - 2.0;
        assert_eq!(layout.column_at(fixed_x, outer_scroll_x), None);
    }

    #[test]
    fn structure_layout_exposes_description_as_inner_scroll_target() {
        let layout = layout(600.0, true, true, false);
        let description = layout.description.expect("description column");

        assert!(layout.ascii.is_none());
        assert_eq!(layout.max_offset(HorizontalScrollTarget::Column(ScrollColumn::Description)), 80.0);
        assert_eq!(layout.progress(HorizontalScrollTarget::Column(ScrollColumn::Description), 40.0), 0.5);
        assert_eq!(layout.column_at(description.start + 10.0, 0.0), Some(ScrollColumn::Description));
    }

    #[test]
    fn zero_max_scroll_has_zero_progress() {
        let layout = make_hex_view_layout(1200.0, false, false, 160.0, 0.0, 80.0, 200.0, 240.0, 300.0, 0.0, 0.0, 0.0, 16.0, 8.0, 12.0);

        assert_eq!(layout.progress(HorizontalScrollTarget::Column(ScrollColumn::Hex), 10.0), 0.0);
    }

    #[test]
    fn ascii_width_is_taken_from_the_layout_input() {
        let layout = make_hex_view_layout(600.0, false, true, 320.0, 0.0, 80.0, 200.0, 240.0, 300.0, 0.0, 0.0, 0.0, 16.0, 8.0, 12.0);

        assert_eq!(layout.ascii.expect("ASCII column").width, 320.0);
    }

    #[test]
    fn ascii_column_exposes_an_inner_scroll_range() {
        let layout = make_hex_view_layout(600.0, false, true, 80.0, 80.0, 80.0, 200.0, 240.0, 300.0, 0.0, 0.0, 0.0, 16.0, 8.0, 12.0);

        assert_eq!(layout.max_offset(HorizontalScrollTarget::Column(ScrollColumn::Ascii)), 80.0);
        assert_eq!(layout.progress(HorizontalScrollTarget::Column(ScrollColumn::Ascii), 40.0), 0.5);
    }

    #[test]
    fn ascii_hit_testing_includes_inner_scroll_offset() {
        let layout = make_hex_view_layout(600.0, false, true, 80.0, 80.0, 80.0, 200.0, 240.0, 300.0, 0.0, 0.0, 0.0, 16.0, 8.0, 12.0);
        let ascii = layout.ascii.expect("ASCII column");

        assert_eq!(ascii_byte_index_from_world_x(ascii.start + 5.0, ascii, 0.0), 0);
        assert_eq!(ascii_byte_index_from_world_x(ascii.start + 5.0, ascii, 20.0), 2);
    }

    #[test]
    fn outer_scroll_chains_after_any_column_reaches_its_inner_limit() {
        for column in [ScrollColumn::Hex, ScrollColumn::Ascii, ScrollColumn::Description, ScrollColumn::Comment] {
            assert!(can_chain_to_outer(HorizontalScrollTarget::Column(column), 1.0));
        }
        assert!(!can_chain_to_outer(HorizontalScrollTarget::View, 1.0));
        assert!(!can_chain_to_outer(HorizontalScrollTarget::Column(ScrollColumn::Hex), 0.0));
    }

    #[test]
    fn utf8_character_crossing_row_is_shown_at_previous_row_end() {
        let buffer = "€".as_bytes();
        let map = build_ascii_char_map(Encoding::Utf8, buffer, 0, 2);

        assert_eq!(map, vec![None, Some(('€', 3))]);
    }

    #[test]
    fn utf8_continuation_bytes_on_next_row_are_not_rendered_again() {
        let buffer = "€".as_bytes();
        let map = build_ascii_char_map(Encoding::Utf8, buffer, 1, 2);

        assert_eq!(map, vec![None, None]);
    }

    #[test]
    fn auto_fit_scan_is_bounded_around_the_viewport() {
        let range = bounded_auto_fit_range(1024 * 1024, 500_000, 500_512);

        assert_eq!(range.len(), AUTO_FIT_SCAN_BYTES);
        assert!(range.start <= 500_000);
        assert!(range.end >= 500_512);
    }

    #[test]
    fn auto_fit_scan_stays_inside_small_files_and_end_of_file() {
        assert_eq!(bounded_auto_fit_range(1024, 100, 200), 0..1024);

        let range = bounded_auto_fit_range(1024 * 1024, 1_020_000, 1_020_512);
        assert_eq!(range.end, 1024 * 1024);
        assert!(range.start <= 1_020_000);
        assert_eq!(range.len(), AUTO_FIT_SCAN_BYTES);
    }

    #[test]
    fn description_content_width_aggregates_multiple_fields_in_row() {
        use super::HexView;
        use crate::core::buffer::Buffer;
        use crate::core::document::Document;
        use crate::core::editor::Editor;
        use crate::core::structure::types::{FieldValue, ParseResult, ParsedField};
        use std::path::PathBuf;
        use std::sync::{Arc, RwLock};

        let doc = Arc::new(RwLock::new(Document::new(PathBuf::from("test.bin"), Buffer::new(vec![0; 32]))));
        let editor = Editor::new(doc);

        let field1 = ParsedField {
            id: "magic".into(),
            field_type: "u4".into(),
            offset: 0,
            size: 4,
            value: FieldValue::U32(0x12345678),
            color: gpui::Hsla::default(),
            description: None,
            children: vec![],
            enum_label: None,
            is_instance: false,
        };
        let field2 = ParsedField {
            id: "flags".into(),
            field_type: "u4".into(),
            offset: 4,
            size: 4,
            value: FieldValue::U32(0x00000001),
            color: gpui::Hsla::default(),
            description: None,
            children: vec![],
            enum_label: None,
            is_instance: false,
        };
        let field3 = ParsedField {
            id: "version".into(),
            field_type: "u4".into(),
            offset: 8,
            size: 4,
            value: FieldValue::U32(2),
            color: gpui::Hsla::default(),
            description: None,
            children: vec![],
            enum_label: None,
            is_instance: false,
        };

        let container = ParsedField {
            id: "header".into(),
            field_type: "Header".into(),
            offset: 0,
            size: 12,
            value: FieldValue::Struct,
            color: gpui::Hsla::default(),
            description: None,
            children: vec![field1.clone(), field2.clone(), field3.clone()],
            enum_label: None,
            is_instance: false,
        };

        let parse_result = ParseResult::new("test_struct".into(), vec![container], 12, vec![]);
        let char_w = 8.0;

        // When expanded, width should combine container ID + all 3 leaf expressions + spacing
        let width_expanded = HexView::description_content_width_in_range(&editor, &parse_result, &(0..16), char_w);

        // Single field expression width
        let single_field_width = super::weighted_text_width(&field1.format_expression(), char_w);
        assert!(
            width_expanded > single_field_width * 2.5,
            "Expanded row width ({width_expanded}) must aggregate all fields, strictly greater than a single field ({single_field_width})"
        );

        // When collapsed, only the container id and size are displayed
        let mut editor_collapsed = Editor::new(Arc::new(RwLock::new(Document::new(PathBuf::from("test.bin"), Buffer::new(vec![0; 32])))));
        editor_collapsed.collapsed_struct_ids.insert("header".into());
        let width_collapsed = HexView::description_content_width_in_range(&editor_collapsed, &parse_result, &(0..16), char_w);

        assert!(
            width_collapsed < width_expanded,
            "Collapsed width ({width_collapsed}) should be smaller than expanded width ({width_expanded})"
        );
    }
}

#[cfg(test)]
mod hex_grid_tests {
    use super::{ByteGroupSize, DisplayRadix, build_hex_text_source, centered_glyph_offset, hex_grid_width, hex_grid_x, px};

    #[test]
    fn group_boundaries_use_the_same_fixed_grid_as_the_header() {
        let source = build_hex_text_source(&[0x12, 0x34, 0x56], 0, DisplayRadix::Hexadecimal, ByteGroupSize::One, false);
        let cell_width = px(8.0);

        assert_eq!(source.text.as_ref(), "12 34 56");
        assert_eq!(f32::from(hex_grid_x(source.groups[0].text_start, cell_width)), 0.0);
        assert_eq!(f32::from(hex_grid_x(source.groups[1].text_start, cell_width)), 24.0);
        assert_eq!(f32::from(hex_grid_x(source.groups[2].text_start, cell_width)), 48.0);
        assert_eq!(f32::from(hex_grid_width(source.text.len(), cell_width)), 64.0);
    }

    #[test]
    fn group_grid_keeps_partial_group_slots_aligned() {
        let source = build_hex_text_source(&[0x12], 1, DisplayRadix::Hexadecimal, ByteGroupSize::Four, false);
        let cell_width = px(8.0);

        assert_eq!(source.text.as_ref(), "..12....");
        assert_eq!(f32::from(hex_grid_x(source.groups[0].text_start, cell_width)), 0.0);
        assert_eq!(f32::from(hex_grid_x(source.groups[0].text_end, cell_width)), 64.0);
        assert_eq!(f32::from(hex_grid_width(source.text.len(), cell_width)), 64.0);
    }

    #[test]
    fn glyphs_are_centered_in_fixed_cells() {
        assert_eq!(centered_glyph_offset(10.0, 6.0), 2.0);
        assert_eq!(centered_glyph_offset(10.0, 12.0), 0.0);
    }
}

#[cfg(test)]
mod highlight_render_tests {
    use super::{highlight_color_for_range, hsla};

    #[test]
    fn selection_takes_precedence_over_an_overlapping_highlight() {
        let highlight_color = hsla(0.1, 0.8, 0.5, 0.35);
        let highlights = [(0..16, highlight_color)];

        assert_eq!(highlight_color_for_range(4, 8, true, &highlights), None);
        assert_eq!(highlight_color_for_range(4, 8, false, &highlights), Some(highlight_color));
    }
}

#[cfg(test)]
mod layout_state_tests {
    use super::HexViewLayoutState;

    #[test]
    fn test_layout_state_copy_preserves_dimensions_and_scroll() {
        let state = HexViewLayoutState {
            address_col_width: 120.0,
            hex_col_width: 350.0,
            desc_col_width: 280.0,
            comment_col_width: 400.0,
            ascii_col_width: 180.0,
            show_offset: false,
            show_ascii: true,
            show_header: false,
            scroll_offset: 42,
            outer_scroll_x: 15.0,
            hex_scroll_x: 25.0,
            ascii_scroll_x: 35.0,
            desc_scroll_x: 45.0,
            comment_scroll_x: 55.0,
        };

        let cloned = state.clone();
        assert_eq!(cloned.address_col_width, 120.0);
        assert_eq!(cloned.hex_col_width, 350.0);
        assert_eq!(cloned.desc_col_width, 280.0);
        assert_eq!(cloned.comment_col_width, 400.0);
        assert_eq!(cloned.ascii_col_width, 180.0);
        assert!(!cloned.show_offset);
        assert!(cloned.show_ascii);
        assert!(!cloned.show_header);
        assert_eq!(cloned.scroll_offset, 42);
        assert_eq!(cloned.outer_scroll_x, 15.0);
        assert_eq!(cloned.hex_scroll_x, 25.0);
        assert_eq!(cloned.ascii_scroll_x, 35.0);
        assert_eq!(cloned.desc_scroll_x, 45.0);
        assert_eq!(cloned.comment_scroll_x, 55.0);
    }
}

#[derive(Clone, Copy, Debug)]
struct HexGroupInfo {
    chunk_start: usize,
    chunk_end: usize,
    #[allow(dead_code)]
    start_slot: usize,
    text_start: usize,
    text_end: usize,
}

#[derive(Clone, Debug)]
struct HexTextSource {
    text: SharedString,
    groups: Vec<HexGroupInfo>,
}

const HEX_METRIC_CHARS: &str = "0123456789abcdef.";

/// Build the exact text stream used by the batched Hex renderer and retain the
/// byte/text ranges needed to translate shaped coordinates back to bytes.
fn build_hex_text_source(chunk: &[u8], line_offset: usize, radix: DisplayRadix, group_size: ByteGroupSize, is_big_endian: bool) -> HexTextSource {
    let group_bytes = group_size.byte_count();
    let mut text = String::with_capacity(chunk.len().saturating_mul(3));
    let mut groups = Vec::with_capacity(chunk.len().div_ceil(group_bytes));
    let mut chunk_idx = 0;

    while chunk_idx < chunk.len() {
        let item_start_offset = line_offset + chunk_idx;
        let start_slot = item_start_offset % group_bytes;
        let item_slice_len = (chunk.len() - chunk_idx).min(group_bytes - start_slot);

        if !text.is_empty() {
            text.push(' ');
        }
        let text_start = text.len();
        text.push_str(&format_group(
            &chunk[chunk_idx..chunk_idx + item_slice_len],
            start_slot,
            radix,
            group_size,
            is_big_endian,
        ));
        let text_end = text.len();

        groups.push(HexGroupInfo {
            chunk_start: chunk_idx,
            chunk_end: chunk_idx + item_slice_len,
            start_slot,
            text_start,
            text_end,
        });
        chunk_idx += item_slice_len;
    }

    HexTextSource {
        text: SharedString::from(text),
        groups,
    }
}

/// Measure the widest glyph used by the formatted Hex stream.
///
/// This is intentionally a small, cached probe rather than a per-row
/// measurement. The extra pixel gives centered glyphs a little breathing
/// room for rounding and side bearings in proportional fonts.
fn measure_hex_cell_width(window: &Window, font: Font, font_size: Pixels) -> Pixels {
    let mut max_advance: f32 = 0.0;

    for character in HEX_METRIC_CHARS.chars() {
        let text = SharedString::from(character.to_string());
        let run = gpui::TextRun {
            len: text.len(),
            font: font.clone(),
            color: hsla(0.0, 0.0, 0.0, 0.0),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window.text_system().shape_line(text, font_size, &[run], None);
        max_advance = max_advance.max(f32::from(shaped.width));
    }

    px((max_advance + 1.01).max(1.0))
}

#[inline]
fn hex_grid_x(text_index: usize, cell_width: Pixels) -> Pixels {
    px(text_index as f32 * f32::from(cell_width))
}

#[inline]
fn hex_grid_width(text_len: usize, cell_width: Pixels) -> Pixels {
    hex_grid_x(text_len, cell_width)
}

#[inline]
fn centered_glyph_offset(cell_width: f32, glyph_width: f32) -> f32 {
    ((cell_width - glyph_width) / 2.0).max(0.0)
}

fn bounded_auto_fit_range(total_size: usize, visible_start: usize, visible_end: usize) -> Range<usize> {
    let visible_start = visible_start.min(total_size);
    let visible_end = visible_end.clamp(visible_start, total_size);
    if total_size <= AUTO_FIT_SCAN_BYTES {
        return 0..total_size;
    }

    let visible_len = visible_end - visible_start;
    if visible_len >= AUTO_FIT_SCAN_BYTES {
        let start = visible_start.min(total_size - AUTO_FIT_SCAN_BYTES);
        return start..start + AUTO_FIT_SCAN_BYTES;
    }

    let extra = AUTO_FIT_SCAN_BYTES - visible_len;
    let centered_start = visible_start.saturating_sub(extra / 2);
    let start = centered_start.min(total_size - AUTO_FIT_SCAN_BYTES);
    start..start + AUTO_FIT_SCAN_BYTES
}

fn weighted_text_width(text: &str, char_w: f32) -> f32 {
    text.chars()
        .take(AUTO_FIT_MAX_TEXT_CHARS)
        .map(|character| if character.is_ascii() { char_w } else { char_w * 1.8 })
        .sum()
}

fn hex_group_x(group: HexGroupInfo, origin_x: Pixels, cell_width: Pixels) -> (Pixels, Pixels) {
    (
        origin_x + hex_grid_x(group.text_start, cell_width),
        origin_x + hex_grid_x(group.text_end, cell_width),
    )
}

#[inline]
fn pixel_midpoint(left: Pixels, right: Pixels) -> Pixels {
    px((f32::from(left) + f32::from(right)) * 0.5)
}

#[inline]
fn darken_cursor_color(color: Hsla) -> Hsla {
    Hsla {
        l: (color.l * 0.72).clamp(0.0, 1.0),
        a: color.a.max(0.9),
        ..color
    }
}

/// Paint every glyph centered in its fixed Hex cell while reusing the line
/// shaped for the row. This keeps shaping batched and avoids a per-row glyph
/// position allocation.
fn paint_centered_hex_glyphs(
    shaped: &gpui::ShapedLine,
    groups: &[HexGroupInfo],
    group_colors: &[Hsla],
    cell_width: Pixels,
    origin: Point<Pixels>,
    line_height: Pixels,
    window: &mut Window,
) {
    let natural_width = shaped.width;
    let cell_width_f = f32::from(cell_width);

    let text_width = hex_grid_width(shaped.text.len(), cell_width);
    let line_bounds = Bounds::new(origin, size(text_width, line_height));
    window.paint_layer(line_bounds, |window| {
        let baseline_offset = point(px(0.0), (line_height - shaped.ascent - shaped.descent) / 2.0 + shaped.ascent);
        let mut group_idx = 0;
        let mut glyphs = shaped
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(move |glyph| (run.font_id, glyph)))
            .peekable();

        while let Some((font_id, glyph)) = glyphs.next() {
            let natural_end = glyphs.peek().map(|(_, next)| next.position.x).unwrap_or(natural_width);
            let glyph_width = f32::from(natural_end - glyph.position.x).max(0.0);
            let centered_offset = centered_glyph_offset(cell_width_f, glyph_width);
            let glyph_origin = point(origin.x + hex_grid_x(glyph.index, cell_width) + px(centered_offset), origin.y) + baseline_offset;

            while group_idx < groups.len() && glyph.index >= groups[group_idx].text_end {
                group_idx += 1;
            }
            if group_idx < groups.len()
                && groups[group_idx].text_start <= glyph.index
                && glyph.index < groups[group_idx].text_end
                && let Some(&color) = group_colors.get(group_idx)
            {
                if glyph.is_emoji {
                    let _ = window.paint_emoji(glyph_origin, font_id, glyph.id, shaped.font_size);
                } else {
                    let _ = window.paint_glyph(glyph_origin, font_id, glyph.id, shaped.font_size, color);
                }
            }
        }
    });
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
    let outline = gpui::outline(bounds, color, gpui::BorderStyle::Solid).border_widths(border_width);
    window.paint_quad(outline);
}

#[inline]
fn paint_cursor_border(window: &mut Window, bounds: Bounds<Pixels>, color: Hsla) {
    let padding_x = px(CURSOR_PADDING_X);
    let padding_y = px(CURSOR_PADDING_Y);
    let padded_bounds = Bounds::new(
        point(bounds.origin.x - padding_x, bounds.origin.y - padding_y),
        size(bounds.size.width + padding_x + padding_x, bounds.size.height + padding_y + padding_y),
    );
    paint_border_box(window, padded_bounds, px(CURSOR_BORDER_WIDTH), color);
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

/// Return the smallest matching highlight, unless the item is selected.
///
/// Selection is transient UI state and must remain visible while a persistent
/// highlight is underneath it, so it takes precedence over highlight colors.
#[inline]
fn highlight_color_for_range(item_start: usize, item_end: usize, is_selected: bool, active_highlights: &[(Range<usize>, Hsla)]) -> Option<Hsla> {
    if is_selected {
        return None;
    }

    let mut smallest_len = usize::MAX;
    let mut color = None;
    for (range, highlight_color) in active_highlights {
        if range.start < item_end && range.end > item_start {
            let len = range.end.saturating_sub(range.start);
            if len <= smallest_len {
                smallest_len = len;
                color = Some(*highlight_color);
            }
        }
    }
    color
}

pub fn init(cx: &mut App) {
    #[cfg(target_os = "macos")]
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

    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("left", MoveLeft, Some(CONTEXT)),
        KeyBinding::new("right", MoveRight, Some(CONTEXT)),
        KeyBinding::new("up", MoveUp, Some(CONTEXT)),
        KeyBinding::new("down", MoveDown, Some(CONTEXT)),
        KeyBinding::new("shift-left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("shift-right", SelectRight, Some(CONTEXT)),
        KeyBinding::new("shift-up", SelectUp, Some(CONTEXT)),
        KeyBinding::new("shift-down", SelectDown, Some(CONTEXT)),
        KeyBinding::new("ctrl-a", SelectAll, Some(CONTEXT)),
        KeyBinding::new("ctrl-c", Copy, Some(CONTEXT)),
        KeyBinding::new("ctrl-shift-c", CopyAsHexDump, Some(CONTEXT)),
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
        KeyBinding::new("f3", SearchNext, Some(CONTEXT)),
        KeyBinding::new("ctrl-g", SearchNext, Some(CONTEXT)),
        KeyBinding::new("shift-f3", SearchPrev, Some(CONTEXT)),
        KeyBinding::new("ctrl-shift-g", SearchPrev, Some(CONTEXT)),
        KeyBinding::new("enter", AddCustomBreak, Some(CONTEXT)),
        KeyBinding::new("shift-j", JoinLine, Some(CONTEXT)),
        KeyBinding::new("backspace", RemoveCustomBreakBackward, Some(CONTEXT)),
        KeyBinding::new("delete", RemoveCustomBreakForward, Some(CONTEXT)),
        KeyBinding::new("ctrl-shift-backspace", ClearAllCustomBreaks, Some(CONTEXT)),
    ]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizingColumn {
    Address,
    Hex,
    Ascii,
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
    scroll_offset: usize,
    accum_scroll_y: f32,
    outer_scroll_x: f32,
    outer_scroll_handle: ScrollHandle,
    is_dragging_scrollbar: bool,
    scrollbar_hovered: bool,
    scrollbar_drag_start_y: f32,
    scrollbar_drag_start_row: usize,
    pub hex_scroll_x: f32,
    pub ascii_scroll_x: f32,
    pub desc_scroll_x: f32,
    pub comment_scroll_x: f32,
    scroll_lock_axis: Option<ScrollAxisLock>,
    last_scroll_time: Option<std::time::Instant>,
    scroll_lock_top_row: usize,
    is_selecting: bool,
    bounds: std::cell::Cell<Option<Bounds<Pixels>>>,
    list_bounds: std::cell::Cell<Option<Bounds<Pixels>>>,
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
    hex_cell_width: f32,
    hex_content_width: f32,
    pub ascii_col_width: f32,
    pub desc_col_width: f32,
    pub comment_col_width: f32,
    cached_comment_content_width: std::cell::Cell<Option<f32>>,
    cached_desc_content_width: std::cell::Cell<Option<f32>>,
    resizing_column: Option<(ResizingColumn, f32, f32)>,
    last_cursor_offset: Option<usize>,
    cursor_reveal_pending: bool,
    _editor_subscription: Subscription,
}

impl EventEmitter<HexViewEvent> for HexView {}

#[derive(Clone, Debug)]
pub struct HexViewLayoutState {
    pub address_col_width: f32,
    pub hex_col_width: f32,
    pub desc_col_width: f32,
    pub comment_col_width: f32,
    pub ascii_col_width: f32,
    pub show_offset: bool,
    pub show_ascii: bool,
    pub show_header: bool,
    pub scroll_offset: usize,
    pub outer_scroll_x: f32,
    pub hex_scroll_x: f32,
    pub ascii_scroll_x: f32,
    pub desc_scroll_x: f32,
    pub comment_scroll_x: f32,
}

#[allow(dead_code)]
impl HexView {
    pub fn new(editor: Entity<Editor>, cx: &mut Context<Self>) -> Self {
        let (radix, group_size, is_big_endian, encoding) = {
            let ed = editor.read(cx);
            (ed.radix, ed.group_size, ed.is_big_endian, ed.encoding)
        };
        let font_size_prop = px(14.0);
        // The actual grid width is populated from measured glyph metrics
        // during the first render. This value only controls the initial viewport.
        let hex_col_width = 0.0;

        let _editor_subscription = cx.observe(&editor, |this, editor_entity, cx| {
            this.cached_comment_content_width.set(None);
            this.cached_desc_content_width.set(None);
            let ed = editor_entity.read(cx);
            let new_encoding = ed.encoding;
            let new_radix = ed.radix;
            let new_group_size = ed.group_size;
            let new_endian = ed.is_big_endian;
            let cursor_changed = this.last_cursor_offset != Some(ed.cursor_offset);
            if cursor_changed {
                this.cursor_reveal_pending = true;
            }

            if this.encoding != new_encoding {
                this.encoding = new_encoding;
            }
            if this.radix != new_radix || this.group_size != new_group_size || this.is_big_endian != new_endian {
                this.radix = new_radix;
                this.group_size = new_group_size;
                this.is_big_endian = new_endian;
                this.hex_content_width = 0.0;
                this.cursor_reveal_pending = true;
            }
            this.clamp_scroll_offsets(cx);
            if cursor_changed {
                this.ensure_cursor_visible(cx);
            }
            cx.notify();
        });

        Self {
            editor,
            focus_handle: cx.focus_handle(),
            scroll_offset: 0,
            accum_scroll_y: 0.0,
            outer_scroll_x: 0.0,
            outer_scroll_handle: ScrollHandle::new(),
            is_dragging_scrollbar: false,
            scrollbar_hovered: false,
            scrollbar_drag_start_y: 0.0,
            scrollbar_drag_start_row: 0,
            hex_scroll_x: 0.0,
            ascii_scroll_x: 0.0,
            desc_scroll_x: 0.0,
            comment_scroll_x: 0.0,
            scroll_lock_axis: None,
            last_scroll_time: None,
            scroll_lock_top_row: 0,
            is_selecting: false,
            bounds: std::cell::Cell::new(None),
            list_bounds: std::cell::Cell::new(None),
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
            hex_cell_width: 0.0,
            hex_content_width: 0.0,
            ascii_col_width: 0.0,
            desc_col_width: DESC_WIDTH,
            comment_col_width: COMMENT_WIDTH,
            cached_comment_content_width: std::cell::Cell::new(None),
            cached_desc_content_width: std::cell::Cell::new(None),
            resizing_column: None,
            last_cursor_offset: None,
            cursor_reveal_pending: true,
            _editor_subscription,
        }
    }

    pub fn layout_state(&self) -> HexViewLayoutState {
        HexViewLayoutState {
            address_col_width: self.address_col_width,
            hex_col_width: self.hex_col_width,
            desc_col_width: self.desc_col_width,
            comment_col_width: self.comment_col_width,
            ascii_col_width: self.ascii_col_width,
            show_offset: self.show_offset,
            show_ascii: self.show_ascii,
            show_header: self.show_header,
            scroll_offset: self.scroll_offset,
            outer_scroll_x: self.outer_scroll_x,
            hex_scroll_x: self.hex_scroll_x,
            ascii_scroll_x: self.ascii_scroll_x,
            desc_scroll_x: self.desc_scroll_x,
            comment_scroll_x: self.comment_scroll_x,
        }
    }

    pub fn apply_layout_state(&mut self, state: &HexViewLayoutState) {
        self.address_col_width = state.address_col_width;
        self.hex_col_width = state.hex_col_width;
        self.desc_col_width = state.desc_col_width;
        self.comment_col_width = state.comment_col_width;
        self.ascii_col_width = state.ascii_col_width;
        self.show_offset = state.show_offset;
        self.show_ascii = state.show_ascii;
        self.show_header = state.show_header;
        self.scroll_offset = state.scroll_offset;
        self.outer_scroll_x = state.outer_scroll_x;
        self.hex_scroll_x = state.hex_scroll_x;
        self.ascii_scroll_x = state.ascii_scroll_x;
        self.desc_scroll_x = state.desc_scroll_x;
        self.comment_scroll_x = state.comment_scroll_x;
        self.cached_comment_content_width.set(None);
        self.cached_desc_content_width.set(None);
    }

    pub fn copy_layout_from(&mut self, source: &HexView) {
        self.apply_layout_state(&source.layout_state());
    }

    pub fn max_hex_scroll(&self, cx: &App) -> f32 {
        let _ = cx;
        (self.hex_content_width - self.hex_col_width).max(0.0)
    }

    pub fn max_desc_scroll(&self, cx: &App) -> f32 {
        if let Some(max_content_w) = self.cached_desc_content_width.get() {
            let max_w = (max_content_w + 32.0).max(self.desc_col_width);
            return (max_w - self.desc_col_width).max(0.0);
        }

        let scan_range = self.auto_fit_scan_range(cx);
        let editor = self.editor.read(cx);
        let char_w = f32::from(self.font_size_prop) * 0.61;
        let max_content_w = if let Some(parse_res) = editor.parse_result() {
            Self::description_content_width_in_range(editor, &parse_res, &scan_range, char_w)
        } else {
            0.0
        };
        self.cached_desc_content_width.set(Some(max_content_w));
        let max_w = (max_content_w + 32.0).max(self.desc_col_width);
        (max_w - self.desc_col_width).max(0.0)
    }

    fn auto_fit_scan_range(&self, cx: &App) -> Range<usize> {
        let (visible_start, visible_end) = self.viewport_byte_range(cx);
        let total_size = self.editor.read(cx).total_size();
        bounded_auto_fit_range(total_size, visible_start, visible_end)
    }

    fn description_content_width_in_range(editor: &Editor, parse_result: &ParseResult, scan_range: &Range<usize>, char_w: f32) -> f32 {
        let scan_len = scan_range.end.saturating_sub(scan_range.start);
        if scan_len == 0 {
            return 0.0;
        }

        let line_starts = editor.line_starts();
        let total_size = editor.total_size();
        let collapsed_structs = &editor.collapsed_struct_ids;

        let start_row = Editor::find_line_index(scan_range.start, &line_starts);
        let end_row = if scan_range.end >= total_size {
            line_starts.len()
        } else {
            Editor::find_line_index(scan_range.end, &line_starts) + 1
        };

        let mut max_width: f32 = 0.0;

        for (row_count, row) in (start_row..end_row).enumerate() {
            if row_count >= AUTO_FIT_MAX_ITEMS {
                break;
            }

            let Some(offset) = line_starts.get(row) else {
                continue;
            };
            let next_offset = line_starts.get(row + 1).unwrap_or(total_size);
            let chunk_len = next_offset.saturating_sub(offset);
            if chunk_len == 0 {
                continue;
            }

            let container_structs = parse_result.find_container_structs_starting_at(offset, chunk_len);
            let leaf_fields = parse_result.find_leaf_fields_starting_at(offset, chunk_len);

            if container_structs.is_empty() && leaf_fields.is_empty() {
                continue;
            }

            let active_ranges = parse_result.find_active_struct_ranges(offset, chunk_len);
            let is_collapsed = container_structs.first().map(|c| collapsed_structs.contains(&c.id)).unwrap_or(false);

            let struct_depth = active_ranges.len().saturating_sub(1);
            let indent_level = if !container_structs.is_empty() {
                active_ranges
                    .iter()
                    .find(|r| container_structs.first().map(|c| c.id == r.id).unwrap_or(false))
                    .map(|r| r.depth)
                    .unwrap_or(struct_depth)
            } else {
                active_ranges.len()
            };
            let indent_px = indent_level as f32 * 14.0;

            let mut parts_width = 0.0;
            let mut part_count = 0;

            if let Some(container) = container_structs.first() {
                let text = if is_collapsed {
                    format!("▶ {} ({} bytes)", container.id, container.size)
                } else {
                    format!("▼ {}", container.id)
                };
                parts_width += weighted_text_width(&text, char_w);
                part_count += 1;
            }

            if !is_collapsed {
                for f in leaf_fields {
                    let expr = f.format_expression();
                    parts_width += weighted_text_width(&expr, char_w);
                    part_count += 1;
                }
            }

            if part_count > 0 {
                let spacing_width = (part_count - 1) as f32 * (char_w * 2.0);
                let row_width = indent_px + parts_width + spacing_width + 8.0;
                max_width = max_width.max(row_width);
            }
        }

        max_width
    }

    fn comment_content_width_in_range(editor: &Editor, scan_range: &Range<usize>, char_w: f32) -> f32 {
        if scan_range.is_empty() {
            return 0.0;
        }

        let line_starts = editor.line_starts();
        let highlights_guard = editor.highlights.read().expect("highlights read lock");
        let highlights = &*highlights_guard;
        let mut start_idx = highlights.partition_point(|highlight| highlight.offset < scan_range.start);
        if start_idx > 0 {
            // Include the item immediately before the range because a comment
            // beginning on the previous row can continue into the viewport.
            start_idx -= 1;
        }
        let end_idx = highlights.partition_point(|highlight| highlight.offset < scan_range.end);

        use std::collections::HashMap;
        let mut row_widths: HashMap<usize, f32> = HashMap::new();
        for highlight in highlights[start_idx..end_idx].iter().take(AUTO_FIT_MAX_ITEMS) {
            let trimmed = highlight.comment.trim();
            if trimmed.is_empty() {
                continue;
            }
            let row = Editor::find_line_index(highlight.offset, &line_starts);
            let item_width = 8.0 + 5.0 + weighted_text_width(trimmed, char_w) + 14.0;
            *row_widths.entry(row).or_insert(8.0) += item_width;
        }

        row_widths.values().copied().fold(0.0, f32::max)
    }

    pub fn max_comment_scroll(&self, cx: &App) -> f32 {
        if let Some(max_content_w) = self.cached_comment_content_width.get() {
            let max_w = (max_content_w + 32.0).max(self.comment_col_width);
            return (max_w - self.comment_col_width).max(0.0);
        }

        let scan_range = self.auto_fit_scan_range(cx);
        let editor = self.editor.read(cx);
        let char_w = f32::from(self.font_size_prop) * 0.61;
        let max_content_w = Self::comment_content_width_in_range(&editor, &scan_range, char_w);
        self.cached_comment_content_width.set(Some(max_content_w));
        let max_w = (max_content_w + 32.0).max(self.comment_col_width);
        (max_w - self.comment_col_width).max(0.0)
    }

    fn default_ascii_col_width(max_bytes_per_row: usize) -> f32 {
        max_bytes_per_row.max(1) as f32 * ASCII_CELL_WIDTH
    }

    fn effective_ascii_col_width(&self, max_bytes_per_row: usize) -> f32 {
        if self.ascii_col_width > 0.0 {
            self.ascii_col_width
        } else {
            Self::default_ascii_col_width(max_bytes_per_row)
        }
    }

    fn current_layout(&self, cx: &App) -> HexViewLayout {
        let (max_bytes_per_row, is_struct_mode) = {
            let editor = self.editor.read(cx);
            let line_starts = editor.line_starts();
            (
                line_starts.max_bytes_per_row(),
                editor.show_inline_structure_view && editor.parse_result().is_some(),
            )
        };
        let bounds_width = self
            .list_bounds
            .get()
            .or_else(|| self.bounds.get())
            .map(|bounds| f32::from(bounds.size.width))
            .unwrap_or(800.0);

        make_hex_view_layout(
            bounds_width,
            is_struct_mode,
            self.show_ascii,
            self.effective_ascii_col_width(max_bytes_per_row),
            if !is_struct_mode && self.show_ascii {
                (Self::default_ascii_col_width(max_bytes_per_row) - self.effective_ascii_col_width(max_bytes_per_row)).max(0.0)
            } else {
                0.0
            },
            if is_struct_mode {
                self.address_col_width
            } else if self.show_offset {
                OFFSET_WIDTH
            } else {
                0.0
            },
            self.hex_col_width,
            self.desc_col_width,
            self.comment_col_width,
            self.max_hex_scroll(cx),
            if is_struct_mode { self.max_desc_scroll(cx) } else { 0.0 },
            self.max_comment_scroll(cx),
            SECTION_GAP,
            8.0,
            VERTICAL_SCROLLBAR_WIDTH,
        )
    }

    fn horizontal_offset(&self, target: HorizontalScrollTarget) -> f32 {
        match target {
            HorizontalScrollTarget::View => self.outer_scroll_x,
            HorizontalScrollTarget::Column(ScrollColumn::Hex) => self.hex_scroll_x,
            HorizontalScrollTarget::Column(ScrollColumn::Ascii) => self.ascii_scroll_x,
            HorizontalScrollTarget::Column(ScrollColumn::Description) => self.desc_scroll_x,
            HorizontalScrollTarget::Column(ScrollColumn::Comment) => self.comment_scroll_x,
        }
    }

    fn set_horizontal_offset(&mut self, target: HorizontalScrollTarget, offset: f32, layout: HexViewLayout, emit: bool, cx: &mut Context<Self>) -> bool {
        let max_offset = layout.max_offset(target);
        let new_offset = offset.clamp(0.0, max_offset);
        let current_offset = self.horizontal_offset(target);
        if (new_offset - current_offset).abs() <= 0.01 {
            return false;
        }

        match target {
            HorizontalScrollTarget::View => {
                self.outer_scroll_x = new_offset;
                self.outer_scroll_handle.set_offset(point(-px(new_offset), px(0.0)));
            }
            HorizontalScrollTarget::Column(ScrollColumn::Hex) => self.hex_scroll_x = new_offset,
            HorizontalScrollTarget::Column(ScrollColumn::Ascii) => self.ascii_scroll_x = new_offset,
            HorizontalScrollTarget::Column(ScrollColumn::Description) => self.desc_scroll_x = new_offset,
            HorizontalScrollTarget::Column(ScrollColumn::Comment) => self.comment_scroll_x = new_offset,
        }

        cx.notify();
        if emit {
            cx.emit(HexViewEvent::HorizontalScrolled {
                target,
                progress: layout.progress(target, new_offset),
            });
        }
        true
    }

    fn sync_outer_scroll_from_handle(&mut self, layout: HexViewLayout, cx: &mut Context<Self>) {
        let handle_offset = (-f32::from(self.outer_scroll_handle.offset().x)).max(0.0);
        let _ = self.set_horizontal_offset(HorizontalScrollTarget::View, handle_offset, layout, true, cx);
    }

    pub fn set_horizontal_scroll(&mut self, target: HorizontalScrollTarget, progress: f32, cx: &mut Context<Self>) {
        let layout = self.current_layout(cx);
        let offset = layout.max_offset(target) * progress.clamp(0.0, 1.0);
        let _ = self.set_horizontal_offset(target, offset, layout, false, cx);
    }

    pub fn clamp_scroll_offsets(&mut self, cx: &App) {
        let max_hex = self.max_hex_scroll(cx);
        self.hex_scroll_x = self.hex_scroll_x.clamp(0.0, max_hex);

        let max_desc = self.max_desc_scroll(cx);
        self.desc_scroll_x = self.desc_scroll_x.clamp(0.0, max_desc);

        let max_comment = self.max_comment_scroll(cx);
        self.comment_scroll_x = self.comment_scroll_x.clamp(0.0, max_comment);

        let layout = self.current_layout(cx);
        self.ascii_scroll_x = self
            .ascii_scroll_x
            .clamp(0.0, layout.max_offset(HorizontalScrollTarget::Column(ScrollColumn::Ascii)));
        self.outer_scroll_x = self.outer_scroll_x.clamp(0.0, layout.outer_max);
        self.outer_scroll_handle.set_offset(point(-px(self.outer_scroll_x), px(0.0)));
    }

    pub fn auto_fit_column(&mut self, col: ResizingColumn, cx: &mut Context<Self>) {
        match col {
            ResizingColumn::Address => {
                self.address_col_width = ADDRESS_WIDTH;
            }
            ResizingColumn::Hex => {
                if self.hex_content_width > 0.0 {
                    self.hex_col_width = self.hex_content_width;
                }
            }
            ResizingColumn::Ascii => {
                let max_bytes_per_row = self.editor.read(cx).line_starts().max_bytes_per_row();
                self.ascii_col_width = Self::default_ascii_col_width(max_bytes_per_row);
                self.ascii_scroll_x = 0.0;
            }
            ResizingColumn::Description => {
                let scan_range = self.auto_fit_scan_range(cx);
                let editor = self.editor.read(cx);
                if let Some(parse_res) = editor.parse_result() {
                    let char_w = f32::from(self.font_size_prop) * 0.61;
                    let max_content_w = Self::description_content_width_in_range(editor, &parse_res, &scan_range, char_w);
                    self.cached_desc_content_width.set(Some(max_content_w));
                    self.desc_col_width = if max_content_w > 0.0 {
                        (max_content_w + 24.0).max(DESC_WIDTH)
                    } else {
                        DESC_WIDTH
                    };
                    self.desc_scroll_x = 0.0;
                } else {
                    self.desc_col_width = DESC_WIDTH;
                    self.desc_scroll_x = 0.0;
                }
            }
            ResizingColumn::Comment => {
                let scan_range = self.auto_fit_scan_range(cx);
                let editor = self.editor.read(cx);
                let char_w = f32::from(self.font_size_prop) * 0.61;
                let max_content_w = Self::comment_content_width_in_range(&editor, &scan_range, char_w);
                self.cached_comment_content_width.set(Some(max_content_w));
                self.comment_col_width = if max_content_w > 0.0 {
                    (max_content_w + 24.0).max(COMMENT_WIDTH)
                } else {
                    COMMENT_WIDTH
                };
                self.comment_scroll_x = 0.0;
            }
        }
        self.cursor_reveal_pending = true;
        self.clamp_scroll_offsets(cx);
        cx.notify();
    }

    fn on_scroll_wheel(&mut self, event: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let now = std::time::Instant::now();
        let pixel_delta = event.delta.pixel_delta(px(ROW_HEIGHT));
        let mut delta_x = f32::from(pixel_delta.x);
        let delta_y = f32::from(pixel_delta.y);
        let column_only_horizontal = event.modifiers.shift;

        if delta_x == 0.0 && column_only_horizontal && delta_y != 0.0 {
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

        // Shift explicitly selects column-only horizontal scrolling, even if
        // the previous wheel gesture left the axis lock in vertical mode.
        if column_only_horizontal {
            self.scroll_lock_axis = Some(ScrollAxisLock::Horizontal);
            self.scroll_lock_top_row = self.current_scroll_top_row();
        } else if self.scroll_lock_axis.is_none() && (abs_x > 0.5 || abs_y > 0.5) {
            if abs_x > abs_y * 1.1 {
                self.scroll_lock_axis = Some(ScrollAxisLock::Horizontal);
                self.scroll_lock_top_row = self.current_scroll_top_row();
            } else if abs_y > abs_x * 1.1 {
                self.scroll_lock_axis = Some(ScrollAxisLock::Vertical);
            }
        }

        // 縦スクロールロック中の場合は横スクロールを行わずスルー
        if self.scroll_lock_axis == Some(ScrollAxisLock::Vertical) {
            // 縦スクロール処理を実行
            let total_rows = self.editor.read(cx).line_starts().len().max(1);
            let list_h = self.list_bounds.get().map(|b| f32::from(b.size.height)).unwrap_or(600.0);
            let visible_rows = (list_h / ROW_HEIGHT).floor() as usize;
            let max_top_row = total_rows.saturating_sub(visible_rows.max(1));

            self.accum_scroll_y += delta_y;
            let rows_to_scroll = -(self.accum_scroll_y / ROW_HEIGHT) as isize;
            if rows_to_scroll != 0 {
                self.accum_scroll_y += (rows_to_scroll as f32) * ROW_HEIGHT;
                let new_offset = ((self.scroll_offset as isize) + rows_to_scroll).clamp(0, max_top_row as isize) as usize;
                if new_offset != self.scroll_offset {
                    self.scroll_offset = new_offset;
                    self.cached_comment_content_width.set(None);
                    self.cached_desc_content_width.set(None);
                    cx.notify();
                    cx.emit(HexViewEvent::Scrolled(self.scroll_offset));
                }
            }
            return;
        }

        let is_horizontal = self.scroll_lock_axis == Some(ScrollAxisLock::Horizontal) || column_only_horizontal || abs_x > abs_y;

        if is_horizontal && abs_x > 0.01 {
            // 横スクロールロック中は縦スクロール位置を固定して縦揺れを防止
            if self.scroll_lock_axis == Some(ScrollAxisLock::Horizontal) {
                let lock_row = self.scroll_lock_top_row;
                self.scroll_to_row(lock_row, cx);
            }

            let bounds = if let Some(b) = self.list_bounds.get().or_else(|| self.bounds.get()) {
                b
            } else {
                return;
            };

            let layout = self.current_layout(cx);
            let base_x = f32::from(bounds.left()) + 8.0;
            let relative_x = f32::from(event.position.x) - base_x;
            let target = layout
                .column_at(relative_x, self.outer_scroll_x)
                .map(HorizontalScrollTarget::Column)
                .unwrap_or(HorizontalScrollTarget::View);

            let current_offset = self.horizontal_offset(target);
            let max_offset = layout.max_offset(target);
            let new_offset = (current_offset - delta_x).clamp(0.0, max_offset);
            let consumed_delta = current_offset - new_offset;
            let residual_delta = delta_x - consumed_delta;
            let _ = self.set_horizontal_offset(target, new_offset, layout, true, cx);

            // Once the selected column reaches its own limit, let the
            // remaining gesture move the outer viewport, regardless of which
            // column is under the pointer.
            if can_chain_to_outer(target, residual_delta) {
                let current_outer = self.outer_scroll_x;
                let new_outer = (current_outer - residual_delta).clamp(0.0, layout.outer_max);
                let _ = self.set_horizontal_offset(HorizontalScrollTarget::View, new_outer, layout, true, cx);
            }
        } else if !is_horizontal && abs_y > 0.01 {
            let total_rows = self.editor.read(cx).line_starts().len().max(1);
            let list_h = self.list_bounds.get().map(|b| f32::from(b.size.height)).unwrap_or(600.0);
            let visible_rows = (list_h / ROW_HEIGHT).floor() as usize;
            let max_top_row = total_rows.saturating_sub(visible_rows.max(1));

            self.accum_scroll_y += delta_y;
            let rows_to_scroll = -(self.accum_scroll_y / ROW_HEIGHT) as isize;
            if rows_to_scroll != 0 {
                self.accum_scroll_y += (rows_to_scroll as f32) * ROW_HEIGHT;
                let new_offset = ((self.scroll_offset as isize) + rows_to_scroll).clamp(0, max_top_row as isize) as usize;
                if new_offset != self.scroll_offset {
                    self.scroll_offset = new_offset;
                    self.cached_comment_content_width.set(None);
                    self.cached_desc_content_width.set(None);
                    cx.notify();
                    cx.emit(HexViewEvent::Scrolled(self.scroll_offset));
                }
            }
        }
    }

    fn update_scrollbar_drag(&mut self, current_y: f32, cx: &mut Context<Self>) {
        if !self.is_dragging_scrollbar {
            return;
        }

        let delta_y = current_y - self.scrollbar_drag_start_y;
        let total_rows = self.editor.read(cx).line_starts().len().max(1);
        let list_h = self.list_bounds.get().map(|b| f32::from(b.size.height)).unwrap_or(600.0);
        let visible_rows = (list_h / ROW_HEIGHT).floor() as usize;
        let max_top_row = total_rows.saturating_sub(visible_rows.max(1));
        let ratio = (visible_rows as f64 / total_rows as f64).clamp(0.0, 1.0);
        let thumb_h = (list_h as f64 * ratio).clamp(24.0, list_h as f64) as f32;
        let max_thumb_top = (list_h - thumb_h).max(0.0);

        if max_thumb_top > 0.0 && max_top_row > 0 {
            let delta_ratio = delta_y as f64 / max_thumb_top as f64;
            let delta_rows = delta_ratio * max_top_row as f64;
            let new_row = ((self.scrollbar_drag_start_row as f64 + delta_rows).round() as isize).clamp(0, max_top_row as isize) as usize;
            self.scroll_to_row(new_row, cx);
        }
    }

    fn on_mouse_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.is_dragging_scrollbar {
            self.is_dragging_scrollbar = false;
            cx.notify();
        }
        if self.resizing_column.is_some() {
            self.resizing_column = None;
            cx.notify();
        }
        if self.is_selecting {
            self.is_selecting = false;
            let (start, end, cursor_offset) = {
                let ed = self.editor.read(cx);
                (ed.selection_start, ed.selection_end, ed.cursor_offset)
            };
            cx.emit(HexViewEvent::SelectionChanged { start, end });
            cx.emit(HexViewEvent::CursorMoved(cursor_offset));
        }
    }

    pub fn font_family(mut self, font_family: impl Into<SharedString>) -> Self {
        self.font_family_prop = font_family.into();
        self.hex_cell_width = 0.0;
        self
    }

    pub fn font_size(mut self, font_size: impl Into<Pixels>) -> Self {
        self.font_size_prop = font_size.into();
        self.hex_cell_width = 0.0;
        self
    }

    pub fn set_font_family(&mut self, font_family: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.font_family_prop = font_family.into();
        self.hex_cell_width = 0.0;
        self.hex_content_width = 0.0;
        self.cached_comment_content_width.set(None);
        self.cached_desc_content_width.set(None);
        self.cursor_reveal_pending = true;
        cx.notify();
    }

    pub fn set_font_size(&mut self, font_size: impl Into<Pixels>, cx: &mut Context<Self>) {
        self.font_size_prop = font_size.into();
        self.hex_cell_width = 0.0;
        self.hex_content_width = 0.0;
        self.cached_comment_content_width.set(None);
        self.cached_desc_content_width.set(None);
        self.cursor_reveal_pending = true;
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
        self.cursor_reveal_pending = true;
        cx.notify();
    }

    pub fn set_show_header(&mut self, show: bool, cx: &mut Context<Self>) {
        self.show_header = show;
        cx.notify();
    }

    pub fn set_show_ascii(&mut self, show: bool, cx: &mut Context<Self>) {
        self.show_ascii = show;
        self.cursor_reveal_pending = true;
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
        self.scroll_offset
    }

    pub fn viewport_byte_range(&self, cx: &App) -> (usize, usize) {
        let editor = self.editor.read(cx);
        let line_starts = editor.line_starts();
        let current_top = self.scroll_offset;
        let start_byte = line_starts.get(current_top).unwrap_or(0);
        let visible_rows = if let Some(bounds) = self.list_bounds.get() {
            (f32::from(bounds.size.height) / ROW_HEIGHT).ceil() as usize
        } else {
            30
        };
        let end_row = (current_top + visible_rows + 2).min(line_starts.len());
        let end_byte = if end_row < line_starts.len() {
            line_starts.get(end_row).unwrap_or_else(|| editor.total_size())
        } else {
            editor.total_size()
        };
        (start_byte, end_byte)
    }

    pub fn scroll_to_row(&mut self, row: usize, cx: &mut Context<Self>) {
        let total_rows = self.editor.read(cx).line_starts().len().max(1);
        let list_h = self.list_bounds.get().map(|b| f32::from(b.size.height)).unwrap_or(600.0);
        let visible_rows = (list_h / ROW_HEIGHT).floor() as usize;
        let max_offset = total_rows.saturating_sub(visible_rows.max(1));
        let new_offset = row.min(max_offset);

        if self.scroll_offset != new_offset {
            self.scroll_offset = new_offset;
            self.cached_comment_content_width.set(None);
            self.cached_desc_content_width.set(None);
            self.accum_scroll_y = 0.0;
            cx.notify();
            cx.emit(HexViewEvent::Scrolled(self.scroll_offset));
        }
    }

    pub fn scroll_to_bottom_row(&mut self, row: usize, cx: &mut Context<Self>) {
        let list_h = self.list_bounds.get().map(|b| f32::from(b.size.height)).unwrap_or(600.0);
        let visible_rows = (list_h / ROW_HEIGHT).floor() as usize;
        let target_top = row.saturating_sub(visible_rows.saturating_sub(1));
        self.scroll_to_row(target_top, cx);
    }

    fn ensure_cursor_visible(&mut self, cx: &mut Context<Self>) {
        let editor = self.editor.read(cx);
        let cursor_offset = editor.cursor_offset;
        let line_starts = editor.line_starts();
        let cursor_row = Editor::find_line_index(cursor_offset, &line_starts);

        let visible_rows = if let Some(bounds) = self.list_bounds.get() {
            (f32::from(bounds.size.height) / ROW_HEIGHT).floor() as usize
        } else {
            30
        };
        let top_row = self.scroll_offset;
        let bottom_row = top_row + visible_rows.saturating_sub(1);

        if cursor_row < top_row {
            self.scroll_to_row(cursor_row, cx);
        } else if cursor_row > bottom_row {
            self.scroll_to_bottom_row(cursor_row, cx);
        }

        // Horizontal visibility is adjusted from the row's shaped layout in
        // `render`, where GPUI's actual glyph positions are available.
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
        let visible_rows = if let Some(bounds) = self.list_bounds.get() {
            (f32::from(bounds.size.height) / ROW_HEIGHT).floor() as usize
        } else {
            30
        };
        let count = visible_rows.saturating_sub(2).max(1);
        self.exec_move(window, cx, |e| {
            for _ in 0..count {
                e.move_up();
            }
        });
    }

    fn page_down(&mut self, _: &PageDown, window: &mut Window, cx: &mut Context<Self>) {
        let visible_rows = if let Some(bounds) = self.list_bounds.get() {
            (f32::from(bounds.size.height) / ROW_HEIGHT).floor() as usize
        } else {
            30
        };
        let count = visible_rows.saturating_sub(2).max(1);
        self.exec_move(window, cx, |e| {
            for _ in 0..count {
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
        let visible_rows = if let Some(bounds) = self.list_bounds.get() {
            (f32::from(bounds.size.height) / ROW_HEIGHT).floor() as usize
        } else {
            30
        };
        let count = visible_rows.saturating_sub(2).max(1);
        self.exec_select(window, cx, |e| {
            for _ in 0..count {
                e.select_up();
            }
        });
    }

    fn select_page_down(&mut self, _: &SelectPageDown, window: &mut Window, cx: &mut Context<Self>) {
        let visible_rows = if let Some(bounds) = self.list_bounds.get() {
            (f32::from(bounds.size.height) / ROW_HEIGHT).floor() as usize
        } else {
            30
        };
        let count = visible_rows.saturating_sub(2).max(1);
        self.exec_select(window, cx, |e| {
            for _ in 0..count {
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

    fn notify_document_changed(&self, cx: &mut App) {
        let path = self.editor.read(cx).document.read().ok().map(|d| d.path().to_path_buf());
        if let Some(path) = path {
            let service = crate::app_state::AppState::global(cx).editor_service.clone();
            service.notify_document_changed(&path, cx);
        }
    }

    pub fn add_custom_break(&mut self, _: &AddCustomBreak, window: &mut Window, cx: &mut Context<Self>) {
        cx.focus_self(window);
        self.editor.update(cx, |editor, cx| {
            let offset = editor.cursor_offset;
            if offset > 0 {
                editor.add_custom_break(offset);
            }
            cx.notify();
        });
        self.notify_document_changed(cx);
    }

    pub fn remove_custom_break_backward(&mut self, _: &RemoveCustomBreakBackward, window: &mut Window, cx: &mut Context<Self>) {
        cx.focus_self(window);
        self.editor.update(cx, |editor, cx| {
            let offset = editor.cursor_offset;
            if offset > 0 && editor.custom_breaks.read().expect("custom_breaks read lock").contains(&(offset - 1)) {
                editor.remove_custom_break(offset - 1);
            }
            cx.notify();
        });
        self.notify_document_changed(cx);
    }

    pub fn remove_custom_break_forward(&mut self, _: &RemoveCustomBreakForward, window: &mut Window, cx: &mut Context<Self>) {
        cx.focus_self(window);
        self.editor.update(cx, |editor, cx| {
            let offset = editor.cursor_offset;
            if editor.custom_breaks.read().expect("custom_breaks read lock").contains(&offset) {
                editor.remove_custom_break(offset);
            }
            cx.notify();
        });
        self.notify_document_changed(cx);
    }

    pub fn join_line(&mut self, _: &JoinLine, window: &mut Window, cx: &mut Context<Self>) {
        cx.focus_self(window);
        self.cursor_reveal_pending = true;
        self.editor.update(cx, |editor, cx| {
            editor.join_line();
            cx.notify();
        });
        self.notify_document_changed(cx);
    }

    pub fn clear_all_custom_breaks(&mut self, _: &ClearAllCustomBreaks, window: &mut Window, cx: &mut Context<Self>) {
        cx.focus_self(window);
        self.editor.update(cx, |editor, cx| {
            editor.clear_all_custom_breaks();
            cx.notify();
        });
        self.notify_document_changed(cx);
    }

    fn copy_formatted(&self, format: CopyFormat, window: &mut Window, cx: &mut Context<Self>) {
        let formatted = {
            let editor = self.editor.read(cx);
            let selected_range = editor.selected_range_or_cursor();
            let doc = editor.document.read().expect("document read lock");
            let total = doc.buffer.len();
            if total == 0 {
                String::new()
            } else {
                let (start_offset, slice) = if let Some(range) = selected_range {
                    (range.start, doc.buffer.get_range(range.start, range.len()))
                } else {
                    let off = editor.cursor_offset.min(total.saturating_sub(1));
                    (off, doc.buffer.get_range(off, 1))
                };
                format_bytes(slice, start_offset, format)
            }
        };

        self.focus_handle.focus(window);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(formatted));
    }

    pub fn copy(&mut self, _: &Copy, window: &mut Window, cx: &mut Context<Self>) {
        let formatted = {
            let editor = self.editor.read(cx);
            let selected_range = editor.selected_range_or_cursor();
            let doc = editor.document.read().expect("document read lock");
            let total = doc.buffer.len();
            if total == 0 {
                String::new()
            } else if let Some(range) = selected_range {
                let radix = editor.radix;
                let group_size = editor.group_size;
                let is_big_endian = editor.is_big_endian;
                let line_starts = editor.line_starts();
                crate::core::radix::format_display_content_with_lines(doc.buffer.data(), range, &line_starts, radix, group_size, is_big_endian)
            } else {
                String::new()
            }
        };

        self.focus_handle.focus(window);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(formatted));
    }

    pub fn copy_as_hexdump(&mut self, _: &CopyAsHexDump, window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::HexDump, window, cx);
    }

    pub fn copy_as_cpp_array(&mut self, _: &CopyAsCppArray, window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::CppArray, window, cx);
    }

    pub fn copy_as_hex_stream(&mut self, _: &CopyAsHexStream, window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::HexStream, window, cx);
    }

    pub fn copy_as_hex_spaces(&mut self, _: &CopyAsHexSpaces, window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::HexWithSpaces, window, cx);
    }

    pub fn copy_as_printable_text(&mut self, _: &CopyAsPrintableText, window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::PrintableText, window, cx);
    }

    pub fn copy_as_base64(&mut self, _: &CopyAsBase64, window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::Base64, window, cx);
    }

    pub fn copy_as_escaped_string(&mut self, _: &CopyAsEscapedString, window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::EscapedString, window, cx);
    }

    pub fn copy_as_binary(&mut self, _: &CopyAsBinary, window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::Binary, window, cx);
    }

    pub fn copy_as_rust_array(&mut self, _: &CopyAsRustArray, window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::RustArray, window, cx);
    }

    pub fn copy_as_json_array(&mut self, _: &CopyAsJsonArray, window: &mut Window, cx: &mut Context<Self>) {
        self.copy_formatted(CopyFormat::JsonArray, window, cx);
    }

    fn apply_highlight(&mut self, color: Option<Hsla>, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window);
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
        self.notify_document_changed(cx);
    }

    pub fn highlight_red(&mut self, _: &HighlightRed, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(Some(hsla(0.0, 0.75, 0.55, 0.35)), window, cx);
    }

    pub fn highlight_orange(&mut self, _: &HighlightOrange, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(Some(hsla(30.0 / 360.0, 0.85, 0.55, 0.35)), window, cx);
    }

    pub fn highlight_yellow(&mut self, _: &HighlightYellow, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(Some(hsla(50.0 / 360.0, 0.85, 0.50, 0.35)), window, cx);
    }

    pub fn highlight_green(&mut self, _: &HighlightGreen, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(Some(hsla(120.0 / 360.0, 0.65, 0.45, 0.35)), window, cx);
    }

    pub fn highlight_cyan(&mut self, _: &HighlightCyan, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(Some(hsla(180.0 / 360.0, 0.70, 0.45, 0.35)), window, cx);
    }

    pub fn highlight_blue(&mut self, _: &HighlightBlue, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(Some(hsla(215.0 / 360.0, 0.75, 0.55, 0.35)), window, cx);
    }

    pub fn highlight_purple(&mut self, _: &HighlightPurple, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(Some(hsla(280.0 / 360.0, 0.70, 0.55, 0.35)), window, cx);
    }

    pub fn highlight_pink(&mut self, _: &HighlightPink, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(Some(hsla(330.0 / 360.0, 0.75, 0.55, 0.35)), window, cx);
    }

    pub fn clear_highlight(&mut self, _: &ClearHighlight, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_highlight(None, window, cx);
    }

    pub fn clear_all_highlights(&mut self, _: &ClearAllHighlights, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.editor.read(cx).highlights_snapshot().len();
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
        let doc_path = editor.read(cx).document.read().ok().map(|d| d.path().to_path_buf());
        cx.spawn_in(window, async move |_this, window| {
            if let Ok(0) = prompt.await {
                window
                    .update(|_, cx| {
                        editor.update(cx, |editor, cx| {
                            editor.clear_all_custom_highlights();
                            cx.notify();
                        });
                        if let Some(ref path) = doc_path {
                            let service = crate::app_state::AppState::global(cx).editor_service.clone();
                            service.notify_document_changed(path, cx);
                        }
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

    fn offset_from_point(&self, point: Point<Pixels>, _window: &Window, cx: &App) -> Option<usize> {
        let root_bounds = self.bounds.get()?;
        let header_h = if self.show_header { HEADER_HEIGHT } else { 0.0 };

        if point.x < root_bounds.left() || point.y < root_bounds.top() + px(header_h) {
            return None;
        }

        let list_bounds = self.list_bounds.get()?;

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

        let rel_y = f32::from(point.y - list_bounds.top()).max(0.0);
        let row_offset_in_view = (rel_y / ROW_HEIGHT).floor() as usize;
        let row_idx = (self.scroll_offset + row_offset_in_view).min(line_starts.len().saturating_sub(1));
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

        let parse_result = editor.parse_result();
        let is_struct_mode = editor.show_inline_structure_view && parse_result.is_some();
        let base_x = f32::from(list_bounds.left()) + 8.0;
        let layout = self.current_layout(cx);
        let relative_x = f32::from(point.x) - base_x;
        if relative_x < layout.fixed_width {
            return Some(line_offset);
        }
        let world_x = relative_x + self.outer_scroll_x;

        if is_struct_mode && layout.description.map(|column| world_x >= column.start).unwrap_or(false) {
            if let Some(ref parse_res) = parse_result {
                let leaf_fields = parse_res.find_leaf_fields_starting_at(line_offset, chunk_len);
                if let Some(first) = leaf_fields.first() {
                    return Some(first.offset);
                }
            }
            return Some(line_offset);
        }

        let byte_offset_in_row = if !is_struct_mode
            && let Some(ascii_column) = layout.ascii
            && world_x >= ascii_column.start
            && world_x < ascii_column.end()
        {
            let raw_idx = ascii_byte_index_from_world_x(world_x, ascii_column, self.ascii_scroll_x);
            let group_bytes = self.group_size.byte_count();
            let abs_offset = line_offset + raw_idx;
            let group_start_abs = (abs_offset / group_bytes) * group_bytes;
            group_start_abs.saturating_sub(line_offset)
        } else {
            let source = build_hex_text_source(
                doc.buffer.get_range(line_offset, chunk_len),
                line_offset,
                self.radix,
                self.group_size,
                self.is_big_endian,
            );
            let cell_width = px(self.hex_cell_width.max(1.0));
            let origin_x = px(base_x + layout.hex.start - self.outer_scroll_x - self.hex_scroll_x);

            let mut target_group_idx = 0;
            for (idx, group) in source.groups.iter().enumerate() {
                let (group_start, group_end) = hex_group_x(*group, origin_x, cell_width);
                let next_start = source
                    .groups
                    .get(idx + 1)
                    .map(|next| origin_x + hex_grid_x(next.text_start, cell_width))
                    .unwrap_or(group_end);
                if point.x < next_start || idx + 1 == source.groups.len() {
                    target_group_idx = idx;
                    break;
                }
                if point.x >= group_start {
                    target_group_idx = idx;
                }
            }

            let group = source.groups[target_group_idx];
            group.chunk_start
        };

        let byte_idx = byte_offset_in_row.min(chunk_len.saturating_sub(1));
        Some(line_offset + byte_idx)
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_hex_row(
        row_idx: usize,
        bounds: Bounds<Pixels>,
        top_visible_row: usize,
        doc: &Document,
        line_starts: &crate::core::editor::LineMap,
        parse_result: Option<&ParseResult>,
        collapsed_structs: &std::collections::HashSet<String>,
        encoding: Encoding,
        radix: DisplayRadix,
        group_size: ByteGroupSize,
        is_big_endian: bool,
        cursor_offset: usize,
        min_sel: usize,
        max_sel: usize,
        highlights: &[(Range<usize>, Hsla)],
        highlight_items: &[crate::core::highlight::HighlightItem],
        max_highlight_len: usize,
        show_offset: bool,
        show_ascii: bool,
        ascii_col_width: f32,
        ascii_scroll_x: f32,
        is_focused: bool,
        outer_scroll_x: f32,
        hex_scroll_x: f32,
        desc_scroll_x: f32,
        comment_scroll_x: f32,
        address_col_width: f32,
        hex_col_width: f32,
        hex_cell_width: Pixels,
        desc_col_width: f32,
        comment_col_width: f32,
        font_family: SharedString,
        font_size: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) {
        let offset = match line_starts.get(row_idx) {
            Some(o) => o,
            None => return,
        };
        let next_offset = if row_idx + 1 < line_starts.len() {
            line_starts.get(row_idx + 1).unwrap_or(doc.buffer.len())
        } else {
            doc.buffer.len()
        };

        let chunk_len = next_offset - offset;
        let chunk = doc.buffer.get_range(offset, chunk_len);

        let is_struct_mode = parse_result.is_some();

        let active_row_highlights = row_highlights(highlights, max_highlight_len, offset, next_offset);
        let (selection_bg, cursor_bg, muted_color, fg_color, accent_fg_color, border_color, _sidebar_bg, bg_color_theme) = {
            let theme = cx.theme();
            (
                if is_focused { theme.selection } else { theme.muted_foreground.opacity(0.3) },
                darken_cursor_color(theme.accent),
                theme.muted_foreground,
                theme.foreground,
                theme.accent_foreground,
                theme.border,
                theme.sidebar,
                theme.background,
            )
        };
        let line_height = px(ROW_HEIGHT);
        let font = gpui::font(font_family);

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
        let hex_start_x = base_x + px(offset_w + gap) - px(outer_scroll_x);
        let hex_end_x = hex_start_x + px(hex_col_width);
        let (comment_start_x, ascii_width) = if is_struct_mode {
            let desc_start_x = hex_end_x + px(gap);
            let comment_start_x = desc_start_x + px(desc_col_width + gap);
            (comment_start_x, 0.0)
        } else {
            let ascii_w = if show_ascii { ascii_col_width } else { 0.0 };
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
        let scrollable_mask_bounds = {
            let left = base_x + px(offset_w + gap);
            let right = bounds.right() - px(VERTICAL_SCROLLBAR_WIDTH);
            Bounds::new(point(left, bounds.top()), size((right - left).max(px(0.0)), px(ROW_HEIGHT)))
        };
        window.with_content_mask(
            Some(gpui::ContentMask {
                bounds: scrollable_mask_bounds,
            }),
            |window| {
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
            },
        );

        // Build and shape the exact text stream before painting any geometry.
        // Group geometry uses the fixed cell grid; glyphs are centered in that
        // grid during the text pass.
        let hex_source = build_hex_text_source(chunk, offset, radix, group_size, is_big_endian);
        let mut hex_runs: Vec<gpui::TextRun> = Vec::with_capacity(hex_source.groups.len() * 2);
        let mut group_visuals: Vec<(Hsla, bool)> = Vec::with_capacity(hex_source.groups.len());
        let mut group_text_colors: Vec<Hsla> = Vec::with_capacity(hex_source.groups.len());

        for (group_idx, group) in hex_source.groups.iter().enumerate() {
            let item_start_offset = offset + group.chunk_start;
            let item_end_offset = offset + group.chunk_end;
            let item_slice = &chunk[group.chunk_start..group.chunk_end];
            let is_cursor = cursor_offset >= item_start_offset && cursor_offset < item_end_offset;
            let is_zero = is_group_zero(item_slice);
            let text_color = if is_cursor && is_focused {
                fg_color
            } else if is_zero {
                muted_color.opacity(0.5)
            } else {
                fg_color
            };

            let is_selected = if min_sel <= max_sel {
                item_start_offset <= max_sel && item_end_offset > min_sel
            } else {
                false
            };

            let current_hl_color = highlight_color_for_range(item_start_offset, item_end_offset, is_selected, active_row_highlights);
            let bg_color = if is_selected {
                selection_bg
            } else {
                current_hl_color.unwrap_or_else(|| hsla(0.0, 0.0, 0.0, 0.0))
            };
            group_visuals.push((bg_color, is_cursor));
            group_text_colors.push(text_color);

            if group_idx > 0 {
                hex_runs.push(gpui::TextRun {
                    len: 1,
                    font: font.clone(),
                    color: hsla(0.0, 0.0, 0.0, 0.0),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                });
            }
            hex_runs.push(gpui::TextRun {
                len: group.text_end - group.text_start,
                font: font.clone(),
                color: text_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }

        let shaped_hex = window.text_system().shape_line(hex_source.text.clone(), font_size, &hex_runs, None);
        let text_origin_x = hex_start_x - px(hex_scroll_x);
        let total_data_width = f32::from(hex_grid_width(hex_source.text.len(), hex_cell_width));

        // 2. Background Quads Pass for Data Items (with clipping mask)
        let hex_mask_bounds = Bounds::new(point(hex_start_x, bounds.top()), size(px(hex_col_width), px(ROW_HEIGHT)));

        window.with_content_mask(
            Some(gpui::ContentMask {
                bounds: scrollable_mask_bounds,
            }),
            |window| {
                window.with_content_mask(Some(gpui::ContentMask { bounds: hex_mask_bounds }), |window| {
                    for (item_idx, group) in hex_source.groups.iter().enumerate() {
                        let (bg_color, is_cursor) = group_visuals[item_idx];
                        let (group_start_x, group_end_x) = hex_group_x(*group, text_origin_x, hex_cell_width);
                        let previous_group_end_x = if item_idx > 0 {
                            hex_group_x(hex_source.groups[item_idx - 1], text_origin_x, hex_cell_width).1
                        } else {
                            group_start_x
                        };
                        let previous_has_background = item_idx > 0 && group_visuals[item_idx - 1].0.a > 0.0;
                        let has_next = item_idx + 1 < hex_source.groups.len();
                        let next_group_start_x = if has_next {
                            hex_group_x(hex_source.groups[item_idx + 1], text_origin_x, hex_cell_width).0
                        } else {
                            group_end_x
                        };
                        let next_has_background = has_next && group_visuals[item_idx + 1].0.a > 0.0;
                        let fill_start_x = if previous_has_background {
                            pixel_midpoint(previous_group_end_x, group_start_x)
                        } else {
                            group_start_x
                        };
                        let fill_end_x = if next_has_background {
                            pixel_midpoint(group_end_x, next_group_start_x)
                        } else {
                            group_end_x
                        };

                        if bg_color.a > 0.0 {
                            let item_fill_bounds = Bounds::new(
                                point(fill_start_x, bounds.top() + px(1.0)),
                                size(fill_end_x - fill_start_x, px(ROW_HEIGHT - 2.0)),
                            );
                            window.paint_quad(gpui::fill(item_fill_bounds, bg_color));
                        }

                        if is_cursor {
                            let cursor_border_color = if is_focused {
                                cursor_bg
                            } else {
                                darken_cursor_color(muted_color).opacity(0.8)
                            };
                            let cursor_start_x = if bg_color.a > 0.0 { fill_start_x } else { group_start_x };
                            let cursor_end_x = if bg_color.a > 0.0 { fill_end_x } else { group_end_x };
                            let item_box_bounds = Bounds::new(
                                point(cursor_start_x, bounds.top() + px(1.0)),
                                size(cursor_end_x - cursor_start_x, px(ROW_HEIGHT - 2.0)),
                            );
                            paint_cursor_border(window, item_box_bounds, cursor_border_color);
                        }
                    }

                    // 3. Text Pass for Data Items (same shaped line as the geometry pass)
                    if !hex_source.text.is_empty() {
                        paint_centered_hex_glyphs(
                            &shaped_hex,
                            &hex_source.groups,
                            &group_text_colors,
                            hex_cell_width,
                            point(text_origin_x, bounds.top() + px(2.0)),
                            line_height,
                            window,
                        );
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
                    if hex_scroll_x + hex_col_width < total_data_width - 1.0 {
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
            },
        );

        // 3. ASCII Column (when not in structure definition mode and ASCII view is enabled)
        if !is_struct_mode && show_ascii {
            window.with_content_mask(
                Some(gpui::ContentMask {
                    bounds: scrollable_mask_bounds,
                }),
                |window| {
                    let ascii_start_x = hex_end_x + px(gap);
                    let ascii_content_start_x = ascii_start_x - px(ascii_scroll_x);
                    let ascii_mask_bounds = Bounds::new(point(ascii_start_x, bounds.top()), size(px(ascii_width), px(ROW_HEIGHT)));
                    window.with_content_mask(Some(gpui::ContentMask { bounds: ascii_mask_bounds }), |window| {
                        let char_map = build_ascii_char_map(encoding, doc.buffer.data(), offset, chunk.len());
                        let group_bytes = group_size.byte_count();

                        // 1. Background Quads Pass for ASCII Cells
                        for (j, _) in chunk.iter().enumerate() {
                            let byte_pos = offset + j;
                            let in_selected_group = if min_sel <= max_sel {
                                let group_start = (byte_pos / group_bytes) * group_bytes;
                                let group_end = group_start + group_bytes;
                                group_start <= max_sel && group_end > min_sel
                            } else {
                                false
                            };

                            let current_hl_color = highlight_color_for_range(byte_pos, byte_pos.saturating_add(1), in_selected_group, active_row_highlights);
                            let bg_color = if in_selected_group {
                                selection_bg
                            } else {
                                current_hl_color.unwrap_or_else(|| hsla(0.0, 0.0, 0.0, 0.0))
                            };

                            let ascii_item_bounds = Bounds::new(
                                point(ascii_content_start_x + px(j as f32 * ASCII_CELL_WIDTH), bounds.top() + px(1.0)),
                                size(px(ASCII_CELL_WIDTH), px(ROW_HEIGHT - 2.0)),
                            );

                            if bg_color.a > 0.0 {
                                window.paint_quad(gpui::fill(ascii_item_bounds, bg_color));
                            }
                        }

                        // 2. Cursor Border Pass for ASCII Groups
                        for group in hex_source.groups.iter() {
                            let item_start_offset = offset + group.chunk_start;
                            let item_end_offset = offset + group.chunk_end;
                            let is_cursor = cursor_offset >= item_start_offset && cursor_offset < item_end_offset;

                            if is_cursor {
                                let cursor_border_color = if is_focused {
                                    cursor_bg
                                } else {
                                    darken_cursor_color(muted_color).opacity(0.8)
                                };
                                let group_start_x = ascii_content_start_x + px(group.chunk_start as f32 * ASCII_CELL_WIDTH);
                                let group_width = px((group.chunk_end - group.chunk_start) as f32 * ASCII_CELL_WIDTH);
                                let ascii_group_bounds = Bounds::new(point(group_start_x, bounds.top() + px(1.0)), size(group_width, px(ROW_HEIGHT - 2.0)));
                                paint_cursor_border(window, ascii_group_bounds, cursor_border_color);
                            }
                        }

                        // Paint each glyph centered in its fixed byte cell.
                        // Shaping remains one character at a time, as before;
                        // only the already-known glyph position is adjusted.
                        for (j, opt) in char_map.into_iter().enumerate() {
                            let c = if let Some((c, _)) = opt { c } else { continue };
                            let is_control = (c as u32) < 0x20 || (c as u32) == 0x7f;
                            let text_color = if is_control || c == '·' { muted_color.opacity(0.4) } else { fg_color };

                            let ch_str = c.to_string();
                            let ascii_run = gpui::TextRun {
                                len: ch_str.len(),
                                font: font.clone(),
                                color: text_color,
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            };
                            let shaped = window.text_system().shape_line(ch_str.into(), font_size, &[ascii_run], None);
                            let glyph_offset = centered_glyph_offset(ASCII_CELL_WIDTH, f32::from(shaped.width));
                            let ascii_pos = point(ascii_content_start_x + px(j as f32 * ASCII_CELL_WIDTH + glyph_offset), bounds.top() + px(2.0));
                            let _ = shaped.paint(ascii_pos, line_height, window, cx);
                        }

                        // Edge fades when the ASCII row is horizontally scrolled or clipped.
                        if ascii_scroll_x > 1.0 {
                            let bg = bg_color_theme;
                            for step in 0..5 {
                                let x = ascii_start_x + px(step as f32 * 3.2);
                                let alpha = 1.0 - (step as f32 / 5.0);
                                window.paint_quad(gpui::fill(
                                    Bounds::new(point(x, bounds.top()), size(px(3.4), px(ROW_HEIGHT))),
                                    bg.opacity(alpha * 0.95),
                                ));
                            }
                        }

                        if ascii_scroll_x + ascii_width < chunk.len() as f32 * ASCII_CELL_WIDTH - 1.0 {
                            let fade_w = 22.0;
                            let fade_start = ascii_start_x + px(ascii_width - fade_w);
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
                },
            );
        }

        // 4. Description Column (when structure definition is present)
        if let Some(ref parse_res) = parse_result {
            let desc_start_x = hex_end_x + px(SECTION_GAP);
            let desc_end_x = desc_start_x + px(desc_col_width);

            let active_ranges = parse_res.find_active_struct_ranges(offset, chunk_len);
            let container_structs = parse_res.find_container_structs_starting_at(offset, chunk_len);
            let leaf_fields = parse_res.find_leaf_fields_starting_at(offset, chunk_len);

            let is_collapsed = container_structs.first().map(|c| collapsed_structs.contains(&c.id)).unwrap_or(false);

            let struct_depth = active_ranges.len().saturating_sub(1);
            let indent_level = if !container_structs.is_empty() {
                active_ranges
                    .iter()
                    .find(|r| container_structs.first().map(|c| c.id == r.id).unwrap_or(false))
                    .map(|r| r.depth)
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
                for f in leaf_fields {
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

                window.with_content_mask(
                    Some(gpui::ContentMask {
                        bounds: scrollable_mask_bounds,
                    }),
                    |window| {
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
                    },
                );
            }
        }

        // 5. Highlight Comments Column
        let row_highlight_comments: Vec<(gpui::Hsla, SharedString)> = highlight_items
            .iter()
            .filter(|h| {
                if h.comment.trim().is_empty() {
                    return false;
                }
                let h_start_row = Editor::find_line_index(h.offset, line_starts);
                let h_end_offset = h.offset.saturating_add(h.size);
                let h_last_byte = h_end_offset.saturating_sub(1).max(h.offset);
                let h_end_row = Editor::find_line_index(h_last_byte, line_starts);
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

            window.with_content_mask(
                Some(gpui::ContentMask {
                    bounds: scrollable_mask_bounds,
                }),
                |window| {
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
                },
            );
        }
    }

    fn paint_scrollbar(
        list_bounds: Bounds<Pixels>,
        scroll_offset: usize,
        total_rows: usize,
        is_dragging: bool,
        is_hovered: bool,
        theme: &gpui_component::Theme,
        window: &mut Window,
    ) {
        let list_h = f32::from(list_bounds.size.height);
        let visible_rows = (list_h / ROW_HEIGHT).floor() as usize;
        if total_rows <= visible_rows || list_h <= 0.0 {
            return;
        }
        let max_top_row = total_rows.saturating_sub(visible_rows.max(1));
        let ratio = (visible_rows as f64 / total_rows as f64).clamp(0.0, 1.0);
        let thumb_h = (list_h as f64 * ratio).clamp(24.0, list_h as f64) as f32;
        let max_thumb_top = (list_h - thumb_h).max(0.0);

        let scroll_ratio = if max_top_row > 0 {
            (scroll_offset as f64 / max_top_row as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let thumb_top = (scroll_ratio * max_thumb_top as f64) as f32;

        let bar_w = 10.0;
        let bar_x = list_bounds.right() - px(bar_w);
        let bar_bounds = Bounds::new(point(bar_x, list_bounds.top()), size(px(bar_w), list_bounds.size.height));

        if is_hovered || is_dragging {
            window.paint_quad(gpui::fill(bar_bounds, theme.border.opacity(0.15)));
        }

        let thumb_bounds = Bounds::new(point(bar_x + px(2.0), list_bounds.top() + px(thumb_top)), size(px(6.0), px(thumb_h)));
        let thumb_color = if is_dragging {
            theme.muted_foreground.opacity(0.85)
        } else if is_hovered {
            theme.muted_foreground.opacity(0.7)
        } else {
            theme.border.opacity(0.6)
        };
        let mut quad = gpui::fill(thumb_bounds, thumb_color);
        quad.corner_radii = gpui::Corners::all(px(3.0));
        window.paint_quad(quad);
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
                editor.show_inline_structure_view && editor.parse_result().is_some(),
            )
        };
        let ascii_col_width = self.effective_ascii_col_width(max_bytes_per_row);

        let container = div()
            .flex()
            .flex_col()
            .bg(theme.background)
            .font_family(font_family.clone())
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .key_context(CONTEXT);

        let container = container.focus_indicator(is_focused, theme);

        let group_bytes = self.group_size.byte_count();
        let items_in_row = max_bytes_per_row.div_ceil(group_bytes).max(1);
        let hex_cell_width = if self.hex_cell_width > 0.0 {
            px(self.hex_cell_width)
        } else {
            let measured = measure_hex_cell_width(window, gpui::font(font_family.clone()), font_size);
            self.hex_cell_width = f32::from(measured);
            measured
        };
        let probe_bytes = vec![0u8; items_in_row * group_bytes];
        let probe_source = build_hex_text_source(&probe_bytes, 0, self.radix, self.group_size, self.is_big_endian);
        let total_data_width = f32::from(hex_grid_width(probe_source.text.len(), hex_cell_width));
        self.hex_content_width = total_data_width;
        if self.hex_col_width <= 0.0 {
            self.hex_col_width = total_data_width;
        }
        let max_hex_scroll = (total_data_width - self.hex_col_width).max(0.0);
        self.hex_scroll_x = self.hex_scroll_x.clamp(0.0, max_hex_scroll);
        let layout = self.current_layout(cx);
        self.outer_scroll_x = self.outer_scroll_x.clamp(0.0, layout.outer_max);
        self.outer_scroll_handle.set_offset(point(-px(self.outer_scroll_x), px(0.0)));

        // Keep the cursor visible using the same fixed grid as the paint pass.
        let cursor_offset = self.editor.read(cx).cursor_offset;
        let should_reveal_cursor = self.cursor_reveal_pending || self.last_cursor_offset != Some(cursor_offset);
        if should_reveal_cursor {
            let cursor_layout = {
                let editor = self.editor.read(cx);
                let line_starts = editor.line_starts();
                let row = Editor::find_line_index(editor.cursor_offset, &line_starts);
                let line_offset = line_starts.get(row).unwrap_or(0);
                let next_offset = line_starts
                    .get(row + 1)
                    .unwrap_or_else(|| editor.document.read().map(|doc| doc.buffer.len()).unwrap_or(line_offset));
                editor.document.read().ok().map(|doc| {
                    let source = build_hex_text_source(
                        doc.buffer.get_range(line_offset, next_offset.saturating_sub(line_offset)),
                        line_offset,
                        self.radix,
                        self.group_size,
                        self.is_big_endian,
                    );
                    (editor.cursor_offset.saturating_sub(line_offset), source)
                })
            };
            if let Some((cursor_in_row, source)) = cursor_layout
                && let Some(group) = source
                    .groups
                    .iter()
                    .find(|group| group.chunk_start <= cursor_in_row && cursor_in_row < group.chunk_end)
            {
                let cursor_left = f32::from(hex_grid_x(group.text_start, hex_cell_width));
                let cursor_right = f32::from(hex_grid_x(group.text_end, hex_cell_width));
                if cursor_left < self.hex_scroll_x {
                    self.hex_scroll_x = cursor_left.clamp(0.0, max_hex_scroll);
                } else if cursor_right > self.hex_scroll_x + self.hex_col_width {
                    self.hex_scroll_x = (cursor_right - self.hex_col_width).clamp(0.0, max_hex_scroll);
                }

                let visual_left = layout.hex.start + cursor_left - self.hex_scroll_x;
                let visual_right = layout.hex.start + cursor_right - self.hex_scroll_x;
                if visual_left - self.outer_scroll_x < layout.fixed_width {
                    self.outer_scroll_x = (visual_left - layout.fixed_width).max(0.0);
                } else if visual_right - self.outer_scroll_x > layout.fixed_width + layout.viewport_width {
                    self.outer_scroll_x = (visual_right - layout.fixed_width - layout.viewport_width).max(0.0);
                }
                self.outer_scroll_x = self.outer_scroll_x.clamp(0.0, layout.outer_max);
                self.outer_scroll_handle.set_offset(point(-px(self.outer_scroll_x), px(0.0)));
            }
            self.last_cursor_offset = Some(cursor_offset);
            self.cursor_reveal_pending = false;
        }
        let is_hex_clipped_left = self.hex_scroll_x > 1.0;
        let is_hex_clipped_right = self.hex_scroll_x < max_hex_scroll - 1.0;
        let is_ascii_clipped_left = self.ascii_scroll_x > 1.0;
        let is_ascii_clipped_right = layout.ascii.map(|column| self.ascii_scroll_x < column.inner_max - 1.0).unwrap_or(false);
        let is_comment_clipped_left = self.comment_scroll_x > 1.0;
        let is_comment_clipped_right = self.comment_scroll_x < layout.comment.inner_max - 1.0;
        let is_desc_clipped_left = self.desc_scroll_x > 1.0;
        let is_desc_clipped_right = layout.description.map(|column| self.desc_scroll_x < column.inner_max - 1.0).unwrap_or(false);

        let header = if self.show_header {
            let mut hex_cols = Vec::with_capacity(items_in_row);
            for (i, group) in probe_source.groups.iter().enumerate() {
                let byte_offset = i * group_bytes;
                let label = SharedString::from(format!("+{:X}", byte_offset));
                let group_start = f32::from(hex_grid_x(group.text_start, hex_cell_width));
                let group_end = f32::from(hex_grid_x(group.text_end, hex_cell_width));
                hex_cols.push(
                    div()
                        .absolute()
                        .left(px(group_start - self.hex_scroll_x))
                        .top_0()
                        .h_full()
                        .w(px(group_end - group_start))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_center()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(label),
                );
            }

            let comment_header_el = |width: f32, can_scroll_left: bool, can_scroll_right: bool, theme: &gpui_component::Theme| {
                h_flex()
                    .w(px(width + SECTION_GAP))
                    .child(
                        div()
                            .w(px(width))
                            .overflow_hidden()
                            .relative()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Comment")
                            .when(can_scroll_left, |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .top_0()
                                        .bottom_0()
                                        .w(px(14.0))
                                        .bg(theme.sidebar.opacity(0.85))
                                        .text_color(theme.muted_foreground)
                                        .child("…"),
                                )
                            })
                            .when(can_scroll_right, |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .right_0()
                                        .top_0()
                                        .bottom_0()
                                        .w(px(14.0))
                                        .bg(theme.sidebar.opacity(0.85))
                                        .text_color(theme.muted_foreground)
                                        .child("…"),
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                    if event.click_count >= 2 {
                                        this.auto_fit_column(ResizingColumn::Comment, cx);
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
                .min_w_0()
                .overflow_hidden()
                .child(if is_struct_mode {
                    h_flex()
                        .flex_shrink_0()
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
                        .flex_shrink_0()
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
                    div().flex_shrink_0().w(px(SECTION_GAP)).into_any_element()
                })
                .child(
                    h_flex()
                        .relative()
                        .left(px(-self.outer_scroll_x))
                        .h(px(HEADER_HEIGHT))
                        .flex_shrink_0()
                        .w(px(self.hex_col_width + SECTION_GAP))
                        .child(
                            div()
                                .w(px(self.hex_col_width))
                                .h(px(HEADER_HEIGHT))
                                .overflow_hidden()
                                .relative()
                                .child(div().relative().w(px(self.hex_col_width)).h(px(HEADER_HEIGHT)).children(hex_cols))
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
                .child(
                    div()
                        .relative()
                        .left(px(-self.outer_scroll_x))
                        .flex_shrink_0()
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
                                                .overflow_hidden()
                                                .relative()
                                                .child("Description")
                                                .when(is_desc_clipped_left, |el| {
                                                    el.child(
                                                        div()
                                                            .absolute()
                                                            .left_0()
                                                            .top_0()
                                                            .bottom_0()
                                                            .w(px(14.0))
                                                            .bg(theme.sidebar.opacity(0.85))
                                                            .child("…"),
                                                    )
                                                })
                                                .when(is_desc_clipped_right, |el| {
                                                    el.child(
                                                        div()
                                                            .absolute()
                                                            .right_0()
                                                            .top_0()
                                                            .bottom_0()
                                                            .w(px(14.0))
                                                            .bg(theme.sidebar.opacity(0.85))
                                                            .child("…"),
                                                    )
                                                })
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
                                                            this.resizing_column =
                                                                Some((ResizingColumn::Description, event.position.x.into(), this.desc_col_width));
                                                            cx.notify();
                                                        }
                                                    }),
                                                ),
                                        ),
                                )
                                .child(comment_header_el(
                                    self.comment_col_width,
                                    is_comment_clipped_left,
                                    is_comment_clipped_right,
                                    theme,
                                ))
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
                                        .w(px(ascii_col_width + SECTION_GAP))
                                        .child(
                                            div()
                                                .w(px(ascii_col_width))
                                                .overflow_hidden()
                                                .relative()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(label)
                                                .when(is_ascii_clipped_left, |el| {
                                                    el.child(
                                                        div()
                                                            .absolute()
                                                            .left_0()
                                                            .top_0()
                                                            .bottom_0()
                                                            .w(px(18.0))
                                                            .bg(theme.sidebar.opacity(0.85))
                                                            .text_xs()
                                                            .font_semibold()
                                                            .text_color(theme.muted_foreground)
                                                            .child("…"),
                                                    )
                                                })
                                                .when(is_ascii_clipped_right, |el| {
                                                    el.child(
                                                        div()
                                                            .absolute()
                                                            .right_0()
                                                            .top_0()
                                                            .bottom_0()
                                                            .w(px(18.0))
                                                            .bg(theme.sidebar.opacity(0.85))
                                                            .text_xs()
                                                            .font_semibold()
                                                            .text_color(theme.muted_foreground)
                                                            .child("…"),
                                                    )
                                                })
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                                        if event.click_count >= 2 {
                                                            this.auto_fit_column(ResizingColumn::Ascii, cx);
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
                                                    cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                                                        if event.click_count >= 2 {
                                                            this.resizing_column = None;
                                                            this.auto_fit_column(ResizingColumn::Ascii, cx);
                                                        } else {
                                                            this.resizing_column = Some((ResizingColumn::Ascii, event.position.x.into(), ascii_col_width));
                                                            cx.notify();
                                                        }
                                                    }),
                                                ),
                                        ),
                                )
                                .child(comment_header_el(
                                    self.comment_col_width,
                                    is_comment_clipped_left,
                                    is_comment_clipped_right,
                                    theme,
                                ))
                                .into_any_element()
                        } else {
                            comment_header_el(self.comment_col_width, is_comment_clipped_left, is_comment_clipped_right, theme).into_any_element()
                        })
                        .into_any_element(),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        };

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

                    // スクロールバー領域（右端12px）でのクリック判定
                    if let Some(list_b) = this.list_bounds.get() {
                        if event.position.y >= list_b.bottom() {
                            return;
                        }
                        let click_x = f32::from(event.position.x);
                        let bar_x = f32::from(list_b.right()) - 12.0;
                        if click_x >= bar_x {
                            let click_y = f32::from(event.position.y);
                            let list_top = f32::from(list_b.top());
                            let rel_y = click_y - list_top;
                            let list_h = f32::from(list_b.size.height);

                            let total_rows = this.editor.read(cx).line_starts().len().max(1);
                            let visible_rows = (list_h / ROW_HEIGHT).floor() as usize;
                            let max_top_row = total_rows.saturating_sub(visible_rows.max(1));
                            let ratio = (visible_rows as f64 / total_rows as f64).clamp(0.0, 1.0);
                            let thumb_h = (list_h as f64 * ratio).clamp(24.0, list_h as f64) as f32;
                            let max_thumb_top = (list_h - thumb_h).max(0.0);

                            let cur_thumb_top = if max_top_row > 0 {
                                ((this.scroll_offset as f64 / max_top_row as f64) * max_thumb_top as f64) as f32
                            } else {
                                0.0
                            };

                            if rel_y >= cur_thumb_top && rel_y <= cur_thumb_top + thumb_h {
                                this.is_dragging_scrollbar = true;
                                this.scrollbar_drag_start_y = click_y;
                                this.scrollbar_drag_start_row = this.scroll_offset;
                            } else {
                                let target_thumb_top = (rel_y - thumb_h / 2.0).clamp(0.0, max_thumb_top);
                                let new_ratio = if max_thumb_top > 0.0 {
                                    target_thumb_top as f64 / max_thumb_top as f64
                                } else {
                                    0.0
                                };
                                let new_row = (new_ratio * max_top_row as f64).round() as usize;
                                this.scroll_to_row(new_row, cx);
                                this.is_dragging_scrollbar = true;
                                this.scrollbar_drag_start_y = click_y;
                                this.scrollbar_drag_start_row = new_row;
                            }
                            cx.notify();
                            return;
                        }
                    }

                    if let Some(target_pos) = this.offset_from_point(event.position, window, cx) {
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
                    if this.list_bounds.get().map(|bounds| event.position.y >= bounds.bottom()).unwrap_or(false) {
                        return;
                    }
                    if let Some(target_pos) = this.offset_from_point(event.position, window, cx) {
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
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                let layout = this.current_layout(cx);
                this.sync_outer_scroll_from_handle(layout, cx);

                if let Some(list_b) = this.list_bounds.get() {
                    let pos = event.position;
                    let is_in_bar = pos.x >= list_b.right() - px(12.0) && pos.x <= list_b.right() && pos.y >= list_b.top() && pos.y <= list_b.bottom();
                    if this.scrollbar_hovered != is_in_bar {
                        this.scrollbar_hovered = is_in_bar;
                        cx.notify();
                    }
                }

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
                        ResizingColumn::Ascii => {
                            this.ascii_col_width = (start_w + delta).max(MIN_ASCII_COLUMN_WIDTH);
                        }
                        ResizingColumn::Description => {
                            this.desc_col_width = (start_w + delta).max(80.0);
                        }
                        ResizingColumn::Comment => {
                            this.comment_col_width = (start_w + delta).max(80.0);
                        }
                    }
                    this.cursor_reveal_pending = true;
                    this.clamp_scroll_offsets(cx);
                    cx.notify();
                    return;
                }
                if this.is_selecting {
                    if let Some(list_b) = this.list_bounds.get() {
                        let y = f32::from(event.position.y);
                        let list_top = f32::from(list_b.top());
                        let list_bottom = f32::from(list_b.bottom());
                        if y < list_top {
                            let rows_up = ((list_top - y) / ROW_HEIGHT).ceil() as usize;
                            let new_row = this.scroll_offset.saturating_sub(rows_up.min(5));
                            this.scroll_to_row(new_row, cx);
                        } else if y > list_bottom {
                            let rows_down = ((y - list_bottom) / ROW_HEIGHT).ceil() as usize;
                            let total_rows = this.editor.read(cx).line_starts().len().max(1);
                            let max_top_row = total_rows.saturating_sub(1);
                            let new_row = (this.scroll_offset + rows_down.min(5)).min(max_top_row);
                            this.scroll_to_row(new_row, cx);
                        }
                    }

                    if let Some(target_pos) = this.offset_from_point(event.position, window, cx) {
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
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
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
            .child(div().flex_1().w_full().min_w_0().min_h_0().relative().overflow_hidden().child({
                let editor_entity = self.editor.clone();
                let scroll_offset = self.scroll_offset;
                let outer_scroll_x = self.outer_scroll_x;
                let hex_scroll_x = self.hex_scroll_x;
                let ascii_scroll_x = self.ascii_scroll_x;
                let desc_scroll_x = self.desc_scroll_x;
                let comment_scroll_x = self.comment_scroll_x;
                let address_col_width = self.address_col_width;
                let hex_col_width = self.hex_col_width;
                let desc_col_width = self.desc_col_width;
                let comment_col_width = self.comment_col_width;
                let show_offset = self.show_offset;
                let show_ascii = self.show_ascii;
                let encoding = self.encoding;
                let radix = self.radix;
                let group_size = self.group_size;
                let is_big_endian = self.is_big_endian;
                let _max_highlight_len = self.max_highlight_len;
                let highlights = self.highlights.clone();
                let is_dragging_scrollbar = self.is_dragging_scrollbar;
                let scrollbar_hovered = self.scrollbar_hovered;
                let scrollbar_view = view.clone();

                canvas(
                    move |bounds, _window, cx| {
                        list_bounds_view.update(cx, |this, _cx| {
                            let changed = this.list_bounds.get() != Some(bounds);
                            this.list_bounds.set(Some(bounds));
                            if changed {
                                _cx.notify();
                            }
                        });
                    },
                    move |bounds, _prepaint, window, cx| {
                        window.on_mouse_event({
                            let scrollbar_view = scrollbar_view.clone();
                            move |event: &MouseMoveEvent, phase, _window, cx| {
                                if !phase.bubble() || !event.dragging() {
                                    return;
                                }

                                scrollbar_view.update(cx, |this, cx| {
                                    this.update_scrollbar_drag(f32::from(event.position.y), cx);
                                });
                            }
                        });
                        window.on_mouse_event({
                            let scrollbar_view = scrollbar_view.clone();
                            move |event: &MouseUpEvent, phase, _window, cx| {
                                if !phase.bubble() || event.button != MouseButton::Left {
                                    return;
                                }

                                scrollbar_view.update(cx, |this, cx| {
                                    if this.is_dragging_scrollbar {
                                        this.is_dragging_scrollbar = false;
                                        cx.notify();
                                    }
                                });
                            }
                        });

                        let (parse_result, collapsed_structs, highlight_items, doc_arc, line_starts, cursor_offset, min_sel, max_sel) = {
                            let editor = editor_entity.read(cx);
                            let (min_sel, max_sel) = if let (Some(s), Some(e)) = (editor.selection_start, editor.selection_end) {
                                if s <= e { (s, e) } else { (e, s) }
                            } else {
                                (usize::MAX, usize::MIN)
                            };
                            (
                                if editor.show_inline_structure_view { editor.parse_result() } else { None },
                                Arc::new(editor.collapsed_struct_ids.clone()),
                                Arc::new(editor.highlights_snapshot()),
                                editor.document.clone(),
                                editor.line_starts(),
                                editor.cursor_offset,
                                min_sel,
                                max_sel,
                            )
                        };
                        let doc = doc_arc.read().expect("document read lock");

                        // Construct combined highlights from the shared snapshot and search results
                        let mut effective_highlights: Vec<(Range<usize>, Hsla)> = highlight_items.iter().map(|h| (h.range(), h.hsla_color())).collect();
                        for (search_range, search_color) in highlights.iter() {
                            if !effective_highlights.iter().any(|(r, _)| r == search_range) {
                                effective_highlights.push((search_range.clone(), *search_color));
                            }
                        }
                        effective_highlights.sort_by_key(|(range, _)| range.start);
                        let effective_max_hl_len = effective_highlights.iter().map(|(r, _)| r.end.saturating_sub(r.start)).max().unwrap_or(0);

                        let list_h = f32::from(bounds.size.height);
                        let visible_rows = (list_h / ROW_HEIGHT).ceil() as usize + 1;
                        let top_row = scroll_offset.min(total_rows.saturating_sub(1));
                        let end_row = (top_row + visible_rows).min(total_rows);

                        for (k, row_idx) in (top_row..end_row).enumerate() {
                            let row_y = bounds.top() + px(k as f32 * ROW_HEIGHT);
                            let row_bounds = Bounds::new(point(bounds.left(), row_y), size(bounds.size.width, px(ROW_HEIGHT)));
                            Self::paint_hex_row(
                                row_idx,
                                row_bounds,
                                top_row,
                                &doc,
                                &line_starts,
                                parse_result.as_deref(),
                                &collapsed_structs,
                                encoding,
                                radix,
                                group_size,
                                is_big_endian,
                                cursor_offset,
                                min_sel,
                                max_sel,
                                effective_highlights.as_slice(),
                                highlight_items.as_slice(),
                                effective_max_hl_len,
                                show_offset,
                                show_ascii,
                                ascii_col_width,
                                ascii_scroll_x,
                                is_focused,
                                outer_scroll_x,
                                hex_scroll_x,
                                desc_scroll_x,
                                comment_scroll_x,
                                address_col_width,
                                hex_col_width,
                                hex_cell_width,
                                desc_col_width,
                                comment_col_width,
                                font_family.clone(),
                                font_size,
                                window,
                                cx,
                            );
                        }

                        let theme = cx.theme();
                        Self::paint_scrollbar(bounds, top_row, total_rows, is_dragging_scrollbar, scrollbar_hovered, theme, window);
                    },
                )
                .size_full()
            }))
            .child(if layout.outer_max > 0.01 {
                div()
                    .h(px(HORIZONTAL_SCROLLBAR_HEIGHT))
                    .flex_shrink_0()
                    .flex()
                    .child(div().w(px(8.0 + layout.fixed_width)))
                    .child(
                        div()
                            .flex_1()
                            .mr(px(VERTICAL_SCROLLBAR_WIDTH))
                            .relative()
                            .child(Scrollbar::horizontal(&self.outer_scroll_handle).scroll_size(size(px(layout.content_width), px(0.0)))),
                    )
                    .into_any_element()
            } else {
                div().h(px(0.0)).into_any_element()
            })
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
