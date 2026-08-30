use crate::ui::icon::IconName;
use gpui::prelude::FluentBuilder;
use gpui::{
    Action, App, AppContext as _, ClickEvent, Context, Corner, DismissEvent, Entity, EventEmitter, Focusable as _, InteractiveElement as _, IntoElement,
    KeyBinding, MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled, Subscription, WeakEntity, Window, anchored,
    deferred, div, px,
};
use gpui_component::button::ButtonVariants;
use gpui_component::menu::PopupMenu;
use gpui_component::{Selectable, Sizable, TitleBar, button::Button, h_flex};

const CONTEXT: &str = "AppMenuBar";

#[derive(Clone, PartialEq, Action)]
pub struct MenuCancel;
#[derive(Clone, PartialEq, Action)]
pub struct MenuSelectLeft;
#[derive(Clone, PartialEq, Action)]
pub struct MenuSelectRight;

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", MenuCancel, Some(CONTEXT)),
        KeyBinding::new("left", MenuSelectLeft, Some(CONTEXT)),
        KeyBinding::new("right", MenuSelectRight, Some(CONTEXT)),
    ]);
}

pub enum AppTitleBarEvent {
    OpenSettings,
}

#[derive(Clone, Copy, Default)]
struct MenuEditorState {
    has_doc: bool,
    is_read_only: bool,
    can_undo: bool,
    can_redo: bool,
    has_selection: bool,
    can_close_others: bool,
    can_close_right: bool,
    has_saved: bool,
}

const MENU_NAMES: [&str; 6] = ["File", "Edit", "View", "Go", "Analysis", "Window"];

pub struct AppTitleBar {
    pub app_menu_bar: Entity<AppMenuBar>,
}

impl EventEmitter<AppTitleBarEvent> for AppTitleBar {}

impl AppTitleBar {
    pub fn new(workspace: WeakEntity<crate::ui::workspace::Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let app_menu_bar = AppMenuBar::new(workspace, window, cx);
        Self { app_menu_bar }
    }
}

impl Render for AppTitleBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        TitleBar::new().child(div().flex().items_center().child(self.app_menu_bar.clone())).child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap_2()
                .child(Button::new("settings").ghost().icon(IconName::Settings).on_click(cx.listener(|_, _, _, cx| {
                    cx.emit(AppTitleBarEvent::OpenSettings);
                })))
                .child(Button::new("help").ghost().icon(IconName::Info)),
        )
    }
}

pub struct AppMenuBar {
    menus: Vec<Entity<AppMenu>>,
    selected_ix: Option<usize>,
}

impl AppMenuBar {
    pub fn new(workspace: WeakEntity<crate::ui::workspace::Workspace>, window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            let menu_bar = cx.entity();
            let menus = MENU_NAMES
                .iter()
                .enumerate()
                .map(|(ix, name)| AppMenu::new(ix, (*name).into(), workspace.clone(), menu_bar.clone(), window, cx))
                .collect();
            Self { menus, selected_ix: None }
        })
    }

    fn on_move_left(&mut self, _: &MenuSelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected_ix) = self.selected_ix else {
            return;
        };
        let new_ix = if selected_ix == 0 {
            self.menus.len().saturating_sub(1)
        } else {
            selected_ix.saturating_sub(1)
        };
        self.set_selected_index(Some(new_ix), window, cx);
    }

    fn on_move_right(&mut self, _: &MenuSelectRight, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected_ix) = self.selected_ix else {
            return;
        };
        let new_ix = if selected_ix + 1 >= self.menus.len() { 0 } else { selected_ix + 1 };
        self.set_selected_index(Some(new_ix), window, cx);
    }

    fn on_cancel(&mut self, _: &MenuCancel, window: &mut Window, cx: &mut Context<Self>) {
        self.set_selected_index(None, window, cx);
    }

    pub fn set_selected_index(&mut self, ix: Option<usize>, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_ix == ix {
            return;
        }
        self.selected_ix = ix;
        cx.notify();
    }

    #[inline]
    fn has_activated_menu(&self) -> bool {
        self.selected_ix.is_some()
    }
}

impl Render for AppMenuBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .id("app-menu-bar")
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::on_move_left))
            .on_action(cx.listener(Self::on_move_right))
            .on_action(cx.listener(Self::on_cancel))
            .size_full()
            .gap_x_1()
            .overflow_x_scroll()
            .children(self.menus.clone())
    }
}

pub struct AppMenu {
    ix: usize,
    name: SharedString,
    workspace: WeakEntity<crate::ui::workspace::Workspace>,
    menu_bar: Entity<AppMenuBar>,
    popup_menu: Option<Entity<PopupMenu>>,
    _subscription: Option<Subscription>,
}

impl AppMenu {
    pub fn new(
        ix: usize,
        name: SharedString,
        workspace: WeakEntity<crate::ui::workspace::Workspace>,
        menu_bar: Entity<AppMenuBar>,
        _: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|_| Self {
            ix,
            name,
            workspace,
            menu_bar,
            popup_menu: None,
            _subscription: None,
        })
    }

    fn query_editor_state(&self, cx: &mut Context<Self>) -> MenuEditorState {
        if let Some(workspace) = self.workspace.upgrade() {
            let ws = workspace.read(cx);
            if let Some(editor) = ws.active_editor(cx) {
                let ed = editor.read(cx);
                let is_ro = ed.is_read_only();
                let can_u = !is_ro && ed.can_undo();
                let can_r = !is_ro && ed.can_redo();
                let has_sel = ed.has_selection();
                let (can_o, can_r_tab, has_s) = if let Some(group) = ws.pane_tree.read(cx).active_group(cx) {
                    let g = group.read(cx);
                    let can_o = g.tabs.len() > 1;
                    let active_ix = g.active_index;
                    let can_r_tab = active_ix + 1 < g.tabs.len();
                    let has_s = g.tabs.iter().any(|t| !t.is_dirty(cx));
                    (can_o, can_r_tab, has_s)
                } else {
                    (false, false, false)
                };
                MenuEditorState {
                    has_doc: true,
                    is_read_only: is_ro,
                    can_undo: can_u,
                    can_redo: can_r,
                    has_selection: has_sel,
                    can_close_others: can_o,
                    can_close_right: can_r_tab,
                    has_saved: has_s,
                }
            } else {
                MenuEditorState {
                    is_read_only: true,
                    ..Default::default()
                }
            }
        } else {
            MenuEditorState {
                is_read_only: true,
                ..Default::default()
            }
        }
    }

    fn build_popup_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<PopupMenu> {
        let popup_menu = match self.popup_menu.as_ref() {
            None => {
                let focus_handle = window.focused(cx);
                let state = self.query_editor_state(cx);
                let ix = self.ix;
                let popup = PopupMenu::build(window, cx, move |menu, window, cx| {
                    let menu = menu.when_some(focus_handle, |this, handle| this.action_context(handle));
                    match ix {
                        0 => Self::build_file_menu(menu, window, cx, &state),
                        1 => Self::build_edit_menu(menu, window, cx, &state),
                        2 => Self::build_view_menu(menu, window, cx, &state),
                        3 => Self::build_go_menu(menu, window, cx, state.has_doc),
                        4 => Self::build_analysis_menu(menu, window, cx, state.has_doc),
                        5 => Self::build_window_menu(menu, window, cx),
                        _ => menu,
                    }
                });
                popup.read(cx).focus_handle(cx).focus(window);
                self._subscription = Some(cx.subscribe_in(&popup, window, Self::handle_dismiss));
                self.popup_menu = Some(popup.clone());
                popup
            }
            Some(menu) => menu.clone(),
        };

        let focus_handle = popup_menu.read(cx).focus_handle(cx);
        if !focus_handle.contains_focused(window, cx) {
            focus_handle.focus(window);
        }

        popup_menu
    }

    fn handle_dismiss(&mut self, _: &Entity<PopupMenu>, _: &DismissEvent, window: &mut Window, cx: &mut Context<Self>) {
        self._subscription.take();
        self.popup_menu.take();
        self.menu_bar.update(cx, |state, cx| {
            state.on_cancel(&MenuCancel, window, cx);
        });
    }

    fn handle_trigger_click(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let is_selected = self.menu_bar.read(cx).selected_ix == Some(self.ix);
        self.menu_bar.update(cx, |state, cx| {
            let new_ix = if is_selected { None } else { Some(self.ix) };
            state.set_selected_index(new_ix, window, cx);
        });
    }

    fn handle_hover(&mut self, hovered: &bool, window: &mut Window, cx: &mut Context<Self>) {
        if !*hovered {
            return;
        }
        let has_activated_menu = self.menu_bar.read(cx).has_activated_menu();
        if !has_activated_menu {
            return;
        }
        self.menu_bar.update(cx, |state, cx| {
            state.set_selected_index(Some(self.ix), window, cx);
        });
    }

    fn build_file_menu(menu: PopupMenu, window: &mut Window, cx: &mut Context<PopupMenu>, state: &MenuEditorState) -> PopupMenu {
        let can_save = !state.is_read_only && state.has_doc;
        menu.menu("New File...", Box::new(crate::actions::NewFile))
            .menu("Open File...", Box::new(crate::actions::OpenFileDialog))
            .menu("Open Folder...", Box::new(crate::actions::OpenFolder))
            .menu("Close Folder", Box::new(crate::actions::CloseFolder))
            .separator()
            .menu_with_disabled("Save", Box::new(crate::actions::Save), !can_save)
            .menu_with_disabled("Save As...", Box::new(crate::actions::SaveAs), !state.has_doc)
            .menu_with_disabled("Toggle Read-only", Box::new(crate::actions::ToggleReadOnly), !state.has_doc)
            .separator()
            .submenu("Import", window, cx, |menu, _window, _cx| {
                menu.menu("Motorola S-Record / Intel HEX...", Box::new(crate::actions::ImportHexOrMot))
            })
            .separator()
            .menu_with_disabled("Close Tab", Box::new(crate::actions::CloseActivePanel), !state.has_doc)
            .submenu("Close Other Tabs", window, cx, {
                let can_close_others = state.can_close_others;
                let can_close_right = state.can_close_right;
                let has_saved = state.has_saved;
                let has_doc = state.has_doc;
                move |menu, _window, _cx| {
                    menu.menu_with_disabled("Close Others", Box::new(crate::actions::CloseOtherTabs), !can_close_others)
                        .menu_with_disabled("Close Tabs to Right", Box::new(crate::actions::CloseTabsToRight), !can_close_right)
                        .menu_with_disabled("Close Saved Tabs", Box::new(crate::actions::CloseSavedTabs), !has_saved)
                        .menu_with_disabled("Close All Tabs", Box::new(crate::actions::CloseAllTabs), !has_doc)
                }
            })
            .separator()
            .menu_with_disabled("Copy Path", Box::new(crate::actions::CopyPath), !state.has_doc)
            .menu_with_disabled("Copy File Name", Box::new(crate::actions::CopyFileName), !state.has_doc)
            .menu_with_disabled("Reveal in File Manager", Box::new(crate::actions::RevealInExplorer), !state.has_doc)
            .separator()
            .menu("Quit", Box::new(crate::actions::Quit))
    }

    fn build_edit_menu(menu: PopupMenu, window: &mut Window, cx: &mut Context<PopupMenu>, state: &MenuEditorState) -> PopupMenu {
        let can_edit = !state.is_read_only && state.has_doc;
        menu.menu_with_disabled("Undo", Box::new(crate::actions::Undo), !state.can_undo)
            .menu_with_disabled("Redo", Box::new(crate::actions::Redo), !state.can_redo)
            .separator()
            .menu_with_disabled("Cut", Box::new(crate::actions::Cut), !can_edit || !state.has_selection)
            .menu_with_disabled("Copy", Box::new(crate::actions::Copy), !state.has_selection)
            .menu_with_disabled("Paste", Box::new(crate::actions::Paste), !can_edit)
            .menu_with_disabled("Toggle Insert Mode", Box::new(crate::actions::ToggleInsertMode), !can_edit)
            .menu_with_disabled("Toggle Read-only", Box::new(crate::actions::ToggleReadOnly), !state.has_doc)
            .separator()
            .submenu("Copy As", window, cx, {
                let has_selection = state.has_selection;
                move |menu, _window, _cx| {
                    menu.menu_with_disabled("as Hex Dump", Box::new(crate::actions::CopyAsHexDump), !has_selection)
                        .menu_with_disabled("as Hex with Spaces", Box::new(crate::actions::CopyAsHexSpaces), !has_selection)
                        .menu_with_disabled("as Hex Stream", Box::new(crate::actions::CopyAsHexStream), !has_selection)
                        .menu_with_disabled("as Printable Text", Box::new(crate::actions::CopyAsPrintableText), !has_selection)
                        .menu_with_disabled("as Escaped String", Box::new(crate::actions::CopyAsEscapedString), !has_selection)
                        .menu_with_disabled("as Base64", Box::new(crate::actions::CopyAsBase64), !has_selection)
                        .menu_with_disabled("as Binary", Box::new(crate::actions::CopyAsBinary), !has_selection)
                        .menu_with_disabled("as C++ Array", Box::new(crate::actions::CopyAsCppArray), !has_selection)
                        .menu_with_disabled("as Rust Array", Box::new(crate::actions::CopyAsRustArray), !has_selection)
                        .menu_with_disabled("as JSON Array", Box::new(crate::actions::CopyAsJsonArray), !has_selection)
                }
            })
            .menu_with_disabled("Select All", Box::new(crate::actions::SelectAll), !state.has_doc)
            .separator()
            .menu_with_disabled("Find", Box::new(crate::actions::ToggleSearch), !state.has_doc)
            .menu_with_disabled("Find in File (Scan All)", Box::new(crate::actions::ToggleSearchPanel), !state.has_doc)
            .menu_with_disabled("Find Next", Box::new(crate::actions::SearchNext), !state.has_doc)
            .menu_with_disabled("Find Previous", Box::new(crate::actions::SearchPrev), !state.has_doc)
            .separator()
            .submenu("Bookmark", window, cx, {
                let has_doc = state.has_doc;
                move |menu, _window, _cx| {
                    menu.menu_with_disabled("Red", Box::new(crate::actions::BookmarkRed), !has_doc)
                        .menu_with_disabled("Orange", Box::new(crate::actions::BookmarkOrange), !has_doc)
                        .menu_with_disabled("Yellow", Box::new(crate::actions::BookmarkYellow), !has_doc)
                        .menu_with_disabled("Green", Box::new(crate::actions::BookmarkGreen), !has_doc)
                        .menu_with_disabled("Cyan", Box::new(crate::actions::BookmarkCyan), !has_doc)
                        .menu_with_disabled("Blue", Box::new(crate::actions::BookmarkBlue), !has_doc)
                        .menu_with_disabled("Purple", Box::new(crate::actions::BookmarkPurple), !has_doc)
                        .menu_with_disabled("Pink", Box::new(crate::actions::BookmarkPink), !has_doc)
                        .separator()
                        .menu_with_disabled("Clear Bookmark", Box::new(crate::actions::ClearBookmark), !has_doc)
                        .menu_with_disabled("Clear All Bookmarks", Box::new(crate::actions::ClearAllBookmarks), !has_doc)
                        .separator()
                        .menu_with_disabled("Import Bookmarks...", Box::new(crate::actions::ImportBookmarks), !has_doc)
                        .menu_with_disabled("Export Bookmarks...", Box::new(crate::actions::ExportBookmarks), !has_doc)
                }
            })
    }

    fn build_view_menu(menu: PopupMenu, window: &mut Window, cx: &mut Context<PopupMenu>, state: &MenuEditorState) -> PopupMenu {
        let has_doc = state.has_doc;
        let can_edit = !state.is_read_only && state.has_doc;
        menu.menu("Toggle Left Panel", Box::new(crate::actions::ToggleLeftPanel))
            .submenu("Panels", window, cx, |menu, _window, _cx| {
                menu.menu("Files", Box::new(crate::actions::ShowFilesTab))
                    .menu("Strings", Box::new(crate::actions::ShowStringsTab))
                    .menu("Structure", Box::new(crate::actions::ShowStructureTab))
                    .menu("Bookmarks", Box::new(crate::actions::ShowBookmarksTab))
                    .menu("Checksum", Box::new(crate::actions::ShowChecksumTab))
                    .menu("2D Visual Map", Box::new(crate::actions::OpenVisualMap))
            })
            .separator()
            .submenu("Radix", window, cx, move |menu, _window, _cx| {
                menu.menu_with_disabled("Hexadecimal (16)", Box::new(crate::actions::SetRadixHex), !has_doc)
                    .menu_with_disabled("Decimal (10)", Box::new(crate::actions::SetRadixDec), !has_doc)
                    .menu_with_disabled("Octal (8)", Box::new(crate::actions::SetRadixOct), !has_doc)
                    .menu_with_disabled("Binary (2)", Box::new(crate::actions::SetRadixBin), !has_doc)
            })
            .submenu("Grouping", window, cx, move |menu, _window, _cx| {
                menu.menu_with_disabled("1 Byte (8-bit)", Box::new(crate::actions::SetGroupSize1), !has_doc)
                    .menu_with_disabled("2 Bytes (16-bit)", Box::new(crate::actions::SetGroupSize2), !has_doc)
                    .menu_with_disabled("4 Bytes (32-bit)", Box::new(crate::actions::SetGroupSize4), !has_doc)
                    .menu_with_disabled("8 Bytes (64-bit)", Box::new(crate::actions::SetGroupSize8), !has_doc)
            })
            .submenu("Byte Order", window, cx, move |menu, _window, _cx| {
                menu.menu_with_disabled("Little Endian", Box::new(crate::actions::SetByteOrderLittleEndian), !has_doc)
                    .menu_with_disabled("Big Endian", Box::new(crate::actions::SetByteOrderBigEndian), !has_doc)
                    .separator()
                    .menu_with_disabled("Toggle Byte Order", Box::new(crate::actions::ToggleByteOrder), !has_doc)
            })
            .submenu("Encoding", window, cx, move |menu, window, cx| {
                crate::core::encoding::Encoding::categories().iter().fold(menu, |menu, (category, encodings)| {
                    menu.submenu(category.label(), window, cx, move |menu, _window, _cx| {
                        encodings.iter().copied().fold(menu, |menu, encoding| {
                            menu.menu_with_disabled(encoding.label(), Box::new(crate::actions::SetEncoding { encoding }), !has_doc)
                        })
                    })
                })
            })
            .separator()
            .submenu("Custom Line Breaks", window, cx, move |menu, _window, _cx| {
                menu.menu_with_disabled("Break Line", Box::new(crate::actions::AddCustomBreak), !can_edit)
                    .menu_with_disabled("Join Lines", Box::new(crate::actions::JoinLine), !can_edit)
                    .separator()
                    .menu_with_disabled("Remove Break Backward", Box::new(crate::actions::RemoveCustomBreakBackward), !can_edit)
                    .menu_with_disabled("Remove Break Forward", Box::new(crate::actions::RemoveCustomBreakForward), !can_edit)
                    .separator()
                    .menu_with_disabled("Reset Custom Breaks", Box::new(crate::actions::ClearAllCustomBreaks), !can_edit)
            })
    }

    fn build_go_menu(menu: PopupMenu, _window: &mut Window, _cx: &mut Context<PopupMenu>, has_doc: bool) -> PopupMenu {
        menu.menu_with_disabled("Go to Address...", Box::new(crate::actions::ToggleGoToAddress), !has_doc)
            .separator()
            .menu_with_disabled("Go to Beginning", Box::new(crate::actions::GoToBeginning), !has_doc)
            .menu_with_disabled("Go to End", Box::new(crate::actions::GoToEnd), !has_doc)
            .separator()
            .menu_with_disabled("Next Difference", Box::new(crate::actions::NextDifference), !has_doc)
            .menu_with_disabled("Previous Difference", Box::new(crate::actions::PrevDifference), !has_doc)
    }

    fn build_analysis_menu(menu: PopupMenu, window: &mut Window, cx: &mut Context<PopupMenu>, has_doc: bool) -> PopupMenu {
        menu.submenu("Structure (Kaitai Struct)", window, cx, move |menu, _window, _cx| {
            menu.menu_with_disabled("Load Definition...", Box::new(crate::actions::LoadStructureDefinition), !has_doc)
                .menu_with_disabled("Clear Definition", Box::new(crate::actions::ClearStructureDefinition), !has_doc)
                .separator()
                .menu_with_disabled("Toggle Inline Structure View", Box::new(crate::actions::ToggleInlineStructureView), !has_doc)
                .separator()
                .menu_with_disabled("Expand All", Box::new(crate::actions::ExpandAllStructure), !has_doc)
                .menu_with_disabled("Collapse All", Box::new(crate::actions::CollapseAllStructure), !has_doc)
        })
        .separator()
        .menu("2D Visual Map", Box::new(crate::actions::OpenVisualMap))
        .menu("Checksum Calculation", Box::new(crate::actions::ShowChecksumTab))
        .separator()
        .submenu("Compare / Diff", window, cx, move |menu, _window, _cx| {
            menu.menu("Compare Open Files...", Box::new(crate::actions::CompareOpenFiles))
                .menu("Compare Visible Split Panes", Box::new(crate::actions::CompareVisiblePanes))
                .separator()
                .menu("Swap Diff Files", Box::new(crate::actions::SwapDiffFiles))
                .menu("Refresh Diff", Box::new(crate::actions::RefreshDiff))
                .separator()
                .menu_with_disabled("Next Difference", Box::new(crate::actions::NextDifference), !has_doc)
                .menu_with_disabled("Previous Difference", Box::new(crate::actions::PrevDifference), !has_doc)
        })
    }

    fn build_window_menu(menu: PopupMenu, _window: &mut Window, _cx: &mut Context<PopupMenu>) -> PopupMenu {
        menu.menu("Split Right", Box::new(crate::actions::SplitRight))
            .menu("Split Down", Box::new(crate::actions::SplitDown))
            .separator()
            .menu("Next Tab", Box::new(crate::actions::ActivateNextTab))
            .menu("Previous Tab", Box::new(crate::actions::ActivatePreviousTab))
    }
}

impl Render for AppMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let menu_bar = self.menu_bar.read(cx);
        let is_selected = menu_bar.selected_ix == Some(self.ix);
        if !is_selected {
            self._subscription.take();
            self.popup_menu.take();
        }

        div()
            .id(self.ix)
            .relative()
            .child(
                Button::new("menu")
                    .small()
                    .py_0p5()
                    .compact()
                    .ghost()
                    .label(self.name.clone())
                    .selected(is_selected)
                    .on_mouse_down(MouseButton::Left, |_, window, cx| {
                        // Stop propagation to avoid dragging the window.
                        window.prevent_default();
                        cx.stop_propagation();
                    })
                    .on_click(cx.listener(Self::handle_trigger_click)),
            )
            .on_hover(cx.listener(Self::handle_hover))
            .when(is_selected, |this| {
                this.child(deferred(
                    anchored()
                        .anchor(Corner::TopLeft)
                        .snap_to_window_with_margin(px(8.))
                        .child(div().size_full().occlude().top_1().child(self.build_popup_menu(window, cx))),
                ))
            })
    }
}
