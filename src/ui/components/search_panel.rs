use crate::core::editor::Editor;
use crate::core::search::{SearchLimit, SearchMode, find_occurrences, parse_hex_pattern};
use crate::ui::components::data_table::{TableColumn, VirtualTable, VirtualTableState};
use crate::ui::icon::IconName;
use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{self, Input, InputState};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{ActiveTheme as _, Disableable, Sizable, Size, StyledExt, WindowExt as _, h_flex, v_flex};

actions!(search_panel, [MoveUp, MoveDown, SelectCurrent, ClearResults]);

#[derive(Clone, PartialEq, Action)]
#[action(namespace = search_panel, no_json)]
struct CopyAddress {
    value: String,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = search_panel, no_json)]
struct CopyValue {
    value: String,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = search_panel, no_json)]
struct CopyText {
    value: String,
}

const CONTEXT: &str = "SearchPanel";
pub const MAX_SEARCH_RESULTS: usize = 10_000;

const SEARCH_ROW_HEIGHT: f32 = 24.0;

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", MoveUp, Some("SearchPanel && !InputFocus")),
        KeyBinding::new("down", MoveDown, Some("SearchPanel && !InputFocus")),
        KeyBinding::new("k", MoveUp, Some("SearchPanel && !InputFocus")),
        KeyBinding::new("j", MoveDown, Some("SearchPanel && !InputFocus")),
        KeyBinding::new("enter", SelectCurrent, Some("SearchPanel && !InputFocus")),
        KeyBinding::new("escape", ClearResults, Some("SearchPanel")),
    ]);
}

fn default_search_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("address", "Address", px(90.0)).min_width(px(60.0)).resizable(true),
        TableColumn::new("value", "Value", px(140.0)).min_width(px(60.0)).resizable(true),
        TableColumn::new("text", "Text", px(120.0)).min_width(px(60.0)).resizable(true),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchResultItem {
    pub offset: usize,
}

pub enum SearchPanelEvent {
    NavigateTo { offset: usize, len: usize },
}

pub struct SearchPanel {
    pub editor: Option<Entity<Editor>>,
    pub focus_handle: FocusHandle,
    pub table_state: VirtualTableState,
    pub last_container_width: Pixels,
    pub input: Entity<InputState>,
    pub mode: SearchMode,
    pub is_searching: bool,
    pub results: Vec<SearchResultItem>,
    pub is_truncated: bool,
    pub selected_index: Option<usize>,
    pub last_query: String,
    pub match_len: usize,
    pub search_task: Option<Task<()>>,
    _input_subscription: Option<Subscription>,
    _editor_subscription: Option<Subscription>,
}

impl EventEmitter<SearchPanelEvent> for SearchPanel {}

impl SearchPanel {
    pub fn new(editor: Option<Entity<Editor>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let table_state = VirtualTableState::new(default_search_columns());
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search pattern in file..."));

        let input_sub = cx.subscribe_in(&input, window, |this, _, event: &input::InputEvent, _window, cx| {
            if let input::InputEvent::PressEnter { .. } = event {
                this.trigger_search(cx);
            }
        });

        let mut this = Self {
            editor: None,
            focus_handle,
            table_state,
            last_container_width: px(300.0),
            input,
            mode: SearchMode::Hex,
            is_searching: false,
            results: Vec::new(),
            is_truncated: false,
            selected_index: None,
            last_query: String::new(),
            match_len: 0,
            search_task: None,
            _input_subscription: Some(input_sub),
            _editor_subscription: None,
        };

        this.set_editor(editor, cx);
        this
    }

    pub fn set_editor(&mut self, editor: Option<Entity<Editor>>, cx: &mut Context<Self>) {
        self._editor_subscription = None;
        self.editor = editor.clone();
        self.results.clear();
        self.is_truncated = false;
        self.selected_index = None;
        self.is_searching = false;
        self.search_task = None;

        if let Some(ed) = &editor {
            self._editor_subscription = Some(cx.observe(ed, |_this, _, cx| {
                cx.notify();
            }));
        }
        cx.notify();
    }

    pub fn trigger_search(&mut self, cx: &mut Context<Self>) {
        let query = self.input.read(cx).value().to_string();
        if query.trim().is_empty() {
            self.results.clear();
            self.is_truncated = false;
            self.selected_index = None;
            self.last_query.clear();
            self.match_len = 0;
            if let Some(ed) = &self.editor {
                ed.update(cx, |ed, cx| {
                    ed.clear_search();
                    cx.notify();
                });
            }
            cx.notify();
            return;
        }

        let Some(editor_entity) = &self.editor else {
            return;
        };

        let mode = self.mode;
        let pattern_len = match mode {
            SearchMode::Text => query.len(),
            SearchMode::Hex => parse_hex_pattern(&query).map(|p| p.len()).unwrap_or(0),
        };

        if pattern_len == 0 {
            return;
        }

        self.last_query = query.clone();
        self.match_len = pattern_len;
        self.is_searching = true;
        self.results.clear();
        self.is_truncated = false;
        self.selected_index = None;

        // Update editor search query so visible ranges in HexView are highlighted
        editor_entity.update(cx, |editor, cx| {
            editor.set_search_query_and_mode(query.clone(), mode);
            cx.notify();
        });

        let buffer_data = {
            let editor = editor_entity.read(cx);
            let doc = editor.document.read().expect("document read lock");
            doc.buffer.clone()
        };

        let task = cx.spawn(async move |this, cx| {
            let buffer_for_search = buffer_data.clone();
            let (offsets, is_truncated) = cx
                .background_executor()
                .spawn(async move {
                    let pattern = match mode {
                        SearchMode::Text => query.as_bytes().iter().map(|&b| crate::core::search::PatternByte::new_exact(b)).collect(),
                        SearchMode::Hex => {
                            if let Some(p) = parse_hex_pattern(&query) {
                                p
                            } else {
                                return (Vec::new(), false);
                            }
                        }
                    };
                    let raw_offsets = find_occurrences(buffer_for_search.data(), &pattern, SearchLimit::Count(MAX_SEARCH_RESULTS + 1), None);
                    let truncated = raw_offsets.len() > MAX_SEARCH_RESULTS;
                    let capped_offsets = if truncated {
                        raw_offsets.into_iter().take(MAX_SEARCH_RESULTS).collect()
                    } else {
                        raw_offsets
                    };
                    (capped_offsets, truncated)
                })
                .await;

            let items: Vec<SearchResultItem> = offsets.iter().map(|&offset| SearchResultItem { offset }).collect();

            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| {
                    this.is_searching = false;
                    this.is_truncated = is_truncated;
                    this.results = items;
                    if !this.results.is_empty() {
                        this.selected_index = Some(0);
                        this.table_state.scroll_to_row(0, ScrollStrategy::Top);
                    }
                    if let Some(ed) = &this.editor {
                        let generation = ed.read(cx).search_state.generation;
                        ed.update(cx, |editor, cx| {
                            editor.set_search_results(offsets, generation, true);
                            cx.notify();
                        });
                    }
                    cx.notify();
                })
                .ok();
            }
        });

        self.search_task = Some(task);
        cx.notify();
    }

    pub fn select_item(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.results.len() {
            self.selected_index = Some(index);
            let offset = self.results[index].offset;
            let len = self.match_len;

            if let Some(ed) = &self.editor {
                ed.update(cx, |editor, cx| {
                    if len > 0 {
                        editor.set_selection_range(offset..offset.saturating_add(len));
                    } else {
                        editor.set_cursor_offset(offset);
                    }
                    if let Some(cur_idx) = editor.search_state.current_result_index.as_mut() {
                        *cur_idx = index;
                    }
                    cx.notify();
                });
            }

            cx.emit(SearchPanelEvent::NavigateTo { offset, len });
            cx.notify();
        }
    }

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.results.is_empty() {
            return;
        }
        let new_idx = match self.selected_index {
            Some(0) | None => self.results.len().saturating_sub(1),
            Some(idx) => idx - 1,
        };
        self.table_state.scroll_to_row(new_idx, ScrollStrategy::Top);
        self.select_item(new_idx, cx);
    }

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.results.is_empty() {
            return;
        }
        let new_idx = match self.selected_index {
            Some(idx) if idx + 1 < self.results.len() => idx + 1,
            _ => 0,
        };
        self.table_state.scroll_to_row(new_idx, ScrollStrategy::Top);
        self.select_item(new_idx, cx);
    }

    fn select_current(&mut self, _: &SelectCurrent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(idx) = self.selected_index {
            self.select_item(idx, cx);
        }
    }

    fn clear_results_action(&mut self, _: &ClearResults, _: &mut Window, cx: &mut Context<Self>) {
        self.results.clear();
        self.is_truncated = false;
        self.selected_index = None;
        self.last_query.clear();
        self.match_len = 0;
        if let Some(ed) = &self.editor {
            ed.update(cx, |ed, cx| {
                ed.clear_search();
                cx.notify();
            });
        }
        cx.notify();
    }

    fn copy_address(&mut self, action: &CopyAddress, window: &mut Window, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(action.value.clone()));
        window.push_notification(gpui_component::notification::Notification::info(format!("Copied address {}", action.value)), cx);
    }

    fn copy_value(&mut self, action: &CopyValue, window: &mut Window, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(action.value.clone()));
        window.push_notification(gpui_component::notification::Notification::info("Copied hex value"), cx);
    }

    fn copy_text(&mut self, action: &CopyText, window: &mut Window, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(action.value.clone()));
        window.push_notification(gpui_component::notification::Notification::info("Copied text value"), cx);
    }

    fn auto_fit_column(&mut self, col_ix: usize, cx: &mut Context<Self>) {
        let buffer = self.editor.as_ref().and_then(|ed| {
            let ed_ref = ed.read(cx);
            ed_ref.document.read().ok().map(|d| d.buffer.clone())
        });
        let buffer_slice = buffer.as_ref().map(|b| b.data());

        let texts = self.results.iter().take(128).map(|item| match col_ix {
            0 => format!("0x{:08X}", item.offset),
            1 => {
                let (hex, _) = format_row_previews(buffer_slice, item.offset, self.match_len);
                hex
            }
            2 => {
                let (_, ascii) = format_row_previews(buffer_slice, item.offset, self.match_len);
                ascii
            }
            _ => String::new(),
        });

        self.table_state.auto_fit_column_with_texts(col_ix, texts);
        cx.notify();
    }
}

fn format_row_previews(buffer_data: Option<&[u8]>, offset: usize, match_len: usize) -> (String, String) {
    let snippet_len = match_len.clamp(4, 16);
    if let Some(data) = buffer_data {
        let end = (offset + snippet_len).min(data.len());
        let slice = if offset < data.len() { &data[offset..end] } else { &[] };
        let mut hex = String::with_capacity(slice.len() * 3);
        for (i, b) in slice.iter().enumerate() {
            if i > 0 {
                hex.push(' ');
            }
            use std::fmt::Write as _;
            let _ = write!(&mut hex, "{:02X}", b);
        }
        let ascii: String = slice.iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' }).collect();
        (hex, ascii)
    } else {
        (String::new(), String::new())
    }
}

impl Render for SearchPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let table_overlay = VirtualTable::render_table_overlay(&self.table_state, cx, |this| &mut this.table_state);

        let theme = cx.theme();
        let is_focused = self.focus_handle.is_focused(window);
        let has_editor = self.editor.is_some();
        let count = self.results.len();

        let badge = if self.is_searching {
            Some(crate::ui::style::panel_badge("Scanning...", theme).into_any_element())
        } else if self.is_truncated {
            Some(crate::ui::style::panel_badge(format!("{}+ matches", MAX_SEARCH_RESULTS), theme).into_any_element())
        } else if !self.last_query.is_empty() {
            Some(crate::ui::style::panel_badge(format!("{} matches", count), theme).into_any_element())
        } else {
            None
        };

        let actions = h_flex().items_center().gap_1().child(
            Button::new("clear-search")
                .ghost()
                .icon(IconName::Eraser)
                .with_size(Size::XSmall)
                .tooltip("Clear results")
                .disabled(!has_editor || (self.results.is_empty() && self.last_query.is_empty()))
                .on_click(cx.listener(|this, _, _window, cx| {
                    this.results.clear();
                    this.is_truncated = false;
                    this.selected_index = None;
                    this.last_query.clear();
                    this.match_len = 0;
                    if let Some(ed) = &this.editor {
                        ed.update(cx, |ed, cx| {
                            ed.clear_search();
                            cx.notify();
                        });
                    }
                    cx.notify();
                })),
        );

        let header = crate::ui::style::panel_header("SEARCH", is_focused, theme, badge, Some(actions.into_any_element()));

        // Search controls: Mode toggle + Search Input + Scan Button
        let search_controls = v_flex()
            .p_2()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        if self.mode == SearchMode::Hex {
                            Button::new("mode_hex").label("Hex").primary().with_size(Size::XSmall)
                        } else {
                            Button::new("mode_hex").label("Hex").ghost().with_size(Size::XSmall)
                        }
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.mode = SearchMode::Hex;
                            cx.notify();
                        })),
                    )
                    .child(
                        if self.mode == SearchMode::Text {
                            Button::new("mode_text").label("Text").primary().with_size(Size::XSmall)
                        } else {
                            Button::new("mode_text").label("Text").ghost().with_size(Size::XSmall)
                        }
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.mode = SearchMode::Text;
                            cx.notify();
                        })),
                    ),
            )
            .child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(div().flex_1().child(Input::new(&self.input).prefix(IconName::Search).cleanable(true)))
                    .child(
                        Button::new("scan_btn")
                            .label(if self.is_searching { "Scanning..." } else { "Find All" })
                            .primary()
                            .with_size(Size::Small)
                            .disabled(self.is_searching || !has_editor)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.trigger_search(cx);
                            })),
                    ),
            );

        // Body content
        let body = if !has_editor {
            crate::ui::style::panel_empty_state(
                IconName::Search,
                "No Active File",
                Some("Open a binary file to search across the entire buffer"),
                None,
                theme,
            )
            .into_any_element()
        } else if self.is_searching {
            crate::ui::style::panel_empty_state(
                IconName::Loader,
                "Scanning File...",
                Some("Searching entire file buffer in background"),
                None,
                theme,
            )
            .into_any_element()
        } else if self.results.is_empty() && !self.last_query.is_empty() {
            crate::ui::style::panel_empty_state(
                IconName::Search,
                "No Matches Found",
                Some(format!("No occurrences found for '{}'", self.last_query)),
                None,
                theme,
            )
            .into_any_element()
        } else if self.results.is_empty() {
            crate::ui::style::panel_empty_state(
                IconName::Search,
                "Search in File",
                Some("Enter hex pattern or text to scan the entire file"),
                None,
                theme,
            )
            .into_any_element()
        } else {
            let view = cx.entity().clone();
            let is_truncated = self.is_truncated;
            let context_focus_handle = self.focus_handle.clone();
            let total_visible_width = self.table_state.total_visible_width();
            let scroll_offset_x = self.table_state.scroll_offset_x;

            let truncation_notice = if is_truncated {
                Some(
                    div()
                        .px_2()
                        .py_1()
                        .bg(theme.accent.opacity(0.1))
                        .border_b_1()
                        .border_color(theme.border)
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!("Showing first {} matches (capped for performance)", MAX_SEARCH_RESULTS)),
                )
            } else {
                None
            };

            let header_row = VirtualTable::render_header_row(
                &self.table_state,
                "search-column-header",
                theme,
                cx,
                None::<fn(usize, &TableColumn) -> Option<AnyElement>>,
                Self::auto_fit_column,
                None::<fn(&mut Self, usize, &mut Context<Self>)>,
                |this| &mut this.table_state,
            );

            let visible_cols: Vec<(usize, Pixels)> = self.table_state.visible_columns().map(|(ix, col)| (ix, col.width)).collect();
            let list_view = view.clone();

            let list = uniform_list("search-panel-results-list", self.results.len(), move |range, window, cx| {
                let this = list_view.read(cx);
                let theme = cx.theme();
                let buffer = this.editor.as_ref().and_then(|ed| {
                    let ed_ref = ed.read(cx);
                    ed_ref.document.read().ok().map(|d| d.buffer.clone())
                });

                range
                    .map(|idx| {
                        if let Some(item) = this.results.get(idx) {
                            let is_selected = this.selected_index == Some(idx);
                            let (bg_color, border_color) = if is_selected {
                                if is_focused {
                                    (theme.selection, theme.accent)
                                } else {
                                    (theme.muted, theme.muted_foreground.opacity(0.4))
                                }
                            } else {
                                (theme.sidebar, theme.border.opacity(0.5))
                            };

                            let hover_bg = if is_selected {
                                if is_focused { theme.selection.opacity(0.7) } else { theme.muted.opacity(0.8) }
                            } else {
                                theme.selection.opacity(0.3)
                            };

                            let offset = item.offset;
                            let offset_value = format!("0x{:08X}", offset);
                            let (preview_hex, preview_ascii) = format_row_previews(buffer.as_ref().map(|b| b.data()), offset, this.match_len);
                            h_flex()
                                .id(("search-result-item", idx))
                                .w_full()
                                .h(px(SEARCH_ROW_HEIGHT))
                                .flex_shrink_0()
                                .overflow_hidden()
                                .border_b_1()
                                .border_color(border_color)
                                .bg(bg_color)
                                .cursor_pointer()
                                .hover(move |style| style.bg(hover_bg))
                                .on_click(window.listener_for(&list_view, move |this, _, _, cx| {
                                    this.select_item(idx, cx);
                                }))
                                .on_mouse_down(
                                    MouseButton::Right,
                                    window.listener_for(&list_view, move |this, _, _, cx| {
                                        this.select_item(idx, cx);
                                    }),
                                )
                                .child(
                                    h_flex()
                                        .w(total_visible_width)
                                        .h_full()
                                        .ml(-scroll_offset_x)
                                        .children(visible_cols.iter().enumerate().map(|(vis_ix, &(col_ix, width))| {
                                            let is_first = vis_ix == 0;
                                            let cell_content = match col_ix {
                                                0 => div()
                                                    .text_xs()
                                                    .font_family("Courier New")
                                                    .font_semibold()
                                                    .text_color(if is_selected { theme.accent_foreground } else { theme.muted_foreground })
                                                    .child(offset_value.clone())
                                                    .into_any_element(),
                                                1 => div()
                                                    .text_xs()
                                                    .font_family("Courier New")
                                                    .text_color(theme.muted_foreground)
                                                    .child(preview_hex.clone())
                                                    .into_any_element(),
                                                2 => div()
                                                    .text_xs()
                                                    .font_family("Courier New")
                                                    .text_color(theme.foreground)
                                                    .child(preview_ascii.clone())
                                                    .into_any_element(),
                                                _ => div().into_any_element(),
                                            };
                                            VirtualTable::render_data_cell(col_ix, width, is_first, theme.border.opacity(0.35), cell_content)
                                        })),
                                )
                                .into_any_element()
                        } else {
                            div().into_any_element()
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .track_scroll(self.table_state.vertical_scroll_handle.clone())
            .size_full();

            let horizontal_scrollbar = VirtualTable::render_horizontal_scrollbar(&self.table_state, self.last_container_width);
            let vertical_scrollbar = VirtualTable::render_vertical_scrollbar(&self.table_state);
            let context_view = view.clone();

            v_flex()
                .id("search-table-container")
                .flex_1()
                .overflow_hidden()
                .relative()
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                    if this.table_state.resizing_column.is_some() {
                        if event.pressed_button != Some(MouseButton::Left) {
                            this.table_state.end_resize();
                            cx.notify();
                        } else {
                            this.table_state.update_resize(event.position.x);
                            cx.notify();
                        }
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                        if this.table_state.resizing_column.is_some() {
                            this.table_state.end_resize();
                            cx.notify();
                        }
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                        if this.table_state.resizing_column.is_some() {
                            this.table_state.end_resize();
                            cx.notify();
                        }
                    }),
                )
                .child(table_overlay)
                .children(truncation_notice)
                .child(header_row)
                .child(
                    div()
                        .id("search-results-container")
                        .flex_1()
                        .overflow_hidden()
                        .relative()
                        .child(list)
                        .child(vertical_scrollbar)
                        .children(horizontal_scrollbar),
                )
                .context_menu(move |menu, _window, cx| {
                    let selected_info = {
                        let this = context_view.read(cx);
                        this.selected_index.and_then(|idx| {
                            let item = this.results.get(idx)?;
                            let offset = item.offset;
                            let offset_value = format!("0x{:08X}", offset);
                            let buffer = this.editor.as_ref().and_then(|ed| {
                                let ed_ref = ed.read(cx);
                                ed_ref.document.read().ok().map(|d| d.buffer.clone())
                            });
                            let (preview_hex, preview_ascii) = format_row_previews(buffer.as_ref().map(|b| b.data()), offset, this.match_len);
                            Some((offset_value, preview_hex, preview_ascii))
                        })
                    };
                    let Some((menu_offset, menu_hex, menu_ascii)) = selected_info else {
                        return menu;
                    };
                    menu.action_context(context_focus_handle.clone())
                        .menu_with_icon("Copy Address", IconName::Hash, Box::new(CopyAddress { value: menu_offset }))
                        .menu_with_icon("Copy Hex Value", IconName::Binary, Box::new(CopyValue { value: menu_hex }))
                        .menu_with_icon("Copy Text", IconName::TextInitial, Box::new(CopyText { value: menu_ascii }))
                })
                .into_any_element()
        };

        crate::ui::style::panel_container(is_focused, theme)
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                if this.table_state.resizing_column.is_some() {
                    if event.pressed_button != Some(MouseButton::Left) {
                        this.table_state.end_resize();
                        cx.notify();
                    } else {
                        this.table_state.update_resize(event.position.x);
                        cx.notify();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                    if this.table_state.resizing_column.is_some() {
                        this.table_state.end_resize();
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                    if this.table_state.resizing_column.is_some() {
                        this.table_state.end_resize();
                        cx.notify();
                    }
                }),
            )
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::select_current))
            .on_action(cx.listener(Self::clear_results_action))
            .on_action(cx.listener(Self::copy_address))
            .on_action(cx.listener(Self::copy_value))
            .on_action(cx.listener(Self::copy_text))
            .child(header)
            .child(search_controls)
            .child(body)
    }
}

impl Focusable for SearchPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
