use crate::core::editor::Editor;
use crate::ui::style::StyleExt as _;
use gpui::prelude::*;
use gpui::*;
use gpui_component::dock::{Panel, PanelEvent};
use gpui_component::scroll::*;
use gpui_component::{ActiveTheme, Icon, IconName, PixelsExt, button::Button, button::ButtonVariants, h_flex, v_flex};
use std::cell::RefCell;
use std::cmp;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum ColorMode {
    Grayscale,
    DataCategory,
    Rainbow,
}

pub type CachedImageKey = (usize, usize, usize, ColorMode, usize, f32, f32, u32);
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
    hovered_info: Option<(usize, u8)>,
    last_bounds: std::cell::Cell<Option<Bounds<Pixels>>>,
    cached_image: RefCell<Option<CachedImage>>,
    is_dragging: bool,
    _editor_subscription: Option<Subscription>,
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
            cols: 256,
            pixel_size: 4,
            scroll_offset: 0,
            scroll_remainder: 0.0,
            scroll_handle: ScrollHandle::new(),
            color_mode: ColorMode::DataCategory,
            hovered_info: None,
            last_bounds: std::cell::Cell::new(None),
            cached_image: RefCell::new(None),
            is_dragging: false,
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

    fn update_scrollbar(&mut self, cx: &mut Context<Self>) {
        let buffer_len = self.buffer_len(cx);
        let total_rows = buffer_len.div_ceil(self.cols);
        let pixel_size_px = px(self.pixel_size as f32);
        self.scroll_offset = self.scroll_offset.min(total_rows.saturating_sub(1));
        self.scroll_handle.set_offset(point(px(0.), -(self.scroll_offset as f32 * pixel_size_px)));
    }

    fn on_scroll_wheel(&mut self, event: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
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
        // Sync scroll handle to scroll offset if changed by scrollbar drag
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
        };
        state.info = gpui_component::dock::PanelInfo::panel(serde_json::to_value(map_state).expect("serialize VisualMapPanelState"));
        state
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct VisualMapPanelState {
    pub path: std::path::PathBuf,
    pub cols: usize,
    pub pixel_size: usize,
    pub color_mode: ColorMode,
}

impl Render for VisualMapPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg_color, border_color, muted_color) = (theme.sidebar, theme.border, theme.muted_foreground);
        let is_focused = self.focus_handle.is_focused(window);

        let header = div()
            .p_2()
            .text_sm()
            .text_color(crate::ui::style::header_text_color(is_focused, theme))
            .child("2D VISUAL MAP");

        let editor = match &self.editor {
            Some(ed) => ed,
            None => {
                let container = v_flex().size_full().min_w_0().min_h_0().overflow_hidden().bg(bg_color);

                let container = container.focus_indicator(is_focused, theme);

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
                    .child(
                        div().flex_1().min_h_0().w_full().overflow_hidden().child(
                            v_flex()
                                .size_full()
                                .pt_8()
                                .items_center()
                                .child(div().text_sm().text_color(muted_color).child("No active editor")),
                        ),
                    );
            }
        };

        let buffer_len = self.buffer_len(cx);
        let total_rows = buffer_len.div_ceil(self.cols);
        let total_height = total_rows as f32 * self.pixel_size as f32;

        // Sync scroll offset from scroll handle (e.g. if changed by scrollbar drag)
        let pixel_size_px = px(self.pixel_size as f32);
        let handle_y = self.scroll_handle.offset().y;
        let handle_row = ((-handle_y).max(px(0.)) / pixel_size_px).round() as usize;
        let synced_offset = handle_row.min(total_rows.saturating_sub(1));
        if self.scroll_offset != synced_offset {
            self.scroll_offset = synced_offset;
        }

        let width_button = |preset: usize, label: &'static str, cx: &mut Context<Self>| {
            let is_selected = self.cols == preset;
            let mut btn = Button::new(("w_preset", preset)).label(label);
            if is_selected {
                btn = btn.primary();
            } else {
                btn = btn.ghost();
            }
            btn.on_click(cx.listener(move |this, _, _, cx| {
                this.cols = preset;
                this.update_scrollbar(cx);
                cx.notify();
            }))
        };

        let pixel_button = |preset: usize, label: &'static str, cx: &mut Context<Self>| {
            let is_selected = self.pixel_size == preset;
            let mut btn = Button::new(("p_preset", preset)).label(label);
            if is_selected {
                btn = btn.primary();
            } else {
                btn = btn.ghost();
            }
            btn.on_click(cx.listener(move |this, _, _, cx| {
                this.pixel_size = preset;
                this.update_scrollbar(cx);
                cx.notify();
            }))
        };

        let color_button = |mode: ColorMode, label: &'static str, id_str: &'static str, cx: &mut Context<Self>| {
            let is_selected = self.color_mode == mode;
            let mut btn = Button::new(id_str).label(label);
            if is_selected {
                btn = btn.primary();
            } else {
                btn = btn.ghost();
            }
            btn.on_click(cx.listener(move |this, _, _, cx| {
                this.color_mode = mode;
                cx.notify();
            }))
        };

        let width_controls = h_flex()
            .gap_1()
            .items_center()
            .child(div().text_xs().text_color(muted_color).child("Width: "))
            .child(Button::new("dec_w").label("-").ghost().on_click(cx.listener(move |this, _, _, cx| {
                this.cols = cmp::max(4, this.cols.saturating_sub(4));
                this.update_scrollbar(cx);
                cx.notify();
            })))
            .child(div().text_sm().px_1().child(format!("{}", self.cols)))
            .child(Button::new("inc_w").label("+").ghost().on_click(cx.listener(move |this, _, _, cx| {
                this.cols = this.cols.saturating_add(4);
                this.update_scrollbar(cx);
                cx.notify();
            })));

        let footer_text = if let Some((offset, byte)) = self.hovered_info {
            let char_repr = if (32..=126).contains(&byte) {
                format!("'{}'", byte as char)
            } else {
                ".".to_string()
            };
            format!("Offset: 0x{:08X} ({}) | Value: 0x{:02X} ({})", offset, offset, byte, char_repr)
        } else {
            "Hover over pixels to view details".to_string()
        };

        let container = v_flex().size_full().bg(bg_color);

        let container = container.focus_indicator(is_focused, theme);

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
            .child(
                // Toolbar row 1: Presets
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .p_2()
                    .border_b_1()
                    .border_color(border_color)
                    .child(
                        h_flex()
                            .gap_1()
                            .child(width_button(64, "64", cx))
                            .child(width_button(128, "128", cx))
                            .child(width_button(256, "256", cx)),
                    )
                    .child(div().w_0p5().h_4().bg(border_color))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(pixel_button(2, "2px", cx))
                            .child(pixel_button(4, "4px", cx))
                            .child(pixel_button(8, "8px", cx)),
                    ),
            )
            .child(
                // Toolbar row 2: Advanced Controls
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .p_2()
                    .border_b_1()
                    .border_color(border_color)
                    .child(width_controls)
                    .child(div().w_0p5().h_4().bg(border_color))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(color_button(ColorMode::Grayscale, "Gray", "c_gray", cx))
                            .child(color_button(ColorMode::DataCategory, "Type", "c_type", cx))
                            .child(color_button(ColorMode::Rainbow, "Rainbow", "c_rainbow", cx)),
                    ),
            )
            .child(
                // Canvas Area
                div()
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
                    })
                    .child(
                        div().absolute().top_0().right_0().bottom_0().w_4().child(
                            Scrollbar::vertical(&self.scroll_handle)
                                .axis(ScrollbarAxis::Vertical)
                                .scroll_size(size(px(0.), px(total_height))),
                        ),
                    ),
            )
            .child(
                // Footer status bar
                div()
                    .p_2()
                    .border_t_1()
                    .border_color(border_color)
                    .text_xs()
                    .text_color(muted_color)
                    .child(footer_text),
            )
    }
}

struct VisualMapElement {
    panel: WeakEntity<VisualMapPanel>,
    document: Arc<std::sync::RwLock<crate::core::document::Document>>,
    cols: usize,
    pixel_size: usize,
    scroll_offset: usize,
    color_mode: ColorMode,
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
                buffer_len,
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

            if cached_image.is_none() {
                // Pre-calculate RGBA color Lookup Table (LUT) for all 256 possible bytes directly
                let mut rgba_lut = [[0u8; 4]; 256];
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
                            0 => theme.muted_foreground.opacity(0.15),
                            1..=31 => theme.red.opacity(0.6),
                            32 => theme.blue.opacity(0.4),
                            33..=126 => theme.green.opacity(0.8),
                            127 => theme.red.opacity(0.6),
                            _ => theme.accent.opacity(0.7),
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
                    };
                    let rgb = color.to_rgb();
                    rgba_lut[byte as usize] = [
                        (rgb.r * 255.0).clamp(0.0, 255.0) as u8,
                        (rgb.g * 255.0).clamp(0.0, 255.0) as u8,
                        (rgb.b * 255.0).clamp(0.0, 255.0) as u8,
                        (rgb.a * 255.0).clamp(0.0, 255.0) as u8,
                    ];
                }

                if physical_width > 0 && physical_height > 0 {
                    let mut pixels = vec![0u8; physical_width * physical_height * 4];

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
                            let [r_val, g_val, b_val, a_val] = rgba_lut[byte as usize];

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
                                    pixels[pixel_offset] = r_val;
                                    pixels[pixel_offset + 1] = g_val;
                                    pixels[pixel_offset + 2] = b_val;
                                    pixels[pixel_offset + 3] = a_val;
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
    }
}

impl IntoElement for VisualMapElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}
