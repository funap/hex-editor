use crate::actions::*;
use crate::app_state::InsertModeState;
use crate::core::appearance::Appearance;
use crate::core::editor::Editor;
use crate::ui::icon::IconName;
use crate::ui::style::{decode_uint_value, format_binary_repr, format_size_friendly, format_text_repr};
use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::{ContextMenuExt as _, DropdownMenu as _};
use gpui_component::{ActiveTheme, Icon, Sizable as _, Size, StyledExt};

pub enum StatusBarEvent {
    #[allow(dead_code)]
    ToggleLeftPanel,
}

pub struct StatusBar {
    active_editor: Option<WeakEntity<Editor>>,
    editor_subscription: Option<Subscription>,
    _insert_mode_subscription: Subscription,
}

impl EventEmitter<StatusBarEvent> for StatusBar {}

impl StatusBar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let _insert_mode_subscription = cx.observe_global::<InsertModeState>(|_, cx| {
            cx.notify();
        });
        Self {
            active_editor: None,
            editor_subscription: None,
            _insert_mode_subscription,
        }
    }

    pub fn set_active_editor(&mut self, editor: Option<Entity<Editor>>, cx: &mut Context<Self>) {
        self.editor_subscription = None;
        self.active_editor = editor.as_ref().map(|e| e.downgrade());
        if let Some(editor) = editor {
            self.editor_subscription = Some(cx.observe(&editor, |_, _, cx| {
                cx.notify();
            }));
        }
        cx.notify();
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

        let (current_byte_info, raw_byte_val) = if let Some(editor) = &active_editor {
            let editor = editor.read(cx);
            let doc = editor.document.read().ok();

            let target = if editor.has_selection() {
                if let Some(range) = editor.selected_range_or_cursor() {
                    let len = range.len();
                    if len <= 8 { Some((range.start, len)) } else { None }
                } else {
                    Some((editor.cursor_offset, editor.group_size.byte_count().min(8)))
                }
            } else {
                Some((editor.cursor_offset, editor.group_size.byte_count().min(8)))
            };

            if let (Some(d), Some((offset, len))) = (doc, target) {
                let slice = d.buffer.get_range(offset, len);
                if slice.len() == len && len > 0 {
                    let (uint_val, hex_str) = decode_uint_value(slice, editor.is_big_endian);
                    let text_repr = format_text_repr(slice, editor.encoding);
                    let bin_repr = format_binary_repr(uint_val, len);
                    let display_str = format!("Val: {} ({}, {}, {})", hex_str, uint_val, text_repr, bin_repr);
                    let copy_str = format!("{} ({})", hex_str, uint_val);
                    (Some(display_str), Some(copy_str))
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        let (position_text, position_copy_val) = if let Some(editor) = &active_editor {
            let editor = editor.read(cx);
            if editor.has_selection() {
                if let Some(range) = editor.selected_range_or_cursor() {
                    let len = range.len();
                    let end_inclusive = range.end.saturating_sub(1);
                    let byte_word = if len == 1 { "byte" } else { "bytes" };
                    let text = format!("0x{:X} - 0x{:X} (0x{:X} | {} {})", range.start, end_inclusive, len, len, byte_word);
                    let copy_val = format!("0x{:X}..0x{:X}", range.start, end_inclusive);
                    (text, copy_val)
                } else {
                    (
                        format!("0x{:X} ({})", editor.cursor_offset, editor.cursor_offset),
                        format!("0x{:X}", editor.cursor_offset),
                    )
                }
            } else {
                (
                    format!("0x{:X} ({})", editor.cursor_offset, editor.cursor_offset),
                    format!("0x{:X}", editor.cursor_offset),
                )
            }
        } else {
            ("0x0 (0)".to_string(), "0x0".to_string())
        };

        let is_read_only = if let Some(editor) = &active_editor {
            editor.read(cx).is_read_only()
        } else {
            false
        };
        let file_mode_label = if is_read_only { "Read-only" } else { "Writable" };
        let file_mode_icon = if is_read_only { IconName::Eye } else { IconName::File };
        let insert_mode = InsertModeState::is_enabled(cx);
        let edit_mode_label = if insert_mode { "Insert" } else { "Overwrite" };

        let radix_badge_info = if let Some(editor) = &active_editor {
            let editor = editor.read(cx);
            Some(editor.radix.short_label())
        } else {
            None
        };

        let grouping_badge_info = if let Some(editor) = &active_editor {
            let editor = editor.read(cx);
            let group_str = editor.group_size.short_label();
            let endian_str = if editor.group_size.byte_count() > 1 {
                if editor.is_big_endian { " BE" } else { " LE" }
            } else {
                ""
            };
            Some(format!("{}{}", group_str, endian_str))
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
            .px_3()
            .context_menu(move |menu, window, cx| {
                menu.menu_with_icon("Find...", IconName::Search, Box::new(ToggleSearch))
                    .menu_with_icon("Toggle Read-only", IconName::Eye, Box::new(ToggleReadOnly))
                    .separator()
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
                    .submenu("Encoding", window, cx, move |menu, _window, _cx| {
                        menu.menu("ASCII", Box::new(SetEncodingAscii))
                            .menu("UTF-8", Box::new(SetEncodingUtf8))
                            .menu("UTF-16 LE", Box::new(SetEncodingUtf16Le))
                            .menu("UTF-16 BE", Box::new(SetEncodingUtf16Be))
                    })
                    .separator()
                    .menu("Toggle Left Panel", Box::new(ToggleLeftPanel))
            })
            .child(
                // Left side: cursor position / selection range, current byte
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(
                        div()
                            .id("status-position")
                            .font_family("Courier New")
                            .font_medium()
                            .text_color(theme.foreground)
                            .cursor_pointer()
                            .hover(|s| s.text_color(theme.accent))
                            .tooltip(|_window, cx| cx.new(|_| gpui_component::tooltip::Tooltip::new("Click to copy position")).into())
                            .on_mouse_down(MouseButton::Left, {
                                let copy_str = position_copy_val.clone();
                                move |_, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_str.clone()));
                                }
                            })
                            .context_menu({
                                let offset_hex = format!("0x{:X}", cursor_offset);
                                let offset_dec = format!("{}", cursor_offset);
                                let pos_str = position_copy_val.clone();
                                move |menu, _window, _cx| {
                                    let off_h = offset_hex.clone();
                                    let off_d = offset_dec.clone();
                                    let p_str = pos_str.clone();
                                    menu.menu_with_icon("Go to Beginning", IconName::ChevronUp, Box::new(GoToBeginning))
                                        .menu_with_icon("Go to End", IconName::ChevronDown, Box::new(GoToEnd))
                                        .separator()
                                        .menu(format!("Copy Position ({})", p_str), Box::new(Copy))
                                        .menu(format!("Copy Hex Offset ({})", off_h), Box::new(Copy))
                                        .menu(format!("Copy Dec Offset ({})", off_d), Box::new(Copy))
                                }
                            })
                            .child(position_text),
                    )
                    .when_some(current_byte_info, |el, val_str| {
                        let raw_copy = raw_byte_val.unwrap_or_default();
                        el.child(div().w_px().h_3().bg(theme.border)).child(
                            div()
                                .id("status-val-pill")
                                .font_family("Courier New")
                                .text_color(theme.muted_foreground)
                                .cursor_pointer()
                                .hover(|s| s.text_color(theme.foreground))
                                .tooltip(|_window, cx| cx.new(|_| gpui_component::tooltip::Tooltip::new("Click to copy value")).into())
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    if !raw_copy.is_empty() {
                                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(raw_copy.clone()));
                                    }
                                })
                                .child(val_str),
                        )
                    }),
            )
            .child(
                // Right side: document state, file size, format dropdown, encoding dropdown
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(
                        div()
                            .id("status-size-pill")
                            .px_2()
                            .py_0p5()
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.muted.opacity(0.4)))
                            .tooltip(|_window, cx| cx.new(|_| gpui_component::tooltip::Tooltip::new("Click to copy exact file size")).into())
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(total_size.to_string()));
                            })
                            .child(
                                div()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("Size: {}", format_size_friendly(total_size))),
                            ),
                    )
                    .when(active_editor.is_some(), |el| {
                        el.child(
                            Button::new("status-file-mode-btn")
                                .icon(file_mode_icon)
                                .label(file_mode_label)
                                .ghost()
                                .with_size(Size::XSmall)
                                .tooltip(if is_read_only {
                                    "Read-only. Click to make this file writable"
                                } else {
                                    "Writable. Click to make this file read-only"
                                })
                                .on_click(cx.listener(|_, _, window, cx| {
                                    window.dispatch_action(Box::new(ToggleReadOnly), cx);
                                })),
                        )
                    })
                    .when(active_editor.is_some(), |el| {
                        el.child(div().w_px().h_3().bg(theme.border)).child(
                            Button::new("status-edit-mode-btn")
                                .label(edit_mode_label)
                                .ghost()
                                .with_size(Size::XSmall)
                                .tooltip(if insert_mode {
                                    "Insert Mode. Click to switch to Overwrite Mode"
                                } else {
                                    "Overwrite Mode. Click to switch to Insert Mode"
                                })
                                .on_click(cx.listener(|_, _, window, cx| {
                                    window.dispatch_action(Box::new(ToggleInsertMode), cx);
                                })),
                        )
                    })
                    .when_some(radix_badge_info, |el, radix_label| {
                        el.child(div().w_px().h_3().bg(theme.border)).child(
                            Button::new("status-radix-btn")
                                .label(radix_label)
                                .ghost()
                                .with_size(Size::XSmall)
                                .tooltip("Click to change Radix (Hex, Dec, Oct, Bin)")
                                .dropdown_menu_with_anchor(Corner::BottomRight, move |menu, _window, _cx| {
                                    menu.menu("Hexadecimal (16)", Box::new(SetRadixHex))
                                        .menu("Decimal (10)", Box::new(SetRadixDec))
                                        .menu("Octal (8)", Box::new(SetRadixOct))
                                        .menu("Binary (2)", Box::new(SetRadixBin))
                                }),
                        )
                    })
                    .when_some(grouping_badge_info, |el, grouping_label| {
                        el.child(div().w_px().h_3().bg(theme.border)).child(
                            Button::new("status-grouping-btn")
                                .label(grouping_label)
                                .ghost()
                                .with_size(Size::XSmall)
                                .tooltip("Click to change Grouping & Byte Order")
                                .dropdown_menu_with_anchor(Corner::BottomRight, move |menu, window, cx| {
                                    menu.menu("1 Byte (8-bit)", Box::new(SetGroupSize1))
                                        .menu("2 Bytes (16-bit)", Box::new(SetGroupSize2))
                                        .menu("4 Bytes (32-bit)", Box::new(SetGroupSize4))
                                        .menu("8 Bytes (64-bit)", Box::new(SetGroupSize8))
                                        .separator()
                                        .submenu("Byte Order", window, cx, move |menu, _window, _cx| {
                                            menu.menu("Little Endian", Box::new(SetByteOrderLittleEndian))
                                                .menu("Big Endian", Box::new(SetByteOrderBigEndian))
                                        })
                                }),
                        )
                    })
                    .when_some(encoding_info, |el, enc| {
                        el.child(div().w_px().h_3().bg(theme.border)).child(
                            Button::new("status-encoding-btn")
                                .label(enc)
                                .ghost()
                                .with_size(Size::XSmall)
                                .tooltip("Click to change Text Encoding")
                                .dropdown_menu_with_anchor(Corner::BottomRight, move |menu, _window, _cx| {
                                    menu.menu("ASCII", Box::new(SetEncodingAscii))
                                        .menu("UTF-8", Box::new(SetEncodingUtf8))
                                        .menu("UTF-16 LE", Box::new(SetEncodingUtf16Le))
                                        .menu("UTF-16 BE", Box::new(SetEncodingUtf16Be))
                                }),
                        )
                    }),
            )
    }
}
