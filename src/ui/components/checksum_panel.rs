use crate::core::checksum;
use crate::core::editor::Editor;
use crate::ui::icon::IconName;
use gpui::prelude::*;
use gpui::*;
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::scroll::ScrollableElement;
use gpui_component::{ActiveTheme as _, button::Button, button::ButtonVariants, h_flex, v_flex};
use gpui_component::{Disableable, Selectable, Sizable, Size};
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CalculationRange {
    Selection,
    EntireFile,
}

fn selected_range_for_checksum(editor: &Editor) -> Option<Range<usize>> {
    if editor.selection_start.is_some() && editor.selection_end.is_some() {
        editor.selected_range_or_cursor()
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub struct ChecksumResults {
    pub sum8: u8,
    pub sum16: u16,
    pub sum32: u32,
    pub sum64: u64,
    pub crc16_ccitt: u16,
    pub crc16_arc: u16,
    pub crc32: u32,
    pub adler32: u32,
    pub md5: [u8; 16],
    pub sha256: [u8; 32],
    #[allow(dead_code)]
    pub data_len: usize,
    #[allow(dead_code)]
    pub range_start: usize,
    #[allow(dead_code)]
    pub range_end: usize,
}

pub struct ChecksumPanel {
    pub editor: Option<Entity<Editor>>,
    pub focus_handle: FocusHandle,
    pub calculation_range: CalculationRange,
    pub auto_calculate: bool,
    pub is_calculating: bool,
    pub results: Option<ChecksumResults>,
    _editor_subscription: Option<Subscription>,
    calculation_task: Option<Task<()>>,
}

impl ChecksumPanel {
    pub fn new(editor: Option<Entity<Editor>>, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let mut this = Self {
            editor: None,
            focus_handle,
            calculation_range: CalculationRange::Selection,
            auto_calculate: true,
            is_calculating: false,
            results: None,
            _editor_subscription: None,
            calculation_task: None,
        };
        this.set_editor(editor, cx);
        this
    }

    pub fn set_editor(&mut self, editor: Option<Entity<Editor>>, cx: &mut Context<Self>) {
        self._editor_subscription = None;
        self.calculation_task = None;
        self.editor = editor.clone();
        self.results = None;
        self.is_calculating = false;

        if let Some(ed) = &editor {
            self._editor_subscription = Some(cx.observe(ed, |this, _, cx| {
                this.on_editor_changed(cx);
            }));
            self.on_editor_changed(cx);
        }
        cx.notify();
    }

    fn on_editor_changed(&mut self, cx: &mut Context<Self>) {
        if self.auto_calculate {
            self.trigger_calculation(cx);
        } else {
            cx.notify();
        }
    }

    fn trigger_calculation(&mut self, cx: &mut Context<Self>) {
        let Some(editor_entity) = &self.editor else {
            self.results = None;
            self.is_calculating = false;
            cx.notify();
            return;
        };

        // Determine range and buffer parameters in a nested scope to free the immutable borrow on cx
        let (range, data) = {
            let editor = editor_entity.read(cx);
            let selected_range = if self.calculation_range == CalculationRange::Selection {
                selected_range_for_checksum(editor)
            } else {
                None
            };
            let doc = editor.document.read().unwrap();
            let buffer = &doc.buffer;
            let total_len = buffer.len();

            let r = match self.calculation_range {
                CalculationRange::Selection => selected_range.unwrap_or(editor.cursor_offset..editor.cursor_offset),
                CalculationRange::EntireFile => 0..total_len,
            };

            let data_len = r.len();
            if data_len == 0 {
                (r, Vec::new())
            } else {
                (r.clone(), buffer.get_range(r.start, data_len).to_vec())
            }
        };

        let data_len = range.len();
        if data_len == 0 {
            self.results = None;
            self.is_calculating = false;
            cx.notify();
            return;
        }

        // If auto-calculating and data is > 1MB, skip automatic calculation to prevent lag
        if self.auto_calculate && data_len > 1_000_000 {
            self.results = None;
            self.is_calculating = false;
            cx.notify();
            return;
        }

        self.is_calculating = true;
        self.results = None;
        cx.notify();

        self.calculation_task = None;

        let start_offset = range.start;
        let end_offset = range.end;

        let task = cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move {
                    let sum8 = checksum::sum8(&data);
                    let sum16 = checksum::sum16(&data);
                    let sum32 = checksum::sum32(&data);
                    let sum64 = checksum::sum64(&data);
                    let adler32 = checksum::adler32(&data);
                    let crc16_ccitt = checksum::crc16_ccitt(&data);
                    let crc16_arc = checksum::crc16_arc(&data);
                    let crc32 = checksum::crc32(&data);
                    let md5 = checksum::md5(&data);
                    let sha256 = checksum::sha256(&data);

                    ChecksumResults {
                        sum8,
                        sum16,
                        sum32,
                        sum64,
                        crc16_ccitt,
                        crc16_arc,
                        crc32,
                        adler32,
                        md5,
                        sha256,
                        data_len,
                        range_start: start_offset,
                        range_end: end_offset,
                    }
                })
                .await;

            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| {
                    this.results = Some(results);
                    this.is_calculating = false;
                    cx.notify();
                })
                .ok();
            }
        });

        self.calculation_task = Some(task);
    }

    fn format_all_results(&self) -> Option<String> {
        let res = self.results.as_ref()?;
        let md5_str = res.md5.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        let sha256_str = res.sha256.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        Some(format!(
            "Sum 8-bit:       0x{:02X} ({})\n\
             Sum 16-bit:      0x{:04X} ({})\n\
             Sum 32-bit:      0x{:08X} ({})\n\
             Sum 64-bit:      0x{:016X} ({})\n\
             Adler-32:        0x{:08X}\n\
             CRC-16 (CCITT):  0x{:04X}\n\
             CRC-16 (ARC):    0x{:04X}\n\
             CRC-32:          0x{:08X}\n\
             MD5:             {}\n\
             SHA-256:         {}",
            res.sum8,
            res.sum8,
            res.sum16,
            res.sum16,
            res.sum32,
            res.sum32,
            res.sum64,
            res.sum64,
            res.adler32,
            res.crc16_ccitt,
            res.crc16_arc,
            res.crc32,
            md5_str,
            sha256_str
        ))
    }

    fn render_row(
        &self,
        label: &'static str,
        display_value: String,
        copy_value: String,
        all_text: Option<String>,
        theme: &gpui_component::Theme,
    ) -> impl IntoElement {
        let copy_val = copy_value.clone();
        let copy_val_for_click = copy_value.clone();
        let row_copy = format!("{}: {}", label, copy_value);
        let all_copy = all_text.clone();

        h_flex()
            .id(label)
            .w_full()
            .justify_between()
            .items_center()
            .py_1()
            .px_3()
            .rounded_sm()
            .cursor_pointer()
            .hover(|style| style.bg(theme.muted.opacity(0.4)))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_val_for_click.clone()));
            })
            .context_menu(move |menu, _window, _cx| {
                let v = copy_val.clone();
                let r = row_copy.clone();
                let all = all_copy.clone();

                let mut menu = menu
                    .menu(format!("Copy Value ({})", v), Box::new(crate::actions::Copy))
                    .menu(format!("Copy Row ({})", r), Box::new(crate::actions::Copy));
                if all.is_some() {
                    menu = menu.separator().menu("Copy All Checksums", Box::new(crate::actions::Copy));
                }
                menu
            })
            .child(div().flex_shrink_0().w(px(110.0)).text_xs().text_color(theme.muted_foreground).child(label))
            .child(
                h_flex()
                    .flex_1()
                    .justify_end()
                    .items_center()
                    .gap_1()
                    .overflow_hidden()
                    .min_w_0()
                    .child(
                        div()
                            .flex_1()
                            .text_right()
                            .font_family("Courier New")
                            .text_xs()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_color(theme.foreground)
                            .child(display_value),
                    )
                    .child(
                        Button::new(label)
                            .ghost()
                            .icon(IconName::Copy)
                            .with_size(Size::XSmall)
                            .tooltip("Copy Value")
                            .on_click({
                                let copy_v = copy_value.clone();
                                move |_, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_v.clone()));
                                }
                            }),
                    ),
            )
    }
}

impl Render for ChecksumPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let is_focused = self.focus_handle.is_focused(window);

        let all_formatted = self.format_all_results();
        let header_actions = if let Some(ref all_str) = all_formatted {
            let all_copy = all_str.clone();
            Some(
                Button::new("copy_all_checksums_header")
                    .ghost()
                    .icon(IconName::Copy)
                    .with_size(Size::XSmall)
                    .tooltip("Copy All Checksums")
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(all_copy.clone()));
                    })
                    .into_any_element(),
            )
        } else {
            None
        };

        // Header
        let header = crate::ui::style::panel_header("CHECKSUM & SUM", is_focused, theme, None, header_actions);

        // Context info
        let mut info_text = "No Active File".to_string();
        let mut range_desc = String::new();
        let mut data_len = 0;

        if let Some(editor_entity) = &self.editor {
            let editor = editor_entity.read(cx);
            let total_len = editor.total_size();
            info_text = format!("File Size: {} bytes", total_len);

            let range = match self.calculation_range {
                CalculationRange::Selection => selected_range_for_checksum(editor).unwrap_or(editor.cursor_offset..editor.cursor_offset),
                CalculationRange::EntireFile => 0..total_len,
            };
            data_len = range.len();
            if data_len > 0 {
                let end_inclusive = range.end.saturating_sub(1);
                range_desc = format!("Range: 0x{:08X} - 0x{:08X} ({} bytes)", range.start, end_inclusive, data_len);
            } else {
                range_desc = "No selection (0 bytes)".to_string();
            }
        }

        let info_section = v_flex()
            .p_2()
            .gap_1()
            .border_b_1()
            .border_color(theme.border)
            .child(div().text_xs().text_color(theme.muted_foreground).child(info_text))
            .child(div().text_xs().font_family("Courier New").text_color(theme.foreground).child(range_desc));

        // Range selection
        let range_selector = h_flex()
            .p_2()
            .gap_2()
            .items_center()
            .child(
                Button::new("range_selection")
                    .label("Selection")
                    .ghost()
                    .selected(self.calculation_range == CalculationRange::Selection)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.calculation_range != CalculationRange::Selection {
                            this.calculation_range = CalculationRange::Selection;
                            this.results = None;
                            if this.auto_calculate {
                                this.trigger_calculation(cx);
                            } else {
                                cx.notify();
                            }
                        }
                    })),
            )
            .child(
                Button::new("range_entire_file")
                    .label("Entire File")
                    .ghost()
                    .selected(self.calculation_range == CalculationRange::EntireFile)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.calculation_range != CalculationRange::EntireFile {
                            this.calculation_range = CalculationRange::EntireFile;
                            this.results = None;
                            if this.auto_calculate {
                                this.trigger_calculation(cx);
                            } else {
                                cx.notify();
                            }
                        }
                    })),
            );

        // Buttons for calculation control
        let control_section = h_flex()
            .p_2()
            .gap_2()
            .items_center()
            .justify_between()
            .child(
                Button::new("auto_calc_toggle")
                    .label("Auto")
                    .ghost()
                    .selected(self.auto_calculate)
                    .tooltip("Automatically calculate when selection changes (for ranges < 1MB)")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.auto_calculate = !this.auto_calculate;
                        if this.auto_calculate && this.results.is_none() {
                            this.trigger_calculation(cx);
                        } else {
                            cx.notify();
                        }
                    })),
            )
            .child(
                Button::new("calc_button")
                    .label("Calculate")
                    .disabled(self.editor.is_none() || data_len == 0 || self.is_calculating)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.trigger_calculation(cx);
                    })),
            );

        // Results or Status Area
        let results_container = if self.is_calculating {
            v_flex()
                .flex_1()
                .items_center()
                .pt_8()
                .p_4()
                .child(div().text_sm().text_color(theme.accent).child("Calculating sums..."))
                .into_any_element()
        } else if let Some(res) = &self.results {
            let sum8_str = format!("0x{:02X} ({})", res.sum8, res.sum8);
            let sum16_str = format!("0x{:04X} ({})", res.sum16, res.sum16);
            let sum32_str = format!("0x{:08X} ({})", res.sum32, res.sum32);
            let sum64_str = format!("0x{:016X} ({})", res.sum64, res.sum64);
            let adler32_str = format!("0x{:08X}", res.adler32);
            let crc16_ccitt_str = format!("0x{:04X}", res.crc16_ccitt);
            let crc16_arc_str = format!("0x{:04X}", res.crc16_arc);
            let crc32_str = format!("0x{:08X}", res.crc32);
            let md5_str = res.md5.iter().map(|b| format!("{:02x}", b)).collect::<String>();
            let sha256_str = res.sha256.iter().map(|b| format!("{:02x}", b)).collect::<String>();
            let all_opt = all_formatted.clone();

            v_flex()
                .flex_1()
                .p_2()
                .child(self.render_row("Sum 8-bit", sum8_str, format!("0x{:02X}", res.sum8), all_opt.clone(), theme))
                .child(self.render_row("Sum 16-bit", sum16_str, format!("0x{:04X}", res.sum16), all_opt.clone(), theme))
                .child(self.render_row("Sum 32-bit", sum32_str, format!("0x{:08X}", res.sum32), all_opt.clone(), theme))
                .child(self.render_row("Sum 64-bit", sum64_str, format!("0x{:016X}", res.sum64), all_opt.clone(), theme))
                .child(self.render_row("Adler-32", adler32_str.clone(), adler32_str, all_opt.clone(), theme))
                .child(self.render_row("CRC-16 (CCITT)", crc16_ccitt_str.clone(), crc16_ccitt_str, all_opt.clone(), theme))
                .child(self.render_row("CRC-16 (ARC)", crc16_arc_str.clone(), crc16_arc_str, all_opt.clone(), theme))
                .child(self.render_row("CRC-32", crc32_str.clone(), crc32_str, all_opt.clone(), theme))
                .child(self.render_row("MD5", md5_str.clone(), md5_str, all_opt.clone(), theme))
                .child(self.render_row("SHA-256", sha256_str.clone(), sha256_str, all_opt, theme))
                .overflow_y_scrollbar()
                .into_any_element()
        } else {
            let (title, msg) = if self.editor.is_none() {
                ("No Active File", "Open a binary file to compute checksums")
            } else if data_len == 0 {
                ("Selection Empty", "Select bytes in hex view or switch to Entire File")
            } else if self.auto_calculate && data_len > 1_000_000 {
                ("Large Data Range", "Range exceeds 1MB. Click Calculate to compute.")
            } else {
                ("Ready to Compute", "Click Calculate to compute checksums")
            };

            crate::ui::style::panel_empty_state(IconName::Hash, title, Some(msg), None, theme).into_any_element()
        };

        let container = crate::ui::style::panel_container(is_focused, theme);

        container
            .id("checksum-panel")
            .track_focus(&self.focus_handle)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, _| {
                    this.focus_handle.focus(window);
                }),
            )
            .child(header)
            .child(info_section)
            .child(range_selector)
            .child(control_section)
            .child(div().h_px().bg(theme.border))
            .child(results_container)
    }
}

impl Focusable for ChecksumPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
