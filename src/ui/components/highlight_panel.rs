use crate::core::editor::Editor;
use crate::core::highlight::{HighlightColor, HighlightItem};
use crate::ui::style::StyleExt as _;
use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{self, Input, InputState};
use gpui_component::{ActiveTheme as _, Disableable, Icon, IconName, Sizable, Size, StyledExt, h_flex, v_flex};

#[allow(dead_code)]
pub enum HighlightPanelEvent {
    NavigateTo { offset: usize, size: usize },
    Export,
    Import,
}

pub struct HighlightPanel {
    pub editor: Option<Entity<Editor>>,
    pub focus_handle: FocusHandle,
    pub selected_id: Option<String>,
    pub editing_id: Option<String>,
    pub comment_input: Entity<InputState>,
    pub color_picker_id: Option<String>,
    _editor_subscription: Option<Subscription>,
    _input_subscription: Option<Subscription>,
}

impl EventEmitter<HighlightPanelEvent> for HighlightPanel {}

impl HighlightPanel {
    pub fn new(editor: Option<Entity<Editor>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let comment_input = cx.new(|cx| InputState::new(window, cx).placeholder("Add a comment..."));

        let input_sub = cx.subscribe(&comment_input, |this, _, event: &input::InputEvent, cx| {
            if let input::InputEvent::PressEnter { .. } = event {
                this.save_editing_comment(cx);
            }
        });

        let mut this = Self {
            editor: None,
            focus_handle,
            selected_id: None,
            editing_id: None,
            comment_input,
            color_picker_id: None,
            _editor_subscription: None,
            _input_subscription: Some(input_sub),
        };

        this.set_editor(editor, cx);
        this
    }

    pub fn set_editor(&mut self, editor: Option<Entity<Editor>>, cx: &mut Context<Self>) {
        self._editor_subscription = None;
        self.editor = editor.clone();
        self.selected_id = None;
        self.editing_id = None;
        self.color_picker_id = None;

        if let Some(ed) = &editor {
            self._editor_subscription = Some(cx.observe(ed, |_, _, cx| {
                cx.notify();
            }));
        }
        cx.notify();
    }

    fn add_highlight_from_selection(&mut self, cx: &mut Context<Self>) {
        let Some(editor_entity) = &self.editor else { return };
        let (range, _) = {
            let editor = editor_entity.read(cx);
            if let Some(r) = editor.selected_range_or_cursor() {
                (r, editor.total_size())
            } else {
                return;
            }
        };

        let new_item = HighlightItem::new(range.start, range.len(), HighlightColor::Yellow, "");
        let mut actual_id = String::new();

        editor_entity.update(cx, |editor, cx| {
            actual_id = editor.add_highlight(new_item);
            cx.notify();
        });

        if !actual_id.is_empty() {
            self.selected_id = Some(actual_id);
        }
        cx.notify();
    }

    fn start_editing_comment(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let current_comment = self
            .editor
            .as_ref()
            .and_then(|ed| {
                let editor = ed.read(cx);
                editor.highlights.iter().find(|h| h.id == id).map(|h| h.comment.clone())
            })
            .unwrap_or_default();

        self.editing_id = Some(id.clone());
        self.selected_id = Some(id);
        self.color_picker_id = None;

        self.comment_input.update(cx, |input, cx| {
            input.set_value(current_comment, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn save_editing_comment(&mut self, cx: &mut Context<Self>) {
        let Some(editing_id) = self.editing_id.take() else { return };
        let new_comment = self.comment_input.read(cx).value().to_string();

        if let Some(ed) = &self.editor {
            ed.update(cx, |editor, cx| {
                editor.update_highlight_comment(&editing_id, new_comment);
                cx.notify();
            });
        }
        cx.notify();
    }

    fn cancel_editing_comment(&mut self, cx: &mut Context<Self>) {
        self.editing_id = None;
        cx.notify();
    }

    fn set_highlight_color(&mut self, id: &str, color: HighlightColor, cx: &mut Context<Self>) {
        if let Some(ed) = &self.editor {
            ed.update(cx, |editor, cx| {
                editor.update_highlight_color(id, color);
                cx.notify();
            });
        }
        self.color_picker_id = None;
        cx.notify();
    }

    fn delete_highlight(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(ed) = &self.editor {
            ed.update(cx, |editor, cx| {
                editor.remove_highlight_by_id(id);
                cx.notify();
            });
        }
        if self.selected_id.as_deref() == Some(id) {
            self.selected_id = None;
        }
        if self.editing_id.as_deref() == Some(id) {
            self.editing_id = None;
        }
        if self.color_picker_id.as_deref() == Some(id) {
            self.color_picker_id = None;
        }
        cx.notify();
    }

    fn clear_all_highlights(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor_entity) = &self.editor else { return };
        let count = editor_entity.read(cx).highlights.len();
        if count == 0 {
            return;
        }

        let prompt = window.prompt(
            gpui::PromptLevel::Warning,
            "Clear all highlights?",
            Some(&format!(
                "Are you sure you want to clear all {} highlight{} and comments? This action cannot be undone.",
                count,
                if count == 1 { "" } else { "s" }
            )),
            &["Clear All", "Cancel"],
            cx,
        );

        let editor_entity = editor_entity.clone();
        cx.spawn_in(window, async move |this, window| {
            if let Ok(0) = prompt.await {
                window
                    .update(|_, cx| {
                        editor_entity.update(cx, |editor, cx| {
                            editor.clear_all_custom_highlights();
                            cx.notify();
                        });
                        let _ = this.update(cx, |panel, cx| {
                            panel.selected_id = None;
                            panel.editing_id = None;
                            panel.color_picker_id = None;
                            cx.notify();
                        });
                    })
                    .ok();
            }
        })
        .detach();
    }

    fn navigate_to_highlight(&mut self, offset: usize, size: usize, cx: &mut Context<Self>) {
        if let Some(ed) = &self.editor {
            ed.update(cx, |editor, cx| {
                editor.set_cursor_offset(offset);
                if size > 1 {
                    editor.selection_start = Some(offset);
                    editor.selection_end = Some(offset + size - 1);
                } else {
                    editor.selection_start = None;
                    editor.selection_end = None;
                }
                cx.notify();
            });
        }
        cx.emit(HighlightPanelEvent::NavigateTo { offset, size });
        cx.notify();
    }
}

impl Focusable for HighlightPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for HighlightPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let is_focused = self.focus_handle.is_focused(window);

        let (highlights, has_editor) = if let Some(ed) = &self.editor {
            let editor = ed.read(cx);
            (editor.highlights.clone(), true)
        } else {
            (Vec::new(), false)
        };

        let count = highlights.len();

        // Header toolbar
        let header = h_flex()
            .justify_between()
            .items_center()
            .px_2()
            .py_1p5()
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .items_center()
                    .gap_1p5()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(crate::ui::style::header_text_color(is_focused, &theme))
                            .child("HIGHLIGHTS"),
                    )
                    .child(
                        div()
                            .px_1p5()
                            .py_0p5()
                            .rounded_sm()
                            .bg(theme.muted.opacity(0.6))
                            .text_xs()
                            .font_family("Courier New")
                            .text_color(theme.muted_foreground)
                            .child(count.to_string()),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new("add-hl")
                            .ghost()
                            .icon(IconName::Plus)
                            .with_size(Size::XSmall)
                            .tooltip("Add highlight at current selection / cursor")
                            .disabled(!has_editor)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.add_highlight_from_selection(cx);
                            })),
                    )
                    .child(
                        Button::new("import-hl")
                            .ghost()
                            .icon(IconName::FolderOpen)
                            .with_size(Size::XSmall)
                            .tooltip("Import highlights from JSON file")
                            .disabled(!has_editor)
                            .on_click(cx.listener(|_, _, _window, cx| {
                                cx.emit(HighlightPanelEvent::Import);
                            })),
                    )
                    .child(
                        Button::new("export-hl")
                            .ghost()
                            .icon(IconName::ExternalLink)
                            .with_size(Size::XSmall)
                            .tooltip("Export highlights to JSON file")
                            .disabled(!has_editor || count == 0)
                            .on_click(cx.listener(|_, _, _window, cx| {
                                cx.emit(HighlightPanelEvent::Export);
                            })),
                    )
                    .child(
                        Button::new("clear-hl")
                            .ghost()
                            .icon(IconName::Delete)
                            .with_size(Size::XSmall)
                            .tooltip("Clear all highlights")
                            .disabled(!has_editor || count == 0)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.clear_all_highlights(window, cx);
                            })),
                    ),
            );

        // Content body
        let body = if !has_editor {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .p_4()
                .gap_2()
                .child(Icon::new(IconName::Palette).size(px(28.0)).text_color(theme.muted_foreground.opacity(0.5)))
                .child(div().text_xs().text_color(theme.muted_foreground).child("No Active File"))
                .into_any_element()
        } else if highlights.is_empty() {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .p_4()
                .gap_2()
                .child(Icon::new(IconName::Palette).size(px(28.0)).text_color(theme.muted_foreground.opacity(0.5)))
                .child(div().text_xs().font_medium().text_color(theme.foreground).child("No Highlights"))
                .child(
                    div()
                        .text_xs()
                        .text_center()
                        .text_color(theme.muted_foreground)
                        .child("Select bytes in hex view and choose a color, or click '+' above to add."),
                )
                .into_any_element()
        } else {
            let mut list = v_flex().flex_1().gap_1().p_1();

            for item in highlights {
                list = list.child(self.render_highlight_item(&item, &theme, window, cx));
            }

            v_flex()
                .flex_1()
                .overflow_hidden()
                .child(div().id("highlights-scroll").flex_1().overflow_y_scroll().child(list))
                .into_any_element()
        };

        v_flex()
            .track_focus(&self.focus_handle)
            .focus_indicator(is_focused, &theme)
            .w_full()
            .h_full()
            .bg(theme.sidebar)
            .overflow_hidden()
            .child(header)
            .child(body)
    }
}

impl HighlightPanel {
    fn render_highlight_item(&self, item: &HighlightItem, theme: &gpui_component::Theme, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let item_id = item.id.clone();
        let is_selected = self.selected_id.as_deref() == Some(&item_id);
        let is_editing = self.editing_id.as_deref() == Some(&item_id);
        let show_color_picker = self.color_picker_id.as_deref() == Some(&item_id);

        let offset = item.offset;
        let size = item.size;
        let badge_color = item.color.to_badge_hsla();

        let item_id_edit = item_id.clone();
        let item_id_del = item_id.clone();
        let item_id_color = item_id.clone();
        let item_id_select = item_id.clone();

        let bg_color = if is_selected { theme.selection } else { theme.sidebar };

        let mut row_container = v_flex()
            .w_full()
            .rounded_md()
            .border_1()
            .border_color(if is_selected { theme.accent } else { theme.border.opacity(0.5) })
            .bg(bg_color)
            .p_2()
            .gap_1p5();

        // 1. Header row: Color Dot + Offset + Size + Action Buttons
        let header_row = h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id(SharedString::from(format!("color-badge-{}", item_id)))
                            .w(px(12.0))
                            .h(px(12.0))
                            .rounded_full()
                            .bg(badge_color)
                            .cursor_pointer()
                            .on_click(cx.listener({
                                let item_id = item_id_color.clone();
                                move |this, _, _window, cx| {
                                    if this.color_picker_id.as_deref() == Some(&item_id) {
                                        this.color_picker_id = None;
                                    } else {
                                        this.color_picker_id = Some(item_id.clone());
                                    }
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("nav-link-{}", item_id)))
                            .cursor_pointer()
                            .on_click(cx.listener({
                                let item_id = item_id_select.clone();
                                move |this, _, _window, cx| {
                                    this.selected_id = Some(item_id.clone());
                                    this.navigate_to_highlight(offset, size, cx);
                                }
                            }))
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .font_family("Courier New")
                                            .text_xs()
                                            .font_semibold()
                                            .text_color(theme.foreground)
                                            .child(item.format_offset()),
                                    )
                                    .child(
                                        div()
                                            .px_1()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(theme.muted.opacity(0.5))
                                            .text_xs()
                                            .font_family("Courier New")
                                            .text_color(theme.muted_foreground)
                                            .child(format!("{} B", item.size)),
                                    ),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_0p5()
                    .child(
                        Button::new(SharedString::from(format!("nav-{}", item_id)))
                            .ghost()
                            .icon(IconName::Search)
                            .with_size(Size::XSmall)
                            .tooltip("Go to offset")
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.navigate_to_highlight(offset, size, cx);
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("edit-{}", item_id)))
                            .ghost()
                            .icon(IconName::Settings2)
                            .with_size(Size::XSmall)
                            .tooltip("Edit comment")
                            .on_click(cx.listener({
                                let item_id = item_id_edit.clone();
                                move |this, _, window, cx| {
                                    this.start_editing_comment(item_id.clone(), window, cx);
                                }
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("del-{}", item_id)))
                            .ghost()
                            .icon(IconName::Close)
                            .with_size(Size::XSmall)
                            .tooltip("Delete highlight")
                            .on_click(cx.listener({
                                let item_id = item_id_del.clone();
                                move |this, _, _window, cx| {
                                    this.delete_highlight(&item_id, cx);
                                }
                            })),
                    ),
            );

        row_container = row_container.child(header_row);

        // 2. Optional Color Picker row
        if show_color_picker {
            let mut picker_row = h_flex().items_center().gap_1().py_1().px_1().bg(theme.muted.opacity(0.3)).rounded_sm();
            for &preset in HighlightColor::ALL_PRESETS {
                let p_badge = preset.to_badge_hsla();
                let item_id_for_preset = item_id.clone();
                let is_current = item.color == preset;

                picker_row = picker_row.child(
                    div()
                        .id(SharedString::from(format!("palette-{}-{}", item_id, preset.name())))
                        .w(px(14.0))
                        .h(px(14.0))
                        .rounded_full()
                        .bg(p_badge)
                        .cursor_pointer()
                        .border_1()
                        .border_color(if is_current { theme.foreground } else { hsla(0.0, 0.0, 0.0, 0.0) })
                        .hover(|s| s.opacity(0.8))
                        .on_click(cx.listener({
                            let item_id = item_id_for_preset.clone();
                            move |this, _, _window, cx| {
                                this.set_highlight_color(&item_id, preset, cx);
                            }
                        })),
                );
            }
            row_container = row_container.child(picker_row);
        }

        // 3. Comment Display or Edit Mode
        if is_editing {
            let edit_box = v_flex()
                .gap_1p5()
                .p_1()
                .bg(theme.background)
                .rounded_sm()
                .child(Input::new(&self.comment_input).cleanable(true))
                .child(
                    h_flex()
                        .justify_end()
                        .gap_1()
                        .child(
                            Button::new("cancel-edit")
                                .ghost()
                                .with_size(Size::XSmall)
                                .label("Cancel")
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.cancel_editing_comment(cx);
                                })),
                        )
                        .child(
                            Button::new("save-edit")
                                .primary()
                                .with_size(Size::XSmall)
                                .label("Save")
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.save_editing_comment(cx);
                                })),
                        ),
                );
            row_container = row_container.child(edit_box);
        } else {
            let comment_text = if item.comment.trim().is_empty() {
                div().text_xs().italic().text_color(theme.muted_foreground.opacity(0.7)).child("(No comment)")
            } else {
                div().text_xs().text_color(theme.foreground).child(item.comment.clone())
            };

            let comment_row = div()
                .id(SharedString::from(format!("comment-row-{}", item_id)))
                .w_full()
                .cursor_pointer()
                .on_click(cx.listener({
                    let item_id = item_id.clone();
                    move |this, _, window, cx| {
                        this.start_editing_comment(item_id.clone(), window, cx);
                    }
                }))
                .child(comment_text);

            row_container = row_container.child(comment_row);
        }

        row_container
    }
}
