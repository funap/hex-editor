use gpui::prelude::*;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, KeyBinding, SharedString, Subscription, Task, WeakEntity, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dock::{Panel, PanelEvent, TabPanel};
use gpui_component::menu::PopupMenu;
use gpui_component::{ActiveTheme, Sizable};

use crate::actions::{FocusHexView, GoToBeginning, GoToEnd, SearchNext, SearchPrev, SelectAll, ToggleSearch};
use crate::app_state::AppState;
use crate::core::appearance::Appearance;
use crate::core::editor::Editor;
use crate::core::search::SearchMode;
use crate::ui::components::hex_view::{self, HexView};
use crate::ui::components::search_bar::{SearchBar, SearchBarEvent};

const CONTEXT: &str = "EditorPanel";

pub(crate) fn init(cx: &mut App) {
    // Initialize HexView actions and keybindings
    hex_view::init(cx);
    cx.bind_keys([
        KeyBinding::new("ctrl-f", ToggleSearch, Some(CONTEXT)),
        KeyBinding::new("cmd-f", ToggleSearch, Some(CONTEXT)),
        KeyBinding::new("f3", SearchNext, Some(CONTEXT)),
        KeyBinding::new("ctrl-g", SearchNext, Some(CONTEXT)),
        KeyBinding::new("cmd-g", SearchNext, Some(CONTEXT)),
        KeyBinding::new("shift-f3", SearchPrev, Some(CONTEXT)),
        KeyBinding::new("ctrl-shift-g", SearchPrev, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-g", SearchPrev, Some(CONTEXT)),
    ]);
}

pub struct EditorPanel {
    editor: Entity<Editor>,
    focus_handle: FocusHandle,
    hex_view: Entity<HexView>,
    is_search_visible: bool,
    search_bar: Entity<SearchBar>,
    search_task: Option<Task<()>>,
    viewport_search_task: Option<Task<()>>,
    tab_panel: Option<WeakEntity<TabPanel>>,
    _appearance_subscription: Subscription,
    _editor_subscription: Subscription,
}

impl EditorPanel {
    pub fn new(editor: Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let appearance = cx.global::<Appearance>().clone();
        let hex_view = cx.new(|cx| {
            HexView::new(editor.clone(), cx)
                .font_family(appearance.font_family.clone())
                .font_size(px(appearance.font_size))
        });
        let search_bar = cx.new(|cx| SearchBar::new(window, cx));

        cx.subscribe(&search_bar, |this, _, event: &SearchBarEvent, cx| match event {
            SearchBarEvent::IncrementalSearch(query, mode) => {
                this.perform_incremental_search(query, *mode, cx);
            }
            SearchBarEvent::FullSearch(query, mode) => {
                this.perform_full_search(query, *mode, cx);
            }
            SearchBarEvent::Next => {
                this.perform_search_next(cx);
            }
            SearchBarEvent::Prev => {
                this.perform_search_prev(cx);
            }
            SearchBarEvent::Dismiss => {
                this.is_search_visible = false;
                this.update_highlights(cx);
                cx.dispatch_action(&FocusHexView);
                cx.notify();
            }
        })
        .detach();

        let hex_focus_handle = hex_view.read(cx).focus_handle(cx);
        cx.on_focus_in(&focus_handle, window, {
            let hex_focus_handle = hex_focus_handle.clone();
            let focus_handle = focus_handle.clone();
            move |_, window, cx| {
                if window.focused(cx).as_ref() == Some(&focus_handle) {
                    hex_focus_handle.focus(window);
                }
            }
        })
        .detach();

        cx.on_focus_in(&hex_focus_handle, window, |_, _, cx| {
            cx.notify();
        })
        .detach();

        // Subscribe to HexView scroll events to update highlights when scrolling
        cx.subscribe(&hex_view, |this, _, event: &crate::ui::components::hex_view::HexViewEvent, cx| {
            if let crate::ui::components::hex_view::HexViewEvent::Scrolled(_) = event {
                // Update highlights if there's an active search
                if this.is_search_visible {
                    let editor = this.editor.read(cx);
                    if !editor.search_state.is_full_search_complete {
                        this.perform_viewport_search(cx);
                    }
                }
            }
        })
        .detach();

        let _appearance_subscription = cx.observe_global::<Appearance>(|this, cx| {
            let appearance = cx.global::<Appearance>();
            let font_family = appearance.font_family.clone();
            let font_size = appearance.font_size;
            this.hex_view.update(cx, |this_hex_view, cx| {
                this_hex_view.set_font_family(font_family, cx);
                this_hex_view.set_font_size(px(font_size), cx);
            });
        });

        let _editor_subscription = cx.observe(&editor, |this, _, cx| {
            this.update_search_bar_results(cx);
            this.update_highlights(cx);
            cx.notify();
        });

        // Observe search bar for incremental search
        cx.observe(&search_bar, |this, search_bar, cx| {
            if this.is_search_visible {
                let query = search_bar.read(cx).query(cx);
                let mode = search_bar.read(cx).mode();
                if query != this.editor.read(cx).search_state.query {
                    this.perform_incremental_search(&query, mode, cx);
                }
            }
        })
        .detach();

        Self {
            editor,
            focus_handle,
            hex_view,
            is_search_visible: false,
            search_bar,
            search_task: None,
            viewport_search_task: None,
            tab_panel: None,
            _appearance_subscription,
            _editor_subscription,
        }
    }

    pub fn editor(&self) -> Entity<Editor> {
        self.editor.clone()
    }

    pub fn path(&self, cx: &App) -> std::path::PathBuf {
        self.editor.read(cx).document.read().expect("document read lock").path().to_path_buf()
    }

    #[allow(dead_code)]
    pub fn tab_panel(&self) -> Option<WeakEntity<TabPanel>> {
        self.tab_panel.clone()
    }

    #[allow(dead_code)]
    pub fn create_split_clone(&self, window: &mut Window, cx: &mut App) -> Entity<EditorPanel> {
        let ed = self.editor.read(cx);
        let doc = ed.document.clone();
        let encoding = ed.encoding;
        let radix = ed.radix;
        let group_size = ed.group_size;
        let is_big_endian = ed.is_big_endian;
        let ksy_definition = ed.ksy_definition.clone();
        let parse_result = ed.parse_result.clone();
        let custom_breaks = ed.custom_breaks.clone();
        let custom_joins = ed.custom_joins.clone();
        let empty_lines = ed.empty_lines.clone();

        let new_editor = cx.new(|_| {
            let mut editor = Editor::new(doc);
            editor.encoding = encoding;
            editor.radix = radix;
            editor.group_size = group_size;
            editor.is_big_endian = is_big_endian;
            editor.ksy_definition = ksy_definition;
            editor.parse_result = parse_result;
            editor.custom_breaks = custom_breaks;
            editor.custom_joins = custom_joins;
            editor.empty_lines = empty_lines;
            editor
        });

        cx.new(|cx| EditorPanel::new(new_editor, window, cx))
    }

    fn toggle_search(&mut self, _: &ToggleSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.is_search_visible = !self.is_search_visible;
        if self.is_search_visible {
            self.search_bar.update(cx, |bar, cx| {
                bar.focus(window, cx);
            });
        } else {
            self.hex_view.read(cx).focus_handle(cx).focus(window);
        }
        cx.notify();
    }

    fn perform_incremental_search(&mut self, query: &str, mode: SearchMode, cx: &mut Context<Self>) {
        if query.is_empty() {
            self.editor.update(cx, |editor: &mut Editor, cx| {
                editor.clear_search();
                cx.notify();
            });
            self.update_highlights(cx);
            return;
        }

        self.editor.update(cx, |editor: &mut Editor, cx| {
            editor.set_search_query(query.to_string());
            cx.notify();
        });

        self.update_highlights(cx);
        self.perform_viewport_search(cx);
        self.perform_full_search(query, mode, cx);
    }

    fn perform_viewport_search(&mut self, cx: &mut Context<Self>) {
        let (start, end) = self.hex_view.read(cx).viewport_byte_range(cx);
        let query = self.editor.read(cx).search_state.query.clone();
        if query.is_empty() {
            return;
        }

        let mode = self.search_bar.read(cx).mode();
        let app_state = AppState::global(cx);

        let (_, viewport_task) = app_state.editor_service.incremental_search(self.editor.clone(), query, mode, start..end, cx);
        self.viewport_search_task = Some(viewport_task);
    }

    fn perform_full_search(&mut self, query: &str, mode: SearchMode, cx: &mut Context<Self>) {
        let (start, end) = self.hex_view.read(cx).viewport_byte_range(cx);
        let app_state = AppState::global(cx);

        let (viewport_task, full_task) = app_state
            .editor_service
            .incremental_search(self.editor.clone(), query.to_string(), mode, start..end, cx);
        self.viewport_search_task = Some(viewport_task);
        self.search_task = Some(full_task);
    }

    fn update_search_bar_results(&mut self, cx: &mut Context<Self>) {
        let editor = self.editor.read(cx);
        let count = editor.search_state.results.len();
        let current = editor.search_state.current_result_index;
        self.search_bar.update(cx, |bar, cx| {
            bar.set_results(count, current, cx);
        });
    }

    fn update_highlights(&mut self, cx: &mut Context<Self>) {
        let mut highlights = Vec::new();

        // 1. Add user custom highlights from editor
        let editor = self.editor.read(cx);
        highlights.extend(editor.custom_highlights.iter().cloned());

        // 2. Add search highlights if search is active
        let search_query = if self.is_search_visible {
            self.search_bar.read(cx).query(cx)
        } else {
            String::new()
        };

        if self.is_search_visible && !search_query.is_empty() {
            let bar = self.search_bar.read(cx);
            let query = bar.query(cx);
            let mode = bar.mode();
            let pattern_len = match mode {
                crate::core::search::SearchMode::Text => query.len(),
                crate::core::search::SearchMode::Hex => crate::core::search::parse_hex_pattern(&query).map(|pat| pat.len()).unwrap_or(0),
            };

            if pattern_len > 0 {
                let theme = cx.theme();
                let search_color = theme.accent;
                let current_result_color = theme.success;
                let current_offset = editor.current_search_result();

                for &result_offset in &editor.search_state.results {
                    let color = if Some(result_offset) == current_offset {
                        current_result_color
                    } else {
                        search_color
                    };
                    highlights.push((result_offset..result_offset + pattern_len, color));
                }
            }
        }

        self.hex_view.update(cx, |view, cx| {
            view.set_highlights(highlights, cx);
        });
    }

    fn highlight_current_result(&mut self, preserve_scroll: bool, cx: &mut Context<Self>) {
        let editor = self.editor.read(cx);
        if let Some(offset) = editor.current_search_result() {
            self.update_highlights(cx);

            // Scroll to current result if not preserving
            if !preserve_scroll {
                self.hex_view.update(cx, |view, cx| {
                    view.scroll_to_row(offset / 16, cx);
                });
                self.editor.update(cx, |editor, cx| {
                    editor.set_cursor_offset(offset);
                    cx.notify();
                });
            }
        }
    }

    fn search_next(&mut self, _: &SearchNext, _window: &mut Window, cx: &mut Context<Self>) {
        self.perform_search_next(cx);
    }

    fn perform_search_next(&mut self, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor: &mut Editor, _| {
            editor.next_search_result();
        });
        self.highlight_current_result(false, cx);
        cx.notify();
    }

    fn search_prev(&mut self, _: &SearchPrev, _window: &mut Window, cx: &mut Context<Self>) {
        self.perform_search_prev(cx);
    }

    fn perform_search_prev(&mut self, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor: &mut Editor, _| {
            editor.prev_search_result();
        });
        self.highlight_current_result(false, cx);
        cx.notify();
    }

    fn focus_hex_view(&mut self, _: &FocusHexView, window: &mut Window, cx: &mut Context<Self>) {
        self.hex_view.read(cx).focus_handle(cx).focus(window);
    }

    fn select_all(&mut self, _: &SelectAll, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor: &mut Editor, _| {
            editor.select_all();
        });
        cx.notify();
    }

    fn go_to_beginning(&mut self, _: &GoToBeginning, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor: &mut Editor, _| {
            editor.go_to_beginning();
        });
        cx.notify();
    }

    fn go_to_end(&mut self, _: &GoToEnd, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor: &mut Editor, _| {
            editor.go_to_end();
        });
        cx.notify();
    }
}

impl EventEmitter<PanelEvent> for EditorPanel {}

impl Focusable for EditorPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for EditorPanel {
    fn panel_name(&self) -> &'static str {
        "EditorPanel"
    }

    fn title(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.editor.read(cx);
        let doc = editor.document.read().expect("document read lock");

        let mut name = doc
            .path()
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "(untitled)".to_string());

        if doc.is_dirty() {
            name.push_str(" *");
        }

        name
    }

    fn tab_name(&self, cx: &App) -> Option<SharedString> {
        let editor = self.editor.read(cx);
        let doc = editor.document.read().expect("document read lock");

        let mut name = doc
            .path()
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "(untitled)".to_string());

        if doc.is_dirty() {
            name.push_str(" *");
        }

        Some(name.into())
    }

    fn closable(&self, _cx: &App) -> bool {
        true
    }

    fn zoomable(&self, _cx: &App) -> Option<gpui_component::dock::PanelControl> {
        Some(gpui_component::dock::PanelControl::Both)
    }

    fn visible(&self, _cx: &App) -> bool {
        true
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }

    fn on_added_to(&mut self, tab_panel: WeakEntity<TabPanel>, _window: &mut Window, _cx: &mut Context<Self>) {
        self.tab_panel = Some(tab_panel);
    }

    fn toolbar_buttons(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Option<Vec<Button>> {
        Some(vec![
            Button::new("split-right")
                .icon(gpui_component::IconName::PanelRight)
                .xsmall()
                .ghost()
                .tab_stop(false)
                .tooltip("Split Right (cmd-\\)")
                .on_click(cx.listener(|_, _, window, cx| {
                    window.dispatch_action(Box::new(crate::actions::SplitRight), cx);
                })),
            Button::new("split-down")
                .icon(gpui_component::IconName::PanelBottom)
                .xsmall()
                .ghost()
                .tab_stop(false)
                .tooltip("Split Down (cmd-shift-d)")
                .on_click(cx.listener(|_, _, window, cx| {
                    window.dispatch_action(Box::new(crate::actions::SplitDown), cx);
                })),
        ])
    }

    fn dropdown_menu(&mut self, this: PopupMenu, _window: &mut Window, _cx: &mut Context<Self>) -> PopupMenu {
        this.menu("Split Right", Box::new(crate::actions::SplitRight))
            .menu("Split Down", Box::new(crate::actions::SplitDown))
            .separator()
            .menu("Close Tab", Box::new(crate::actions::CloseActivePanel))
    }

    fn set_active(&mut self, active: bool, window: &mut Window, _cx: &mut Context<Self>) {
        if active {
            self.focus_handle.focus(window);
        }
    }

    fn set_zoomed(&mut self, _zoomed: bool, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn dump(&self, cx: &App) -> gpui_component::dock::PanelState {
        let mut state = gpui_component::dock::PanelState::new(self);
        let panel_state = EditorPanelState {
            path: Some(self.editor.read(cx).document.read().expect("document read lock").path().to_path_buf()),
        };
        state.info = gpui_component::dock::PanelInfo::panel(panel_state.to_value());
        state
    }
}

impl Render for EditorPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let container = div().size_full().flex().flex_col().key_context(CONTEXT).track_focus(&self.focus_handle);

        container
            .on_action(cx.listener(Self::toggle_search))
            .on_action(cx.listener(Self::search_next))
            .on_action(cx.listener(Self::search_prev))
            .on_action(cx.listener(Self::focus_hex_view))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::go_to_beginning))
            .on_action(cx.listener(Self::go_to_end))
            .when(self.is_search_visible, |el| el.child(self.search_bar.clone()))
            .child(div().flex_1().w_full().min_h_0().child(self.hex_view.clone()))
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct EditorPanelState {
    pub path: Option<std::path::PathBuf>,
}

impl EditorPanelState {
    #[allow(dead_code)]
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serialize EditorPanelState")
    }

    #[allow(dead_code)]
    pub fn from_value(value: serde_json::Value) -> Option<Self> {
        serde_json::from_value(value).ok()
    }
}
