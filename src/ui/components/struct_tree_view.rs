use crate::core::editor::Editor;
use crate::core::structure::ParsedField;
use crate::ui::icon::IconName;
use gpui::prelude::*;
use gpui::*;
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{ActiveTheme as _, Sizable as _, button::ButtonVariants as _, h_flex};
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

#[allow(dead_code)]
pub enum StructTreeViewEvent {
    NavigateTo { offset: usize, size: usize },
}

pub struct StructTreeView {
    pub parse_result: Option<std::sync::Arc<crate::core::structure::types::ParseResult>>,
    pub flattened_fields: Vec<FlattenedField>,
    pub expanded_paths: HashSet<Vec<usize>>,
    pub editor: Option<Entity<Editor>>,
    pub scroll_handle: UniformListScrollHandle,
    pub focus_handle: FocusHandle,
    pub selected_index: Option<usize>,
    last_parse_id: Option<(String, usize, usize, usize, usize, bool)>,
    last_selection_cursor: Option<usize>,
    last_selection_scan_len: usize,
    _editor_subscription: Option<Subscription>,
}

#[derive(Clone)]
pub struct FlattenedField {
    pub path: Vec<usize>,
    pub offset: usize,
    pub size: usize,
    pub depth: usize,
    pub has_children: bool,
    pub is_collapsed: bool,
}

impl EventEmitter<StructTreeViewEvent> for StructTreeView {}

impl StructTreeView {
    pub fn new(
        parse_result: Option<std::sync::Arc<crate::core::structure::types::ParseResult>>,
        editor: Option<Entity<Editor>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let expanded_paths = HashSet::new();
        let mut flattened = Vec::new();
        if let Some(ref res) = parse_result {
            Self::flatten_fields(res.fields.iter(), 0, &[], &expanded_paths, &mut flattened, 0);
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
            last_selection_cursor: None,
            last_selection_scan_len: 0,
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
        let same_editor = match (&self.editor, &editor) {
            (Some(current), Some(next)) => current.entity_id() == next.entity_id(),
            (None, None) => true,
            _ => false,
        };
        if same_editor {
            return;
        }

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
        let (current_parse_id, cursor_offset) = {
            let editor_lock = editor.read(cx);
            let doc_version = editor_lock.document.read().ok().map(|d| d.history.version()).unwrap_or(0);
            let parse_id = editor_lock.parse_result().map(|r| {
                (
                    r.definition_id.clone(),
                    r.total_parsed_bytes,
                    r.fields.len(),
                    doc_version,
                    editor_lock.parse_generation,
                    editor_lock.is_parsing_structure,
                )
            });
            (parse_id, editor_lock.cursor_offset)
        };

        if current_parse_id != self.last_parse_id {
            let parse_res = editor.read(cx).parse_result();
            let can_append = match (&self.last_parse_id, &current_parse_id, &self.parse_result) {
                (Some(previous), Some(current), Some(existing)) => {
                    previous.3 == current.3 && previous.4 == current.4 && current.2 >= previous.2 && existing.fields.len() == previous.2
                }
                _ => false,
            };
            if can_append {
                let previous_field_count = self.last_parse_id.as_ref().map(|parse_id| parse_id.2).unwrap_or(0);
                let is_finishing =
                    self.last_parse_id.as_ref().is_some_and(|parse_id| parse_id.5) && current_parse_id.as_ref().is_some_and(|parse_id| !parse_id.5);
                if is_finishing {
                    let old = self.parse_result.take();
                    self.parse_result = parse_res;
                    if let Some(old) = old {
                        std::thread::spawn(move || drop(old));
                    }
                } else {
                    self.parse_result = parse_res;
                }
                if let Some(ref res) = self.parse_result
                    && current_parse_id.as_ref().map(|parse_id| parse_id.2).unwrap_or(0) > previous_field_count
                {
                    Self::flatten_fields(
                        res.fields.iter_from(previous_field_count),
                        0,
                        &[],
                        &self.expanded_paths,
                        &mut self.flattened_fields,
                        previous_field_count,
                    );
                }
                cx.notify();
            } else {
                self.set_parse_result(parse_res, cx);
            }
            self.last_parse_id = current_parse_id;
        }

        self.sync_selected_index_from_cursor(cursor_offset, cx);
    }

    fn sync_selected_index_from_cursor(&mut self, cursor_offset: usize, cx: &mut Context<Self>) {
        if self.flattened_fields.is_empty() {
            self.last_selection_cursor = Some(cursor_offset);
            self.last_selection_scan_len = 0;
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
                self.last_selection_cursor = Some(cursor_offset);
                self.last_selection_scan_len = self.flattened_fields.len();
                return;
            }
        }

        let cursor_changed = self.last_selection_cursor != Some(cursor_offset);
        let scan_start = if !cursor_changed && self.selected_index.is_none() && self.last_selection_scan_len <= self.flattened_fields.len() {
            self.last_selection_scan_len
        } else {
            0
        };
        if scan_start == self.flattened_fields.len() {
            return;
        }

        let mut best_match: Option<(usize, bool, usize, usize)> = None; // (index, is_leaf, depth, size)
        for (i, field) in self.flattened_fields.iter().enumerate().skip(scan_start) {
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

        self.last_selection_cursor = Some(cursor_offset);
        self.last_selection_scan_len = self.flattened_fields.len();
        if let Some((idx, _, _, _)) = best_match
            && self.selected_index != Some(idx)
        {
            self.selected_index = Some(idx);
            self.scroll_handle.scroll_to_item(idx, ScrollStrategy::Top);
            cx.notify();
        } else if best_match.is_none() {
            self.selected_index = None;
        }
    }

    pub fn set_parse_result(&mut self, parse_result: Option<std::sync::Arc<crate::core::structure::types::ParseResult>>, cx: &mut Context<Self>) {
        self.expanded_paths.clear();
        let old = std::mem::replace(&mut self.parse_result, parse_result);
        if let Some(old) = old {
            std::thread::spawn(move || drop(old));
        }
        self.rebuild_flattened();
        self.selected_index = None;
        self.last_selection_cursor = None;
        self.last_selection_scan_len = 0;
        self.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn rebuild_flattened(&mut self) {
        let mut flattened = Vec::new();
        if let Some(ref res) = self.parse_result {
            Self::flatten_fields(res.fields.iter(), 0, &[], &self.expanded_paths, &mut flattened, 0);
        }
        self.flattened_fields = flattened;
        self.last_selection_cursor = None;
        self.last_selection_scan_len = 0;
    }

    fn flatten_fields<'a, I>(
        fields: I,
        depth: usize,
        parent_path: &[usize],
        expanded_paths: &HashSet<Vec<usize>>,
        results: &mut Vec<FlattenedField>,
        index_offset: usize,
    ) where
        I: IntoIterator<Item = &'a ParsedField>,
    {
        struct Frame<'a, I> {
            root: Option<I>,
            children: Option<std::slice::Iter<'a, ParsedField>>,
            depth: usize,
            parent_path: Vec<usize>,
            index_offset: usize,
            next_index: usize,
        }

        let mut frames = vec![Frame {
            root: Some(fields.into_iter()),
            children: None,
            depth,
            parent_path: parent_path.to_vec(),
            index_offset,
            next_index: 0,
        }];

        while !frames.is_empty() {
            let Some((relative_idx, field, frame_depth, frame_index_offset, parent_path)) = ({
                let frame = frames.last_mut().expect("flattening stack must not be empty");
                let next = if let Some(root) = frame.root.as_mut() {
                    root.next().map(|field| (frame.next_index, field))
                } else if let Some(children) = frame.children.as_mut() {
                    children.next().map(|field| (frame.next_index, field))
                } else {
                    None
                };

                next.map(|(relative_idx, field)| {
                    frame.next_index += 1;
                    (relative_idx, field, frame.depth, frame.index_offset, frame.parent_path.clone())
                })
            }) else {
                frames.pop();
                continue;
            };

            let idx = relative_idx + frame_index_offset;
            let mut current_path = parent_path;
            current_path.push(idx);

            let has_children = !field.children.is_empty();
            let is_expanded = expanded_paths.contains(&current_path);

            results.push(FlattenedField {
                path: current_path.clone(),
                offset: field.offset,
                size: field.size,
                depth: frame_depth,
                has_children,
                is_collapsed: !is_expanded,
            });

            if has_children && is_expanded {
                frames.push(Frame {
                    root: None,
                    children: Some(field.children.iter()),
                    depth: frame_depth + 1,
                    parent_path: current_path,
                    index_offset: 0,
                    next_index: 0,
                });
            }
        }
    }

    pub fn field_at_path<'a>(&'a self, path: &[usize]) -> Option<&'a ParsedField> {
        let res = self.parse_result.as_ref()?;
        let (&first, rest) = path.split_first()?;
        let mut target_field = res.fields.get(first)?;
        for &idx in rest {
            target_field = target_field.children.get(idx)?;
        }
        Some(target_field)
    }

    #[allow(dead_code)]
    pub fn get_field_at_path<'a>(&'a self, path: &[usize]) -> Option<&'a ParsedField> {
        self.field_at_path(path)
    }

    fn find_first_leaf_offset(field: &ParsedField) -> usize {
        let mut current = field;
        while let Some(first_child) = current.children.first() {
            current = first_child;
        }
        current.offset
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

        let mut nav_offset = offset;
        if let Some(editor) = &self.editor {
            editor.update(cx, |editor, cx| {
                let total = editor.total_size();
                if total > 0 && item.size > 0 {
                    let start = item.offset.min(total.saturating_sub(1));
                    let end = (item.offset + item.size.saturating_sub(1)).min(total.saturating_sub(1));
                    editor.set_selection_range(start..end.saturating_add(1));
                    editor.cursor_offset = start;
                    nav_offset = start;
                } else {
                    let clamped = offset.min(total);
                    editor.set_cursor_offset_exact(clamped);
                    nav_offset = clamped;
                }
                cx.notify();
            });
        }

        cx.emit(StructTreeViewEvent::NavigateTo {
            offset: nav_offset,
            size: item.size,
        });
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

    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.expanded_paths.clear();
        self.rebuild_flattened();
        self.selected_index = self.selected_index.map(|idx| idx.min(self.flattened_fields.len().saturating_sub(1)));
        cx.notify();
    }

    pub fn expand_all(&mut self, cx: &mut Context<Self>) {
        if let Some(ref res) = self.parse_result {
            fn collect_branches<'a>(fields: impl IntoIterator<Item = &'a ParsedField>, parent_path: Vec<usize>, out: &mut HashSet<Vec<usize>>) {
                for (idx, field) in fields.into_iter().enumerate() {
                    let mut path = parent_path.clone();
                    path.push(idx);
                    if !field.children.is_empty() {
                        out.insert(path.clone());
                        collect_branches(field.children.iter(), path, out);
                    }
                }
            }
            collect_branches(res.fields.iter(), Vec::new(), &mut self.expanded_paths);
        }
        self.rebuild_flattened();
        cx.notify();
    }

    fn on_action_expand_all(&mut self, _: &crate::actions::ExpandAllStructure, _window: &mut Window, cx: &mut Context<Self>) {
        self.expand_all(cx);
    }

    fn on_action_collapse_all(&mut self, _: &crate::actions::CollapseAllStructure, _window: &mut Window, cx: &mut Context<Self>) {
        self.collapse_all(cx);
    }

    #[allow(clippy::too_many_arguments)]
    fn render_list_item(
        ix: usize,
        field: &FlattenedField,
        parsed_field: &ParsedField,
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

        let focus_handle_left = focus_handle.clone();
        let focus_handle_right = focus_handle.clone();
        let id = SharedString::from(parsed_field.id.clone());
        let value = if let Some(label) = &parsed_field.enum_label {
            SharedString::from(format!("{} ({})", parsed_field.value, label))
        } else {
            SharedString::from(format!("{}", parsed_field.value))
        };
        let chevron_symbol = if field.has_children {
            if field.is_collapsed { "▶" } else { "▼" }
        } else {
            " "
        };

        let field_offset = parsed_field.offset;
        let field_id = parsed_field.id.clone();
        let field_val = value.to_string();

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
            .on_mouse_down(
                gpui::MouseButton::Left,
                window.listener_for(&view, move |this, _event: &gpui::MouseDownEvent, window, cx| {
                    focus_handle_left.focus(window);
                    this.select_item(ix, cx);
                }),
            )
            .on_mouse_down(
                gpui::MouseButton::Right,
                window.listener_for(&view, move |this, _event: &gpui::MouseDownEvent, window, cx| {
                    focus_handle_right.focus(window);
                    this.select_item(ix, cx);
                }),
            )
            .context_menu({
                let val_copy = field_val.clone();
                let id_copy = field_id.clone();
                let offset_hex = format!("0x{:08X}", field_offset);
                move |menu, _window, _cx| {
                    let off_h = offset_hex.clone();
                    let v_val = val_copy.clone();
                    let v_id = id_copy.clone();

                    menu.menu(format!("Go to Offset ({})", off_h), Box::new(crate::actions::GoToBeginning))
                        .separator()
                        .menu(format!("Copy Value ({})", v_val), Box::new(crate::actions::Copy))
                        .menu(format!("Copy Field Name ({})", v_id), Box::new(crate::actions::Copy))
                        .menu(format!("Copy Offset ({})", off_h), Box::new(crate::actions::Copy))
                        .separator()
                        .menu("Expand All", Box::new(crate::actions::ExpandAllStructure))
                        .menu("Collapse All", Box::new(crate::actions::CollapseAllStructure))
                }
            })
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
            .child(div().text_sm().text_color(theme.foreground).child(id))
            .child(div().text_sm().ml_auto().text_color(theme.muted_foreground).child(value))
            .into_any_element()
    }
}

impl Render for StructTreeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity().clone();
        let is_parsing = self.editor.as_ref().is_some_and(|ed| ed.read(cx).is_parsing_structure);
        let is_empty = self.flattened_fields.is_empty() && self.parse_result.is_none();
        let show_parsing_placeholder = is_parsing && self.flattened_fields.is_empty();
        let is_focused = self.focus_handle.is_focused(window);
        let theme = cx.theme();
        let parse_progress = self.editor.as_ref().and_then(|editor| {
            let editor = editor.read(cx);
            if !editor.is_parsing_structure && !editor.is_finalizing_structure {
                return None;
            }

            let total_bytes = if editor.parse_total_size > 0 {
                editor.parse_total_size
            } else {
                editor.total_size()
            };
            let progress = if total_bytes > 0 {
                (editor.parse_progress_offset as f32 / total_bytes as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };
            Some((progress, editor.is_finalizing_structure))
        });

        let has_structure = self.parse_result.is_some() || !self.flattened_fields.is_empty();

        let header_actions = if has_structure {
            Some(
                h_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        gpui_component::button::Button::new("expand-all-struct-btn")
                            .ghost()
                            .icon(IconName::ListTree)
                            .with_size(gpui_component::Size::XSmall)
                            .tooltip("Expand All")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.expand_all(cx);
                            })),
                    )
                    .child(
                        gpui_component::button::Button::new("collapse-all-struct-btn")
                            .ghost()
                            .icon(IconName::Minimize)
                            .with_size(gpui_component::Size::XSmall)
                            .tooltip("Collapse All")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.collapse_all(cx);
                            })),
                    )
                    .child(
                        gpui_component::button::Button::new("clear-structure-btn")
                            .ghost()
                            .icon(IconName::Close)
                            .with_size(gpui_component::Size::XSmall)
                            .tooltip("Clear Structure Definition")
                            .on_click(cx.listener(|_, _, window, cx| {
                                window.dispatch_action(Box::new(crate::actions::ClearStructureDefinition), cx);
                            })),
                    )
                    .into_any_element(),
            )
        } else {
            Some(
                gpui_component::button::Button::new("load-structure-header-btn")
                    .ghost()
                    .icon(IconName::FolderOpen)
                    .with_size(gpui_component::Size::XSmall)
                    .tooltip(if cfg!(target_os = "macos") {
                        "Load Structure Definition (cmd-shift-s)"
                    } else {
                        "Load Structure Definition (ctrl-shift-s)"
                    })
                    .on_click(cx.listener(|_, _, window, cx| {
                        window.dispatch_action(Box::new(crate::actions::LoadStructureDefinition), cx);
                    }))
                    .into_any_element(),
            )
        };

        let header = crate::ui::style::panel_header("STRUCTURE", is_focused, theme, None, header_actions);

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
            .on_action(cx.listener(Self::on_action_expand_all))
            .on_action(cx.listener(Self::on_action_collapse_all))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, _| {
                    this.focus_handle.focus(window);
                }),
            )
            .child(header)
            .when_some(parse_progress, |el, (progress, is_finalizing)| {
                el.child(
                    div()
                        .id("structure-parse-progress")
                        .h(px(3.0))
                        .w_full()
                        .overflow_hidden()
                        .bg(theme.border.opacity(0.35))
                        .child(div().h_full().w(relative(progress)).bg(if is_finalizing { theme.yellow } else { theme.accent })),
                )
            })
            .child(div().flex_1().min_h_0().w_full().overflow_hidden().child(if show_parsing_placeholder {
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
                                if let Some(parsed_field) = this.field_at_path(&field.path) {
                                    Self::render_list_item(ix, field, parsed_field, is_selected, is_focused, view.clone(), &focus_handle, window, cx)
                                } else {
                                    div().into_any_element()
                                }
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
