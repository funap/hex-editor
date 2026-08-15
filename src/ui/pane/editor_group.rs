use crate::ui::icon::IconName;
use gpui::prelude::*;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{ActiveTheme, Icon, Sizable};

use super::types::{DropPlacement, SplitDirection, TabContent, TabDrag, TabItem};
use crate::core::editor::Editor;
use crate::ui::panels::editor_panel::EditorPanel;

#[allow(dead_code)]
pub enum EditorGroupEvent {
    Focused,
    TabChanged,
    Split { direction: SplitDirection, new_content: TabContent },
    CloseTab(usize),
    CloseGroup,
    DropTab { drag: TabDrag, target_index: usize },
    SplitWithDrop { drag: TabDrag, placement: DropPlacement },
}

pub struct EditorGroup {
    pub id: usize,
    pub tabs: Vec<TabItem>,
    pub active_index: usize,
    pub focus_handle: FocusHandle,
    pub is_active_group: bool,
}

#[allow(dead_code)]
impl EditorGroup {
    pub fn new(id: usize, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        Self {
            id,
            tabs: Vec::new(),
            active_index: 0,
            focus_handle,
            is_active_group: false,
        }
    }

    pub fn tabs(&self) -> &[TabItem] {
        &self.tabs
    }

    pub fn active_index(&self) -> usize {
        self.active_index
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn active_tab(&self) -> Option<&TabItem> {
        self.tabs.get(self.active_index)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut TabItem> {
        self.tabs.get_mut(self.active_index)
    }

    pub fn active_content(&self) -> Option<&TabContent> {
        self.active_tab().map(|t| &t.content)
    }

    pub fn active_editor(&self, cx: &App) -> Option<Entity<Editor>> {
        self.active_content().and_then(|c| c.editor(cx))
    }

    pub fn active_editor_panel(&self) -> Option<Entity<EditorPanel>> {
        self.active_content().and_then(|c| c.editor_panel())
    }

    pub fn add_tab(&mut self, tab: TabItem, activate: bool, window: &mut Window, cx: &mut Context<Self>) {
        let new_idx = self.tabs.len();
        let tab_id = tab.id;
        cx.on_focus_in(&tab.focus_handle(cx), window, move |this, _window, cx| {
            if let Some(idx) = this.tabs.iter().position(|t| t.id == tab_id)
                && this.active_index != idx
            {
                this.active_index = idx;
                cx.emit(EditorGroupEvent::TabChanged);
            }
            cx.emit(EditorGroupEvent::Focused);
            cx.notify();
        })
        .detach();
        cx.on_focus_out(&tab.focus_handle(cx), window, move |_this, _window, _event, cx| {
            cx.notify();
        })
        .detach();

        self.tabs.push(tab);
        if activate || self.tabs.len() == 1 {
            self.activate_tab(new_idx, window, cx);
        } else {
            cx.notify();
        }
    }

    pub fn insert_tab(&mut self, index: usize, tab: TabItem, activate: bool, window: &mut Window, cx: &mut Context<Self>) {
        let clamped = index.min(self.tabs.len());
        let tab_id = tab.id;
        cx.on_focus_in(&tab.focus_handle(cx), window, move |this, _window, cx| {
            if let Some(idx) = this.tabs.iter().position(|t| t.id == tab_id)
                && this.active_index != idx
            {
                this.active_index = idx;
                cx.emit(EditorGroupEvent::TabChanged);
            }
            cx.emit(EditorGroupEvent::Focused);
            cx.notify();
        })
        .detach();
        cx.on_focus_out(&tab.focus_handle(cx), window, move |_this, _window, _event, cx| {
            cx.notify();
        })
        .detach();

        self.tabs.insert(clamped, tab);
        if activate || self.tabs.len() == 1 {
            self.activate_tab(clamped, window, cx);
        } else if clamped <= self.active_index {
            self.active_index += 1;
            cx.notify();
        } else {
            cx.notify();
        }
    }

    pub fn remove_tab_by_id(&mut self, tab_id: usize, window: &mut Window, cx: &mut Context<Self>) -> Option<TabItem> {
        if let Some(pos) = self.tabs.iter().position(|t| t.id == tab_id) {
            let removed = self.tabs.remove(pos);
            if self.tabs.is_empty() {
                self.active_index = 0;
            } else if pos <= self.active_index {
                self.active_index = self.active_index.saturating_sub(1);
                if let Some(tab) = self.tabs.get(self.active_index) {
                    tab.focus_handle(cx).focus(window);
                }
            }
            cx.emit(EditorGroupEvent::TabChanged);
            cx.notify();
            Some(removed)
        } else {
            None
        }
    }

    pub fn activate_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            self.active_index = index;
            if let Some(tab) = self.tabs.get(index) {
                tab.focus_handle(cx).focus(window);
            }
            cx.emit(EditorGroupEvent::Focused);
            cx.emit(EditorGroupEvent::TabChanged);
            cx.notify();
        }
    }

    pub fn activate_next_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() > 1 {
            let next_idx = (self.active_index + 1) % self.tabs.len();
            self.activate_tab(next_idx, window, cx);
        }
    }

    pub fn activate_previous_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() > 1 {
            let prev_idx = if self.active_index == 0 { self.tabs.len() - 1 } else { self.active_index - 1 };
            self.activate_tab(prev_idx, window, cx);
        }
    }

    pub fn close_tab(&mut self, tab_id: usize, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(EditorGroupEvent::CloseTab(tab_id));
        self.remove_tab_by_id(tab_id, window, cx);
        if self.tabs.is_empty() {
            cx.emit(EditorGroupEvent::CloseGroup);
        }
    }

    pub fn close_other_tabs(&mut self, keep_tab_id: usize, window: &mut Window, cx: &mut Context<Self>) {
        let to_remove: Vec<usize> = self.tabs.iter().filter(|t| t.id != keep_tab_id).map(|t| t.id).collect();
        for id in to_remove {
            cx.emit(EditorGroupEvent::CloseTab(id));
            self.remove_tab_by_id(id, window, cx);
        }
    }

    pub fn close_tabs_to_right(&mut self, target_tab_id: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pos) = self.tabs.iter().position(|t| t.id == target_tab_id) {
            let to_remove: Vec<usize> = self.tabs[pos + 1..].iter().map(|t| t.id).collect();
            for id in to_remove {
                cx.emit(EditorGroupEvent::CloseTab(id));
                self.remove_tab_by_id(id, window, cx);
            }
        }
    }

    pub fn close_saved_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let to_remove: Vec<usize> = self.tabs.iter().filter(|t| !t.is_dirty(cx)).map(|t| t.id).collect();
        for id in to_remove {
            cx.emit(EditorGroupEvent::CloseTab(id));
            self.remove_tab_by_id(id, window, cx);
        }
        if self.tabs.is_empty() {
            cx.emit(EditorGroupEvent::CloseGroup);
        }
    }

    pub fn close_all_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let to_remove: Vec<usize> = self.tabs.iter().map(|t| t.id).collect();
        for id in to_remove {
            cx.emit(EditorGroupEvent::CloseTab(id));
            self.remove_tab_by_id(id, window, cx);
        }
        cx.emit(EditorGroupEvent::CloseGroup);
    }

    pub fn split_active_tab(&mut self, direction: SplitDirection, window: &mut Window, cx: &mut Context<Self>) {
        let Some(active_content) = self.active_content() else {
            return;
        };

        match active_content {
            TabContent::Editor(editor_panel) => {
                let editor_panel = editor_panel.clone();
                let new_editor_panel = editor_panel.update(cx, |ep, cx| ep.create_split_clone(window, cx));
                cx.emit(EditorGroupEvent::Split {
                    direction,
                    new_content: TabContent::Editor(new_editor_panel),
                });
            }
            TabContent::Diff(diff_panel) => {
                let (left_doc, right_doc) = {
                    let dp = diff_panel.read(cx);
                    (dp.left_document.clone(), dp.right_document.clone())
                };
                let new_diff = cx.new(|cx| crate::ui::panels::diff_panel::DiffPanel::new(left_doc, right_doc, window, cx));
                cx.emit(EditorGroupEvent::Split {
                    direction,
                    new_content: TabContent::Diff(new_diff),
                });
            }
            TabContent::Settings(_) => {
                let new_settings = cx.new(|cx| crate::ui::panels::settings_panel::SettingsPanel::new(window, cx));
                cx.emit(EditorGroupEvent::Split {
                    direction,
                    new_content: TabContent::Settings(new_settings),
                });
            }
            TabContent::VisualMap(vm_panel) => {
                let ed = vm_panel.read(cx).editor.clone();
                let new_vm = cx.new(|cx| crate::ui::panels::visual_map_panel::VisualMapPanel::new(ed, cx));
                cx.emit(EditorGroupEvent::Split {
                    direction,
                    new_content: TabContent::VisualMap(new_vm),
                });
            }
        }
    }
}

impl EventEmitter<EditorGroupEvent> for EditorGroup {}

impl Focusable for EditorGroup {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for EditorGroup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let tab_bar_bg = theme.tab_bar;
        let group_id = self.id;
        let active_index = self.active_index;

        div()
            .id(ElementId::NamedInteger("editor-group".into(), group_id as u64))
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .bg(theme.background)
            // --- Custom Tab Bar Header ---
            .child(
                div()
                    .id("editor-group-tabbar")
                    .flex()
                    .flex_row()
                    .items_center()
                    .w_full()
                    .h(px(34.0))
                    .min_w_0()
                    .flex_shrink_0()
                    .bg(tab_bar_bg)
                    .border_b_1()
                    .border_color(theme.border)
                    // Tabs list on left
                    .child(
                        div()
                            .id("tab-list")
                            .flex()
                            .flex_row()
                            .items_center()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_x_scroll()
                            .on_drop(cx.listener(move |this, drag: &TabDrag, _, cx| {
                                cx.emit(EditorGroupEvent::DropTab {
                                    drag: *drag,
                                    target_index: this.tabs.len(),
                                });
                            }))
                            .children(self.tabs.iter().enumerate().map(|(idx, tab)| {
                                let tab_id = tab.id;
                                let is_active = idx == active_index;
                                let is_dirty = tab.is_dirty(cx);
                                let title = tab.title(cx);
                                let title_for_drag = title.clone();
                                let tab_focus_handle = tab.focus_handle(cx);
                                let is_tab_focused = is_active && (tab_focus_handle.is_focused(window) || tab_focus_handle.contains_focused(window, cx));

                                div()
                                    .id(ElementId::NamedInteger("tab-item".into(), tab_id as u64))
                                    .track_focus(&tab_focus_handle)
                                    .relative()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .px_3()
                                    .py_1()
                                    .h_full()
                                    .min_w(px(110.0))
                                    .max_w(px(220.0))
                                    .cursor_pointer()
                                    .border_r_1()
                                    .border_color(theme.border)
                                    .when(is_active, |s| {
                                        s.bg(theme.background).text_color(theme.foreground).font_weight(gpui::FontWeight::MEDIUM)
                                    })
                                    .when(!is_active, |s| {
                                        s.bg(tab_bar_bg)
                                            .text_color(theme.muted_foreground)
                                            .hover(|style| style.bg(theme.accent.opacity(0.12)))
                                    })
                                    .when(is_tab_focused, |s| {
                                        s.child(div().absolute().top_0().left_0().right_0().h(px(2.0)).bg(theme.primary))
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, window, cx| {
                                            this.activate_tab(idx, window, cx);
                                        }),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Middle,
                                        cx.listener(move |this, _, window, cx| {
                                            this.close_tab(tab_id, window, cx);
                                        }),
                                    )
                                    .on_drag(
                                        TabDrag {
                                            from_group_id: group_id,
                                            tab_id,
                                        },
                                        move |_, _, _window, cx| {
                                            let title = title_for_drag.clone();
                                            cx.new(|_| DragPreview { title })
                                        },
                                    )
                                    .on_drop(cx.listener(move |_, drag: &TabDrag, _, cx| {
                                        cx.emit(EditorGroupEvent::DropTab {
                                            drag: *drag,
                                            target_index: idx,
                                        });
                                    }))
                                    .context_menu({
                                        let tab_path = tab.path(cx);
                                        let can_close_others = self.tabs.len() > 1;
                                        let can_close_right = idx + 1 < self.tabs.len();
                                        let has_saved = self.tabs.iter().any(|t| !t.is_dirty(cx));
                                        move |menu, _window, _cx| {
                                            let mut menu = menu
                                                .menu_with_icon("Close", IconName::Close, Box::new(crate::actions::CloseActivePanel))
                                                .menu_with_icon_and_disabled(
                                                    "Close Others",
                                                    IconName::Close,
                                                    Box::new(crate::actions::CloseOtherTabs),
                                                    !can_close_others,
                                                )
                                                .menu_with_icon_and_disabled(
                                                    "Close to the Right",
                                                    IconName::ChevronRight,
                                                    Box::new(crate::actions::CloseTabsToRight),
                                                    !can_close_right,
                                                )
                                                .menu_with_icon_and_disabled(
                                                    "Close Saved",
                                                    IconName::Check,
                                                    Box::new(crate::actions::CloseSavedTabs),
                                                    !has_saved,
                                                )
                                                .menu("Close All", Box::new(crate::actions::CloseAllTabs))
                                                .separator()
                                                .menu_with_icon("Split Right", IconName::PanelRight, Box::new(crate::actions::SplitRight))
                                                .menu_with_icon("Split Down", IconName::PanelBottom, Box::new(crate::actions::SplitDown));

                                            if tab_path.is_some() {
                                                menu = menu
                                                    .separator()
                                                    .menu("Copy Path", Box::new(crate::actions::CopyPath))
                                                    .menu("Copy File Name", Box::new(crate::actions::CopyFileName))
                                                    .menu("Reveal in File Explorer", Box::new(crate::actions::RevealInExplorer));
                                            }
                                            menu
                                        }
                                    })
                                    .child(
                                        Icon::new(IconName::File)
                                            .size(px(14.0))
                                            .text_color(if is_active { theme.accent } else { theme.muted_foreground }),
                                    )
                                    .child(div().flex_1().min_w_0().truncate().text_sm().child(title))
                                    .child(
                                        div()
                                            .id(ElementId::NamedInteger("tab-close-btn".into(), tab_id as u64))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .w(px(18.0))
                                            .h(px(18.0))
                                            .rounded_sm()
                                            .hover(|style| style.bg(theme.accent.opacity(0.2)).text_color(theme.accent_foreground))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |this, _, window, cx| {
                                                    this.close_tab(tab_id, window, cx);
                                                }),
                                            )
                                            .child(if is_dirty && !is_active {
                                                div().w(px(6.0)).h(px(6.0)).rounded_full().bg(theme.accent).into_any_element()
                                            } else {
                                                Icon::new(IconName::Close).size(px(12.0)).text_color(theme.muted_foreground).into_any_element()
                                            }),
                                    )
                            })),
                    )
                    // Action buttons on right (Split Right, Split Down, Close)
                    .child(
                        div()
                            .id("tabbar-actions")
                            .flex()
                            .flex_row()
                            .items_center()
                            .flex_shrink_0()
                            .gap_1()
                            .px_2()
                            .child(
                                Button::new("group-split-right")
                                    .icon(IconName::PanelRight)
                                    .xsmall()
                                    .ghost()
                                    .tooltip(if cfg!(target_os = "macos") {
                                        "Split Right (cmd-\\)"
                                    } else {
                                        "Split Right (ctrl-\\)"
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.split_active_tab(SplitDirection::Horizontal, window, cx);
                                    })),
                            )
                            .child(
                                Button::new("group-split-down")
                                    .icon(IconName::PanelBottom)
                                    .xsmall()
                                    .ghost()
                                    .tooltip(if cfg!(target_os = "macos") {
                                        "Split Down (cmd-shift-d)"
                                    } else {
                                        "Split Down (ctrl-shift-d)"
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.split_active_tab(SplitDirection::Vertical, window, cx);
                                    })),
                            )
                            .child(
                                Button::new("group-close")
                                    .icon(IconName::Close)
                                    .xsmall()
                                    .ghost()
                                    .tooltip("Close Pane")
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        cx.emit(EditorGroupEvent::CloseGroup);
                                    })),
                            ),
                    ),
            )
            // --- Content View Area with Interactive Drop Zones ---
            .child(
                div()
                    .id("editor-group-content")
                    .relative()
                    .flex_1()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    // Active content rendering
                    .children(self.tabs.get(active_index).map(|t| t.content.render()))
                    // Visual Drop Zones (ONLY rendered during active drag-and-drop)
                    .when(cx.has_active_drag(), |this| {
                        this.child(
                            div()
                                .id("drop-zone-left")
                                .absolute()
                                .top_0()
                                .left_0()
                                .w(px(80.0))
                                .h_full()
                                .drag_over::<TabDrag>(|style, _drag, _window, _cx| style.bg(rgba(0x3182ce40)).border_r_2().border_color(rgb(0x3182ce)))
                                .on_drop(cx.listener(|_, drag: &TabDrag, _, cx| {
                                    cx.emit(EditorGroupEvent::SplitWithDrop {
                                        drag: *drag,
                                        placement: DropPlacement::Left,
                                    });
                                })),
                        )
                        .child(
                            div()
                                .id("drop-zone-right")
                                .absolute()
                                .top_0()
                                .right_0()
                                .w(px(80.0))
                                .h_full()
                                .drag_over::<TabDrag>(|style, _drag, _window, _cx| style.bg(rgba(0x3182ce40)).border_l_2().border_color(rgb(0x3182ce)))
                                .on_drop(cx.listener(|_, drag: &TabDrag, _, cx| {
                                    cx.emit(EditorGroupEvent::SplitWithDrop {
                                        drag: *drag,
                                        placement: DropPlacement::Right,
                                    });
                                })),
                        )
                        .child(
                            div()
                                .id("drop-zone-top")
                                .absolute()
                                .top_0()
                                .left(px(80.0))
                                .right(px(80.0))
                                .h(px(50.0))
                                .drag_over::<TabDrag>(|style, _drag, _window, _cx| style.bg(rgba(0x3182ce40)).border_b_2().border_color(rgb(0x3182ce)))
                                .on_drop(cx.listener(|_, drag: &TabDrag, _, cx| {
                                    cx.emit(EditorGroupEvent::SplitWithDrop {
                                        drag: *drag,
                                        placement: DropPlacement::Top,
                                    });
                                })),
                        )
                        .child(
                            div()
                                .id("drop-zone-bottom")
                                .absolute()
                                .bottom_0()
                                .left(px(80.0))
                                .right(px(80.0))
                                .h(px(50.0))
                                .drag_over::<TabDrag>(|style, _drag, _window, _cx| style.bg(rgba(0x3182ce40)).border_t_2().border_color(rgb(0x3182ce)))
                                .on_drop(cx.listener(|_, drag: &TabDrag, _, cx| {
                                    cx.emit(EditorGroupEvent::SplitWithDrop {
                                        drag: *drag,
                                        placement: DropPlacement::Bottom,
                                    });
                                })),
                        )
                    }),
            )
    }
}

struct DragPreview {
    title: String,
}

impl Render for DragPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_1()
            .bg(rgb(0x2d3748))
            .text_color(rgb(0xffffff))
            .border_1()
            .border_color(rgb(0x4a5568))
            .rounded_sm()
            .shadow_md()
            .text_sm()
            .child(self.title.clone())
    }
}
