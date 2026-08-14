use crate::core::appearance::Appearance;
use crate::core::editor::Editor;
use gpui::prelude::*;
use gpui::*;
use gpui_component::{ActiveTheme, StyledExt};

pub enum StatusBarEvent {
    #[allow(dead_code)]
    ToggleLeftPanel,
}

pub struct StatusBar {
    active_editor: Option<WeakEntity<Editor>>,
}

impl EventEmitter<StatusBarEvent> for StatusBar {}

impl StatusBar {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self { active_editor: None }
    }

    pub fn set_active_editor(&mut self, editor: Option<Entity<Editor>>) {
        self.active_editor = editor.map(|e| e.downgrade());
    }
}

impl Render for StatusBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let active_editor = self.active_editor.as_ref().and_then(|e| e.upgrade());

        let (cursor_offset, total_size) = if let Some(editor) = &active_editor {
            let editor = editor.read(cx);
            (editor.cursor_offset, editor.total_size())
        } else {
            (0, 0)
        };

        let current_byte_info = if let Some(editor) = &active_editor {
            let editor = editor.read(cx);
            let doc = editor.document.read().ok();
            doc.and_then(|d| d.buffer.get_range(editor.cursor_offset, 1).first().copied()).map(|b| {
                let ascii_repr = if (0x20..=0x7E).contains(&b) {
                    format!(", '{}'", b as char)
                } else {
                    String::new()
                };
                format!("Val: 0x{:02X} ({}{}, 0b{:08b})", b, b, ascii_repr, b)
            })
        } else {
            None
        };

        let selection_info = if let Some(editor) = &active_editor {
            let editor = editor.read(cx);
            if editor.selection_start.is_some() && editor.selection_end.is_some() {
                editor.selected_range_or_cursor().map(|range| {
                    let end_inclusive = range.end.saturating_sub(1);
                    format!("Sel: 0x{:08X}..0x{:08X} ({} B)", range.start, end_inclusive, range.len())
                })
            } else {
                None
            }
        } else {
            None
        };

        let is_dirty = if let Some(editor) = &active_editor {
            let editor = editor.read(cx);
            editor.document.read().map(|d| d.is_dirty()).unwrap_or(false)
        } else {
            false
        };

        let (is_parsing, parse_offset, parse_total, parse_result_info) = if let Some(editor) = &active_editor {
            let editor = editor.read(cx);
            let parse_info = editor
                .parse_result()
                .map(|res| (res.definition_id.clone(), res.total_parsed_bytes, res.errors.len()));
            let total = if editor.parse_total_size > 0 {
                editor.parse_total_size
            } else {
                editor.total_size()
            };
            (editor.is_parsing_structure, editor.parse_progress_offset, total, parse_info)
        } else {
            (false, 0, 0, None)
        };

        let format_badge_info = if let Some(editor) = &active_editor {
            let editor = editor.read(cx);
            let radix_str = editor.radix.short_label();
            let group_str = editor.group_size.short_label();
            let endian_str = if editor.group_size.byte_count() > 1 {
                if editor.is_big_endian { " BE" } else { " LE" }
            } else {
                ""
            };
            Some(format!("{} {}{}", radix_str, group_str, endian_str))
        } else {
            None
        };

        let encoding_info = if let Some(editor) = &active_editor {
            let editor = editor.read(cx);
            Some(match editor.encoding {
                crate::core::encoding::Encoding::Ascii => "ASCII",
                crate::core::encoding::Encoding::Utf8 => "UTF-8",
                crate::core::encoding::Encoding::Utf16Le => "UTF-16 LE",
                crate::core::encoding::Encoding::Utf16Be => "UTF-16 BE",
            })
        } else {
            None
        };

        div()
            .flex()
            .items_center()
            .justify_between()
            .h_8()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.background)
            .font_family(cx.global::<Appearance>().font_family.clone())
            .px_4()
            .child(
                // Left side: cursor offset, selection, current byte, and structure parsing status
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .text_xs()
                    .child(
                        div()
                            .text_color(theme.foreground)
                            .child(format!("Offset: 0x{:08X} ({})", cursor_offset, cursor_offset)),
                    )
                    .when_some(selection_info, |el, sel_str| {
                        el.child(div().w_px().h_3().bg(theme.border)).child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded_md()
                                .bg(theme.blue.opacity(0.15))
                                .text_color(theme.blue)
                                .child(sel_str),
                        )
                    })
                    .when_some(current_byte_info, |el, val_str| {
                        el.child(div().w_px().h_3().bg(theme.border))
                            .child(div().text_color(theme.muted_foreground).child(val_str))
                    })
                    .when(is_parsing || parse_result_info.is_some(), |el| {
                        el.child(div().w_px().h_3().bg(theme.border)).child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .when(is_parsing, |el| {
                                    let pct = if parse_total > 0 {
                                        ((parse_offset as f64 / parse_total as f64) * 100.0).min(100.0)
                                    } else {
                                        0.0
                                    };
                                    let editor_handle = active_editor.clone();
                                    el.child(
                                        div()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_md()
                                            .bg(theme.blue.opacity(0.2))
                                            .text_color(theme.blue)
                                            .font_bold()
                                            .child("Parsing..."),
                                    )
                                    .child(
                                        div()
                                            .text_color(theme.foreground)
                                            .child(format!("0x{:08X} / 0x{:08X} ({:.1}%)", parse_offset, parse_total, pct)),
                                    )
                                    .child(
                                        div()
                                            .id("stop-parsing-button")
                                            .px_2()
                                            .py_0p5()
                                            .rounded_md()
                                            .bg(theme.red.opacity(0.2))
                                            .hover(|s| s.bg(theme.red.opacity(0.35)))
                                            .text_color(theme.red)
                                            .font_bold()
                                            .cursor_pointer()
                                            .child("⏹ Stop")
                                            .on_click(cx.listener(move |_, _, _window, cx| {
                                                if let Some(ref editor) = editor_handle {
                                                    editor.update(cx, |ed, cx| {
                                                        ed.cancel_structure_parsing();
                                                        cx.notify();
                                                    });
                                                }
                                            })),
                                    )
                                })
                                .when(!is_parsing, |el| {
                                    if let Some((def_id, _parsed_bytes, err_count)) = parse_result_info {
                                        el.child(
                                            div()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded_md()
                                                .bg(theme.green.opacity(0.2))
                                                .text_color(theme.green)
                                                .font_bold()
                                                .child(format!("Struct: {}", def_id)),
                                        )
                                        .when(err_count > 0, |el| {
                                            el.child(div().px_1p5().py_0p5().rounded_md().bg(theme.red.opacity(0.2)).text_color(theme.red).child(
                                                if err_count == 1 {
                                                    "1 error".to_string()
                                                } else {
                                                    format!("{} errors", err_count)
                                                },
                                            ))
                                        })
                                    } else {
                                        el
                                    }
                                }),
                        )
                    }),
            )
            .child(
                // Right side: document state, custom layout, file size, format badge, encoding
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .text_xs()
                    .when(is_dirty, |el| {
                        el.child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded_md()
                                .bg(theme.yellow.opacity(0.2))
                                .text_color(theme.yellow)
                                .font_bold()
                                .child("Modified"),
                        )
                    })
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(format!("Size: {} B (0x{:X})", total_size, total_size)),
                    )
                    .when_some(format_badge_info, |el, badge| {
                        el.child(div().w_px().h_3().bg(theme.border)).child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded_md()
                                .bg(theme.accent.opacity(0.15))
                                .text_color(theme.accent_foreground)
                                .font_bold()
                                .child(badge),
                        )
                    })
                    .when_some(encoding_info, |el, enc| {
                        el.child(div().w_px().h_3().bg(theme.border)).child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded_md()
                                .bg(theme.muted_foreground.opacity(0.15))
                                .text_color(theme.muted_foreground)
                                .child(enc),
                        )
                    }),
            )
    }
}
