use crate::core::editor::Editor;
use crate::core::structure::ParsedField;
use crate::ui::icon::IconName;
use gpui::prelude::*;
use gpui::*;
use gpui_component::{ActiveTheme as _, Sizable as _, button::ButtonVariants as _};
use std::collections::HashSet;

actions!(struct_tree, [MoveUp, MoveDown, ToggleExpand, Expand, Collapse]);

const CONTEXT: &str = "StructTreeView";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", MoveUp, Some(CONTEXT)),
        KeyBinding::new("down", MoveDown, Some(CONTEXT)),
        KeyBinding::new("k", MoveUp, Some(CONTEXT)),
        KeyBinding::new("j", MoveDown, Some(CONTEXT)),
        KeyBinding::new("space", ToggleExpand, Some(CONTEXT)),
        KeyBinding::new("enter", ToggleExpand, Some(CONTEXT)),
        KeyBinding::new("right", Expand, Some(CONTEXT)),
        KeyBinding::new("left", Collapse, Some(CONTEXT)),
        KeyBinding::new("l", Expand, Some(CONTEXT)),
        KeyBinding::new("h", Collapse, Some(CONTEXT)),
    ]);
}

pub struct StructTreeView {
    pub parse_result: Option<std::sync::Arc<crate::core::structure::types::ParseResult>>,
    pub flattened_fields: Vec<FlattenedField>,
    pub expanded_paths: HashSet<Vec<usize>>,
    pub editor: Option<Entity<Editor>>,
    pub scroll_handle: UniformListScrollHandle,
    pub focus_handle: FocusHandle,
    pub selected_index: Option<usize>,
    last_parse_id: Option<(String, usize, usize, usize)>,
    _editor_subscription: Option<Subscription>,
}

#[derive(Clone)]
pub struct FlattenedField {
    pub path: Vec<usize>,
    pub id: SharedString,
    pub _field_type: SharedString,
    pub offset: usize,
    pub size: usize,
    pub value_str: SharedString,
    pub depth: usize,
    pub has_children: bool,
    pub is_collapsed: bool,
}

impl StructTreeView {
    pub fn new(
        parse_result: Option<std::sync::Arc<crate::core::structure::types::ParseResult>>,
        editor: Option<Entity<Editor>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let expanded_paths = HashSet::new();
        let mut flattened = Vec::new();
        if let Some(ref res) = parse_result {
            Self::flatten_fields(&res.fields, 0, &Vec::new(), &expanded_paths, &mut flattened);
        }
        let scroll_handle = UniformListScrollHandle::new();
        let focus_handle = cx.focus_handle();

        let mut this = Self {
            parse_result,
            flattened_fields: flattened,
            expanded_paths,
            editor: editor.clone(),
            scroll_handle,
            focus_handle,
            selected_index: None,
            last_parse_id: None,
            _editor_subscription: None,
        };

        if let Some(ed) = editor {
            this._editor_subscription = Some(cx.observe(&ed, |this, editor, cx| {
                this.sync_fields(&editor, cx);
            }));
            this.sync_fields(&ed, cx);
        }

        this
    }

    pub fn set_editor(&mut self, editor: Option<Entity<Editor>>, cx: &mut Context<Self>) {
        self._editor_subscription = None;
        self.editor = editor.clone();

        if let Some(ed) = &editor {
            self._editor_subscription = Some(cx.observe(ed, |this, editor, cx| {
                this.sync_fields(&editor, cx);
            }));
            self.sync_fields(ed, cx);
        } else {
            self.last_parse_id = None;
            self.set_parse_result(None, cx);
        }
        cx.notify();
    }

    fn sync_fields(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let (current_parse_id, cursor_offset, is_parsing) = {
            let editor_lock = editor.read(cx);
            let doc_version = editor_lock.document.read().ok().map(|d| d.history.version()).unwrap_or(0);
            let parse_id = editor_lock
                .parse_result()
                .map(|r| (r.definition_id.clone(), r.total_parsed_bytes, r.fields.len(), doc_version));
            (parse_id, editor_lock.cursor_offset, editor_lock.is_parsing_structure)
        };

        if is_parsing {
            if self.parse_result.is_some() {
                self.set_parse_result(None, cx);
            }
            return;
        }

        if current_parse_id != self.last_parse_id {
            let parse_res = editor.read(cx).parse_result();
            self.set_parse_result(parse_res, cx);
            self.last_parse_id = current_parse_id;
        }

        self.sync_selected_index_from_cursor(cursor_offset, cx);
    }

    fn sync_selected_index_from_cursor(&mut self, cursor_offset: usize, cx: &mut Context<Self>) {
        if self.flattened_fields.is_empty() {
            return;
        }

        // If current selected item already covers cursor_offset, keep it
        if let Some(curr_idx) = self.selected_index
            && let Some(field) = self.flattened_fields.get(curr_idx)
        {
            let end = field.offset + field.size;
            let matches = if field.size > 0 {
                cursor_offset >= field.offset && cursor_offset < end
            } else {
                cursor_offset == field.offset
            };
            if matches {
                return;
            }
        }

        let mut best_match: Option<(usize, bool, usize, usize)> = None; // (index, is_leaf, depth, size)
        for (i, field) in self.flattened_fields.iter().enumerate() {
            let end = field.offset + field.size;
            let matches = if field.size > 0 {
                cursor_offset >= field.offset && cursor_offset < end
            } else {
                cursor_offset == field.offset
            };

            if matches {
                let is_leaf = !field.has_children;
                match best_match {
                    None => {
                        best_match = Some((i, is_leaf, field.depth, field.size));
                    }
                    Some((_, best_is_leaf, best_depth, best_size)) => {
                        // Prefer leaf nodes over container nodes, then deeper nodes, then smaller size
                        if (is_leaf && !best_is_leaf)
                            || (is_leaf == best_is_leaf && field.depth > best_depth)
                            || (is_leaf == best_is_leaf && field.depth == best_depth && field.size < best_size)
                        {
                            best_match = Some((i, is_leaf, field.depth, field.size));
                        }
                    }
                }
            }
        }

        if let Some((idx, _, _, _)) = best_match
            && self.selected_index != Some(idx)
        {
            self.selected_index = Some(idx);
            self.scroll_handle.scroll_to_item(idx, ScrollStrategy::Top);
            cx.notify();
        }
    }

    pub fn set_parse_result(&mut self, parse_result: Option<std::sync::Arc<crate::core::structure::types::ParseResult>>, cx: &mut Context<Self>) {
        self.expanded_paths.clear();
        self.parse_result = parse_result;
        self.rebuild_flattened();
        self.selected_index = None;
        self.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn rebuild_flattened(&mut self) {
        let mut flattened = Vec::new();
        if let Some(ref res) = self.parse_result {
            Self::flatten_fields(&res.fields, 0, &Vec::new(), &self.expanded_paths, &mut flattened);
        }
        self.flattened_fields = flattened;
    }

    fn flatten_fields(fields: &[ParsedField], depth: usize, parent_path: &[usize], expanded_paths: &HashSet<Vec<usize>>, results: &mut Vec<FlattenedField>) {
        for (idx, field) in fields.iter().enumerate() {
            let mut current_path = parent_path.to_vec();
            current_path.push(idx);

            let val_str = if let Some(label) = &field.enum_label {
                SharedString::from(format!("{} ({})", field.value, label))
            } else {
                SharedString::from(format!("{}", field.value))
            };

            let has_children = !field.children.is_empty();
            let is_expanded = expanded_paths.contains(&current_path);

            results.push(FlattenedField {
                path: current_path.clone(),
                id: SharedString::from(field.id.clone()),
                _field_type: SharedString::from(field.field_type.clone()),
                offset: field.offset,
                size: field.size,
                value_str: val_str,
                depth,
                has_children,
                is_collapsed: !is_expanded,
            });

            if has_children && is_expanded {
                Self::flatten_fields(&field.children, depth + 1, &current_path, expanded_paths, results);
            }
        }
    }

    pub fn field_at_path<'a>(&'a self, path: &[usize]) -> Option<&'a ParsedField> {
        let res = self.parse_result.as_ref()?;
        let mut current_fields = &res.fields;
        let mut target_field: Option<&'a ParsedField> = None;
        for &idx in path {
            target_field = current_fields.get(idx);
            if let Some(f) = target_field {
                current_fields = &f.children;
            } else {
                return None;
            }
        }
        target_field
    }

    #[allow(dead_code)]
    pub fn get_field_at_path<'a>(&'a self, path: &[usize]) -> Option<&'a ParsedField> {
        self.field_at_path(path)
    }

    fn find_first_leaf_offset(field: &ParsedField) -> usize {
        if let Some(first_child) = field.children.first() {
            Self::find_first_leaf_offset(first_child)
        } else {
            field.offset
        }
    }

    fn select_item(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx >= self.flattened_fields.len() {
            return;
        }

        self.selected_index = Some(idx);
        self.scroll_handle.scroll_to_item(idx, ScrollStrategy::Top);

        let item = &self.flattened_fields[idx];
        let offset = if item.size == 0 && item.has_children {
            if let Some(parsed_field) = self.field_at_path(&item.path) {
                Self::find_first_leaf_offset(parsed_field)
            } else {
                item.offset
            }
        } else {
            item.offset
        };

        if let Some(editor) = &self.editor {
            editor.update(cx, |editor, cx| {
                let total = editor.total_size();
                if total > 0 && item.size > 0 {
                    let start = item.offset.min(total.saturating_sub(1));
                    let end = (item.offset + item.size.saturating_sub(1)).min(total.saturating_sub(1));
                    editor.selection_start = Some(start);
                    editor.selection_end = Some(end);
                    editor.cursor_offset = start;
                } else {
                    editor.selection_start = None;
                    editor.selection_end = None;
                    editor.cursor_offset = offset.min(total);
                }
                cx.notify();
            });
        }

        cx.notify();
    }

    fn toggle_collapse_at(&mut self, idx: usize, cx: &mut Context<Self>) {
        if let Some(field) = self.flattened_fields.get(idx) {
            if !field.has_children {
                return;
            }

            let path = field.path.clone();
            if self.expanded_paths.contains(&path) {
                self.expanded_paths.remove(&path);
            } else {
                self.expanded_paths.insert(path);
            }

            self.rebuild_flattened();

            // Maintain selection index if possible by path
            if idx < self.flattened_fields.len() {
                self.selected_index = Some(idx);
            } else if !self.flattened_fields.is_empty() {
                self.selected_index = Some(self.flattened_fields.len() - 1);
            }

            cx.notify();
        }
    }

    fn move_up(&mut self, _: &MoveUp, _window: &mut Window, cx: &mut Context<Self>) {
        if self.flattened_fields.is_empty() {
            return;
        }

        let next_idx = match self.selected_index {
            Some(idx) => idx.saturating_sub(1),
            None => 0,
        };

        self.select_item(next_idx, cx);
    }

    fn move_down(&mut self, _: &MoveDown, _window: &mut Window, cx: &mut Context<Self>) {
        if self.flattened_fields.is_empty() {
            return;
        }

        let max_idx = self.flattened_fields.len() - 1;
        let next_idx = match self.selected_index {
            Some(idx) => (idx + 1).min(max_idx),
            None => 0,
        };

        self.select_item(next_idx, cx);
    }

    fn toggle_expand(&mut self, _: &ToggleExpand, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(idx) = self.selected_index {
            self.toggle_collapse_at(idx, cx);
        }
    }

    fn expand(&mut self, _: &Expand, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(idx) = self.selected_index
            && let Some(field) = self.flattened_fields.get(idx)
            && field.has_children
            && field.is_collapsed
        {
            self.toggle_collapse_at(idx, cx);
        }
    }

    fn collapse(&mut self, _: &Collapse, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(idx) = self.selected_index
            && let Some(field) = self.flattened_fields.get(idx)
        {
            if field.has_children && !field.is_collapsed {
                self.toggle_collapse_at(idx, cx);
            } else if field.depth > 0 {
                // Move to parent field
                let target_depth = field.depth - 1;
                for p_idx in (0..idx).rev() {
                    if self.flattened_fields[p_idx].depth == target_depth {
                        self.select_item(p_idx, cx);
                        break;
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_list_item(
        ix: usize,
        field: &FlattenedField,
        is_selected: bool,
        is_focused: bool,
        view: Entity<Self>,
        focus_handle: &FocusHandle,
        window: &mut Window,
        cx: &App,
    ) -> AnyElement {
        let padding_left = px(14.0 * field.depth as f32 + 8.0);
        let theme = cx.theme();
        let bg_color = if is_selected {
            if is_focused { theme.selection } else { theme.muted_foreground.opacity(0.3) }
        } else {
            hsla(0.0, 0.0, 0.0, 0.0)
        };

        let focus_handle = focus_handle.clone();
        let chevron_symbol = if field.has_children {
            if field.is_collapsed { "▶" } else { "▼" }
        } else {
            " "
        };

        div()
            .id(ix)
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h(px(24.0))
            .bg(bg_color)
            .px_2()
            .pl(padding_left)
            .gap_1()
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(12.0))
                    .h(px(12.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .cursor_pointer()
                    .child(chevron_symbol)
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        window.listener_for(&view, move |this, _event: &gpui::MouseDownEvent, _window, cx| {
                            this.toggle_collapse_at(ix, cx);
                        }),
                    ),
            )
            .child(div().text_sm().text_color(theme.foreground).child(field.id.clone()))
            .child(div().text_sm().ml_auto().text_color(theme.muted_foreground).child(field.value_str.clone()))
            .on_mouse_down(
                gpui::MouseButton::Left,
                window.listener_for(&view, move |this, _event: &gpui::MouseDownEvent, window, cx| {
                    focus_handle.focus(window);
                    this.select_item(ix, cx);
                }),
            )
            .into_any_element()
    }
}

impl Render for StructTreeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity().clone();
        let is_parsing = self.editor.as_ref().is_some_and(|ed| ed.read(cx).is_parsing_structure);
        let is_empty = self.flattened_fields.is_empty() && self.parse_result.is_none();
        let is_focused = self.focus_handle.is_focused(window);
        let theme = cx.theme();

        let badge = if !self.flattened_fields.is_empty() {
            Some(crate::ui::style::panel_badge(format!("{}", self.flattened_fields.len()), theme).into_any_element())
        } else {
            None
        };

        let has_structure = self.parse_result.is_some() || !self.flattened_fields.is_empty();

        let header_actions = if has_structure {
            Some(
                gpui_component::button::Button::new("clear-structure-btn")
                    .ghost()
                    .icon(IconName::Close)
                    .with_size(gpui_component::Size::XSmall)
                    .tooltip("Clear Structure Definition")
                    .on_click(cx.listener(|_, _, window, cx| {
                        window.dispatch_action(Box::new(crate::actions::ClearStructureDefinition), cx);
                    }))
                    .into_any_element(),
            )
        } else {
            Some(
                gpui_component::button::Button::new("load-structure-header-btn")
                    .ghost()
                    .icon(IconName::FolderOpen)
                    .with_size(gpui_component::Size::XSmall)
                    .tooltip("Load Structure Definition (cmd-shift-s)")
                    .on_click(cx.listener(|_, _, window, cx| {
                        window.dispatch_action(Box::new(crate::actions::LoadStructureDefinition), cx);
                    }))
                    .into_any_element(),
            )
        };

        let header = crate::ui::style::panel_header("STRUCTURE", is_focused, theme, badge, header_actions);

        let container = crate::ui::style::panel_container(is_focused, theme);

        container
            .id("struct-tree-view")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::toggle_expand))
            .on_action(cx.listener(Self::expand))
            .on_action(cx.listener(Self::collapse))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, _| {
                    this.focus_handle.focus(window);
                }),
            )
            .child(header)
            .child(div().flex_1().min_h_0().w_full().overflow_hidden().child(if is_parsing {
                crate::ui::style::panel_empty_state(
                    IconName::LoaderCircle,
                    "Parsing Structure...",
                    Some("Analyzing binary data with Kaitai Struct..."),
                    None,
                    theme,
                )
                .into_any_element()
            } else if is_empty {
                let load_btn = gpui_component::button::Button::new("load-ksy-btn")
                    .label("Load Structure (.ksy)")
                    .primary()
                    .with_size(gpui_component::Size::Small)
                    .on_click(cx.listener(|_, _, window, cx| {
                        window.dispatch_action(Box::new(crate::actions::LoadStructureDefinition), cx);
                    }))
                    .into_any_element();

                crate::ui::style::panel_empty_state(
                    IconName::ListTree,
                    "No Structure Loaded",
                    Some("Open a Kaitai Struct (.ksy) YAML file to inspect binary fields"),
                    Some(load_btn),
                    theme,
                )
                .into_any_element()
            } else {
                let focus_handle = self.focus_handle.clone();
                uniform_list("struct-tree-list", self.flattened_fields.len(), move |range, window, cx| {
                    let this = view.read(cx);
                    range
                        .map(|ix| {
                            if let Some(field) = this.flattened_fields.get(ix) {
                                let is_selected = this.selected_index == Some(ix);
                                Self::render_list_item(ix, field, is_selected, is_focused, view.clone(), &focus_handle, window, cx)
                            } else {
                                div().into_any_element()
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .track_scroll(self.scroll_handle.clone())
                .size_full()
                .into_any_element()
            }))
    }
}

impl Focusable for StructTreeView {
    fn focus_handle(&self, _cx: &App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}
