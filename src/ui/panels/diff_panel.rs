use crate::core::diff::{DiffChunk, DiffResult};
use crate::core::document::Document;
use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::dock::{Panel, PanelEvent};
use gpui_component::{ActiveTheme, IconName};
use std::sync::{Arc, RwLock};

use crate::actions::{NextDifference, PrevDifference, ToggleSyncScroll};
use crate::core::appearance::Appearance;
use crate::core::editor::Editor;
use crate::ui::components::hex_view::{HexView, HexViewEvent};

const CONTEXT: &str = "DiffPanel";

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("f3", NextDifference, Some(CONTEXT)),
        KeyBinding::new("shift-f3", PrevDifference, Some(CONTEXT)),
        KeyBinding::new("ctrl-l", ToggleSyncScroll, Some(CONTEXT)),
    ]);
}

pub struct DiffPanel {
    left_document: Arc<RwLock<Document>>,
    right_document: Arc<RwLock<Document>>,
    left_view: Entity<HexView>,
    right_view: Entity<HexView>,
    diff_result: Option<DiffResult>,
    current_diff_index: usize,
    focus_handle: FocusHandle,
    sync_scroll: bool,
    is_syncing: bool,
    _subscriptions: Vec<Subscription>,
}

impl DiffPanel {
    pub fn new(left_document: Arc<RwLock<Document>>, right_document: Arc<RwLock<Document>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let left_editor = cx.new(|_cx| Editor::new(left_document.clone()));
        let right_editor = cx.new(|_cx| Editor::new(right_document.clone()));
        let appearance = cx.global::<Appearance>().clone();
        let left_view = cx.new(|cx| {
            HexView::new(left_editor, cx)
                .font_family(appearance.font_family.clone())
                .font_size(px(appearance.font_size))
        });
        let right_view = cx.new(|cx| {
            HexView::new(right_editor, cx)
                .font_family(appearance.font_family.clone())
                .font_size(px(appearance.font_size))
        });

        let focus_handle = cx.focus_handle();

        let left_focus_handle = left_view.read(cx).focus_handle(cx);
        cx.on_focus_in(&focus_handle, window, {
            let left_focus_handle = left_focus_handle.clone();
            let focus_handle = focus_handle.clone();
            move |_, window, cx| {
                if window.focused(cx).as_ref() == Some(&focus_handle) {
                    left_focus_handle.focus(window);
                }
            }
        })
        .detach();

        let mut subscriptions = Vec::new();

        subscriptions.push(cx.subscribe_in(&left_view, window, |this, _left_view, event, _window, cx| {
            if this.sync_scroll
                && !this.is_syncing
                && let HexViewEvent::Scrolled(offset) = event
            {
                this.is_syncing = true;
                this.right_view.update(cx, |view, cx| {
                    view.scroll_to_row(*offset, cx);
                });
                this.is_syncing = false;
            }
        }));

        subscriptions.push(cx.subscribe_in(&right_view, window, |this, _right_view, event, _window, cx| {
            if this.sync_scroll
                && !this.is_syncing
                && let HexViewEvent::Scrolled(offset) = event
            {
                this.is_syncing = true;
                this.left_view.update(cx, |view, cx| {
                    view.scroll_to_row(*offset, cx);
                });
                this.is_syncing = false;
            }
        }));

        subscriptions.push(cx.observe_global::<Appearance>(|this, cx| {
            let appearance = cx.global::<Appearance>();
            let font_family = appearance.font_family.clone();
            let font_size = appearance.font_size;
            this.left_view.update(cx, |view, cx| {
                view.set_font_family(font_family.clone(), cx);
                view.set_font_size(px(font_size), cx);
            });
            this.right_view.update(cx, |view, cx| {
                view.set_font_family(font_family, cx);
                view.set_font_size(px(font_size), cx);
            });
        }));

        Self {
            left_document,
            right_document,
            left_view,
            right_view,
            diff_result: None,
            current_diff_index: 0,
            focus_handle,
            sync_scroll: true,
            is_syncing: false,
            _subscriptions: subscriptions,
        }
    }

    pub fn set_diff_result(&mut self, result: DiffResult, cx: &mut Context<Self>) {
        self.diff_result = Some(result);
        self.current_diff_index = 0;
        self.update_highlights(cx);
    }

    fn update_highlights(&mut self, cx: &mut Context<Self>) {
        if let Some(diff_result) = &self.diff_result {
            let mut left_highlights = Vec::new();
            let mut right_highlights = Vec::new();

            for chunk in &diff_result.chunks {
                if let DiffChunk::Modified { offset, length } = chunk {
                    left_highlights.push(*offset..*offset + *length);
                    right_highlights.push(*offset..*offset + *length);
                }
            }

            self.left_view.update(cx, |view, cx| {
                view.set_highlight_ranges(left_highlights, cx);
            });

            self.right_view.update(cx, |view, cx| {
                view.set_highlight_ranges(right_highlights, cx);
            });
        }
    }

    fn next_difference(&mut self, _: &NextDifference, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(diff_result) = &self.diff_result {
            let modified_chunks: Vec<_> = diff_result.chunks.iter().filter(|c| matches!(c, DiffChunk::Modified { .. })).collect();

            if !modified_chunks.is_empty() {
                self.current_diff_index = (self.current_diff_index + 1) % modified_chunks.len();
                self.scroll_to_current_diff(cx);
            }
        }
    }

    fn prev_difference(&mut self, _: &PrevDifference, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(diff_result) = &self.diff_result {
            let modified_chunks: Vec<_> = diff_result.chunks.iter().filter(|c| matches!(c, DiffChunk::Modified { .. })).collect();

            if !modified_chunks.is_empty() {
                if self.current_diff_index == 0 {
                    self.current_diff_index = modified_chunks.len() - 1;
                } else {
                    self.current_diff_index -= 1;
                }
                self.scroll_to_current_diff(cx);
            }
        }
    }

    fn toggle_sync_scroll(&mut self, _: &ToggleSyncScroll, _window: &mut Window, cx: &mut Context<Self>) {
        self.sync_scroll = !self.sync_scroll;
        cx.notify();
    }

    fn scroll_to_current_diff(&mut self, cx: &mut Context<Self>) {
        if let Some(diff_result) = &self.diff_result {
            let modified_chunks: Vec<_> = diff_result.chunks.iter().filter(|c| matches!(c, DiffChunk::Modified { .. })).collect();

            if let Some(DiffChunk::Modified { offset, .. }) = modified_chunks.get(self.current_diff_index) {
                let offset = *offset;
                self.left_view.update(cx, |view, cx| {
                    view.scroll_to_byte(offset, cx);
                });
                self.right_view.update(cx, |view, cx| {
                    view.scroll_to_byte(offset, cx);
                });
            }
        }
    }

    fn left_path(&self) -> std::path::PathBuf {
        self.left_document.read().expect("left document read lock").path().to_path_buf()
    }

    fn right_path(&self) -> std::path::PathBuf {
        self.right_document.read().expect("right document read lock").path().to_path_buf()
    }
}

impl EventEmitter<PanelEvent> for DiffPanel {}

impl Focusable for DiffPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for DiffPanel {
    fn panel_name(&self) -> &'static str {
        "DiffPanel"
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let left_name = self
            .left_path()
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or("Unknown".to_string());
        let right_name = self
            .right_path()
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or("Unknown".to_string());
        format!("Diff: {} ↔ {}", left_name, right_name)
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

    fn set_active(&mut self, active: bool, window: &mut Window, _cx: &mut Context<Self>) {
        if active {
            self.focus_handle.focus(window);
        }
    }

    fn set_zoomed(&mut self, _zoomed: bool, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn dump(&self, _cx: &App) -> gpui_component::dock::PanelState {
        let mut state = gpui_component::dock::PanelState::new(self);
        let diff_state = DiffPanelState {
            left_path: self.left_path().to_string_lossy().to_string(),
            right_path: self.right_path().to_string_lossy().to_string(),
        };
        state.info = gpui_component::dock::PanelInfo::panel(serde_json::to_value(diff_state).expect("serialize diff_state"));
        state
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DiffPanelState {
    pub left_path: String,
    pub right_path: String,
}

impl Render for DiffPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let sync_scroll = self.sync_scroll;
        let diff_count = self
            .diff_result
            .as_ref()
            .map(|r| r.chunks.iter().filter(|c| matches!(c, DiffChunk::Modified { .. })).count())
            .unwrap_or(0);
        let current_index = if diff_count > 0 { self.current_diff_index + 1 } else { 0 };
        let container = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle);

        container
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        if sync_scroll {
                            Button::new("sync-scroll").icon(IconName::Check).primary().label("Sync")
                        } else {
                            Button::new("sync-scroll").icon(IconName::Minus).ghost().label("Sync")
                        }
                        .on_click(cx.listener(|this, _, _window, cx| {
                            this.sync_scroll = !this.sync_scroll;
                            cx.notify();
                        })),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(format!("Diff {}/{}", current_index, diff_count)),
                    )
                    .child(
                        Button::new("prev-diff")
                            .icon(IconName::ChevronUp)
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.prev_difference(&PrevDifference, window, cx);
                            })),
                    )
                    .child(
                        Button::new("next-diff")
                            .icon(IconName::ChevronDown)
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.next_difference(&NextDifference, window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .child(div().flex_1().h_full().border_r_1().border_color(theme.border).child(self.left_view.clone()))
                    .child(div().flex_1().h_full().child(self.right_view.clone())),
            )
            .on_action(cx.listener(Self::next_difference))
            .on_action(cx.listener(Self::prev_difference))
            .on_action(cx.listener(Self::toggle_sync_scroll))
    }
}
