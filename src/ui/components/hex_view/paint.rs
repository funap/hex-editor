use super::layout::*;
use super::types::*;
use crate::core::document::Document;
use crate::core::editor::LineMap;
use crate::core::encoding::Encoding;
use crate::core::highlight::HighlightItem;
use crate::core::radix::{ByteGroupSize, DisplayRadix, is_group_zero};
use crate::core::structure::ParseResult;
use gpui::*;
use gpui_component::ActiveTheme;
use std::collections::HashSet;
use std::ops::Range;

#[inline]
pub fn paint_border_box(window: &mut Window, bounds: Bounds<Pixels>, border_width: Pixels, color: Hsla) {
    let outline = gpui::outline(bounds, color, gpui::BorderStyle::Solid).border_widths(border_width);
    window.paint_quad(outline);
}

#[inline]
pub fn paint_cursor_border(window: &mut Window, bounds: Bounds<Pixels>, color: Hsla) {
    let padding_x = px(CURSOR_PADDING_X);
    let padding_y = px(CURSOR_PADDING_Y);
    let padded_bounds = Bounds::new(
        point(bounds.origin.x - padding_x, bounds.origin.y - padding_y),
        size(bounds.size.width + padding_x + padding_x, bounds.size.height + padding_y + padding_y),
    );
    paint_border_box(window, padded_bounds, px(CURSOR_BORDER_WIDTH), color);
}

pub fn row_highlights(highlights: &[(Range<usize>, Hsla)], max_len: usize, offset: usize, next_offset: usize) -> &[(Range<usize>, Hsla)] {
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
pub fn highlight_color_for_range(item_start: usize, item_end: usize, is_selected: bool, active_highlights: &[(Range<usize>, Hsla)]) -> Option<Hsla> {
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

#[inline]
pub fn pixel_midpoint(left: Pixels, right: Pixels) -> Pixels {
    px((f32::from(left) + f32::from(right)) * 0.5)
}

#[inline]
pub fn darken_cursor_color(color: Hsla) -> Hsla {
    Hsla {
        l: (color.l * 0.72).clamp(0.0, 1.0),
        a: color.a.max(0.9),
        ..color
    }
}

/// Paint every glyph centered in its fixed Hex cell while reusing the line
/// shaped for the row. This keeps shaping batched and avoids a per-row glyph
/// position allocation.
pub fn paint_centered_hex_glyphs(
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

/// Paint every glyph centered in its fixed ASCII cell while reusing the line
/// shaped for the row. This keeps shaping batched and avoids per-character
/// string allocations and shaping passes.
pub fn paint_centered_ascii_glyphs(shaped: &gpui::ShapedLine, entries: &[AsciiCellEntry], origin: Point<Pixels>, line_height: Pixels, window: &mut Window) {
    let natural_width = shaped.width;
    let baseline_offset = point(px(0.0), (line_height - shaped.ascent - shaped.descent) / 2.0 + shaped.ascent);
    let mut entry_idx = 0;
    let mut glyphs = shaped
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter().map(move |glyph| (run.font_id, glyph)))
        .peekable();

    while let Some((font_id, glyph)) = glyphs.next() {
        let natural_end = glyphs.peek().map(|(_, next)| next.position.x).unwrap_or(natural_width);
        let glyph_width = f32::from(natural_end - glyph.position.x).max(0.0);
        let centered_offset = centered_glyph_offset(ASCII_CELL_WIDTH, glyph_width);

        while entry_idx < entries.len() && glyph.index >= entries[entry_idx].text_byte_end {
            entry_idx += 1;
        }

        if entry_idx < entries.len() && entries[entry_idx].text_byte_start <= glyph.index && glyph.index < entries[entry_idx].text_byte_end {
            let cell_idx = entries[entry_idx].cell_idx;
            let glyph_origin = point(origin.x + px(cell_idx as f32 * ASCII_CELL_WIDTH + centered_offset), origin.y) + baseline_offset;
            let color = entries[entry_idx].color;
            if glyph.is_emoji {
                let _ = window.paint_emoji(glyph_origin, font_id, glyph.id, shaped.font_size);
            } else {
                let _ = window.paint_glyph(glyph_origin, font_id, glyph.id, shaped.font_size, color);
            }
        }
    }
}

#[inline]
pub fn format_offset_08(offset: usize) -> SharedString {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut buf = [b'0'; 8];
    let mut val = offset;
    for i in (0..8).rev() {
        buf[i] = DIGITS[val & 0xf];
        val >>= 4;
    }
    SharedString::from(std::str::from_utf8(&buf).expect("valid ascii utf8").to_string())
}

pub struct RowPaintParams<'a> {
    pub row_idx: usize,
    pub bounds: Bounds<Pixels>,
    pub top_visible_row: usize,
    pub doc: &'a Document,
    pub line_starts: &'a LineMap,
    pub parse_result: Option<&'a ParseResult>,
    pub collapsed_structs: &'a HashSet<String>,
    pub encoding: Encoding,
    pub radix: DisplayRadix,
    pub group_size: ByteGroupSize,
    pub is_big_endian: bool,
    pub cursor_offset: usize,
    pub min_sel: usize,
    pub max_sel: usize,
    pub highlights: &'a [(Range<usize>, Hsla)],
    pub highlight_items: &'a [HighlightItem],
    pub max_highlight_len: usize,
    pub show_offset: bool,
    pub show_ascii: bool,
    pub ascii_col_width: f32,
    pub ascii_scroll_x: f32,
    pub is_focused: bool,
    pub outer_scroll_x: f32,
    pub hex_scroll_x: f32,
    pub desc_scroll_x: f32,
    pub comment_scroll_x: f32,
    pub address_col_width: f32,
    pub hex_col_width: f32,
    pub hex_cell_width: Pixels,
    pub desc_col_width: f32,
    pub comment_col_width: f32,
    pub font_family: SharedString,
    pub font_size: Pixels,
}

pub fn paint_hex_row(params: RowPaintParams, window: &mut Window, cx: &mut App) {
    let offset = match params.line_starts.get(params.row_idx) {
        Some(o) => o,
        None => return,
    };
    let next_offset = if params.row_idx + 1 < params.line_starts.len() {
        params.line_starts.get(params.row_idx + 1).unwrap_or(params.doc.buffer.len())
    } else {
        params.doc.buffer.len()
    };

    let chunk_len = next_offset - offset;
    let chunk = params.doc.buffer.get_range(offset, chunk_len);

    let is_struct_mode = params.parse_result.is_some();

    let active_row_highlights = row_highlights(params.highlights, params.max_highlight_len, offset, next_offset);
    let (selection_bg, cursor_bg, muted_color, fg_color, accent_fg_color, border_color, _sidebar_bg, bg_color_theme) = {
        let theme = cx.theme();
        (
            if params.is_focused {
                theme.selection
            } else {
                theme.muted_foreground.opacity(0.3)
            },
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
    let font = gpui::font(params.font_family);

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
        let shaped = window.text_system().shape_line(addr_str, params.font_size, &[run], None);
        let addr_pos = point(params.bounds.left() + px(8.0), params.bounds.top() + px(2.0));
        let _ = shaped.paint(addr_pos, line_height, window, cx);
        (params.address_col_width, SECTION_GAP)
    } else {
        if params.show_offset {
            let offset_str = format_offset_08(offset);
            let run = gpui::TextRun {
                len: offset_str.len(),
                font: font.clone(),
                color: muted_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = window.text_system().shape_line(offset_str, params.font_size, &[run], None);
            let offset_pos = point(params.bounds.left() + px(8.0), params.bounds.top() + px(2.0));
            let _ = shaped.paint(offset_pos, line_height, window, cx);
        }
        (if params.show_offset { OFFSET_WIDTH } else { 0.0 }, SECTION_GAP)
    };

    let base_x = params.bounds.left() + px(8.0);
    let hex_start_x = base_x + px(offset_w + gap) - px(params.outer_scroll_x);
    let hex_end_x = hex_start_x + px(params.hex_col_width);
    let (comment_start_x, ascii_width) = if is_struct_mode {
        let desc_start_x = hex_end_x + px(gap);
        let comment_start_x = desc_start_x + px(params.desc_col_width + gap);
        (comment_start_x, 0.0)
    } else {
        let ascii_w = if params.show_ascii { params.ascii_col_width } else { 0.0 };
        let comment_start_x = if params.show_ascii {
            hex_end_x + px(gap + ascii_w + gap)
        } else {
            hex_end_x + px(gap)
        };
        (comment_start_x, ascii_w)
    };

    // Vertical Column Divider Borders (matching header splitters exactly)
    let border_line_color = border_color.opacity(0.4);
    if is_struct_mode || params.show_offset {
        let div1_x = base_x + px(offset_w + (gap / 2.0));
        window.paint_quad(gpui::fill(
            Bounds::new(point(div1_x, params.bounds.top()), size(px(1.0), px(ROW_HEIGHT))),
            border_line_color,
        ));
    }
    let scrollable_mask_bounds = {
        let left = base_x + px(offset_w + gap);
        let right = params.bounds.right() - px(VERTICAL_SCROLLBAR_WIDTH);
        Bounds::new(point(left, params.bounds.top()), size((right - left).max(px(0.0)), px(ROW_HEIGHT)))
    };
    window.with_content_mask(
        Some(gpui::ContentMask {
            bounds: scrollable_mask_bounds,
        }),
        |window| {
            let div2_x = hex_start_x + px(params.hex_col_width + (gap / 2.0));
            window.paint_quad(gpui::fill(
                Bounds::new(point(div2_x, params.bounds.top()), size(px(1.0), px(ROW_HEIGHT))),
                border_line_color,
            ));
            if is_struct_mode {
                let desc_start_x = hex_end_x + px(gap);
                let div3_x = desc_start_x + px(params.desc_col_width + (gap / 2.0));
                let div4_x = comment_start_x + px(params.comment_col_width + (gap / 2.0));
                window.paint_quad(gpui::fill(
                    Bounds::new(point(div3_x, params.bounds.top()), size(px(1.0), px(ROW_HEIGHT))),
                    border_line_color,
                ));
                window.paint_quad(gpui::fill(
                    Bounds::new(point(div4_x, params.bounds.top()), size(px(1.0), px(ROW_HEIGHT))),
                    border_line_color,
                ));
            } else {
                if params.show_ascii {
                    let div3_x = hex_end_x + px(gap + ascii_width + (gap / 2.0));
                    window.paint_quad(gpui::fill(
                        Bounds::new(point(div3_x, params.bounds.top()), size(px(1.0), px(ROW_HEIGHT))),
                        border_line_color,
                    ));
                }
                let div4_x = comment_start_x + px(params.comment_col_width + (gap / 2.0));
                window.paint_quad(gpui::fill(
                    Bounds::new(point(div4_x, params.bounds.top()), size(px(1.0), px(ROW_HEIGHT))),
                    border_line_color,
                ));
            }
        },
    );

    // Build and shape the exact text stream before painting any geometry.
    // Group geometry uses the fixed cell grid; glyphs are centered in that
    // grid during the text pass.
    let hex_source = build_hex_text_source(chunk, offset, params.radix, params.group_size, params.is_big_endian);
    let mut hex_runs: Vec<gpui::TextRun> = Vec::with_capacity(hex_source.groups.len() * 2);
    let mut group_visuals: Vec<(Hsla, bool)> = Vec::with_capacity(hex_source.groups.len());
    let mut group_text_colors: Vec<Hsla> = Vec::with_capacity(hex_source.groups.len());

    for (group_idx, group) in hex_source.groups.iter().enumerate() {
        let item_start_offset = offset + group.chunk_start;
        let item_end_offset = offset + group.chunk_end;
        let item_slice = &chunk[group.chunk_start..group.chunk_end];
        let is_cursor = params.cursor_offset >= item_start_offset && params.cursor_offset < item_end_offset;
        let is_zero = is_group_zero(item_slice);
        let text_color = if is_cursor && params.is_focused {
            fg_color
        } else if is_zero {
            muted_color.opacity(0.5)
        } else {
            fg_color
        };

        let is_selected = if params.min_sel <= params.max_sel {
            item_start_offset <= params.max_sel && item_end_offset > params.min_sel
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

    let shaped_hex = window.text_system().shape_line(hex_source.text.clone(), params.font_size, &hex_runs, None);
    let text_origin_x = hex_start_x - px(params.hex_scroll_x);
    let total_data_width = f32::from(hex_grid_width(hex_source.text.len(), params.hex_cell_width));

    // 2. Background Quads Pass for Data Items (with clipping mask)
    let hex_mask_bounds = Bounds::new(point(hex_start_x, params.bounds.top()), size(px(params.hex_col_width), px(ROW_HEIGHT)));

    window.with_content_mask(
        Some(gpui::ContentMask {
            bounds: scrollable_mask_bounds,
        }),
        |window| {
            window.with_content_mask(Some(gpui::ContentMask { bounds: hex_mask_bounds }), |window| {
                for (item_idx, group) in hex_source.groups.iter().enumerate() {
                    let (bg_color, is_cursor) = group_visuals[item_idx];
                    let (group_start_x, group_end_x) = hex_group_x(*group, text_origin_x, params.hex_cell_width);
                    let previous_group_end_x = if item_idx > 0 {
                        hex_group_x(hex_source.groups[item_idx - 1], text_origin_x, params.hex_cell_width).1
                    } else {
                        group_start_x
                    };
                    let previous_has_background = item_idx > 0 && group_visuals[item_idx - 1].0.a > 0.0;
                    let has_next = item_idx + 1 < hex_source.groups.len();
                    let next_group_start_x = if has_next {
                        hex_group_x(hex_source.groups[item_idx + 1], text_origin_x, params.hex_cell_width).0
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
                            point(fill_start_x, params.bounds.top() + px(1.0)),
                            size(fill_end_x - fill_start_x, px(ROW_HEIGHT - 2.0)),
                        );
                        window.paint_quad(gpui::fill(item_fill_bounds, bg_color));
                    }

                    if is_cursor {
                        let cursor_border_color = if params.is_focused {
                            cursor_bg
                        } else {
                            darken_cursor_color(muted_color).opacity(0.8)
                        };
                        let cursor_start_x = if bg_color.a > 0.0 { fill_start_x } else { group_start_x };
                        let cursor_end_x = if bg_color.a > 0.0 { fill_end_x } else { group_end_x };
                        let item_box_bounds = Bounds::new(
                            point(cursor_start_x, params.bounds.top() + px(1.0)),
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
                        params.hex_cell_width,
                        point(text_origin_x, params.bounds.top() + px(2.0)),
                        line_height,
                        window,
                    );
                }

                // Left-edge subtle gradient fade when hex_scroll_x > 1.0
                if params.hex_scroll_x > 1.0 {
                    let bg = bg_color_theme;
                    for step in 0..5 {
                        let x = hex_start_x + px(step as f32 * 3.2);
                        let alpha = 1.0 - (step as f32 / 5.0);
                        window.paint_quad(gpui::fill(
                            Bounds::new(point(x, params.bounds.top()), size(px(3.4), px(ROW_HEIGHT))),
                            bg.opacity(alpha * 0.95),
                        ));
                    }
                }

                // Right-edge subtle gradient fade when row data overflows hex_col_width
                if params.hex_scroll_x + params.hex_col_width < total_data_width - 1.0 {
                    let fade_w = 22.0;
                    let fade_start = hex_end_x - px(fade_w);
                    let bg = bg_color_theme;
                    for step in 0..6 {
                        let x = fade_start + px(step as f32 * 3.6);
                        let alpha = (step + 1) as f32 / 7.0;
                        window.paint_quad(gpui::fill(
                            Bounds::new(point(x, params.bounds.top()), size(px(3.8), px(ROW_HEIGHT))),
                            bg.opacity(alpha * 0.95),
                        ));
                    }
                }
            });
        },
    );

    // 3. ASCII Column (when not in structure definition mode and ASCII view is enabled)
    if !is_struct_mode && params.show_ascii {
        window.with_content_mask(
            Some(gpui::ContentMask {
                bounds: scrollable_mask_bounds,
            }),
            |window| {
                let ascii_start_x = hex_end_x + px(gap);
                let ascii_content_start_x = ascii_start_x - px(params.ascii_scroll_x);
                let ascii_mask_bounds = Bounds::new(point(ascii_start_x, params.bounds.top()), size(px(ascii_width), px(ROW_HEIGHT)));
                window.with_content_mask(Some(gpui::ContentMask { bounds: ascii_mask_bounds }), |window| {
                    let char_map = build_ascii_char_map(params.encoding, params.doc.buffer.data(), offset, chunk.len());
                    let group_bytes = params.group_size.byte_count();

                    // 1. Background Quads Pass for ASCII Cells
                    for (j, _) in chunk.iter().enumerate() {
                        let byte_pos = offset + j;
                        let in_selected_group = if params.min_sel <= params.max_sel {
                            let group_start = (byte_pos / group_bytes) * group_bytes;
                            let group_end = group_start + group_bytes;
                            group_start <= params.max_sel && group_end > params.min_sel
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
                            point(ascii_content_start_x + px(j as f32 * ASCII_CELL_WIDTH), params.bounds.top() + px(1.0)),
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
                        let is_cursor = params.cursor_offset >= item_start_offset && params.cursor_offset < item_end_offset;

                        if is_cursor {
                            let cursor_border_color = if params.is_focused {
                                cursor_bg
                            } else {
                                darken_cursor_color(muted_color).opacity(0.8)
                            };
                            let group_start_x = ascii_content_start_x + px(group.chunk_start as f32 * ASCII_CELL_WIDTH);
                            let group_width = px((group.chunk_end - group.chunk_start) as f32 * ASCII_CELL_WIDTH);
                            let ascii_group_bounds = Bounds::new(point(group_start_x, params.bounds.top() + px(1.0)), size(group_width, px(ROW_HEIGHT - 2.0)));
                            paint_cursor_border(window, ascii_group_bounds, cursor_border_color);
                        }
                    }

                    // Paint each glyph centered in its fixed byte cell using batched line shaping.
                    let mut ascii_text = String::with_capacity(chunk.len());
                    let mut ascii_runs: Vec<gpui::TextRun> = Vec::with_capacity(chunk.len());
                    let mut ascii_entries: Vec<AsciiCellEntry> = Vec::with_capacity(chunk.len());

                    for (j, opt) in char_map.into_iter().enumerate() {
                        let (c, _) = if let Some(pair) = opt { pair } else { continue };
                        let is_control = (c as u32) < 0x20 || (c as u32) == 0x7f;
                        let text_color = if is_control || c == '·' { muted_color.opacity(0.4) } else { fg_color };

                        let text_byte_start = ascii_text.len();
                        ascii_text.push(c);
                        let text_byte_end = ascii_text.len();

                        ascii_runs.push(gpui::TextRun {
                            len: text_byte_end - text_byte_start,
                            font: font.clone(),
                            color: text_color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        });

                        ascii_entries.push(AsciiCellEntry {
                            cell_idx: j,
                            text_byte_start,
                            text_byte_end,
                            color: text_color,
                        });
                    }

                    if !ascii_text.is_empty() {
                        let shaped = window
                            .text_system()
                            .shape_line(SharedString::from(ascii_text), params.font_size, &ascii_runs, None);
                        paint_centered_ascii_glyphs(
                            &shaped,
                            &ascii_entries,
                            point(ascii_content_start_x, params.bounds.top() + px(2.0)),
                            line_height,
                            window,
                        );
                    }

                    // Edge fades when the ASCII row is horizontally scrolled or clipped.
                    if params.ascii_scroll_x > 1.0 {
                        let bg = bg_color_theme;
                        for step in 0..5 {
                            let x = ascii_start_x + px(step as f32 * 3.2);
                            let alpha = 1.0 - (step as f32 / 5.0);
                            window.paint_quad(gpui::fill(
                                Bounds::new(point(x, params.bounds.top()), size(px(3.4), px(ROW_HEIGHT))),
                                bg.opacity(alpha * 0.95),
                            ));
                        }
                    }

                    if params.ascii_scroll_x + ascii_width < chunk.len() as f32 * ASCII_CELL_WIDTH - 1.0 {
                        let fade_w = 22.0;
                        let fade_start = ascii_start_x + px(ascii_width - fade_w);
                        let bg = bg_color_theme;
                        for step in 0..6 {
                            let x = fade_start + px(step as f32 * 3.6);
                            let alpha = (step + 1) as f32 / 7.0;
                            window.paint_quad(gpui::fill(
                                Bounds::new(point(x, params.bounds.top()), size(px(3.8), px(ROW_HEIGHT))),
                                bg.opacity(alpha * 0.95),
                            ));
                        }
                    }
                });
            },
        );
    }

    // 4. Description Column (when structure definition is present)
    if let Some(parse_res) = params.parse_result {
        let desc_start_x = hex_end_x + px(SECTION_GAP);
        let desc_end_x = desc_start_x + px(params.desc_col_width);

        let active_ranges = parse_res.find_active_struct_ranges(offset, chunk_len);
        let container_structs = parse_res.find_container_structs_starting_at(offset, chunk_len);
        let leaf_fields = parse_res.find_leaf_fields_starting_at(offset, chunk_len);

        let is_collapsed = container_structs.first().map(|c| params.collapsed_structs.contains(&c.id)).unwrap_or(false);

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
            let shaped_expr = window.text_system().shape_line(expr_shared, params.font_size, &[run], None);
            let desc_mask_bounds = Bounds::new(point(desc_start_x, params.bounds.top()), size(px(params.desc_col_width), px(ROW_HEIGHT)));
            let desc_text_width = f32::from(shaped_expr.width) + indent_px + 8.0;

            window.with_content_mask(
                Some(gpui::ContentMask {
                    bounds: scrollable_mask_bounds,
                }),
                |window| {
                    window.with_content_mask(Some(gpui::ContentMask { bounds: desc_mask_bounds }), |window| {
                        let _ = shaped_expr.paint(
                            point(desc_start_x - px(params.desc_scroll_x) + px(indent_px), params.bounds.top() + px(2.0)),
                            line_height,
                            window,
                            cx,
                        );

                        // Left fade
                        if params.desc_scroll_x > 1.0 {
                            let bg = bg_color_theme;
                            for step in 0..5 {
                                let x = desc_start_x + px(step as f32 * 3.2);
                                let alpha = 1.0 - (step as f32 / 5.0);
                                window.paint_quad(gpui::fill(
                                    Bounds::new(point(x, params.bounds.top()), size(px(3.4), px(ROW_HEIGHT))),
                                    bg.opacity(alpha * 0.95),
                                ));
                            }
                        }

                        // Right fade
                        if params.desc_scroll_x + params.desc_col_width < desc_text_width - 1.0 {
                            let fade_w = 20.0;
                            let fade_start = desc_end_x - px(fade_w);
                            let bg = bg_color_theme;
                            for step in 0..5 {
                                let x = fade_start + px(step as f32 * 4.0);
                                let alpha = (step + 1) as f32 / 6.0;
                                window.paint_quad(gpui::fill(
                                    Bounds::new(point(x, params.bounds.top()), size(px(4.2), px(ROW_HEIGHT))),
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
    let row_highlight_comments: Vec<(gpui::Hsla, SharedString)> = params
        .highlight_items
        .iter()
        .filter(|h| {
            if h.comment.trim().is_empty() {
                return false;
            }
            let h_start_row = crate::core::editor::Editor::find_line_index(h.offset, params.line_starts);
            let h_end_offset = h.offset.saturating_add(h.size);
            let h_last_byte = h_end_offset.saturating_sub(1).max(h.offset);
            let h_end_row = crate::core::editor::Editor::find_line_index(h_last_byte, params.line_starts);
            let display_row = h_start_row.max(params.top_visible_row);
            params.row_idx == display_row && params.row_idx <= h_end_row
        })
        .map(|h| (h.color.to_badge_hsla(), SharedString::from(h.comment.trim().to_string())))
        .collect();

    if !row_highlight_comments.is_empty() {
        let comment_mask_bounds = Bounds::new(point(comment_start_x, params.bounds.top()), size(px(params.comment_col_width), px(ROW_HEIGHT)));
        let comment_end_x = comment_start_x + px(params.comment_col_width);

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
            let shaped_comment = window.text_system().shape_line(comment_shared.clone(), params.font_size, &[run], None);
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
                    let mut cur_x = comment_start_x - px(params.comment_scroll_x) + px(4.0);
                    let dot_y = params.bounds.top() + px((ROW_HEIGHT - dot_size) / 2.0);

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
                        let _ = shaped_comment.paint(point(text_x, params.bounds.top() + px(2.0)), line_height, window, cx);

                        cur_x = text_x + px(text_w + item_spacing);
                    }

                    // Left fade
                    if params.comment_scroll_x > 1.0 {
                        let bg = bg_color_theme;
                        for step in 0..5 {
                            let x = comment_start_x + px(step as f32 * 3.2);
                            let alpha = 1.0 - (step as f32 / 5.0);
                            window.paint_quad(gpui::fill(
                                Bounds::new(point(x, params.bounds.top()), size(px(3.4), px(ROW_HEIGHT))),
                                bg.opacity(alpha * 0.95),
                            ));
                        }
                    }

                    // Right fade
                    if params.comment_scroll_x + params.comment_col_width < comment_text_width - 1.0 {
                        let fade_w = 20.0;
                        let fade_start = comment_end_x - px(fade_w);
                        let bg = bg_color_theme;
                        for step in 0..5 {
                            let x = fade_start + px(step as f32 * 4.0);
                            let alpha = (step + 1) as f32 / 6.0;
                            window.paint_quad(gpui::fill(
                                Bounds::new(point(x, params.bounds.top()), size(px(4.2), px(ROW_HEIGHT))),
                                bg.opacity(alpha * 0.95),
                            ));
                        }
                    }
                });
            },
        );
    }
}

pub fn paint_scrollbar(
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
