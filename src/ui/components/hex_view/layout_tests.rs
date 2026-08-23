use super::layout::{build_ascii_char_map, centered_glyph_offset};
use super::paint::highlight_color_for_range;
use super::types::{AUTO_FIT_SCAN_BYTES, HexViewLayout, HexViewLayoutState, HorizontalScrollTarget, LayoutInput, ScrollColumn};
use super::{
    HexView, ascii_byte_index_from_world_x, bounded_auto_fit_range, build_hex_text_source, can_chain_to_outer, hex_grid_width, hex_grid_x,
    make_hex_view_layout, weighted_text_width,
};
use crate::core::buffer::Buffer;
use crate::core::document::Document;
use crate::core::editor::Editor;
use crate::core::encoding::Encoding;
use crate::core::radix::{ByteGroupSize, DisplayRadix};
use crate::core::structure::types::{FieldValue, ParseResult, ParsedField};
use gpui::{Hsla, hsla, px};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

fn layout(width: f32, is_struct_mode: bool, show_offset: bool, show_ascii: bool) -> HexViewLayout {
    make_hex_view_layout(LayoutInput {
        bounds_width: width,
        is_struct_mode,
        show_ascii,
        ascii_col_width: 160.0,
        ascii_inner_max: 0.0,
        fixed_column_width: if is_struct_mode || show_offset { 80.0 } else { 0.0 },
        hex_col_width: 200.0,
        desc_col_width: 240.0,
        comment_col_width: 300.0,
        hex_inner_max: 120.0,
        desc_inner_max: 80.0,
        comment_inner_max: 60.0,
        section_gap: 16.0,
        content_padding: 8.0,
        scrollbar_width: 12.0,
    })
}

fn check_layout_and_scrolling() {
    let l = layout(600.0, false, true, true);
    assert_eq!(l.fixed_width, 96.0);
    assert_eq!(l.hex.start, l.fixed_width);
    assert_eq!(l.hex.inner_max, 120.0);
    assert!(l.outer_max > 0.0);
    assert!(l.outer_max < l.content_width);

    let l_no_offset = layout(800.0, false, false, true);
    assert_eq!(l_no_offset.fixed_width, 16.0);
    assert_eq!(l_no_offset.hex.start, 16.0);

    let l_hit = layout(600.0, false, true, true);
    let outer_scroll_x = 40.0;
    let hex_x = l_hit.fixed_width + 10.0;
    assert_eq!(l_hit.column_at(hex_x, outer_scroll_x), Some(ScrollColumn::Hex));
    let ascii_x = l_hit.ascii.expect("ASCII column").start - outer_scroll_x + 10.0;
    assert_eq!(l_hit.column_at(ascii_x, outer_scroll_x), Some(ScrollColumn::Ascii));
    let fixed_x = l_hit.fixed_width - 2.0;
    assert_eq!(l_hit.column_at(fixed_x, outer_scroll_x), None);

    let l_struct = layout(600.0, true, true, false);
    let description = l_struct.description.expect("description column");
    assert!(l_struct.ascii.is_none());
    assert_eq!(l_struct.max_offset(HorizontalScrollTarget::Column(ScrollColumn::Description)), 80.0);
    assert_eq!(l_struct.progress(HorizontalScrollTarget::Column(ScrollColumn::Description), 40.0), 0.5);
    assert_eq!(l_struct.column_at(description.start + 10.0, 0.0), Some(ScrollColumn::Description));

    let l_zero = make_hex_view_layout(LayoutInput {
        bounds_width: 1200.0,
        is_struct_mode: false,
        show_ascii: false,
        ascii_col_width: 160.0,
        ascii_inner_max: 0.0,
        fixed_column_width: 80.0,
        hex_col_width: 200.0,
        desc_col_width: 240.0,
        comment_col_width: 300.0,
        hex_inner_max: 0.0,
        desc_inner_max: 0.0,
        comment_inner_max: 0.0,
        section_gap: 16.0,
        content_padding: 8.0,
        scrollbar_width: 12.0,
    });
    assert_eq!(l_zero.progress(HorizontalScrollTarget::Column(ScrollColumn::Hex), 10.0), 0.0);

    let l_ascii = make_hex_view_layout(LayoutInput {
        bounds_width: 600.0,
        is_struct_mode: false,
        show_ascii: true,
        ascii_col_width: 320.0,
        ascii_inner_max: 0.0,
        fixed_column_width: 80.0,
        hex_col_width: 200.0,
        desc_col_width: 240.0,
        comment_col_width: 300.0,
        hex_inner_max: 0.0,
        desc_inner_max: 0.0,
        comment_inner_max: 0.0,
        section_gap: 16.0,
        content_padding: 8.0,
        scrollbar_width: 12.0,
    });
    assert_eq!(l_ascii.ascii.expect("ASCII column").width, 320.0);

    let l_ascii_scroll = make_hex_view_layout(LayoutInput {
        bounds_width: 600.0,
        is_struct_mode: false,
        show_ascii: true,
        ascii_col_width: 80.0,
        ascii_inner_max: 80.0,
        fixed_column_width: 80.0,
        hex_col_width: 200.0,
        desc_col_width: 240.0,
        comment_col_width: 300.0,
        hex_inner_max: 0.0,
        desc_inner_max: 0.0,
        comment_inner_max: 0.0,
        section_gap: 16.0,
        content_padding: 8.0,
        scrollbar_width: 12.0,
    });
    assert_eq!(l_ascii_scroll.max_offset(HorizontalScrollTarget::Column(ScrollColumn::Ascii)), 80.0);
    assert_eq!(l_ascii_scroll.progress(HorizontalScrollTarget::Column(ScrollColumn::Ascii), 40.0), 0.5);
    let ascii_col = l_ascii_scroll.ascii.expect("ASCII column");
    assert_eq!(ascii_byte_index_from_world_x(ascii_col.start + 5.0, ascii_col, 0.0), 0);
    assert_eq!(ascii_byte_index_from_world_x(ascii_col.start + 5.0, ascii_col, 20.0), 2);

    for column in [ScrollColumn::Hex, ScrollColumn::Ascii, ScrollColumn::Description, ScrollColumn::Comment] {
        assert!(can_chain_to_outer(HorizontalScrollTarget::Column(column), 1.0));
    }
    assert!(!can_chain_to_outer(HorizontalScrollTarget::View, 1.0));
    assert!(!can_chain_to_outer(HorizontalScrollTarget::Column(ScrollColumn::Hex), 0.0));
}

fn check_character_mapping_and_auto_fit() {
    let buffer = "€".as_bytes();
    let map = build_ascii_char_map(Encoding::Utf8, buffer, 0, 2);
    assert_eq!(map, vec![None, Some(('€', 3))]);

    let map2 = build_ascii_char_map(Encoding::Utf8, buffer, 1, 2);
    assert_eq!(map2, vec![None, None]);

    let range = bounded_auto_fit_range(1024 * 1024, 500_000, 500_512);
    assert_eq!(range.len(), AUTO_FIT_SCAN_BYTES);
    assert!(range.start <= 500_000);
    assert!(range.end >= 500_512);

    assert_eq!(bounded_auto_fit_range(1024, 100, 200), 0..1024);
    let range_eof = bounded_auto_fit_range(1024 * 1024, 1_020_000, 1_020_512);
    assert_eq!(range_eof.end, 1024 * 1024);
    assert!(range_eof.start <= 1_020_000);
    assert_eq!(range_eof.len(), AUTO_FIT_SCAN_BYTES);
}

fn check_structure_and_highlights() {
    let doc = Arc::new(RwLock::new(Document::new(PathBuf::from("test.bin"), Buffer::new(vec![0; 32]))));
    let editor = Editor::new(doc);

    let field1 = ParsedField {
        id: "magic".into(),
        field_type: "u4".into(),
        offset: 0,
        size: 4,
        value: FieldValue::U32(0x12345678),
        color: Hsla::default(),
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
        color: Hsla::default(),
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
        color: Hsla::default(),
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
        color: Hsla::default(),
        description: None,
        children: vec![field1.clone(), field2.clone(), field3.clone()],
        enum_label: None,
        is_instance: false,
    };

    let parse_result = ParseResult::new("test_struct".into(), vec![container], 12, vec![]);
    let char_w = 8.0;

    let width_expanded = HexView::description_content_width_in_range(&editor, &parse_result, &(0..16), char_w);
    let single_field_width = weighted_text_width(&field1.format_expression(), char_w);
    assert!(
        width_expanded > single_field_width * 2.5,
        "Expanded row width ({width_expanded}) must aggregate all fields, strictly greater than a single field ({single_field_width})"
    );

    let mut editor_collapsed = Editor::new(Arc::new(RwLock::new(Document::new(PathBuf::from("test.bin"), Buffer::new(vec![0; 32])))));
    editor_collapsed.collapsed_struct_ids.insert("header".into());
    let width_collapsed = HexView::description_content_width_in_range(&editor_collapsed, &parse_result, &(0..16), char_w);
    assert!(
        width_collapsed < width_expanded,
        "Collapsed width ({width_collapsed}) should be smaller than expanded width ({width_expanded})"
    );

    let source = build_hex_text_source(&[0x12, 0x34, 0x56], 0, DisplayRadix::Hexadecimal, ByteGroupSize::One, false);
    let cell_width = px(8.0);
    assert_eq!(source.text.as_ref(), "12 34 56");
    assert_eq!(f32::from(hex_grid_x(source.groups[0].text_start, cell_width)), 0.0);
    assert_eq!(f32::from(hex_grid_x(source.groups[1].text_start, cell_width)), 24.0);
    assert_eq!(f32::from(hex_grid_x(source.groups[2].text_start, cell_width)), 48.0);
    assert_eq!(f32::from(hex_grid_width(source.text.len(), cell_width)), 64.0);

    let source2 = build_hex_text_source(&[0x12], 1, DisplayRadix::Hexadecimal, ByteGroupSize::Four, false);
    assert_eq!(source2.text.as_ref(), "..12....");
    assert_eq!(f32::from(hex_grid_x(source2.groups[0].text_start, cell_width)), 0.0);
    assert_eq!(f32::from(hex_grid_x(source2.groups[0].text_end, cell_width)), 64.0);
    assert_eq!(f32::from(hex_grid_width(source2.text.len(), cell_width)), 64.0);

    assert_eq!(centered_glyph_offset(10.0, 6.0), 2.0);
    assert_eq!(centered_glyph_offset(10.0, 12.0), 0.0);

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
    assert_eq!(cloned.scroll_offset, 42);

    let highlight_color = hsla(0.1, 0.8, 0.5, 0.35);
    let highlights = [(0..16, highlight_color)];
    assert_eq!(highlight_color_for_range(4, 8, &highlights), Some(highlight_color));
    assert_eq!(highlight_color_for_range(20, 24, &highlights), None);
}

#[test]
fn test_hex_view_layout_suite() {
    check_layout_and_scrolling();
    check_character_mapping_and_auto_fit();
    check_structure_and_highlights();
}
