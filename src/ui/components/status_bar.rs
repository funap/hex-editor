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

        let has_custom_layout = if let Some(editor) = &active_editor {
            let editor = editor.read(cx);
            editor.has_custom_layout()
        } else {
            false
        };

        let custom_layout_count = if let Some(editor) = &active_editor {
            let editor = editor.read(cx);
            editor.custom_layout_count()
        } else {
            0
        };

        let encoding_name = if let Some(editor) = &active_editor {
            let editor = editor.read(cx);
            format!("{:?}", editor.encoding)
        } else {
            "--".to_string()
        };

        let (is_parsing, parse_offset, parse_total, parse_result_info) = if let Some(editor) = &active_editor {
            let editor = editor.read(cx);
            let parse_info = editor
                .parse_result
                .as_ref()
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

        div()
            .flex()
            .items_center()
            .h_8()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.background)
            .font_family(cx.global::<Appearance>().font_family.clone())
            .px_4()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .text_sm()
                    .child(format!("Offset: 0x{:08X} ({})", cursor_offset, cursor_offset))
                    .child(format!("Size: {} bytes", total_size))
                    .when(has_custom_layout, |el| {
                        el.child(
                            div()
                                .px_2()
                                .rounded_md()
                                .bg(theme.yellow.opacity(0.2))
                                .text_color(theme.yellow)
                                .child(format!("Layout: {} breaks", custom_layout_count)),
                        )
                    }),
            )
            .when(is_parsing || parse_result_info.is_some(), |el| {
                el.child(div().w_px().h_4().bg(theme.border)).child(
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
                                    .px_2()
                                    .py_0p5()
                                    .rounded_md()
                                    .bg(theme.blue.opacity(0.2))
                                    .text_color(theme.blue)
                                    .text_xs()
                                    .font_bold()
                                    .child("Parsing..."),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.foreground)
                                    .child(format!("Parsed: 0x{:08X} / 0x{:08X} ({:.1}%)", parse_offset, parse_total, pct)),
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
                                    .text_xs()
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
                            if let Some((def_id, parsed_bytes, err_count)) = parse_result_info {
                                let pct = if parse_total > 0 {
                                    ((parsed_bytes as f64 / parse_total as f64) * 100.0).min(100.0)
                                } else {
                                    0.0
                                };
                                el.child(
                                    div()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_md()
                                        .bg(theme.green.opacity(0.2))
                                        .text_color(theme.green)
                                        .text_xs()
                                        .font_bold()
                                        .child(format!("Struct: {}", def_id)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(format!("0x{:08X} / 0x{:08X} ({:.1}%)", parsed_bytes, parse_total, pct)),
                                )
                                .when(err_count > 0, |el| {
                                    el.child(
                                        div()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_md()
                                            .bg(theme.red.opacity(0.2))
                                            .text_color(theme.red)
                                            .text_xs()
                                            .child(format!("{} errors", err_count)),
                                    )
                                })
                            } else {
                                el
                            }
                        }),
                )
            })
            .child(div().w_px().h_4().bg(theme.border))
            .child(
                div()
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("Encoding: {}", encoding_name)),
            )
    }
}
