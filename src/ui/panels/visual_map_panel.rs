use crate::core::appearance::Appearance;
use crate::core::editor::Editor;
use crate::ui::icon::IconName;
use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::dock::{Panel, PanelEvent};
use gpui_component::scroll::*;
use gpui_component::{ActiveTheme, Icon, PixelsExt, Sizable, Size, StyledExt, h_flex, v_flex};
use std::cell::RefCell;
use std::cmp;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

const WIDTH_REPEAT_INITIAL_DELAY: Duration = Duration::from_millis(350);
const WIDTH_REPEAT_MIN_INTERVAL: Duration = Duration::from_millis(15);
const WIDTH_REPEAT_MED_INTERVAL: Duration = Duration::from_millis(30);
const WIDTH_REPEAT_BASE_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum ColorMode {
    Grayscale,
    DataCategory,
    Rainbow,
    Entropy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteCategory {
    Null,
    Control,
    Space,
    Ascii,
    Extended,
}

impl ByteCategory {
    pub fn of(byte: u8) -> Self {
        match byte {
            0 => ByteCategory::Null,
            1..=31 | 127 => ByteCategory::Control,
            32 => ByteCategory::Space,
            33..=126 => ByteCategory::Ascii,
            _ => ByteCategory::Extended,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ByteCategory::Null => "Null (00)",
            ByteCategory::Control => "Control",
            ByteCategory::Space => "Space (20)",
            ByteCategory::Ascii => "ASCII",
            ByteCategory::Extended => "Extended",
        }
    }

    pub fn color(self, theme: &gpui_component::Theme) -> Hsla {
        match self {
            ByteCategory::Null => theme.muted_foreground.opacity(0.4),
            ByteCategory::Control => theme.red.opacity(0.85),
            ByteCategory::Space => theme.blue.opacity(0.75),
            ByteCategory::Ascii => theme.green.opacity(0.9),
            ByteCategory::Extended => theme.accent.opacity(0.85),
        }
    }
}

pub type CachedImageKey = (usize, usize, usize, ColorMode, usize, usize, usize, f32, f32, u32);
pub type CachedImage = (Arc<RenderImage>, CachedImageKey);

pub struct VisualMapPanel {
    pub editor: Option<Entity<Editor>>,
    focus_handle: FocusHandle,
    cols: usize,
    pixel_size: usize,
    scroll_offset: usize,
    scroll_remainder: f32,
    scroll_handle: ScrollHandle,
    color_mode: ColorMode,
    entropy_window: usize,
    hovered_info: Option<(usize, u8)>,
    last_bounds: std::cell::Cell<Option<Bounds<Pixels>>>,
    cached_image: RefCell<Option<CachedImage>>,
    is_dragging: bool,
    _editor_subscription: Option<Subscription>,
    _width_repeat_task: Option<Task<()>>,
}

impl EventEmitter<PanelEvent> for VisualMapPanel {}

impl VisualMapPanel {
    pub fn new(editor: Option<Entity<Editor>>, cx: &mut Context<Self>) -> Self {
        let _editor_subscription = editor.as_ref().map(|ed| {
            cx.observe(ed, |_, _, cx| {
                cx.notify();
            })
        });

        Self {
            editor,
            focus_handle: cx.focus_handle(),
            cols: 64,
            pixel_size: 2,
            scroll_offset: 0,
            scroll_remainder: 0.0,
            scroll_handle: ScrollHandle::new(),
            color_mode: ColorMode::DataCategory,
            entropy_window: 256,
            hovered_info: None,
            last_bounds: std::cell::Cell::new(None),
            cached_image: RefCell::new(None),
            is_dragging: false,
            _editor_subscription,
            _width_repeat_task: None,
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
        self.cached_image.borrow_mut().take();
        cx.notify();
    }

    fn file_path(&self, cx: &App) -> Option<std::path::PathBuf> {
        self.editor
            .as_ref()
            .map(|ed| ed.read(cx).document.read().expect("document read lock").path().to_path_buf())
    }

    fn buffer_len(&self, cx: &App) -> usize {
        self.editor
            .as_ref()
            .map(|ed| ed.read(cx).document.read().expect("document read lock").buffer.len())
            .unwrap_or(0)
    }

    fn state_id(&self, cx: &App) -> usize {
        self.editor
            .as_ref()
            .map(|ed| ed.read(cx).document.read().expect("document read lock").history.state_id())
            .unwrap_or(0)
    }

    pub fn scroll_to_cursor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &self.editor else { return };
        let cursor_offset = editor.read(cx).cursor_offset;
        let buffer_len = self.buffer_len(cx);
        if buffer_len == 0 {
            return;
        }
        let total_rows = buffer_len.div_ceil(self.cols);
        let cursor_row = cursor_offset / self.cols;
        let visible_rows = if let Some(bounds) = self.last_bounds.get() {
            (bounds.size.height.as_f32() / self.pixel_size as f32).ceil() as usize
        } else {
            30
        };
        let target_scroll = cursor_row.saturating_sub(visible_rows / 2);
        self.scroll_offset = cmp::min(target_scroll, total_rows.saturating_sub(1));
        self.update_scrollbar(cx);
        cx.notify();
    }

    fn update_scrollbar(&mut self, cx: &mut Context<Self>) {
        let buffer_len = self.buffer_len(cx);
        let total_rows = buffer_len.div_ceil(self.cols);
        let pixel_size_px = px(self.pixel_size as f32);
        self.scroll_offset = self.scroll_offset.min(total_rows.saturating_sub(1));
        self.scroll_handle.set_offset(point(px(0.), -(self.scroll_offset as f32 * pixel_size_px)));
    }

    fn on_scroll_wheel(&mut self, event: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.modifiers.platform || event.modifiers.control {
            let delta = event.delta.pixel_delta(px(16.0)).y.as_f32();
            if delta.abs() > 0.5 {
                let current = self.pixel_size;
                let new_size = if delta > 0.0 {
                    match current {
                        1 => 2,
                        2 => 4,
                        4 => 8,
                        _ => 8,
                    }
                } else {
                    match current {
                        8 => 4,
                        4 => 2,
                        2 => 1,
                        _ => 1,
                    }
                };
                if new_size != self.pixel_size {
                    self.pixel_size = new_size;
                    self.update_scrollbar(cx);
                    self.cached_image.borrow_mut().take();
                    cx.notify();
                }
            }
            return;
        }

        let pixel_size_px = px(self.pixel_size as f32);
        let buffer_len = self.buffer_len(cx);
        if buffer_len == 0 {
            return;
        }
        let total_rows = buffer_len.div_ceil(self.cols);

        let max_offset = total_rows.saturating_sub(1).max(0) as i32;
        let delta_y_pixels = event.delta.pixel_delta(pixel_size_px).y.as_f32();
        let total_delta = delta_y_pixels + self.scroll_remainder;
        let delta_rows = (total_delta / pixel_size_px.as_f32()) as i32;
        self.scroll_remainder = total_delta - (delta_rows as f32 * pixel_size_px.as_f32());

        let new_scroll_offset = self.scroll_offset as i32 - delta_rows;

        self.scroll_offset = cmp::max(0, cmp::min(new_scroll_offset, max_offset)) as usize;
        self.scroll_handle.set_offset(point(px(0.), -(self.scroll_offset as f32 * pixel_size_px)));
        cx.notify();
    }

    fn offset_from_point_clamped(&self, point: Point<Pixels>, cx: &App) -> Option<usize> {
        let bounds = self.last_bounds.get()?;
        let rel_x = (point.x - bounds.left()).max(px(0.)).min(bounds.size.width - px(1.));
        let rel_y = (point.y - bounds.top()).max(px(0.)).min(bounds.size.height - px(1.));

        let col = (rel_x.as_f32() / self.pixel_size as f32) as usize;
        let col = col.min(self.cols.saturating_sub(1));
        let row = (rel_y.as_f32() / self.pixel_size as f32) as usize + self.scroll_offset;
        let offset = row * self.cols + col;

        let buffer_len = self.buffer_len(cx);
        if buffer_len == 0 {
            return Some(0);
        }
        Some(offset.min(buffer_len.saturating_sub(1)))
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window);
        self.is_dragging = true;
        if let Some(offset) = self.offset_from_point_clamped(event.position, cx)
            && let Some(editor) = &self.editor
        {
            editor.update(cx, |editor, cx| {
                editor.set_cursor_offset(offset);
                cx.notify();
            });
        }
    }

    fn on_mouse_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_dragging = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let pixel_size_px = px(self.pixel_size as f32);
        let handle_y = self.scroll_handle.offset().y;
        let handle_row = ((-handle_y).max(px(0.)) / pixel_size_px).round() as usize;
        let buffer_len = self.buffer_len(cx);
        if buffer_len == 0 {
            return;
        }
        let total_rows = buffer_len.div_ceil(self.cols);
        if handle_row != self.scroll_offset {
            self.scroll_offset = handle_row.min(total_rows.saturating_sub(1));
            cx.notify();
        }

        if self.is_dragging
            && let Some(offset) = self.offset_from_point_clamped(event.position, cx)
            && let Some(editor) = &self.editor
        {
            editor.update(cx, |editor, cx| {
                editor.set_cursor_offset(offset);
                cx.notify();
            });
        }

        let mut hovered = None;
        if let Some(bounds) = self.last_bounds.get()
            && bounds.contains(&event.position)
        {
            let rel_x = event.position.x - bounds.left();
            let rel_y = event.position.y - bounds.top();

            let col = (rel_x.as_f32() / self.pixel_size as f32) as usize;
            if col < self.cols {
                let row = (rel_y.as_f32() / self.pixel_size as f32) as usize + self.scroll_offset;
                let offset = row * self.cols + col;

                if offset < buffer_len
                    && let Some(editor) = &self.editor
                {
                    let doc = editor.read(cx).document.read().expect("document read lock");
                    let byte = doc.buffer.get_range(offset, 1)[0];
                    hovered = Some((offset, byte));
                }
            }
        }

        if self.hovered_info != hovered {
            self.hovered_info = hovered;
            cx.notify();
        }
    }

    fn increment_width(&mut self, cx: &mut Context<Self>) {
        if self.cols < 4096 {
            self.cols = cmp::min(4096, self.cols.saturating_add(1));
            self.update_scrollbar(cx);
            self.cached_image.borrow_mut().take();
            cx.notify();
        }
    }

    fn decrement_width(&mut self, cx: &mut Context<Self>) {
        if self.cols > 1 {
            self.cols = cmp::max(1, self.cols.saturating_sub(1));
            self.update_scrollbar(cx);
            self.cached_image.borrow_mut().take();
            cx.notify();
        }
    }

    fn start_width_repeat(&mut self, is_increment: bool, cx: &mut Context<Self>) {
        if is_increment {
            self.increment_width(cx);
        } else {
            self.decrement_width(cx);
        }

        self._width_repeat_task = Some(cx.spawn(async move |this, cx| {
            Timer::after(WIDTH_REPEAT_INITIAL_DELAY).await;
            let mut count = 0;
            loop {
                let interval = if count < 10 {
                    WIDTH_REPEAT_BASE_INTERVAL
                } else if count < 30 {
                    WIDTH_REPEAT_MED_INTERVAL
                } else {
                    WIDTH_REPEAT_MIN_INTERVAL
                };

                let should_continue = this
                    .update(cx, |this, cx| {
                        if is_increment {
                            this.increment_width(cx);
                            this.cols < 4096
                        } else {
                            this.decrement_width(cx);
                            this.cols > 1
                        }
                    })
                    .unwrap_or(false);

                if !should_continue {
                    break;
                }

                count += 1;
                Timer::after(interval).await;
            }
        }));
    }

    fn stop_width_repeat(&mut self) {
        self._width_repeat_task = None;
    }

    fn render_width_section(&self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted_color = theme.muted_foreground;
        let font_family = cx.global::<Appearance>().font_family.clone();
        h_flex()
            .justify_between()
            .items_center()
            .gap_2()
            .child(
                h_flex()
                    .items_center()
                    .gap_1p5()
                    .child(Icon::new(IconName::SlidersHorizontal).size(px(13.0)).text_color(muted_color))
                    .child(div().text_xs().font_medium().text_color(muted_color).child("Width")),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new("dec_w")
                            .label("-")
                            .ghost()
                            .with_size(Size::XSmall)
                            .tooltip("Decrease width")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.start_width_repeat(false, cx);
                                }),
                            )
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, _| {
                                    this.stop_width_repeat();
                                }),
                            )
                            .on_mouse_up_out(
                                MouseButton::Left,
                                cx.listener(|this, _, _, _| {
                                    this.stop_width_repeat();
                                }),
                            ),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded_sm()
                            .bg(theme.muted.opacity(0.4))
                            .font_family(font_family)
                            .text_xs()
                            .font_semibold()
                            .text_color(theme.foreground)
                            .child(format!("{} B", self.cols)),
                    )
                    .child(
                        Button::new("inc_w")
                            .label("+")
                            .ghost()
                            .with_size(Size::XSmall)
                            .tooltip("Increase width")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.start_width_repeat(true, cx);
                                }),
                            )
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, _| {
                                    this.stop_width_repeat();
                                }),
                            )
                            .on_mouse_up_out(
                                MouseButton::Left,
                                cx.listener(|this, _, _, _| {
                                    this.stop_width_repeat();
                                }),
                            ),
                    ),
            )
    }

    fn render_scale_section(&self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted_color = theme.muted_foreground;
        let pixel_button = |preset: usize, label: &'static str, cx: &mut Context<Self>| {
            let is_selected = self.pixel_size == preset;
            let mut btn = Button::new(("p_preset", preset)).label(label).with_size(Size::XSmall);
            if is_selected {
                btn = btn.primary();
            } else {
                btn = btn.ghost();
            }
            btn.on_click(cx.listener(move |this, _, _, cx| {
                this.pixel_size = preset;
                this.update_scrollbar(cx);
                this.cached_image.borrow_mut().take();
                cx.notify();
            }))
        };

        h_flex()
            .justify_between()
            .items_center()
            .gap_2()
            .child(
                h_flex()
                    .items_center()
                    .gap_1p5()
                    .child(Icon::new(IconName::Grid2x2).size(px(13.0)).text_color(muted_color))
                    .child(div().text_xs().font_medium().text_color(muted_color).child("Scale")),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(pixel_button(1, "x1", cx))
                    .child(pixel_button(2, "x2", cx))
                    .child(pixel_button(4, "x4", cx))
                    .child(pixel_button(8, "x8", cx)),
            )
    }

    fn render_palette_section(&self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted_color = theme.muted_foreground;
        let color_button = |mode: ColorMode, label: &'static str, id_str: &'static str, cx: &mut Context<Self>| {
            let is_selected = self.color_mode == mode;
            let mut btn = Button::new(id_str).label(label).with_size(Size::XSmall);
            if is_selected {
                btn = btn.primary();
            } else {
                btn = btn.ghost();
            }
            btn.on_click(cx.listener(move |this, _, _, cx| {
                this.color_mode = mode;
                this.cached_image.borrow_mut().take();
                cx.notify();
            }))
        };

        h_flex()
            .justify_between()
            .items_center()
            .gap_2()
            .child(
                h_flex()
                    .items_center()
                    .gap_1p5()
                    .child(Icon::new(IconName::Palette).size(px(13.0)).text_color(muted_color))
                    .child(div().text_xs().font_medium().text_color(muted_color).child("Palette")),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_1()
                    .child(color_button(ColorMode::Grayscale, "Gray", "c_gray", cx))
                    .child(color_button(ColorMode::DataCategory, "Type", "c_type", cx))
                    .child(color_button(ColorMode::Rainbow, "Rainbow", "c_rainbow", cx))
                    .child(color_button(ColorMode::Entropy, "Entropy", "c_entropy", cx)),
            )
    }

    fn render_entropy_window_section(&self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted_color = theme.muted_foreground;
        let window_button = |preset: usize, label: &'static str, cx: &mut Context<Self>| {
            let is_selected = self.entropy_window == preset;
            let mut btn = Button::new(("w_preset", preset)).label(label).with_size(Size::XSmall);
            if is_selected {
                btn = btn.primary();
            } else {
                btn = btn.ghost();
            }
            btn.on_click(cx.listener(move |this, _, _, cx| {
                this.entropy_window = preset;
                this.cached_image.borrow_mut().take();
                cx.notify();
            }))
        };

        h_flex()
            .justify_between()
            .items_center()
            .gap_2()
            .child(
                h_flex()
                    .items_center()
                    .gap_1p5()
                    .child(Icon::new(IconName::SlidersHorizontal).size(px(13.0)).text_color(muted_color))
                    .child(div().text_xs().font_medium().text_color(muted_color).child("Window")),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(window_button(64, "64B", cx))
                    .child(window_button(128, "128B", cx))
                    .child(window_button(256, "256B", cx))
                    .child(window_button(512, "512B", cx))
                    .child(window_button(1024, "1K", cx)),
            )
    }

    fn render_toolbar(&self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut toolbar = v_flex()
            .p_2()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .child(self.render_width_section(theme, cx))
            .child(self.render_scale_section(theme, cx))
            .child(self.render_palette_section(theme, cx));

        if self.color_mode == ColorMode::Entropy {
            toolbar = toolbar.child(self.render_entropy_window_section(theme, cx));
        }

        toolbar
    }

    fn render_legend(&self, theme: &gpui_component::Theme) -> Option<impl IntoElement + use<>> {
        let muted_color = theme.muted_foreground;
        match self.color_mode {
            ColorMode::DataCategory => Some(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .border_t_1()
                    .border_color(theme.border)
                    .bg(theme.muted.opacity(0.15))
                    .text_xs()
                    .items_center()
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(div().w_2().h_2().rounded_sm().bg(ByteCategory::Null.color(theme)))
                            .child(div().text_color(muted_color).child("Null")),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(div().w_2().h_2().rounded_sm().bg(ByteCategory::Control.color(theme)))
                            .child(div().text_color(muted_color).child("Control")),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(div().w_2().h_2().rounded_sm().bg(ByteCategory::Space.color(theme)))
                            .child(div().text_color(muted_color).child("Space")),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(div().w_2().h_2().rounded_sm().bg(ByteCategory::Ascii.color(theme)))
                            .child(div().text_color(muted_color).child("ASCII")),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(div().w_2().h_2().rounded_sm().bg(ByteCategory::Extended.color(theme)))
                            .child(div().text_color(muted_color).child("Extended")),
                    ),
            ),
            ColorMode::Entropy => {
                let color_chip = |norm: f32| {
                    let idx = crate::core::entropy::normalized_to_lut_index(norm);
                    let [r, g, b, _] = crate::core::entropy::entropy_lut()[idx];
                    div().w_2().h_2().rounded_sm().bg(gpui::rgb(u32::from_be_bytes([0, r, g, b])))
                };
                Some(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .px_3()
                        .py_1()
                        .border_t_1()
                        .border_color(theme.border)
                        .bg(theme.muted.opacity(0.15))
                        .text_xs()
                        .items_center()
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .child(color_chip(0.0))
                                .child(div().text_color(muted_color).child("0.0 Uniform")),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .child(color_chip(0.35))
                                .child(div().text_color(muted_color).child("Low")),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .child(color_chip(0.60))
                                .child(div().text_color(muted_color).child("4.8 Text/Code")),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .child(color_chip(0.80))
                                .child(div().text_color(muted_color).child("High")),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .child(color_chip(1.0))
                                .child(div().text_color(muted_color).child("8.0 Packed")),
                        ),
                )
            }
            _ => None,
        }
    }

    fn render_footer(&self, buffer_len: usize, total_rows: usize, theme: &gpui_component::Theme, cx: &App) -> impl IntoElement + use<> {
        let border_color = theme.border;
        let muted_color = theme.muted_foreground;
        let font_family = cx.global::<Appearance>().font_family.clone();

        if let Some((offset, byte)) = self.hovered_info {
            let cat = ByteCategory::of(byte);
            let char_repr = if (32..=126).contains(&byte) {
                format!("'{}'", byte as char)
            } else if byte == 0 {
                "NUL".to_string()
            } else {
                format!("0x{:02X}", byte)
            };

            let display_addr = self.editor.as_ref().map(|ed| ed.read(cx).offset_to_address(offset)).unwrap_or(offset);

            let entropy_info = self.editor.as_ref().and_then(|ed| {
                let doc = ed.read(cx).document.read().ok()?;
                let h = crate::core::entropy::shannon_entropy_at(doc.buffer.data(), offset, self.entropy_window);
                let norm = (h / 8.0) as f32;
                let idx = crate::core::entropy::normalized_to_lut_index(norm);
                let [r, g, b, _] = crate::core::entropy::entropy_lut()[idx];
                let color: Hsla = gpui::rgb(u32::from_be_bytes([0, r, g, b])).into();
                Some((h, norm, color))
            });

            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .p_2()
                .border_t_1()
                .border_color(border_color)
                .text_xs()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .font_family(font_family.clone())
                                .text_color(theme.foreground)
                                .child(format!("0x{:08X}", display_addr)),
                        )
                        .child(div().text_color(muted_color).child("|"))
                        .child(
                            div()
                                .font_family(font_family.clone())
                                .text_color(theme.foreground)
                                .child(format!("0x{:02X} ({})", byte, byte)),
                        )
                        .child(
                            div()
                                .px_1()
                                .py_0p5()
                                .rounded_sm()
                                .bg(theme.muted.opacity(0.4))
                                .font_family(font_family.clone())
                                .text_color(theme.foreground)
                                .child(char_repr),
                        ),
                )
                .child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .children(entropy_info.map(|(h, norm, color)| {
                            let label = crate::core::entropy::entropy_level_label(h);
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded_sm()
                                .bg(color.opacity(0.2))
                                .text_color(color)
                                .font_medium()
                                .child(format!("H: {:.2} ({:.0}%) {}", h, norm * 100.0, label))
                        }))
                        .child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded_sm()
                                .bg(cat.color(theme).opacity(0.2))
                                .text_color(cat.color(theme))
                                .font_medium()
                                .child(cat.label()),
                        ),
                )
        } else {
            let cursor_str = self.editor.as_ref().map(|ed| {
                let cur = ed.read(cx).cursor_address();
                format!("Cursor: 0x{:08X}", cur)
            });

            let window_spec = if self.color_mode == ColorMode::Entropy {
                format!(" | Win: {}B", self.entropy_window)
            } else {
                String::new()
            };

            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .p_2()
                .border_t_1()
                .border_color(border_color)
                .text_xs()
                .text_color(muted_color)
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().child(crate::ui::style::format_size_friendly(buffer_len)))
                        .child(div().child("|"))
                        .child(div().child(format!("{} rows", crate::ui::style::format_with_commas(total_rows))))
                        .children(cursor_str.map(|c| {
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(div().child("|"))
                                .child(div().font_family(font_family.clone()).child(c))
                        })),
                )
                .child(div().child(format!("{} cols @ x{}{}", self.cols, self.pixel_size, window_spec)))
        }
    }
}

impl Focusable for VisualMapPanel {
    fn focus_handle(&self, _cx: &App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for VisualMapPanel {
    fn panel_name(&self) -> &'static str {
        "VisualMapPanel"
    }

    fn title(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let path = self.file_path(cx);
        let name = path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "(untitled)".to_string());
        let title = format!("2D Map: {}", name);
        let theme = cx.theme();

        h_flex().gap_2().items_center().child(title).child(
            div()
                .id("close-icon")
                .cursor_pointer()
                .rounded_md()
                .hover(|style| style.bg(theme.accent).text_color(theme.accent_foreground))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.focus_handle.focus(window);
                    window.dispatch_action(Box::new(crate::actions::CloseActivePanel), cx);
                }))
                .child(Icon::new(IconName::Close).size(px(14.0))),
        )
    }

    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("Visual Map".into())
    }

    fn closable(&self, _cx: &App) -> bool {
        true
    }

    fn zoomable(&self, _cx: &App) -> Option<gpui_component::dock::PanelControl> {
        None
    }

    fn visible(&self, _cx: &App) -> bool {
        true
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }

    fn set_active(&mut self, active: bool, window: &mut Window, _cx: &mut Context<Self>) {
        if active {
            self.focus_handle.focus(window);
        }
    }

    fn set_zoomed(&mut self, _zoomed: bool, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn dump(&self, cx: &App) -> gpui_component::dock::PanelState {
        let mut state = gpui_component::dock::PanelState::new(self);
        let path = self.file_path(cx).unwrap_or_default();
        let map_state = VisualMapPanelState {
            path,
            cols: self.cols,
            pixel_size: self.pixel_size,
            color_mode: self.color_mode,
            entropy_window: self.entropy_window,
        };
        state.info = gpui_component::dock::PanelInfo::panel(serde_json::to_value(map_state).expect("serialize VisualMapPanelState"));
        state
    }
}

fn default_entropy_window() -> usize {
    256
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct VisualMapPanelState {
    pub path: std::path::PathBuf,
    pub cols: usize,
    pub pixel_size: usize,
    pub color_mode: ColorMode,
    #[serde(default = "default_entropy_window")]
    pub entropy_window: usize,
}

impl Render for VisualMapPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let is_focused = self.focus_handle.is_focused(window);

        let header_actions = self.editor.as_ref().map(|_| {
            Button::new("center_cursor")
                .ghost()
                .with_size(Size::XSmall)
                .icon(IconName::ScanEye)
                .tooltip("Center on Cursor")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.scroll_to_cursor(cx);
                }))
                .into_any_element()
        });

        let header = crate::ui::style::panel_header("2D VISUAL MAP", is_focused, &theme, None, header_actions);

        let editor = match &self.editor {
            Some(ed) => ed,
            None => {
                let container = crate::ui::style::panel_container(is_focused, &theme);

                return container
                    .id("visual-map-panel")
                    .track_focus(&self.focus_handle)
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, window, _| {
                            this.focus_handle.focus(window);
                        }),
                    )
                    .child(header)
                    .child(div().flex_1().min_h_0().w_full().overflow_hidden().child(crate::ui::style::panel_empty_state(
                        IconName::Map,
                        "No Active File",
                        Some("Open a binary file to visualize byte distribution"),
                        None,
                        &theme,
                    )));
            }
        };

        let buffer_len = self.buffer_len(cx);
        let state_id = self.state_id(cx);
        let total_rows = buffer_len.div_ceil(self.cols);
        let total_height = total_rows as f32 * self.pixel_size as f32;

        let pixel_size_px = px(self.pixel_size as f32);
        let handle_y = self.scroll_handle.offset().y;
        let handle_row = ((-handle_y).max(px(0.)) / pixel_size_px).round() as usize;
        let synced_offset = handle_row.min(total_rows.saturating_sub(1));
        if self.scroll_offset != synced_offset {
            self.scroll_offset = synced_offset;
        }

        let toolbar = self.render_toolbar(&theme, cx);
        let legend = self.render_legend(&theme);
        let footer = self.render_footer(buffer_len, total_rows, &theme, cx);

        let ed_ref = editor.read(cx);
        let cursor_offset = Some(ed_ref.cursor_offset);
        let selection_range = ed_ref.selection_range();
        let hovered_offset = self.hovered_info.map(|(off, _)| off);

        let canvas = div()
            .flex_1()
            .relative()
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(VisualMapElement {
                panel: cx.entity().downgrade(),
                document: editor.read(cx).document.clone(),
                cols: self.cols,
                pixel_size: self.pixel_size,
                scroll_offset: self.scroll_offset,
                color_mode: self.color_mode,
                entropy_window: self.entropy_window,
                state_id,
                cursor_offset,
                selection_range,
                hovered_offset,
            })
            .child(
                div().absolute().top_0().right_0().bottom_0().w_4().child(
                    Scrollbar::vertical(&self.scroll_handle)
                        .axis(ScrollbarAxis::Vertical)
                        .scroll_size(size(px(0.), px(total_height))),
                ),
            );

        let container = crate::ui::style::panel_container(is_focused, &theme);

        container
            .id("visual-map-panel")
            .track_focus(&self.focus_handle)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, _| {
                    this.focus_handle.focus(window);
                }),
            )
            .child(header)
            .child(toolbar)
            .child(canvas)
            .children(legend)
            .child(footer)
    }
}

struct VisualMapElement {
    panel: WeakEntity<VisualMapPanel>,
    document: Arc<std::sync::RwLock<crate::core::document::Document>>,
    cols: usize,
    pixel_size: usize,
    scroll_offset: usize,
    color_mode: ColorMode,
    entropy_window: usize,
    state_id: usize,
    cursor_offset: Option<usize>,
    selection_range: Option<Range<usize>>,
    hovered_offset: Option<usize>,
}

impl Element for VisualMapElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(panel) = self.panel.upgrade() {
            panel.read(cx).last_bounds.set(Some(bounds));
        }

        let doc = self.document.read().expect("document read lock");
        let buffer = &doc.buffer;
        let buffer_len = buffer.len();

        let theme = cx.theme();

        if buffer_len == 0 {
            return;
        }

        let pixel_size = self.pixel_size as f32;
        let cols = self.cols;

        let total_rows = buffer_len.div_ceil(cols);
        let visible_rows = (bounds.size.height.as_f32() / pixel_size).ceil() as usize + 1;
        let max_visible_cols = (bounds.size.width.as_f32() / pixel_size).ceil() as usize + 1;

        let start_row = self.scroll_offset;
        let end_row = (start_row + visible_rows).min(total_rows);

        let scale_factor = window.scale_factor();
        let cell_width = (pixel_size * scale_factor).round().max(1.0) as usize;
        let cell_height = (pixel_size * scale_factor).round().max(1.0) as usize;

        let physical_width = cols * cell_width;
        let physical_height = visible_rows * cell_height;

        let mut cached_image = None;
        if let Some(panel) = self.panel.upgrade() {
            let panel_ref = panel.read(cx);
            let cache_key = (
                self.cols,
                self.pixel_size,
                self.scroll_offset,
                self.color_mode,
                self.entropy_window,
                buffer_len,
                self.state_id,
                bounds.size.width.as_f32(),
                bounds.size.height.as_f32(),
                scale_factor.to_bits(),
            );

            let mut cache = panel_ref.cached_image.borrow_mut();
            if let Some((img, key)) = &*cache
                && key == &cache_key
            {
                cached_image = Some(img.clone());
            }

            if cached_image.is_none() && physical_width > 0 && physical_height > 0 {
                let mut pixels = vec![0u8; physical_width * physical_height * 4];

                if self.color_mode == ColorMode::Entropy {
                    let visible_start_offset = start_row * cols;
                    let visible_end_offset = cmp::min(buffer_len, end_row * cols);

                    let entropies = crate::core::entropy::compute_sliding_entropy(buffer.data(), visible_start_offset, visible_end_offset, self.entropy_window);
                    let lut = crate::core::entropy::entropy_bgra_lut();

                    for r in start_row..end_row {
                        let row_y = r - start_row;
                        let row_offset = r * self.cols;
                        let chunk_len = cmp::min(self.cols, buffer_len.saturating_sub(row_offset));
                        let chunk_len = cmp::min(chunk_len, max_visible_cols);
                        if chunk_len == 0 {
                            break;
                        }

                        for c in 0..chunk_len {
                            let byte_idx = row_offset + c;
                            let [b_val, g_val, r_val, a_val] =
                                if byte_idx >= visible_start_offset && byte_idx < visible_end_offset && (byte_idx - visible_start_offset) < entropies.len() {
                                    let norm = entropies[byte_idx - visible_start_offset];
                                    let lut_idx = crate::core::entropy::normalized_to_lut_index(norm);
                                    lut[lut_idx]
                                } else {
                                    [0, 0, 0, 255]
                                };

                            for dy in 0..cell_height {
                                let py = row_y * cell_height + dy;
                                if py >= physical_height {
                                    continue;
                                }
                                for dx in 0..cell_width {
                                    let px_idx = c * cell_width + dx;
                                    if px_idx >= physical_width {
                                        continue;
                                    }
                                    let pixel_offset = (py * physical_width + px_idx) * 4;
                                    pixels[pixel_offset] = b_val;
                                    pixels[pixel_offset + 1] = g_val;
                                    pixels[pixel_offset + 2] = r_val;
                                    pixels[pixel_offset + 3] = a_val;
                                }
                            }
                        }
                    }
                } else {
                    let mut bgra_lut = [[0u8; 4]; 256];
                    for byte in 0..=255 {
                        let color = match self.color_mode {
                            ColorMode::Grayscale => {
                                let val = byte as f32 / 255.0;
                                Hsla {
                                    h: 0.0,
                                    s: 0.0,
                                    l: val * 0.8 + 0.1,
                                    a: 1.0,
                                }
                            }
                            ColorMode::DataCategory => match byte {
                                0 => theme.muted_foreground.opacity(0.18),
                                1..=31 | 127 => theme.red.opacity(0.75),
                                32 => theme.blue.opacity(0.55),
                                33..=126 => theme.green.opacity(0.85),
                                _ => theme.accent.opacity(0.8),
                            },
                            ColorMode::Rainbow => {
                                let val = byte as f32 / 255.0;
                                Hsla {
                                    h: val * 360.0,
                                    s: 0.8,
                                    l: 0.5,
                                    a: 1.0,
                                }
                            }
                            ColorMode::Entropy => unreachable!(),
                        };
                        let rgb = color.to_rgb();
                        bgra_lut[byte as usize] = [
                            (rgb.b * 255.0).clamp(0.0, 255.0) as u8,
                            (rgb.g * 255.0).clamp(0.0, 255.0) as u8,
                            (rgb.r * 255.0).clamp(0.0, 255.0) as u8,
                            (rgb.a * 255.0).clamp(0.0, 255.0) as u8,
                        ];
                    }

                    for r in start_row..end_row {
                        let row_y = r - start_row;
                        let row_offset = r * self.cols;
                        let chunk_len = cmp::min(self.cols, buffer_len.saturating_sub(row_offset));
                        let chunk_len = cmp::min(chunk_len, max_visible_cols);
                        if chunk_len == 0 {
                            break;
                        }

                        let chunk = buffer.get_range(row_offset, chunk_len);
                        for (c, &byte) in chunk.iter().take(chunk_len).enumerate() {
                            let [b_val, g_val, r_val, a_val] = bgra_lut[byte as usize];

                            for dy in 0..cell_height {
                                let py = row_y * cell_height + dy;
                                if py >= physical_height {
                                    continue;
                                }
                                for dx in 0..cell_width {
                                    let px_idx = c * cell_width + dx;
                                    if px_idx >= physical_width {
                                        continue;
                                    }
                                    let pixel_offset = (py * physical_width + px_idx) * 4;
                                    pixels[pixel_offset] = b_val;
                                    pixels[pixel_offset + 1] = g_val;
                                    pixels[pixel_offset + 2] = r_val;
                                    pixels[pixel_offset + 3] = a_val;
                                }
                            }
                        }
                    }
                }

                if let Some(rgba_img) = image::RgbaImage::from_raw(physical_width as u32, physical_height as u32, pixels) {
                    let frame = image::Frame::new(rgba_img);
                    let render_img = Arc::new(RenderImage::new(vec![frame]));
                    *cache = Some((render_img.clone(), cache_key));
                    cached_image = Some(render_img);
                }
            }
        }

        if let Some(img) = cached_image {
            let logical_width = physical_width as f32 / scale_factor;
            let logical_height = physical_height as f32 / scale_factor;
            window
                .paint_image(
                    Bounds::new(bounds.origin, size(px(logical_width), px(logical_height))),
                    Corners::default(),
                    img,
                    0,
                    false,
                )
                .ok();
        }

        // Selection Highlight Overlay
        if let Some(sel) = &self.selection_range
            && sel.start < sel.end
        {
            for r in start_row..end_row {
                let row_start = r * cols;
                let row_end = (r + 1) * cols;
                let sel_row_start = cmp::max(sel.start, row_start);
                let sel_row_end = cmp::min(sel.end, row_end);
                if sel_row_start < sel_row_end {
                    let c_start = sel_row_start - row_start;
                    let c_count = sel_row_end - sel_row_start;
                    let sel_x = bounds.origin.x + px(c_start as f32 * pixel_size);
                    let sel_y = bounds.origin.y + px((r - start_row) as f32 * pixel_size);
                    let sel_w = px(c_count as f32 * pixel_size);
                    let sel_h = px(pixel_size);
                    window.paint_quad(gpui::fill(Bounds::new(point(sel_x, sel_y), size(sel_w, sel_h)), theme.accent.opacity(0.35)));
                }
            }
        }

        // Hover Highlight
        if let Some(hov) = self.hovered_offset {
            let hov_row = hov / cols;
            let hov_col = hov % cols;
            if hov_row >= start_row && hov_row < end_row && hov < buffer_len {
                let cell_x = bounds.origin.x + px(hov_col as f32 * pixel_size);
                let cell_y = bounds.origin.y + px((hov_row - start_row) as f32 * pixel_size);
                let cell_w = px(pixel_size);
                let cell_h = px(pixel_size);
                let cell_bounds = Bounds::new(point(cell_x, cell_y), size(cell_w, cell_h));

                let outline_color = theme.foreground.opacity(0.75);
                let border_w = if pixel_size >= 4.0 { px(1.0) } else { px(0.5) };
                window.paint_quad(gpui::outline(cell_bounds, outline_color, gpui::BorderStyle::Solid).border_widths(border_w));
            }
        }

        // Cursor Highlight
        if let Some(cursor) = self.cursor_offset {
            let cur_row = cursor / cols;
            let cur_col = cursor % cols;
            if cur_row >= start_row && cur_row < end_row && cursor <= buffer_len {
                let cell_x = bounds.origin.x + px(cur_col as f32 * pixel_size);
                let cell_y = bounds.origin.y + px((cur_row - start_row) as f32 * pixel_size);

                if pixel_size <= 2.0 {
                    let indicator_size = px(6.0);
                    let center_x = cell_x + px(pixel_size * 0.5);
                    let center_y = cell_y + px(pixel_size * 0.5);
                    let cur_bounds = Bounds::new(
                        point(center_x - indicator_size * 0.5, center_y - indicator_size * 0.5),
                        size(indicator_size, indicator_size),
                    );
                    window.paint_quad(gpui::outline(cur_bounds, theme.accent, gpui::BorderStyle::Solid).border_widths(px(1.5)));
                    window.paint_quad(gpui::fill(
                        Bounds::new(point(cell_x, cell_y), size(px(pixel_size), px(pixel_size))),
                        theme.foreground,
                    ));
                } else {
                    let cur_bounds = Bounds::new(point(cell_x, cell_y), size(px(pixel_size), px(pixel_size)));
                    window.paint_quad(gpui::outline(cur_bounds, theme.accent, gpui::BorderStyle::Solid).border_widths(px(1.5)));
                    window.paint_quad(gpui::fill(cur_bounds, theme.accent.opacity(0.3)));
                }
            }
        }
    }
}

impl IntoElement for VisualMapElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}
