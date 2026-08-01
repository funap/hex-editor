use crate::core::editor::Editor;
use crate::core::structure::ParsedField;
use crate::ui::style::StyleExt as _;
use gpui::prelude::*;
use gpui::*;
use gpui_component::{ActiveTheme as _, v_flex};

actions!(struct_tree, [MoveUp, MoveDown,]);

const CONTEXT: &str = "StructTreeView";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", MoveUp, Some(CONTEXT)),
        KeyBinding::new("down", MoveDown, Some(CONTEXT)),
        KeyBinding::new("k", MoveUp, Some(CONTEXT)),
        KeyBinding::new("j", MoveDown, Some(CONTEXT)),
    ]);
}

pub struct StructTreeView {
    pub fields: Vec<crate::core::structure::ParsedField>,
    pub flattened_fields: Vec<FlattenedField>,
    pub editor: Option<Entity<Editor>>,
    pub scroll_handle: UniformListScrollHandle,
    pub focus_handle: FocusHandle,
    pub selected_index: Option<usize>,
    last_parse_id: Option<(String, usize, usize, usize)>,
    _editor_subscription: Option<Subscription>,
}

#[derive(Clone)]
pub struct FlattenedField {
    pub id: SharedString,
    pub _field_type: SharedString,
    pub offset: usize,
    pub size: usize,
    pub value_str: SharedString,
    pub color: Hsla,
    pub depth: usize,
}

impl StructTreeView {
    pub fn new(fields: Vec<crate::core::structure::ParsedField>, editor: Option<Entity<Editor>>, cx: &mut Context<Self>) -> Self {
        let mut flattened = Vec::new();
        Self::flatten_fields(&fields, 0, &mut flattened);
        let scroll_handle = UniformListScrollHandle::new();
        let focus_handle = cx.focus_handle();

        let mut this = Self {
            fields,
            flattened_fields: flattened,
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
        self.last_parse_id = None;

        self.set_fields(Vec::new(), cx);

        if let Some(ed) = editor {
            self._editor_subscription = Some(cx.observe(&ed, |this, editor, cx| {
                this.sync_fields(&editor, cx);
            }));
            self.sync_fields(&ed, cx);
        }
        cx.notify();
    }

    fn sync_fields(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let (current_parse_id, cursor_offset) = {
            let editor_lock = editor.read(cx);
            let doc_version = editor_lock.document.read().ok().map(|d| d.history.version()).unwrap_or(0);
            let parse_id = editor_lock
                .parse_result
                .as_ref()
                .map(|r| (r.definition_id.clone(), r.total_parsed_bytes, r.fields.len(), doc_version));
            (parse_id, editor_lock.cursor_offset)
        };

        if current_parse_id != self.last_parse_id {
            let fields = editor.read(cx).parse_result.as_ref().map(|res| res.fields.clone()).unwrap_or_default();
            self.set_fields(fields, cx);
            self.last_parse_id = current_parse_id;
        }

        self.sync_selected_index_from_cursor(cursor_offset, cx);
    }

    fn sync_selected_index_from_cursor(&mut self, cursor_offset: usize, cx: &mut Context<Self>) {
        if self.flattened_fields.is_empty() {
            return;
        }

        if let Some(curr_idx) = self.selected_index {
            if curr_idx < self.flattened_fields.len() {
                let field = &self.flattened_fields[curr_idx];
                let end = field.offset + field.size;
                if cursor_offset >= field.offset && (cursor_offset < end || (field.size == 0 && cursor_offset == field.offset)) {
                    return;
                }
            }
        }

        let upper_bound = self.flattened_fields.partition_point(|f| f.offset <= cursor_offset);
        if upper_bound == 0 {
            return;
        }

        let mut best_match: Option<(usize, usize, usize)> = None;
        for i in (0..upper_bound).rev() {
            let field = &self.flattened_fields[i];
            let end = field.offset + field.size;
            let matches = if field.size > 0 {
                cursor_offset >= field.offset && cursor_offset < end
            } else {
                cursor_offset == field.offset
            };

            if matches {
                match best_match {
                    None => {
                        best_match = Some((i, field.depth, field.size));
                    }
                    Some((_, best_depth, best_size)) => {
                        if field.depth > best_depth || (field.depth == best_depth && field.size < best_size) {
                            best_match = Some((i, field.depth, field.size));
                        }
                    }
                }
            } else if field.offset + field.size < cursor_offset && best_match.is_some() {
                break;
            }
        }

        if let Some((idx, _, _)) = best_match {
            if self.selected_index != Some(idx) {
                self.selected_index = Some(idx);
                self.scroll_handle.scroll_to_item(idx, ScrollStrategy::Top);
                cx.notify();
            }
        }
    }

    pub fn set_fields(&mut self, fields: Vec<ParsedField>, cx: &mut Context<Self>) {
        let mut flattened = Vec::new();
        Self::flatten_fields(&fields, 0, &mut flattened);
        self.fields = fields;
        self.flattened_fields = flattened;
        self.selected_index = None;
        self.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn flatten_fields(fields: &[ParsedField], depth: usize, results: &mut Vec<FlattenedField>) {
        for field in fields {
            let val_str = if let Some(label) = &field.enum_label {
                SharedString::from(format!("{} ({})", field.value, label))
            } else {
                SharedString::from(format!("{}", field.value))
            };

            results.push(FlattenedField {
                id: SharedString::from(field.id.clone()),
                _field_type: SharedString::from(field.field_type.clone()),
                offset: field.offset,
                size: field.size,
                value_str: val_str,
                color: field.color,
                depth,
            });

            if !field.children.is_empty() {
                Self::flatten_fields(&field.children, depth + 1, results);
            }
        }
    }

    fn select_item(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx >= self.flattened_fields.len() {
            return;
        }

        self.selected_index = Some(idx);
        self.scroll_handle.scroll_to_item(idx, ScrollStrategy::Top);

        let offset = self.flattened_fields[idx].offset;
        if let Some(editor) = &self.editor {
            editor.update(cx, |editor, cx| {
                editor.set_cursor_offset(offset);
                cx.notify();
            });
        }

        cx.notify();
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

    fn render_list_item(
        ix: usize,
        field: &FlattenedField,
        is_selected: bool,
        is_focused: bool,
        view: Entity<Self>,
        focus_handle: &FocusHandle,
        cx: &App,
    ) -> AnyElement {
        let padding_left = px(16.0 * field.depth as f32 + 12.0);
        let theme = cx.theme();
        let bg_color = if is_selected {
            if is_focused { theme.selection } else { theme.muted_foreground.opacity(0.3) }
        } else {
            hsla(0.0, 0.0, 0.0, 0.0)
        };

        let focus_handle = focus_handle.clone();
        div()
            .id(ix)
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h(px(24.0))
            .bg(bg_color)
            .px_3()
            .pl(padding_left)
            .gap_2()
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(10.0))
                    .h(px(10.0))
                    .bg(field.color)
                    .border_1()
                    .border_color(theme.border),
            )
            .child(div().text_sm().text_color(theme.foreground).child(field.id.clone()))
            .child(div().text_sm().ml_auto().text_color(theme.muted_foreground).child(field.value_str.clone()))
            .on_click(move |_, window, cx| {
                focus_handle.focus(window);
                view.update(cx, |this, cx| {
                    this.select_item(ix, cx);
                });
            })
            .into_any_element()
    }
}

impl Render for StructTreeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity().clone();
        let is_empty = self.fields.is_empty();
        let is_focused = self.focus_handle.is_focused(window);
        let theme = cx.theme();

        let container = v_flex().size_full().flex_shrink_0().bg(theme.sidebar);
        let container = container.focus_indicator(is_focused, theme);

        container
            .id("struct-tree-view")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, _| {
                    this.focus_handle.focus(window);
                }),
            )
            .child(
                div()
                    .p_2()
                    .text_sm()
                    .text_color(crate::ui::style::header_text_color(is_focused, theme))
                    .child("STRUCTURE"),
            )
            .child(if is_empty {
                v_flex()
                    .size_full()
                    .justify_center()
                    .items_center()
                    .child(div().text_color(theme.muted_foreground).child("No structure loaded"))
                    .into_any_element()
            } else {
                let focus_handle = self.focus_handle.clone();
                uniform_list("struct-tree-list", self.flattened_fields.len(), move |range, _window, cx| {
                    let this = view.read(cx);
                    range
                        .map(|ix| {
                            if let Some(field) = this.flattened_fields.get(ix) {
                                let is_selected = this.selected_index == Some(ix);
                                Self::render_list_item(ix, field, is_selected, is_focused, view.clone(), &focus_handle, cx)
                            } else {
                                div().into_any_element()
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .track_scroll(self.scroll_handle.clone())
                .size_full()
                .into_any_element()
            })
    }
}

impl Focusable for StructTreeView {
    fn focus_handle(&self, _cx: &App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}
