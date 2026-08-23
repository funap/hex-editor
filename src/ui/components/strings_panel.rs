use crate::core::editor::Editor;
use crate::core::encoding::Encoding;
use crate::core::strings::{DEFAULT_MIN_STRING_LENGTH, StringMatch, find_strings_limited};
use crate::ui::components::data_table::{TableColumn, TableSortDirection, VirtualTable, VirtualTableState};
use crate::ui::icon::IconName;
use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{self, Input, InputState};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{ActiveTheme as _, Disableable, Sizable, Size, h_flex, v_flex};
use std::collections::HashMap;

actions!(strings_panel, [MoveUp, MoveDown, SelectCurrent, ClearResults]);

#[derive(Clone, PartialEq, Action)]
#[action(namespace = strings_panel, no_json)]
struct CopyAddress {
    value: String,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = strings_panel, no_json)]
struct CopyValue {
    value: String,
}

const CONTEXT: &str = "StringsPanel";
pub const MAX_STRING_RESULTS: usize = 10_000;

const STRINGS_ROW_HEIGHT: f32 = 24.0;

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", MoveUp, Some("StringsPanel && !InputFocus")),
        KeyBinding::new("down", MoveDown, Some("StringsPanel && !InputFocus")),
        KeyBinding::new("k", MoveUp, Some("StringsPanel && !InputFocus")),
        KeyBinding::new("j", MoveDown, Some("StringsPanel && !InputFocus")),
        KeyBinding::new("enter", SelectCurrent, Some("StringsPanel && !InputFocus")),
        KeyBinding::new("escape", ClearResults, Some("StringsPanel")),
    ]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScanSignature {
    encoding: Encoding,
    content_state: usize,
    byte_len: usize,
}

struct CachedStringsState {
    results: Vec<StringMatch>,
    is_truncated: bool,
    selected_index: Option<usize>,
    has_scanned: bool,
    scan_signature: Option<ScanSignature>,
    scan_min_length: Option<usize>,
}

pub enum StringsPanelEvent {
    NavigateTo { offset: usize, len: usize },
}

pub struct StringsPanel {
    pub editor: Option<Entity<Editor>>,
    pub focus_handle: FocusHandle,
    pub table_state: VirtualTableState,
    pub min_length_input: Entity<InputState>,
    pub filter_input: Entity<InputState>,
    pub is_scanning: bool,
    pub results: Vec<StringMatch>,
    pub is_truncated: bool,
    pub selected_index: Option<usize>,
    pub has_scanned: bool,
    pub scan_task: Option<Task<()>>,
    scan_generation: usize,
    scan_signature: Option<ScanSignature>,
    scan_min_length: Option<usize>,
    cached_states: HashMap<EntityId, CachedStringsState>,
    last_container_width: Pixels,
    _input_subscription: Option<Subscription>,
    _filter_subscription: Option<Subscription>,
    _editor_subscription: Option<Subscription>,
}

impl EventEmitter<StringsPanelEvent> for StringsPanel {}

fn default_strings_columns() -> Vec<TableColumn> {
    vec![
        TableColumn::new("address", "Address", px(88.0))
            .min_width(px(50.0))
            .sortable(true)
            .resizable(true),
        TableColumn::new("size", "Size", px(58.0)).min_width(px(40.0)).sortable(true).resizable(true),
        TableColumn::new("value", "Value", px(220.0)).min_width(px(80.0)).sortable(true).resizable(true),
    ]
}

impl StringsPanel {
    pub fn new(editor: Option<Entity<Editor>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let table_state = VirtualTableState::new(default_strings_columns());
        let min_length_input = cx.new(|cx| InputState::new(window, cx));
        min_length_input.update(cx, |input, cx| {
            input.set_value(DEFAULT_MIN_STRING_LENGTH.to_string(), window, cx);
        });

        let filter_input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter by value..."));

        let input_subscription = cx.subscribe_in(&min_length_input, window, |this, _, event: &input::InputEvent, _window, cx| {
            if let input::InputEvent::Change = event {
                this.invalidate_scan();
                cx.notify();
            }
        });

        let filter_subscription = cx.subscribe_in(&filter_input, window, |this, _, event: &input::InputEvent, _window, cx| {
            if let input::InputEvent::Change = event {
                this.sync_selection_to_filter(cx);
                cx.notify();
            }
        });

        let mut this = Self {
            editor: None,
            focus_handle,
            table_state,
            min_length_input,
            filter_input,
            is_scanning: false,
            results: Vec::new(),
            is_truncated: false,
            selected_index: None,
            has_scanned: false,
            scan_task: None,
            scan_generation: 0,
            scan_signature: None,
            scan_min_length: None,
            cached_states: HashMap::new(),
            last_container_width: px(300.0),
            _input_subscription: Some(input_subscription),
            _filter_subscription: Some(filter_subscription),
            _editor_subscription: None,
        };

        this.set_editor(editor, cx);
        this
    }

    pub fn set_editor(&mut self, editor: Option<Entity<Editor>>, cx: &mut Context<Self>) {
        let current_editor_id = self.editor.as_ref().map(Entity::entity_id);
        let next_editor_id = editor.as_ref().map(Entity::entity_id);
        if current_editor_id == next_editor_id {
            cx.notify();
            return;
        }

        self.cache_current_state();
        self._editor_subscription = None;
        self.invalidate_scan();
        self.editor = editor.clone();
        self.scan_signature = self.current_signature(cx);

        if let Some(ed) = &editor {
            self._editor_subscription = Some(cx.observe(ed, |this, _, cx| {
                this.scan_if_content_changed(cx);
            }));
        }
        if let Some(editor_id) = next_editor_id {
            self.restore_cached_state(editor_id, cx);
        }
        cx.notify();
    }

    fn cache_current_state(&mut self) {
        let Some(editor) = &self.editor else {
            return;
        };

        self.cached_states.insert(
            editor.entity_id(),
            CachedStringsState {
                results: std::mem::take(&mut self.results),
                is_truncated: self.is_truncated,
                selected_index: self.selected_index,
                has_scanned: self.has_scanned,
                scan_signature: self.scan_signature,
                scan_min_length: self.scan_min_length,
            },
        );
    }

    fn restore_cached_state(&mut self, editor_id: EntityId, cx: &mut Context<Self>) {
        let Some(cached) = self.cached_states.remove(&editor_id) else {
            return;
        };

        let is_current_scan = cached.has_scanned && cached.scan_signature == self.scan_signature && cached.scan_min_length == self.minimum_length(cx);
        if !is_current_scan && cached.has_scanned {
            return;
        }

        self.results = cached.results;
        self.is_truncated = cached.is_truncated;
        self.selected_index = cached.selected_index;
        self.has_scanned = cached.has_scanned;
        self.scan_min_length = cached.scan_min_length;
        self.sync_selection_to_filter(cx);
    }

    fn current_signature(&self, cx: &App) -> Option<ScanSignature> {
        let editor = self.editor.as_ref()?.read(cx);
        let document = editor.document.read().expect("document read lock");
        Some(ScanSignature {
            encoding: editor.encoding,
            content_state: document.history.state_id(),
            byte_len: document.buffer.len(),
        })
    }

    fn scan_if_content_changed(&mut self, cx: &mut Context<Self>) {
        let signature = self.current_signature(cx);
        if signature == self.scan_signature {
            cx.notify();
        } else {
            self.invalidate_scan();
            self.scan_signature = signature;
            cx.notify();
        }
    }

    fn invalidate_scan(&mut self) {
        self.scan_generation = self.scan_generation.wrapping_add(1);
        self.scan_task = None;
        self.is_scanning = false;
        self.results.clear();
        self.is_truncated = false;
        self.selected_index = None;
        self.has_scanned = false;
        self.scan_min_length = None;
    }

    pub fn trigger_scan(&mut self, cx: &mut Context<Self>) {
        self.invalidate_scan();
        let generation = self.scan_generation;
        self.scan_signature = self.current_signature(cx);

        let Some(editor_entity) = &self.editor else {
            self.is_scanning = false;
            cx.notify();
            return;
        };

        let Some(min_length) = self.minimum_length(cx) else {
            self.is_scanning = false;
            cx.notify();
            return;
        };
        self.scan_min_length = Some(min_length);

        let (encoding, buffer_data) = {
            let editor = editor_entity.read(cx);
            let document = editor.document.read().expect("document read lock");
            (editor.encoding, document.buffer.clone())
        };

        self.is_scanning = true;
        let task = cx.spawn(async move |this, cx| {
            let (results, is_truncated) = cx
                .background_executor()
                .spawn(async move { find_strings_limited(buffer_data.data(), encoding, min_length, MAX_STRING_RESULTS) })
                .await;

            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| {
                    if this.scan_generation != generation {
                        return;
                    }

                    this.scan_task = None;
                    this.is_scanning = false;
                    this.is_truncated = is_truncated;
                    this.results = results;
                    this.has_scanned = true;
                    this.selected_index = this.sorted_indices(cx).first().copied();
                    if this.selected_index.is_some() {
                        this.table_state.scroll_to_row(0, ScrollStrategy::Top);
                    }
                    cx.notify();
                })
                .ok();
            }
        });

        self.scan_task = Some(task);
        cx.notify();
    }

    fn minimum_length(&self, cx: &App) -> Option<usize> {
        self.min_length_input.read(cx).value().trim().parse::<usize>().ok().filter(|length| *length > 0)
    }

    fn filter_query(&self, cx: &App) -> String {
        self.filter_input.read(cx).value().trim().to_lowercase()
    }

    fn filtered_indices(&self, cx: &App) -> Vec<usize> {
        let filter = self.filter_query(cx);
        if filter.is_empty() {
            return (0..self.results.len()).collect();
        }

        self.results
            .iter()
            .enumerate()
            .filter(|(_, item)| item.text.to_lowercase().contains(&filter))
            .map(|(index, _)| index)
            .collect()
    }

    fn sorted_indices(&self, cx: &App) -> Vec<usize> {
        let mut indices = self.filtered_indices(cx);

        // Check which column is sorted
        let sorted_col = self
            .table_state
            .columns
            .iter()
            .enumerate()
            .find_map(|(ix, col)| col.sort_direction.map(|dir| (ix, dir)));

        let Some((col_ix, direction)) = sorted_col else {
            return indices;
        };

        indices.sort_by(|left, right| {
            let left_match = &self.results[*left];
            let right_match = &self.results[*right];
            let ordering = match col_ix {
                0 => left_match.offset.cmp(&right_match.offset),
                1 => left_match.byte_len.cmp(&right_match.byte_len),
                2 => left_match.text.cmp(&right_match.text),
                _ => std::cmp::Ordering::Equal,
            };
            let ordering = if direction == TableSortDirection::Descending {
                ordering.reverse()
            } else {
                ordering
            };
            ordering.then_with(|| left.cmp(right))
        });
        indices
    }

    fn sort_by(&mut self, col_ix: usize, cx: &mut Context<Self>) {
        self.table_state.toggle_column_sort(col_ix);
        self.sync_selection_to_filter(cx);
        cx.notify();
    }

    fn auto_fit_column(&mut self, col_ix: usize, cx: &mut Context<Self>) {
        let visible_indices = self.sorted_indices(cx);
        let texts = visible_indices.iter().take(128).filter_map(|&index| {
            self.results.get(index).map(|item| match col_ix {
                0 => format!("0x{:08X}", item.offset),
                1 => item.byte_len.to_string(),
                2 => preview_text(&item.text),
                _ => String::new(),
            })
        });

        self.table_state.auto_fit_column_with_texts(col_ix, texts);
        cx.notify();
    }

    fn result_summary(&self) -> String {
        if self.is_truncated {
            format!("{}+ entries found (showing first {} entries)", MAX_STRING_RESULTS, MAX_STRING_RESULTS)
        } else {
            let label = if self.results.len() == 1 { "entry" } else { "entries" };
            format!("{} {label} found", self.results.len())
        }
    }

    fn sync_selection_to_filter(&mut self, cx: &mut Context<Self>) {
        let visible_indices = self.sorted_indices(cx);
        if let Some(index) = self.selected_index {
            if !visible_indices.contains(&index) {
                self.selected_index = visible_indices.first().copied();
            }
        } else {
            self.selected_index = visible_indices.first().copied();
        }

        if let Some(index) = self.selected_index
            && let Some(position) = visible_indices.iter().position(|&visible_index| visible_index == index)
        {
            self.table_state.scroll_to_row(position, ScrollStrategy::Top);
        }
    }

    fn adjust_minimum_length(&mut self, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.minimum_length(cx).unwrap_or(DEFAULT_MIN_STRING_LENGTH);
        let next = if delta < 0 {
            current.saturating_sub(delta.unsigned_abs() as usize).max(1)
        } else {
            current.saturating_add(delta as usize)
        };

        self.min_length_input.update(cx, |input, cx| {
            input.set_value(next.to_string(), window, cx);
        });
        self.invalidate_scan();
        cx.notify();
    }

    pub fn select_item(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(item) = self.results.get(index) {
            self.selected_index = Some(index);
            let offset = item.offset;
            let len = item.byte_len;
            if let Some(ed) = &self.editor {
                ed.update(cx, |editor, cx| {
                    editor.set_cursor_offset(offset);
                    cx.notify();
                });
            }
            cx.emit(StringsPanelEvent::NavigateTo { offset, len });
        }
        cx.notify();
    }

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        let visible_indices = self.sorted_indices(cx);
        if visible_indices.is_empty() {
            return;
        }
        let current_position = self
            .selected_index
            .and_then(|index| visible_indices.iter().position(|&visible_index| visible_index == index));
        let new_position = match current_position {
            Some(0) | None => visible_indices.len().saturating_sub(1),
            Some(position) => position - 1,
        };
        self.table_state.scroll_to_row(new_position, ScrollStrategy::Top);
        if let Some(&index) = visible_indices.get(new_position) {
            self.select_item(index, cx);
        }
    }

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        let visible_indices = self.sorted_indices(cx);
        if visible_indices.is_empty() {
            return;
        }
        let current_position = self
            .selected_index
            .and_then(|index| visible_indices.iter().position(|&visible_index| visible_index == index));
        let new_position = match current_position {
            Some(position) if position + 1 < visible_indices.len() => position + 1,
            _ => 0,
        };
        self.table_state.scroll_to_row(new_position, ScrollStrategy::Top);
        if let Some(&index) = visible_indices.get(new_position) {
            self.select_item(index, cx);
        }
    }

    fn select_current(&mut self, _: &SelectCurrent, _: &mut Window, cx: &mut Context<Self>) {
        let visible_indices = self.sorted_indices(cx);
        if let Some(index) = self.selected_index.filter(|index| visible_indices.contains(index)) {
            self.select_item(index, cx);
        } else if let Some(&index) = visible_indices.first() {
            self.select_item(index, cx);
        }
    }

    fn copy_address(&mut self, action: &CopyAddress, _: &mut Window, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(action.value.clone()));
    }

    fn copy_value(&mut self, action: &CopyValue, _: &mut Window, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(action.value.clone()));
    }

    fn clear_results(&mut self, _: &ClearResults, _: &mut Window, cx: &mut Context<Self>) {
        self.invalidate_scan();
        cx.notify();
    }
}

impl Render for StringsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let table_overlay = VirtualTable::render_table_overlay(&self.table_state, cx, |this| &mut this.table_state);

        let theme = cx.theme();
        let is_focused = self.focus_handle.is_focused(window);
        let has_editor = self.editor.is_some();
        let minimum_length_is_valid = self.minimum_length(cx).is_some();
        let visible_indices = self.sorted_indices(cx);

        let actions = h_flex().items_center().gap_1().child(
            Button::new("clear-strings")
                .ghost()
                .icon(IconName::Eraser)
                .with_size(Size::XSmall)
                .tooltip("Clear results")
                .disabled(!self.has_scanned && !self.is_scanning)
                .on_click(cx.listener(|this, _, _window, cx| {
                    this.invalidate_scan();
                    cx.notify();
                })),
        );
        let header = crate::ui::style::panel_header("STRINGS", is_focused, theme, None, Some(actions.into_any_element()));

        let minimum_length = self.minimum_length(cx).unwrap_or(DEFAULT_MIN_STRING_LENGTH);
        let scan_controls = h_flex()
            .p_2()
            .gap_1()
            .border_b_1()
            .border_color(theme.border)
            .items_center()
            .child(div().text_xs().text_color(theme.muted_foreground).child("Min chars"))
            .child(div().w_16().child(Input::new(&self.min_length_input)))
            .child(
                Button::new("strings-min-decrease")
                    .ghost()
                    .icon(IconName::Minus)
                    .with_size(Size::XSmall)
                    .tooltip("Decrease minimum length")
                    .disabled(!has_editor || self.is_scanning || minimum_length <= 1)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.adjust_minimum_length(-1, window, cx);
                    })),
            )
            .child(
                Button::new("strings-min-increase")
                    .ghost()
                    .icon(IconName::Plus)
                    .with_size(Size::XSmall)
                    .tooltip("Increase minimum length")
                    .disabled(!has_editor || self.is_scanning)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.adjust_minimum_length(1, window, cx);
                    })),
            )
            .child(
                Button::new("scan-strings")
                    .label("Scan")
                    .primary()
                    .with_size(Size::Small)
                    .disabled(self.is_scanning || !has_editor || !minimum_length_is_valid)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.trigger_scan(cx);
                    })),
            );

        let result_summary = div()
            .px_2()
            .py_1()
            .bg(theme.accent.opacity(0.1))
            .border_b_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(self.result_summary());

        let filter_controls = h_flex()
            .p_2()
            .gap_1p5()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .child(div().flex_1().child(Input::new(&self.filter_input).prefix(IconName::Search).cleanable(true)));

        let body = if !has_editor {
            crate::ui::style::panel_empty_state(
                IconName::ScanEye,
                "No Active File",
                Some("Open a binary file to find printable strings"),
                None,
                theme,
            )
            .into_any_element()
        } else if !minimum_length_is_valid {
            crate::ui::style::panel_empty_state(
                IconName::TriangleAlert,
                "Invalid Minimum Length",
                Some("Enter a positive number of characters"),
                None,
                theme,
            )
            .into_any_element()
        } else if self.is_scanning {
            crate::ui::style::panel_empty_state(
                IconName::Loader,
                "Scanning File...",
                Some("Finding printable strings in the background"),
                None,
                theme,
            )
            .into_any_element()
        } else if !self.has_scanned {
            crate::ui::style::panel_empty_state(
                IconName::ScanEye,
                "Find Printable Strings",
                Some("Scan the file using the current encoding"),
                None,
                theme,
            )
            .into_any_element()
        } else if self.results.is_empty() {
            crate::ui::style::panel_empty_state(
                IconName::Search,
                "No Strings Found",
                Some("No printable strings matched the minimum length"),
                None,
                theme,
            )
            .into_any_element()
        } else if visible_indices.is_empty() {
            crate::ui::style::panel_empty_state(
                IconName::Search,
                "No Matching Strings",
                Some("No string values match the current filter"),
                None,
                theme,
            )
            .into_any_element()
        } else {
            let view = cx.entity().clone();
            let context_focus_handle = self.focus_handle.clone();
            let total_visible_width = self.table_state.total_visible_width();
            let scroll_offset_x = self.table_state.scroll_offset_x;

            let header_row = VirtualTable::render_header_row(
                &self.table_state,
                "strings-column-header",
                theme,
                cx,
                None::<fn(usize, &TableColumn) -> Option<AnyElement>>,
                Self::auto_fit_column,
                Some(Self::sort_by),
                |this| &mut this.table_state,
            );

            let visible_cols: Vec<(usize, Pixels)> = self.table_state.visible_columns().map(|(ix, col)| (ix, col.width)).collect();
            let list_view = view.clone();

            let list = uniform_list("strings-panel-results-list", visible_indices.len(), move |range, window, cx| {
                let this = list_view.read(cx);
                let theme = cx.theme();

                range
                    .map(|visible_index| {
                        let Some(&index) = visible_indices.get(visible_index) else {
                            return div().into_any_element();
                        };
                        if let Some(item) = this.results.get(index) {
                            let is_selected = this.selected_index == Some(index);
                            let (bg_color, border_color, offset_color) = if is_selected {
                                if is_focused {
                                    (theme.selection, theme.accent, theme.accent_foreground)
                                } else {
                                    (theme.muted, theme.muted_foreground.opacity(0.4), theme.muted_foreground)
                                }
                            } else {
                                (theme.sidebar, theme.border.opacity(0.5), theme.muted_foreground)
                            };
                            let hover_bg = if is_selected {
                                if is_focused { theme.selection.opacity(0.7) } else { theme.muted.opacity(0.8) }
                            } else {
                                theme.selection.opacity(0.3)
                            };

                            let preview = preview_text(&item.text);
                            let offset = item.offset;
                            let byte_len = item.byte_len;
                            let offset_value = format!("0x{:08X}", offset);

                            h_flex()
                                .id(("strings-result-item", index))
                                .w_full()
                                .h(px(STRINGS_ROW_HEIGHT))
                                .flex_shrink_0()
                                .overflow_hidden()
                                .border_b_1()
                                .border_color(border_color)
                                .bg(bg_color)
                                .cursor_pointer()
                                .hover(move |style| style.bg(hover_bg))
                                .on_click(window.listener_for(&list_view, move |this, _, _, cx| {
                                    this.select_item(index, cx);
                                }))
                                .on_mouse_down(
                                    MouseButton::Right,
                                    window.listener_for(&list_view, move |this, _, _, cx| {
                                        this.select_item(index, cx);
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
                                                    .text_color(offset_color)
                                                    .child(offset_value.clone())
                                                    .into_any_element(),
                                                1 => div()
                                                    .text_xs()
                                                    .font_family("Courier New")
                                                    .text_color(theme.muted_foreground)
                                                    .child(byte_len.to_string())
                                                    .into_any_element(),
                                                2 => div()
                                                    .text_xs()
                                                    .font_family("Courier New")
                                                    .text_color(theme.foreground)
                                                    .child(preview.clone())
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
                .id("strings-table-container")
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
                .child(header_row)
                .child(
                    div()
                        .id("strings-results-container")
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
                            let offset_value = format!("0x{:08X}", item.offset);
                            let value = item.text.clone();
                            Some((offset_value, value))
                        })
                    };
                    let Some((menu_offset, value)) = selected_info else {
                        return menu;
                    };
                    menu.action_context(context_focus_handle.clone())
                        .menu_with_icon("Copy Address", IconName::Hash, Box::new(CopyAddress { value: menu_offset }))
                        .menu_with_icon("Copy Value", IconName::TextInitial, Box::new(CopyValue { value }))
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
            .on_action(cx.listener(Self::copy_address))
            .on_action(cx.listener(Self::copy_value))
            .on_action(cx.listener(Self::clear_results))
            .child(header)
            .child(scan_controls)
            .child(result_summary)
            .child(filter_controls)
            .child(body)
    }
}

fn preview_text(text: &str) -> String {
    const PREVIEW_LIMIT: usize = 256;
    let mut characters = text.chars();
    let preview: String = characters.by_ref().take(PREVIEW_LIMIT).collect();
    if characters.next().is_some() { format!("{preview}…") } else { preview }
}

impl Focusable for StringsPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
