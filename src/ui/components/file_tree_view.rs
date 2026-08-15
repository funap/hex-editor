use crate::actions::{LoadChildren, OpenDiff, OpenFile, Rename, SelectItem};
use std::collections::HashSet;
use std::path::PathBuf;

use crate::ui::icon::IconName;
use autocorrect::ignorer::Ignorer;
use gpui::{
    App, AppContext, AsyncApp, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement, ParentElement, Render, ScrollStrategy,
    SharedString, Styled, WeakEntity, Window, actions, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    button::ButtonVariants as _,
    h_flex,
    list::ListItem,
    menu::ContextMenuExt,
    tree::{TreeItem, TreeState, tree},
};

actions!(file_tree, [MoveUp, MoveDown]);

const CONTEXT: &str = "TreeStory";
pub fn init(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("up", MoveUp, Some(CONTEXT)),
        gpui::KeyBinding::new("down", MoveDown, Some(CONTEXT)),
        gpui::KeyBinding::new("enter", SelectItem, Some(CONTEXT)),
    ]);
}

pub enum FileTreeViewEvent {
    OpenFile(PathBuf),
}

pub struct FileTreeView {
    tree_state: Entity<TreeState>,
    selected_item: Option<TreeItem>,
    selected_items: Vec<TreeItem>,
    _title: SharedString,
    focus_handle: FocusHandle,
    root_path: Option<PathBuf>,
    loaded_paths: HashSet<String>,
    items: Vec<TreeItem>,
}

fn build_file_items(ignorer: &Ignorer, root: &PathBuf, path: &PathBuf) -> Vec<TreeItem> {
    let mut items = Vec::new();
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            let relative_path = path.strip_prefix(root).unwrap_or(&path);
            if ignorer.is_ignored(&relative_path.to_string_lossy()) || relative_path.ends_with(".git") {
                continue;
            }
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown").to_string();
            let id = path.to_string_lossy().to_string();
            if path.is_dir() {
                items.push(TreeItem::new(id, file_name).child(TreeItem::new("loading", "Loading...")));
            } else {
                items.push(TreeItem::new(id, file_name));
            }
        }
    }
    items.sort_by(|a, b| b.is_folder().cmp(&a.is_folder()).then(a.label.cmp(&b.label)));
    items
}

fn update_item_children(items: &mut [TreeItem], target_id: &str, children: Vec<TreeItem>) -> bool {
    let mut pending = vec![items];
    let mut replacement = Some(children);

    while let Some(items) = pending.pop() {
        for item in items.iter_mut() {
            if item.id == target_id {
                item.children = replacement.take().expect("tree child replacement must be available");
                return true;
            }
            if item.is_folder() {
                pending.push(item.children.as_mut_slice());
            }
        }
    }

    false
}

impl FileTreeView {
    pub fn new(title: impl Into<SharedString>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));
        let focus_handle = cx.focus_handle();

        cx.on_focus_in(&focus_handle, window, |this, _, cx| {
            cx.notify();
            this.clear_tree_selection(cx);
        })
        .detach();
        cx.on_focus_out(&focus_handle, window, |this, _, _, cx| {
            this.clear_tree_selection(cx);
            cx.notify();
        })
        .detach();

        Self {
            tree_state: tree_state.clone(),
            selected_item: None,
            selected_items: Vec::new(),
            _title: title.into(),
            focus_handle,
            root_path: None,
            loaded_paths: HashSet::new(),
            items: Vec::new(),
        }
    }

    fn load_root(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(|view: WeakEntity<FileTreeView>, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let ignorer = Ignorer::new(&path.to_string_lossy());
                let items = build_file_items(&ignorer, &path, &path);

                view.update(&mut cx, |this, cx| {
                    this.items = items.clone();
                    this.tree_state.update(cx, |state, cx| {
                        state.set_items(items, cx);
                    });
                })
                .ok();
            }
        })
        .detach();
    }

    fn load_children(&mut self, item_id: &str, cx: &mut Context<Self>) {
        if self.loaded_paths.contains(item_id) {
            return;
        }

        let path = PathBuf::from(item_id);
        if path.is_dir() {
            let item_id_clone = item_id.to_string();
            let root_path = self.root_path.clone();

            cx.spawn(|view: WeakEntity<FileTreeView>, cx: &mut AsyncApp| {
                let mut cx = cx.clone();
                async move {
                    if let Some(root_path) = root_path {
                        let ignorer = Ignorer::new(&root_path.to_string_lossy());
                        let children = build_file_items(&ignorer, &root_path, &PathBuf::from(&item_id_clone));

                        view.update(&mut cx, |this, cx| {
                            if update_item_children(&mut this.items, &item_id_clone, children) {
                                this.tree_state.update(cx, |state, cx| {
                                    state.set_items(this.items.clone(), cx);
                                });
                            }
                        })
                        .ok();
                    }
                }
            })
            .detach();

            self.loaded_paths.insert(item_id.to_string());
        }
    }

    fn on_action_select_item(&mut self, _: &SelectItem, _: &mut Window, cx: &mut gpui::Context<Self>) {
        let item = self
            .selected_item
            .clone()
            .or_else(|| self.tree_state.read(cx).selected_entry().map(|entry| entry.item().clone()));

        if let Some(item) = item {
            self.selected_item = Some(item.clone());
            self.selected_items = vec![item.clone()];
            self.clear_tree_selection(cx);

            if !item.is_folder() {
                cx.emit(FileTreeViewEvent::OpenFile(PathBuf::from(item.id.to_string())));
            }
            cx.notify();
        }
    }

    fn on_action_rename(&mut self, _: &Rename, _: &mut Window, cx: &mut gpui::Context<Self>) {
        let item = self
            .selected_item
            .clone()
            .or_else(|| self.tree_state.read(cx).selected_entry().map(|entry| entry.item().clone()));

        if let Some(item) = item {
            println!("Renaming item: {} ({})", item.label, item.id);
        }
    }

    pub fn prompt_open_folder(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        let path = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select a folder".into()),
        });

        let view = cx.entity().clone();
        cx.spawn_in(window, async move |_, window| {
            if let Some(path) = path.await.ok().and_then(|r| r.ok()).flatten().and_then(|mut p| p.pop()) {
                window
                    .update(|_, cx| {
                        view.update(cx, |this, cx| {
                            this.set_root_path(path, cx);
                        });
                    })
                    .ok();
            }
        })
        .detach();
    }

    pub fn close_folder(&mut self, cx: &mut gpui::Context<Self>) {
        self.root_path = None;
        self.loaded_paths.clear();
        self.items.clear();
        self.selected_item = None;
        self.selected_items.clear();
        self.clear_tree_selection(cx);
        self.tree_state.update(cx, |state, cx| {
            state.set_items(vec![], cx);
        });
        cx.notify();
    }

    pub fn set_root_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.root_path = Some(path.clone());
        self.loaded_paths.clear();
        self.loaded_paths.insert(path.to_string_lossy().to_string());
        self.selected_item = None;
        self.selected_items.clear();
        self.clear_tree_selection(cx);
        self.load_root(path, cx);
        cx.notify();
    }

    fn on_action_set_file_tree_folder(&mut self, action: &crate::actions::SetFileTreeFolder, _: &mut Window, cx: &mut Context<Self>) {
        let path = PathBuf::from(&action.path);
        self.set_root_path(path, cx);
    }

    fn on_action_load_children(&mut self, action: &LoadChildren, _: &mut Window, cx: &mut Context<Self>) {
        self.load_children(&action.path, cx);
    }

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(-1, cx);
    }

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(1, cx);
    }

    fn move_cursor(&mut self, direction: i8, cx: &mut Context<Self>) {
        let visible_items = self.visible_items();
        if visible_items.is_empty() {
            return;
        }

        let current_index = self
            .selected_item
            .as_ref()
            .and_then(|selected| visible_items.iter().position(|item| item.id == selected.id));
        let last_index = visible_items.len() - 1;
        let next_index = match (current_index, direction) {
            (Some(index), -1) => index.saturating_sub(1),
            (Some(index), 1) => (index + 1).min(last_index),
            (Some(index), _) => index,
            (None, _) => 0,
        };
        let item = visible_items[next_index].clone();

        self.selected_item = Some(item.clone());
        self.selected_items = vec![item];
        self.tree_state.update(cx, |state, _| {
            let strategy = if direction < 0 { ScrollStrategy::Top } else { ScrollStrategy::Bottom };
            state.scroll_to_item(next_index, strategy);
        });
        self.clear_tree_selection(cx);
        cx.notify();
    }

    fn visible_items(&self) -> Vec<TreeItem> {
        let mut visible_items = Vec::new();
        Self::collect_visible_items(&self.items, &mut visible_items);
        visible_items
    }

    fn collect_visible_items(items: &[TreeItem], visible_items: &mut Vec<TreeItem>) {
        let mut pending: Vec<&TreeItem> = items.iter().rev().collect();
        while let Some(item) = pending.pop() {
            visible_items.push(item.clone());
            if item.is_expanded() {
                pending.extend(item.children.iter().rev());
            }
        }
    }

    fn clear_tree_selection(&mut self, cx: &mut Context<Self>) {
        if self.tree_state.read(cx).selected_index().is_some() {
            self.tree_state.update(cx, |state, cx| {
                state.set_selected_index(None, cx);
            });
        }
    }

    fn toggle_selection(&mut self, item: TreeItem, cx: &mut Context<Self>) {
        if let Some(pos) = self.selected_items.iter().position(|i| i.id == item.id) {
            self.selected_items.remove(pos);
        } else {
            self.selected_items.push(item);
        }
        cx.notify();
    }
}

impl Render for FileTreeView {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        let view = cx.entity();
        let is_empty = self.root_path.is_none();
        let is_focused = self.focus_handle.is_focused(window);
        let theme = cx.theme();

        let header_actions = if !is_empty {
            Some(
                gpui_component::button::Button::new("close-folder")
                    .ghost()
                    .icon(IconName::Close)
                    .with_size(gpui_component::Size::XSmall)
                    .tooltip("Close Folder")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_folder(cx);
                    }))
                    .into_any_element(),
            )
        } else {
            Some(
                gpui_component::button::Button::new("open-folder-header")
                    .ghost()
                    .icon(IconName::FolderOpen)
                    .with_size(gpui_component::Size::XSmall)
                    .tooltip("Open Folder")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.prompt_open_folder(window, cx);
                    }))
                    .into_any_element(),
            )
        };

        let header = crate::ui::style::panel_header("FILES", is_focused, theme, None, header_actions);

        let container = crate::ui::style::panel_container(is_focused, theme);

        container
            .id("file-tree-view")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, _| {
                    this.focus_handle.focus(window);
                }),
            )
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::on_action_rename))
            .on_action(cx.listener(Self::on_action_select_item))
            .on_action(cx.listener(Self::on_action_set_file_tree_folder))
            .on_action(cx.listener(Self::on_action_load_children))
            .child(header)
            .child(div().flex_1().min_h_0().w_full().overflow_hidden().child(if is_empty {
                let open_btn = gpui_component::button::Button::new("open-folder-btn")
                    .label("Open Folder")
                    .primary()
                    .with_size(gpui_component::Size::Small)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.prompt_open_folder(window, cx);
                    }))
                    .into_any_element();

                crate::ui::style::panel_empty_state(
                    IconName::FolderOpen,
                    "No Folder Opened",
                    Some("Open a directory to explore files"),
                    Some(open_btn),
                    theme,
                )
                .into_any_element()
            } else {
                tree(&self.tree_state, {
                    let selected_ids: HashSet<_> = self.selected_items.iter().map(|i| i.id.clone()).collect();
                    let focus_handle = self.focus_handle.clone();
                    let loaded_paths = self.loaded_paths.clone();
                    move |ix, entry, _selected, window, cx| {
                        let item = entry.item();
                        let icon = if !entry.is_folder() {
                            IconName::File
                        } else if entry.is_expanded() {
                            IconName::FolderOpen
                        } else {
                            IconName::Folder
                        };

                        let is_multi_selected = selected_ids.contains(&item.id);
                        let is_focused = focus_handle.is_focused(window);

                        if entry.is_expanded() && entry.is_folder() && !loaded_paths.contains(&item.id.to_string()) {
                            let item_id = item.id.to_string();
                            window.dispatch_action(Box::new(crate::actions::LoadChildren { path: item_id }), cx);
                        }

                        let selection_bg = if is_focused { cx.theme().selection } else { cx.theme().muted };

                        ListItem::new(ix)
                            .selected(false)
                            .when(is_multi_selected, |this| this.bg(selection_bg))
                            .w_full()
                            .rounded(cx.theme().radius)
                            .px_3()
                            .pl(px(16.) * entry.depth() + px(12.))
                            .child(h_flex().gap_2().child(icon).child(item.label.clone()).size_full().context_menu({
                                let view = view.clone();
                                let item_id = item.id.clone();
                                move |menu, _window, cx| {
                                    let (can_compare, left_path, right_path) = view.update(cx, |this, _cx| {
                                        let can_compare = this.selected_items.len() == 2 && this.selected_items.iter().all(|item| !item.is_folder());
                                        if can_compare {
                                            (true, Some(this.selected_items[0].id.to_string()), Some(this.selected_items[1].id.to_string()))
                                        } else {
                                            (false, None, None)
                                        }
                                    });

                                    let mut menu = menu
                                        .menu_with_icon("Open", IconName::FolderOpen, Box::new(OpenFile { path: item_id.to_string() }))
                                        .separator();

                                    if can_compare {
                                        menu = menu.menu_with_icon(
                                            "Compare Files",
                                            IconName::GitCompare,
                                            Box::new(OpenDiff {
                                                left_path: left_path.unwrap_or_default(),
                                                right_path: right_path.unwrap_or_default(),
                                            }),
                                        );
                                    } else {
                                        menu = menu.menu_with_icon_and_disabled(
                                            "Compare Files",
                                            IconName::GitCompare,
                                            Box::new(OpenDiff {
                                                left_path: String::new(),
                                                right_path: String::new(),
                                            }),
                                            true,
                                        );
                                    }

                                    menu.separator().menu("Rename", Box::new(Rename))
                                }
                            }))
                            .on_click(window.listener_for(&view, {
                                let item = item.clone();
                                let focus_handle = focus_handle.clone();
                                move |this, event: &gpui::ClickEvent, window, cx| {
                                    focus_handle.focus(window);
                                    if event.modifiers().control || event.modifiers().platform {
                                        this.toggle_selection(item.clone(), cx);
                                    } else {
                                        this.selected_items = vec![item.clone()];
                                        this.selected_item = Some(item.clone());
                                    }

                                    if !item.is_folder() && this.selected_items.len() == 1 {
                                        println!("Emitting FileTreeViewEvent::OpenFile for path: {}", item.id);
                                        // cx.focus_self(window);
                                        // window.dispatch_action(Box::new(OpenFile { path: item.id.to_string() }), cx);
                                        cx.emit(FileTreeViewEvent::OpenFile(PathBuf::from(item.id.to_string())));
                                    }
                                    this.clear_tree_selection(cx);
                                    cx.notify();
                                }
                            }))
                    }
                })
                .into_any_element()
            }))
    }
}

impl EventEmitter<FileTreeViewEvent> for FileTreeView {}

impl Focusable for FileTreeView {
    fn focus_handle(&self, _cx: &App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct FileTreeViewState {
    pub root_path: Option<PathBuf>,
}

impl FileTreeViewState {
    #[allow(dead_code)]
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serialize FileTreeViewState")
    }

    #[allow(dead_code)]
    pub fn from_value(value: serde_json::Value) -> Option<Self> {
        serde_json::from_value(value).ok()
    }
}
