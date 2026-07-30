use crate::core::checksum;
use crate::core::editor::Editor;
use crate::ui::style::StyleExt as _;
use gpui::prelude::*;
use gpui::*;
use gpui_component::scroll::ScrollableElement;
use gpui_component::{ActiveTheme as _, IconName, button::Button, button::ButtonVariants, h_flex, v_flex};
use gpui_component::{Disableable, Selectable, Sizable, Size};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CalculationRange {
    Selection,
    EntireFile,
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
            let doc = editor.document.read().unwrap();
            let buffer = &doc.buffer;
            let total_len = buffer.len();

            let r = match self.calculation_range {
                CalculationRange::Selection => {
                    if let Some(r) = editor.selection_range() {
                        r
                    } else {
                        editor.cursor_offset..editor.cursor_offset
                    }
                }
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

    fn render_row(&self, label: &'static str, display_value: String, copy_value: String, theme: &gpui_component::Theme) -> impl IntoElement {
        let copy_val = copy_value.clone();
        h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .py_1()
            .px_2()
            .hover(|style| style.bg(theme.accent.opacity(0.1)))
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
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_val.clone()));
                            }),
                    ),
            )
    }
}

impl Render for ChecksumPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let is_focused = self.focus_handle.is_focused(window);

        // Header
        let header = h_flex().justify_between().items_center().p_2().border_b_1().border_color(theme.border).child(
            div()
                .text_sm()
                .text_color(crate::ui::style::header_text_color(is_focused, theme))
                .child("CHECKSUM & SUM"),
        );

        // Context info
        let mut info_text = "No Active File".to_string();
        let mut range_desc = String::new();
        let mut data_len = 0;

        if let Some(editor_entity) = &self.editor {
            let editor = editor_entity.read(cx);
            let total_len = editor.total_size();
            info_text = format!("File Size: {} bytes", total_len);

            let range = match self.calculation_range {
                CalculationRange::Selection => {
                    if let Some(r) = editor.selection_range() {
                        r
                    } else {
                        editor.cursor_offset..editor.cursor_offset
                    }
                }
                CalculationRange::EntireFile => 0..total_len,
            };
            data_len = range.len();
            if data_len > 0 {
                range_desc = format!("Range: 0x{:08X} - 0x{:08X} ({} bytes)", range.start, range.end, data_len);
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
                Button::new("range_entire")
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
        let calc_button_disabled = self.is_calculating || self.editor.is_none() || data_len == 0;
        let control_section = h_flex()
            .px_2()
            .pb_2()
            .justify_between()
            .items_center()
            .child(
                Button::new("auto_calc_toggle")
                    .label(if self.auto_calculate { "Auto: ON" } else { "Auto: OFF" })
                    .ghost()
                    .selected(self.auto_calculate)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.auto_calculate = !this.auto_calculate;
                        if this.auto_calculate {
                            this.trigger_calculation(cx);
                        } else {
                            cx.notify();
                        }
                    })),
            )
            .child(
                Button::new("calculate_btn")
                    .label(if self.is_calculating { "Calculating..." } else { "Calculate" })
                    .primary()
                    .disabled(calc_button_disabled)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.trigger_calculation(cx);
                    })),
            );

        // Results or Status Area
        let results_container = if self.is_calculating {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
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

            v_flex()
                .flex_1()
                .p_2()
                .child(self.render_row("Sum 8-bit", sum8_str, format!("0x{:02X}", res.sum8), theme))
                .child(self.render_row("Sum 16-bit", sum16_str, format!("0x{:04X}", res.sum16), theme))
                .child(self.render_row("Sum 32-bit", sum32_str, format!("0x{:08X}", res.sum32), theme))
                .child(self.render_row("Sum 64-bit", sum64_str, format!("0x{:016X}", res.sum64), theme))
                .child(self.render_row("Adler-32", adler32_str.clone(), adler32_str, theme))
                .child(self.render_row("CRC-16 (CCITT)", crc16_ccitt_str.clone(), crc16_ccitt_str, theme))
                .child(self.render_row("CRC-16 (ARC)", crc16_arc_str.clone(), crc16_arc_str, theme))
                .child(self.render_row("CRC-32", crc32_str.clone(), crc32_str, theme))
                .child(self.render_row("MD5", md5_str.clone(), md5_str, theme))
                .child(self.render_row("SHA-256", sha256_str.clone(), sha256_str, theme))
                .overflow_y_scrollbar()
                .into_any_element()
        } else {
            let msg = if self.editor.is_none() {
                "No active editor"
            } else if data_len == 0 {
                "Selection is empty"
            } else if self.auto_calculate && data_len > 1_000_000 {
                "Range too large for Auto-Calc.\nClick Calculate."
            } else {
                "Click Calculate to compute"
            };

            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .p_4()
                .child(div().text_sm().text_color(theme.muted_foreground).child(msg))
                .into_any_element()
        };

        let container = v_flex().size_full().min_w_0().overflow_hidden().bg(theme.sidebar);
        let container = container.focus_indicator(is_focused, theme);

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
