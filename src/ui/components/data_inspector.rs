use crate::core::editor::Editor;
use crate::ui::style::StyleExt as _;
use gpui::prelude::*;
use gpui::*;
use gpui_component::scroll::ScrollableElement;
use gpui_component::{ActiveTheme as _, button::Button, button::ButtonVariants, h_flex, v_flex};
pub struct DataInspector {
    pub editor: Option<Entity<Editor>>,
    pub focus_handle: FocusHandle,
    pub is_big_endian: bool,
    _editor_subscription: Option<Subscription>,
}

impl DataInspector {
    pub fn new(editor: Option<Entity<Editor>>, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let _editor_subscription = editor.as_ref().map(|ed| {
            cx.observe(ed, |_, _, cx| {
                cx.notify();
            })
        });

        Self {
            editor,
            focus_handle,
            is_big_endian: false,
            _editor_subscription,
        }
    }

    pub fn set_editor(&mut self, editor: Option<Entity<Editor>>, cx: &mut Context<Self>) {
        self._editor_subscription = None;
        self.editor = editor.clone();
        if let Some(ed) = &editor {
            self._editor_subscription = Some(cx.observe(ed, |_, _, cx| {
                cx.notify();
            }));
        }
        cx.notify();
    }

    fn render_row(&self, label: &'static str, value: String, theme: &gpui_component::Theme) -> impl IntoElement {
        h_flex()
            .w_full()
            .justify_between()
            .py_1()
            .px_2()
            .hover(|style| style.bg(theme.accent.opacity(0.1)))
            .child(div().flex_shrink_0().w(px(110.0)).text_color(theme.muted_foreground).child(label))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_end()
                    .overflow_hidden()
                    .min_w_0()
                    .font_family("Courier New")
                    .text_color(theme.foreground)
                    .child(value),
            )
    }

    fn render_section_header(&self, label: &'static str, theme: &gpui_component::Theme) -> impl IntoElement {
        div()
            .mt_3()
            .mb_1()
            .px_2()
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .text_color(theme.accent)
            .child(label)
    }

    fn format_unix_time(&self, timestamp: i64) -> String {
        if timestamp < 0 || timestamp > 253402300799 {
            // up to year 9999
            return "Out of range".to_string();
        }

        let seconds = timestamp;
        let day_clock = seconds % 86400;
        let mut days_since_epoch = seconds / 86400;

        let hour = day_clock / 3600;
        let minute = (day_clock % 3600) / 60;
        let second = day_clock % 60;

        let mut year = 1970;
        loop {
            let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
            let year_days = if is_leap { 366 } else { 365 };
            if days_since_epoch < year_days {
                break;
            }
            days_since_epoch -= year_days;
            year += 1;
        }

        let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let month_days = if is_leap {
            [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        } else {
            [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        };

        let mut month = 1;
        for &days in &month_days {
            if days_since_epoch < days {
                break;
            }
            days_since_epoch -= days;
            month += 1;
        }

        let day = days_since_epoch + 1;
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC", year, month, day, hour, minute, second)
    }
}

pub fn format_hex_values(bytes: &[u8], is_big_endian: bool) -> (String, String, String, String) {
    let mut hex8 = "--".to_string();
    let mut hex16 = "--".to_string();
    let mut hex32 = "--".to_string();
    let mut hex64 = "--".to_string();

    if !bytes.is_empty() {
        hex8 = format!("0x{:02X}", bytes[0]);
    }

    if bytes.len() >= 2 {
        let arr: [u8; 2] = bytes[0..2].try_into().unwrap();
        let val = if is_big_endian { u16::from_be_bytes(arr) } else { u16::from_le_bytes(arr) };
        hex16 = format!("0x{:04X}", val);
    }

    if bytes.len() >= 4 {
        let arr: [u8; 4] = bytes[0..4].try_into().unwrap();
        let val = if is_big_endian { u32::from_be_bytes(arr) } else { u32::from_le_bytes(arr) };
        hex32 = format!("0x{:08X}", val);
    }

    if bytes.len() >= 8 {
        let arr: [u8; 8] = bytes[0..8].try_into().unwrap();
        let val = if is_big_endian { u64::from_be_bytes(arr) } else { u64::from_le_bytes(arr) };
        hex64 = format!("0x{:016X}", val);
    }

    (hex8, hex16, hex32, hex64)
}

impl Render for DataInspector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let active_editor = self.editor.as_ref();

        let bytes_at_cursor = if let Some(editor) = active_editor {
            editor.read(cx).read_bytes_at_cursor(8)
        } else {
            Vec::new()
        };

        let is_big_endian = self.is_big_endian;

        let (hex8_val, hex16_val, hex32_val, hex64_val) = format_hex_values(&bytes_at_cursor, is_big_endian);

        // Data conversion logic
        let mut i8_val = "--".to_string();
        let mut u8_val = "--".to_string();
        let mut i16_val = "--".to_string();
        let mut u16_val = "--".to_string();
        let mut i32_val = "--".to_string();
        let mut u32_val = "--".to_string();
        let mut i64_val = "--".to_string();
        let mut u64_val = "--".to_string();
        let mut f32_val = "--".to_string();
        let mut f64_val = "--".to_string();
        let mut unix_time_32 = "--".to_string();
        let mut unix_time_64 = "--".to_string();
        let mut ascii_val = "--".to_string();
        let mut utf8_val = "--".to_string();
        let mut utf16_val = "--".to_string();

        if !bytes_at_cursor.is_empty() {
            let b = bytes_at_cursor[0];
            i8_val = format!("{}", b as i8);
            u8_val = format!("{}", b);

            let ch = b as char;
            if ch.is_ascii_graphic() || ch == ' ' {
                ascii_val = format!("'{}'", ch);
            } else {
                ascii_val = ".".to_string();
            }
        }

        if bytes_at_cursor.len() >= 2 {
            let arr: [u8; 2] = bytes_at_cursor[0..2].try_into().unwrap();
            let i16_val_raw = if is_big_endian { i16::from_be_bytes(arr) } else { i16::from_le_bytes(arr) };
            let u16_val_raw = if is_big_endian { u16::from_be_bytes(arr) } else { u16::from_le_bytes(arr) };
            i16_val = format!("{}", i16_val_raw);
            u16_val = format!("{}", u16_val_raw);

            let ch = u16_val_raw;
            if let Some(c) = char::from_u32(ch as u32) {
                if !c.is_control() {
                    utf16_val = format!("'{}'", c);
                } else {
                    utf16_val = ".".to_string();
                }
            } else {
                utf16_val = ".".to_string();
            }
        }

        if bytes_at_cursor.len() >= 4 {
            let arr: [u8; 4] = bytes_at_cursor[0..4].try_into().unwrap();
            let i32_val_raw = if is_big_endian { i32::from_be_bytes(arr) } else { i32::from_le_bytes(arr) };
            let u32_val_raw = if is_big_endian { u32::from_be_bytes(arr) } else { u32::from_le_bytes(arr) };
            let f32_val_raw = if is_big_endian { f32::from_be_bytes(arr) } else { f32::from_le_bytes(arr) };
            i32_val = format!("{}", i32_val_raw);
            u32_val = format!("{}", u32_val_raw);
            f32_val = format!("{:.6}", f32_val_raw);
            unix_time_32 = self.format_unix_time(i32_val_raw as i64);
        }

        if bytes_at_cursor.len() >= 8 {
            let arr: [u8; 8] = bytes_at_cursor[0..8].try_into().unwrap();
            let i64_val_raw = if is_big_endian { i64::from_be_bytes(arr) } else { i64::from_le_bytes(arr) };
            let u64_val_raw = if is_big_endian { u64::from_be_bytes(arr) } else { u64::from_le_bytes(arr) };
            let f64_val_raw = if is_big_endian { f64::from_be_bytes(arr) } else { f64::from_le_bytes(arr) };
            i64_val = format!("{}", i64_val_raw);
            u64_val = format!("{}", u64_val_raw);
            f64_val = format!("{:.6}", f64_val_raw);
            unix_time_64 = self.format_unix_time(i64_val_raw);
        }

        if !bytes_at_cursor.is_empty() {
            let first_byte = bytes_at_cursor[0];
            let expected_len = if first_byte & 0x80 == 0 {
                1
            } else if first_byte & 0xE0 == 0xC0 {
                2
            } else if first_byte & 0xF0 == 0xE0 {
                3
            } else if first_byte & 0xF8 == 0xF0 {
                4
            } else {
                0
            };

            let mut decoded = false;
            if expected_len > 0 && expected_len <= bytes_at_cursor.len() {
                if let Ok(s) = std::str::from_utf8(&bytes_at_cursor[0..expected_len]) {
                    if let Some(c) = s.chars().next() {
                        if !c.is_control() {
                            utf8_val = format!("'{}'", c);
                        } else {
                            utf8_val = ".".to_string();
                        }
                        decoded = true;
                    }
                }
            }
            if !decoded {
                utf8_val = ".".to_string();
            }
        }

        let is_focused = self.focus_handle.is_focused(window);

        let container = v_flex().size_full().min_w_0().overflow_hidden().bg(theme.sidebar);

        let container = container.focus_indicator(is_focused, theme);

        container
            .id("data-inspector")
            .track_focus(&self.focus_handle)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, _| {
                    this.focus_handle.focus(window);
                }),
            )
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .p_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .text_sm()
                            .text_color(crate::ui::style::header_text_color(is_focused, theme))
                            .child("DATA INSPECTOR"),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .child(
                                if !is_big_endian {
                                    Button::new("endian_little").label("Little").primary()
                                } else {
                                    Button::new("endian_little").label("Little").ghost()
                                }
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if this.is_big_endian {
                                        this.is_big_endian = false;
                                        cx.notify();
                                    }
                                })),
                            )
                            .child(
                                if is_big_endian {
                                    Button::new("endian_big").label("Big").primary()
                                } else {
                                    Button::new("endian_big").label("Big").ghost()
                                }
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if !this.is_big_endian {
                                        this.is_big_endian = true;
                                        cx.notify();
                                    }
                                })),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .size_full()
                    .min_w_0()
                    .overflow_y_scrollbar()
                    .p_2()
                    .child(self.render_section_header("HEXADECIMAL", theme))
                    .child(self.render_row("Hex (1 byte)", hex8_val, theme))
                    .child(self.render_row("Hex (2 bytes)", hex16_val, theme))
                    .child(self.render_row("Hex (4 bytes)", hex32_val, theme))
                    .child(self.render_row("Hex (8 bytes)", hex64_val, theme))
                    .child(self.render_section_header("INTEGERS", theme))
                    .child(self.render_row("Int8", i8_val, theme))
                    .child(self.render_row("UInt8", u8_val, theme))
                    .child(self.render_row("Int16", i16_val, theme))
                    .child(self.render_row("UInt16", u16_val, theme))
                    .child(self.render_row("Int32", i32_val, theme))
                    .child(self.render_row("UInt32", u32_val, theme))
                    .child(self.render_row("Int64", i64_val, theme))
                    .child(self.render_row("UInt64", u64_val, theme))
                    .child(self.render_section_header("FLOATS", theme))
                    .child(self.render_row("Float32", f32_val, theme))
                    .child(self.render_row("Float64", f64_val, theme))
                    .child(self.render_section_header("TIME", theme))
                    .child(self.render_row("Unix Time (32-bit)", unix_time_32, theme))
                    .child(self.render_row("Unix Time (64-bit)", unix_time_64, theme))
                    .child(self.render_section_header("TEXT", theme))
                    .child(self.render_row("ASCII", ascii_val, theme))
                    .child(self.render_row("UTF-8", utf8_val, theme))
                    .child(self.render_row("UTF-16", utf16_val, theme)),
            )
    }
}

impl Focusable for DataInspector {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
