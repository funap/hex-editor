use crate::core::appearance::Appearance;
use crate::core::editor::Editor;
use crate::core::encoding::Encoding;
use crate::ui::icon::IconName;
use gpui::prelude::*;
use gpui::*;
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::scroll::ScrollableElement;
use gpui_component::{ActiveTheme as _, Selectable as _, Sizable as _, button::Button, button::ButtonVariants, h_flex, v_flex};
const CONTEXT: &str = "DataInspector";

#[derive(Clone, PartialEq, Action)]
#[action(namespace = data_inspector, no_json)]
struct CopyValue {
    value: String,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = data_inspector, no_json)]
struct CopyFieldName {
    name: String,
}

pub struct DataInspector {
    pub editor: Option<Entity<Editor>>,
    pub focus_handle: FocusHandle,
    pub is_big_endian: bool,
    pub selected_row: Option<(&'static str, String)>,
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
            selected_row: None,
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

    fn copy_value(&mut self, action: &CopyValue, _window: &mut Window, cx: &mut Context<Self>) {
        if !action.value.is_empty() && action.value != "-" {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(action.value.clone()));
        }
    }

    fn copy_field_name(&mut self, action: &CopyFieldName, _window: &mut Window, cx: &mut Context<Self>) {
        if !action.name.is_empty() {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(action.name.clone()));
        }
    }

    fn render_row(
        &self,
        label: &'static str,
        value: String,
        font_family: &str,
        view: &Entity<Self>,
        window: &mut Window,
        theme: &gpui_component::Theme,
    ) -> impl IntoElement {
        let val_for_click = value.clone();
        let val_for_right_click = value.clone();

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
                if !val_for_click.is_empty() && val_for_click != "-" {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(val_for_click.clone()));
                }
            })
            .on_mouse_down(
                MouseButton::Right,
                window.listener_for(view, move |this, _, window, cx| {
                    this.focus_handle.focus(window);
                    this.selected_row = Some((label, val_for_right_click.clone()));
                    cx.notify();
                }),
            )
            .child(div().flex_shrink_0().w(px(110.0)).text_xs().text_color(theme.muted_foreground).child(label))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_end()
                    .overflow_hidden()
                    .min_w_0()
                    .text_xs()
                    .font_family(font_family.to_string())
                    .text_color(theme.foreground)
                    .child(value),
            )
    }

    fn render_section_header(&self, label: &'static str, theme: &gpui_component::Theme) -> impl IntoElement {
        crate::ui::style::panel_section_header(label, theme)
    }

    fn format_unix_time(&self, timestamp: i64) -> String {
        if !(0..=253402300799).contains(&timestamp) {
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
        let arr: [u8; 2] = bytes[0..2].try_into().expect("2-byte slice");
        let val = if is_big_endian { u16::from_be_bytes(arr) } else { u16::from_le_bytes(arr) };
        hex16 = format!("0x{:04X}", val);
    }

    if bytes.len() >= 4 {
        let arr: [u8; 4] = bytes[0..4].try_into().expect("4-byte slice");
        let val = if is_big_endian { u32::from_be_bytes(arr) } else { u32::from_le_bytes(arr) };
        hex32 = format!("0x{:08X}", val);
    }

    if bytes.len() >= 8 {
        let arr: [u8; 8] = bytes[0..8].try_into().expect("8-byte slice");
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
            let arr: [u8; 2] = bytes_at_cursor[0..2].try_into().expect("2-byte slice");
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
            let arr: [u8; 4] = bytes_at_cursor[0..4].try_into().expect("4-byte slice");
            let i32_val_raw = if is_big_endian { i32::from_be_bytes(arr) } else { i32::from_le_bytes(arr) };
            let u32_val_raw = if is_big_endian { u32::from_be_bytes(arr) } else { u32::from_le_bytes(arr) };
            let f32_val_raw = if is_big_endian { f32::from_be_bytes(arr) } else { f32::from_le_bytes(arr) };
            i32_val = format!("{}", i32_val_raw);
            u32_val = format!("{}", u32_val_raw);
            f32_val = format!("{:.6}", f32_val_raw);
            unix_time_32 = self.format_unix_time(i32_val_raw as i64);
        }

        if bytes_at_cursor.len() >= 8 {
            let arr: [u8; 8] = bytes_at_cursor[0..8].try_into().expect("8-byte slice");
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
            if expected_len > 0
                && expected_len <= bytes_at_cursor.len()
                && let Ok(s) = std::str::from_utf8(&bytes_at_cursor[0..expected_len])
                && let Some(c) = s.chars().next()
            {
                if !c.is_control() {
                    utf8_val = format!("'{}'", c);
                } else {
                    utf8_val = ".".to_string();
                }
                decoded = true;
            }
            if !decoded {
                utf8_val = ".".to_string();
            }
        }

        let (current_encoding, current_enc_val) = if let Some(ed) = &self.editor {
            let ed = ed.read(cx);
            let enc = ed.encoding;
            if !matches!(enc, Encoding::Ascii | Encoding::Utf8 | Encoding::Utf16Le | Encoding::Utf16Be) {
                let mut val = ".".to_string();
                if !bytes_at_cursor.is_empty()
                    && let Some((c, _)) = enc.decode_char_at(&bytes_at_cursor, 0)
                {
                    val = format!("'{}'", c);
                }
                (Some(enc.label()), Some(val))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        let is_focused = self.focus_handle.is_focused(window);

        let endian_controls = h_flex()
            .items_center()
            .gap_1()
            .child(
                Button::new("endian_little")
                    .label("LE")
                    .ghost()
                    .selected(!is_big_endian)
                    .with_size(gpui_component::Size::XSmall)
                    .tooltip("Little Endian")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if this.is_big_endian {
                            this.is_big_endian = false;
                            cx.notify();
                        }
                    })),
            )
            .child(
                Button::new("endian_big")
                    .label("BE")
                    .ghost()
                    .selected(is_big_endian)
                    .with_size(gpui_component::Size::XSmall)
                    .tooltip("Big Endian")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if !this.is_big_endian {
                            this.is_big_endian = true;
                            cx.notify();
                        }
                    })),
            );

        let view = cx.entity().clone();
        let context_view = view.clone();
        let context_focus_handle = self.focus_handle.clone();

        let font_family = cx.global::<Appearance>().font_family.clone();
        let header = crate::ui::style::panel_header("DATA INSPECTOR", is_focused, theme, None, Some(endian_controls.into_any_element()));
        let container = crate::ui::style::panel_container(is_focused, theme);

        container
            .id("data-inspector")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::copy_value))
            .on_action(cx.listener(Self::copy_field_name))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, _| {
                    this.focus_handle.focus(window);
                }),
            )
            .context_menu(move |menu, _window, cx| {
                let selected = {
                    let this = context_view.read(cx);
                    this.selected_row.clone()
                };
                let Some((label, value)) = selected else {
                    return menu;
                };
                menu.action_context(context_focus_handle.clone())
                    .menu_with_icon(format!("Copy Value ({})", value), IconName::Copy, Box::new(CopyValue { value }))
                    .menu_with_icon(
                        format!("Copy Field Name ({})", label),
                        IconName::Copy,
                        Box::new(CopyFieldName { name: label.to_string() }),
                    )
            })
            .child(header)
            .child(
                v_flex()
                    .size_full()
                    .min_w_0()
                    .overflow_y_scrollbar()
                    .p_2()
                    .child(self.render_section_header("HEXADECIMAL", theme))
                    .child(self.render_row("Hex (1 byte)", hex8_val, &font_family, &view, window, theme))
                    .child(self.render_row("Hex (2 bytes)", hex16_val, &font_family, &view, window, theme))
                    .child(self.render_row("Hex (4 bytes)", hex32_val, &font_family, &view, window, theme))
                    .child(self.render_row("Hex (8 bytes)", hex64_val, &font_family, &view, window, theme))
                    .child(self.render_section_header("INTEGERS", theme))
                    .child(self.render_row("Int8", i8_val, &font_family, &view, window, theme))
                    .child(self.render_row("UInt8", u8_val, &font_family, &view, window, theme))
                    .child(self.render_row("Int16", i16_val, &font_family, &view, window, theme))
                    .child(self.render_row("UInt16", u16_val, &font_family, &view, window, theme))
                    .child(self.render_row("Int32", i32_val, &font_family, &view, window, theme))
                    .child(self.render_row("UInt32", u32_val, &font_family, &view, window, theme))
                    .child(self.render_row("Int64", i64_val, &font_family, &view, window, theme))
                    .child(self.render_row("UInt64", u64_val, &font_family, &view, window, theme))
                    .child(self.render_section_header("FLOATS", theme))
                    .child(self.render_row("Float32", f32_val, &font_family, &view, window, theme))
                    .child(self.render_row("Float64", f64_val, &font_family, &view, window, theme))
                    .child(self.render_section_header("TIME", theme))
                    .child(self.render_row("Unix Time (32-bit)", unix_time_32, &font_family, &view, window, theme))
                    .child(self.render_row("Unix Time (64-bit)", unix_time_64, &font_family, &view, window, theme))
                    .child(self.render_section_header("TEXT", theme))
                    .child(self.render_row("ASCII", ascii_val, &font_family, &view, window, theme))
                    .child(self.render_row("UTF-8", utf8_val, &font_family, &view, window, theme))
                    .child(self.render_row("UTF-16", utf16_val, &font_family, &view, window, theme))
                    .when_some(current_encoding.zip(current_enc_val), |parent, (enc_label, enc_val)| {
                        parent.child(self.render_row(enc_label, enc_val, &font_family, &view, window, theme))
                    }),
            )
    }
}

impl Focusable for DataInspector {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
