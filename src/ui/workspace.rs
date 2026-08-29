use gpui::prelude::*;
use gpui::*;

use crate::actions::*;

use crate::ui::components::activity_bar::{Activity, ActivityBar, ActivityBarEvent};
use crate::ui::components::file_tree_view::{FileTreeView, FileTreeViewEvent};
use crate::ui::components::title_bar::AppTitleBar;
use crate::ui::pane::{PaneTree, PaneTreeEvent, SplitDirection, TabContent};
use crate::ui::panels::editor_panel::EditorPanel;
use crate::ui::panels::left_panel::{LeftPanel, LeftPanelTab};

use crate::app_state::{AppState, InsertModeState};
use crate::core::editor::Editor;
use crate::core::encoding::Encoding;
use crate::ui::components::status_bar::StatusBar;
use gpui_component::menu::AppMenuBar;
use gpui_component::resizable::{h_resizable, resizable_panel};
use gpui_component::{Root, WindowExt, v_flex};
use std::cell::Cell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

pub struct Workspace {
    pub pane_tree: Entity<PaneTree>,
    pub title_bar: Entity<AppTitleBar>,
    pub status_bar: Entity<StatusBar>,
    pub left_panel: Entity<LeftPanel>,
    pub activity_bar: Entity<ActivityBar>,
    pub recent_definition_history: crate::core::structure::DefinitionHistory,
    pub recent_file_history: crate::core::structure::FileHistory,
    pub is_left_panel_visible: bool,
    pub new_file_modal: Option<Entity<crate::ui::components::new_file_modal::NewFileModal>>,
    pub untitled_count: usize,
    focus_handle: FocusHandle,
    last_active_editor_id: Cell<Option<EntityId>>,
}

struct PendingParseUpdate {
    definition_id: String,
    fields: VecDeque<Arc<[crate::core::structure::ParsedField]>>,
    parsed_offset: usize,
    total_bytes: usize,
    is_done: bool,
    is_finalizing: bool,
    parse_result: Option<Arc<crate::core::structure::ParseResult>>,
}

struct ParseUpdateBatch {
    definition_id: String,
    fields: Vec<Arc<[crate::core::structure::ParsedField]>>,
    parsed_offset: usize,
    total_bytes: usize,
    is_done: bool,
    is_finalizing: bool,
    parse_result: Option<Arc<crate::core::structure::ParseResult>>,
    has_more_fields: bool,
}

impl ParseUpdateBatch {
    fn discard_on_background(self, executor: &BackgroundExecutor) {
        executor
            .spawn(async move {
                drop(self);
            })
            .detach();
    }
}

enum ParseUpdateDelivery {
    Applied { should_continue: bool, has_more_fields: bool },
    Stale(ParseUpdateBatch),
}

struct ParseUpdateMailbox {
    pending: Mutex<Option<PendingParseUpdate>>,
    notify: tokio::sync::Notify,
    closed: AtomicBool,
}

impl ParseUpdateMailbox {
    fn publish(&self, progress: crate::core::structure::types::ParseProgress) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }

        let mut pending = self.pending.lock().expect("parse update mailbox lock");
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        let update = pending.get_or_insert_with(|| PendingParseUpdate {
            definition_id: progress.definition_id.clone(),
            fields: VecDeque::new(),
            parsed_offset: progress.parsed_offset,
            total_bytes: progress.total_bytes,
            is_done: false,
            is_finalizing: false,
            parse_result: None,
        });

        update.definition_id = progress.definition_id;
        update.parsed_offset = progress.parsed_offset;
        update.total_bytes = progress.total_bytes;
        update.is_done = progress.is_done;
        update.is_finalizing = progress.is_finalizing;
        if !progress.fields.is_empty() {
            update.fields.push_back(progress.fields);
        }
        if progress.parse_result.is_some() {
            update.parse_result = progress.parse_result;
            // The final result contains every field. Discarding queued partial
            // chunks prevents stale intermediate work from delaying completion.
            update.fields.clear();
        }

        drop(pending);
        self.notify.notify_one();
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.notify.notify_one();
    }

    fn close_and_discard(&self, executor: &BackgroundExecutor) {
        self.closed.store(true, Ordering::Release);
        let pending = self.pending.lock().expect("parse update mailbox lock").take();
        if let Some(pending) = pending {
            executor
                .spawn(async move {
                    drop(pending);
                })
                .detach();
        }
        self.notify.notify_one();
    }

    fn take_batch(&self, max_fields: usize) -> Option<ParseUpdateBatch> {
        let mut pending = self.pending.lock().expect("parse update mailbox lock");
        let update = pending.as_mut()?;

        if update.parse_result.is_some() {
            let update = pending.take().expect("pending parse update");
            return Some(ParseUpdateBatch {
                definition_id: update.definition_id,
                fields: Vec::new(),
                parsed_offset: update.parsed_offset,
                total_bytes: update.total_bytes,
                is_done: update.is_done,
                is_finalizing: update.is_finalizing,
                parse_result: update.parse_result,
                has_more_fields: false,
            });
        }

        let mut fields = Vec::new();
        let mut field_count = 0;
        while let Some(chunk) = update.fields.front()
            && (fields.is_empty() || field_count + chunk.len() <= max_fields)
        {
            let chunk = update.fields.pop_front().expect("parse field chunk");
            field_count += chunk.len();
            fields.push(chunk);
        }

        let has_more_fields = !update.fields.is_empty();
        let batch = ParseUpdateBatch {
            definition_id: update.definition_id.clone(),
            fields,
            parsed_offset: update.parsed_offset,
            total_bytes: update.total_bytes,
            is_done: update.is_done,
            is_finalizing: update.is_finalizing,
            parse_result: None,
            has_more_fields,
        };

        if !has_more_fields {
            pending.take();
        }
        Some(batch)
    }
}

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("shift-escape", gpui_component::dock::ToggleZoom, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-w", crate::actions::CloseActivePanel, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-w", crate::actions::CloseActivePanel, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-f4", crate::actions::CloseActivePanel, None),
        KeyBinding::new("insert", crate::actions::ToggleInsertMode, None),
    ]);

    cx.on_action::<NewFile>(|_, cx| {
        defer_in_active_workspace(cx, |workspace, window, cx| {
            workspace.on_action_new_file(&NewFile, window, cx);
        });
    });
    cx.on_action::<OpenFile>(|action, cx| {
        let action = action.clone();
        defer_in_active_workspace(cx, move |workspace, window, cx| {
            workspace.on_action_open_file(&action, window, cx);
        });
    });
    cx.on_action::<OpenFileDialog>(|_, cx| {
        defer_in_active_workspace(cx, |workspace, window, cx| {
            workspace.on_action_open_file_dialog(&OpenFileDialog, window, cx);
        });
    });
    cx.on_action::<OpenFolder>(|_, cx| {
        defer_in_active_workspace(cx, |workspace, window, cx| {
            workspace.on_action_open_folder(&OpenFolder, window, cx);
        });
    });
    cx.on_action::<LoadStructureDefinition>(|_, cx| {
        defer_in_active_workspace(cx, |workspace, window, cx| {
            workspace.on_action_load_structure_definition(&LoadStructureDefinition, window, cx);
        });
    });
    cx.on_action::<crate::actions::LoadStructureDefinitionFromHistory>(|action, cx| {
        let action = action.clone();
        defer_in_active_workspace(cx, move |workspace, window, cx| {
            workspace.on_action_load_structure_definition_from_history(&action, window, cx);
        });
    });
    cx.on_action::<crate::actions::RemoveStructureDefinitionFromHistory>(|action, cx| {
        let action = action.clone();
        defer_in_active_workspace(cx, move |workspace, window, cx| {
            workspace.on_action_remove_structure_definition_from_history(&action, window, cx);
        });
    });
    cx.on_action::<crate::actions::RemoveFileFromHistory>(|action, cx| {
        let action = action.clone();
        defer_in_active_workspace(cx, move |workspace, window, cx| {
            workspace.on_action_remove_file_from_history(&action, window, cx);
        });
    });
    cx.on_action::<ToggleInsertMode>(|_, cx| {
        InsertModeState::toggle(cx);
    });

    cx.activate(true);
}

fn defer_in_active_workspace(cx: &mut App, handler: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static) {
    let Some(window) = cx.active_window() else {
        return;
    };

    cx.defer(move |cx| {
        let Some(window) = window.downcast::<Root>() else {
            return;
        };

        let _ = window.update(cx, |root, window, cx| {
            let Ok(workspace) = root.view().clone().downcast::<Workspace>() else {
                return;
            };

            workspace.update(cx, |workspace, cx| {
                handler(workspace, window, cx);
            });
        });
    });
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pane_tree = cx.new(|_| PaneTree::new());

        cx.subscribe_in(&pane_tree, window, |this, _, event: &PaneTreeEvent, window, cx| match event {
            PaneTreeEvent::ActiveEditorChanged => {
                this.sync_active_editor(window, cx);
            }
        })
        .detach();

        let app_menu_bar = AppMenuBar::new(window, cx);
        let title_bar = cx.new(|_cx| AppTitleBar { app_menu_bar });

        cx.subscribe_in(&title_bar, window, |this, _, event, window, cx| match event {
            crate::ui::components::title_bar::AppTitleBarEvent::OpenSettings => {
                this.open_settings_panel(window, cx);
            }
        })
        .detach();

        let file_tree = cx.new(|cx| FileTreeView::new("FILES", window, cx));
        let left_panel = cx.new(|cx| LeftPanel::new(file_tree.clone(), window, cx));
        let activity_bar = cx.new(ActivityBar::new);

        cx.subscribe_in(&activity_bar, window, |this, _, event: &ActivityBarEvent, window, cx| match event {
            ActivityBarEvent::Select(activity) => {
                this.select_activity(*activity, window, cx);
            }
            ActivityBarEvent::OpenSettings => {
                this.open_settings_panel(window, cx);
            }
        })
        .detach();

        cx.observe(&left_panel, |this, _, cx| {
            this.sync_activity_bar(cx);
        })
        .detach();

        let status_bar = cx.new(StatusBar::new);
        cx.subscribe_in(&status_bar, window, |this, _, event, window, cx| match event {
            crate::ui::components::status_bar::StatusBarEvent::ToggleLeftPanel => {
                this.set_left_panel_visible(!this.is_left_panel_visible, window, cx);
            }
        })
        .detach();

        let (handles, bookmark_panel, struct_tree) = {
            let left_read = left_panel.read(cx);
            (
                [
                    file_tree.read(cx).focus_handle(cx),
                    left_read.search_panel.read(cx).focus_handle(cx),
                    left_read.strings_panel.read(cx).focus_handle(cx),
                    left_read.struct_tree.read(cx).focus_handle(cx),
                    left_read.data_inspector.read(cx).focus_handle(cx),
                    left_read.visual_map.read(cx).focus_handle(cx),
                    left_read.checksum_panel.read(cx).focus_handle(cx),
                    left_read.bookmark_panel.read(cx).focus_handle(cx),
                ],
                left_read.bookmark_panel.clone(),
                left_read.struct_tree.clone(),
            )
        };

        for handle in handles {
            cx.on_focus_in(&handle, window, |this, _, cx| {
                this.on_focus_changed(cx);
                cx.notify();
            })
            .detach();
        }

        cx.subscribe_in(
            &bookmark_panel,
            window,
            |this, _, event: &crate::ui::components::bookmark_panel::BookmarkPanelEvent, _window, cx| match event {
                crate::ui::components::bookmark_panel::BookmarkPanelEvent::Export => {
                    this.on_action_export_bookmarks(&crate::actions::ExportBookmarks, _window, cx);
                }
                crate::ui::components::bookmark_panel::BookmarkPanelEvent::Import => {
                    this.on_action_import_bookmarks(&crate::actions::ImportBookmarks, _window, cx);
                }
                crate::ui::components::bookmark_panel::BookmarkPanelEvent::NavigateTo { offset, size } => {
                    if let Some(editor_panel) = this.active_editor_panel(cx) {
                        editor_panel.update(cx, |panel, cx| {
                            let len = (*size).max(1);
                            panel.scroll_to_range_if_needed(*offset..offset.saturating_add(len), cx);
                        });
                    }
                }
            },
        )
        .detach();

        cx.subscribe_in(
            &struct_tree,
            window,
            |this, _, event: &crate::ui::components::struct_tree_view::StructTreeViewEvent, _window, cx| match event {
                crate::ui::components::struct_tree_view::StructTreeViewEvent::NavigateTo { offset, size } => {
                    if let Some(editor_panel) = this.active_editor_panel(cx) {
                        editor_panel.update(cx, |panel, cx| {
                            let len = (*size).max(1);
                            panel.scroll_to_range_if_needed(*offset..offset.saturating_add(len), cx);
                        });
                    }
                }
            },
        )
        .detach();

        cx.subscribe(
            &left_panel,
            |this, _, event: &crate::ui::components::search_panel::SearchPanelEvent, cx| match event {
                crate::ui::components::search_panel::SearchPanelEvent::NavigateTo { offset, len } => {
                    if let Some(editor_panel) = this.active_editor_panel(cx) {
                        editor_panel.update(cx, |panel, cx| {
                            let match_len = (*len).max(1);
                            panel.scroll_to_range_if_needed(*offset..offset.saturating_add(match_len), cx);
                        });
                    }
                }
            },
        )
        .detach();

        cx.subscribe(
            &left_panel,
            |this, _, event: &crate::ui::components::strings_panel::StringsPanelEvent, cx| match event {
                crate::ui::components::strings_panel::StringsPanelEvent::NavigateTo { offset, len } => {
                    if let Some(editor_panel) = this.active_editor_panel(cx) {
                        editor_panel.update(cx, |panel, cx| {
                            let match_len = (*len).max(1);
                            panel.scroll_to_range_if_needed(*offset..offset.saturating_add(match_len), cx);
                        });
                    }
                }
            },
        )
        .detach();

        cx.subscribe(&left_panel, |_, _, event: &FileTreeViewEvent, cx| match event {
            FileTreeViewEvent::OpenFile(path) => {
                cx.dispatch_action(&crate::actions::OpenFile {
                    path: path.to_string_lossy().to_string(),
                });
            }
        })
        .detach();

        let recent_history = cx.global::<crate::settings::RecentHistoryState>().clone();
        let recent_definition_paths = recent_history.definitions.paths().to_vec();
        let recent_file_paths = recent_history.files.paths().to_vec();
        let workspace = Self {
            pane_tree,
            title_bar,
            status_bar,
            left_panel,
            activity_bar,
            recent_definition_history: recent_history.definitions,
            recent_file_history: recent_history.files,
            is_left_panel_visible: true,
            new_file_modal: None,
            untitled_count: 0,
            focus_handle: cx.focus_handle(),
            last_active_editor_id: Cell::new(None),
        };

        workspace.left_panel.update(cx, |panel, cx| {
            panel.set_structure_definition_history(&recent_definition_paths, cx);
            panel.set_file_history(&recent_file_paths, cx);
        });

        workspace
    }

    pub fn active_editor(&self, cx: &App) -> Option<Entity<Editor>> {
        self.pane_tree.read(cx).active_editor(cx)
    }

    pub fn active_editor_panel(&self, cx: &App) -> Option<Entity<EditorPanel>> {
        self.pane_tree.read(cx).active_editor_panel(cx)
    }

    fn publish_recent_history(&mut self, cx: &mut Context<Self>) {
        let definition_paths = self.recent_definition_history.paths().to_vec();
        let file_paths = self.recent_file_history.paths().to_vec();
        self.left_panel.update(cx, |panel, cx| {
            panel.set_structure_definition_history(&definition_paths, cx);
            panel.set_file_history(&file_paths, cx);
        });

        let definitions = self.recent_definition_history.clone();
        let files = self.recent_file_history.clone();
        cx.update_global::<crate::settings::RecentHistoryState, _>(|state, _| {
            state.definitions = definitions;
            state.files = files;
        });
        crate::settings::save_current(cx);
    }

    fn record_recent_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.recent_file_history.record(path);
        self.publish_recent_history(cx);
    }

    fn sync_active_editor(&self, window: &mut Window, cx: &mut Context<Self>) {
        let active_editor = self.active_editor(cx);
        let pane_tree_is_empty = self.pane_tree.read(cx).is_empty();

        // A split can emit several state events while its new group is being
        // assembled. Only the active editor entity affects these subscribers,
        // so avoid rebuilding every side-panel subscription for duplicate
        // notifications.
        let active_editor_id = active_editor.as_ref().map(Entity::entity_id);
        if self.last_active_editor_id.get() == active_editor_id {
            if pane_tree_is_empty {
                self.focus_handle.focus(window);
            }
            return;
        }
        self.last_active_editor_id.set(active_editor_id);

        self.status_bar.update(cx, |status_bar, cx| {
            status_bar.set_active_editor(active_editor.clone(), cx);
        });
        self.left_panel.update(cx, |panel, cx| {
            panel.set_editor(active_editor, cx);
        });
        self.on_focus_changed(cx);

        if pane_tree_is_empty {
            self.focus_handle.focus(window);
        }
    }

    fn set_left_panel_visible(&mut self, visible: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.is_left_panel_visible = visible;

        if visible {
            self.left_panel.update(cx, |panel, cx| {
                panel.sync_file_history(cx);
            });
            let focus_handle = self.left_panel.read(cx).focus_handle(cx);
            focus_handle.focus(window);
        } else {
            self.focus_handle.focus(window);
        }

        self.sync_activity_bar(cx);
        cx.notify();
    }

    fn observe_notification(&mut self, notification: &Entity<gpui_component::notification::NotificationList>, cx: &mut Context<Self>) {
        cx.observe(notification, |_, _, cx| {
            cx.notify();
        })
        .detach();
    }

    fn new_local(cx: &mut App) -> Task<anyhow::Result<WindowHandle<Root>>> {
        let mut window_size = size(px(1600.0), px(1200.0));
        if let Some(display) = cx.primary_display() {
            let display_size = display.bounds().size;
            window_size.width = window_size.width.min(display_size.width * 0.85);
            window_size.height = window_size.height.min(display_size.height * 0.85);
        }

        let window_bounds = Bounds::centered(None, window_size, cx);

        cx.spawn(async move |cx| {
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(window_bounds)),
                #[cfg(not(target_os = "linux"))]
                titlebar: Some(gpui_component::TitleBar::title_bar_options()),
                window_min_size: Some(gpui::Size {
                    width: px(640.),
                    height: px(480.),
                }),
                #[cfg(target_os = "linux")]
                window_background: gpui::WindowBackgroundAppearance::Transparent,
                #[cfg(target_os = "linux")]
                window_decorations: Some(gpui::WindowDecorations::Client),
                kind: WindowKind::Normal,
                ..Default::default()
            };

            let window = cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| Self::new(window, cx));
                let root = cx.new(|cx| Root::new(view.clone(), window, cx));
                let notification = root.read(cx).notification.clone();
                view.update(cx, |workspace, cx| {
                    workspace.observe_notification(&notification, cx);
                });
                root
            })?;

            window
                .update(cx, |_, window, cx| {
                    window.activate_window();
                    window.set_window_title("XVW");
                    cx.on_release(|_, cx| {
                        cx.quit();
                    })
                    .detach();
                })
                .expect("failed to update window");

            Ok(window)
        })
    }

    fn open_editor_panel(&mut self, document: Arc<RwLock<crate::core::document::Document>>, window: &mut Window, cx: &mut Context<Self>) {
        let default_encoding = *cx.global::<Encoding>();
        let editor = cx.new(|_| {
            let mut editor = Editor::new(document);
            editor.set_encoding(default_encoding);
            editor
        });

        let editor_panel = cx.new(|cx| EditorPanel::new(editor, window, cx));
        let content = TabContent::Editor(editor_panel);

        self.pane_tree.update(cx, |tree, cx| {
            tree.open_tab(content, window, cx);
        });

        self.sync_active_editor(window, cx);
        cx.notify();
    }

    fn on_action_new_file(&mut self, _: &NewFile, window: &mut Window, cx: &mut Context<Self>) {
        use crate::ui::components::new_file_modal::{NewFileModal, NewFileModalEvent};

        let modal = cx.new(|cx| NewFileModal::new(window, cx));
        cx.subscribe_in(&modal, window, |this, _, event: &NewFileModalEvent, window, cx| match event {
            NewFileModalEvent::Create { size, fill_byte } => {
                this.create_new_file(*size, *fill_byte, window, cx);
            }
            NewFileModalEvent::Cancel => {
                this.close_new_file_modal(window, cx);
            }
        })
        .detach();

        modal.update(cx, |m, cx| {
            m.focus(window, cx);
        });

        self.new_file_modal = Some(modal);
        cx.notify();
    }

    fn close_new_file_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_file_modal = None;
        self.sync_active_editor(window, cx);
        cx.notify();
    }

    fn create_new_file(&mut self, size: usize, fill_byte: u8, window: &mut Window, cx: &mut Context<Self>) {
        self.new_file_modal = None;
        self.untitled_count += 1;
        let title = format!("Untitled-{}.bin", self.untitled_count);
        let path = std::path::PathBuf::from(title);
        let data = vec![fill_byte; size];
        let buffer = crate::core::buffer::Buffer::new(data);
        let document = Arc::new(RwLock::new(crate::core::document::Document::new(path, buffer)));

        self.open_editor_panel(document, window, cx);
        cx.notify();
    }

    fn on_action_open_file_dialog(&mut self, _: &OpenFileDialog, window: &mut Window, cx: &mut Context<Self>) {
        let path = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select a file".into()),
        });

        let view = cx.entity();
        cx.spawn_in(window, async move |_, window| {
            if let Some(path) = path.await.ok().and_then(|r| r.ok()).flatten().and_then(|mut v| v.pop()) {
                window
                    .update(|window, cx| {
                        view.update(cx, |this, cx| {
                            this.left_panel.update(cx, |panel, cx| {
                                panel.sync_file_history(cx);
                            });
                            let action = crate::actions::OpenFile {
                                path: path.to_string_lossy().to_string(),
                            };
                            this.on_action_open_file(&action, window, cx);
                        });
                    })
                    .ok();
            }
        })
        .detach();
    }

    fn on_action_quit(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
    }

    fn on_action_select_all(&mut self, action: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.select_all(action, window, cx);
            });
        }
    }

    fn on_action_go_to_beginning(&mut self, action: &GoToBeginning, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.go_to_beginning(action, window, cx);
            });
        }
    }

    fn on_action_go_to_end(&mut self, action: &GoToEnd, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.go_to_end(action, window, cx);
            });
        }
    }

    fn on_action_toggle_search(&mut self, action: &ToggleSearch, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.toggle_search(action, window, cx);
            });
        }
    }

    fn on_action_toggle_goto_address(&mut self, action: &ToggleGoToAddress, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.toggle_goto_address(action, window, cx);
            });
        }
    }

    fn on_action_search_next(&mut self, action: &SearchNext, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.search_next(action, window, cx);
            });
        }
    }

    fn on_action_search_prev(&mut self, action: &SearchPrev, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.search_prev(action, window, cx);
            });
        }
    }

    fn on_action_copy(&mut self, action: &Copy, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.copy(action, window, cx);
            });
        }
    }

    fn on_action_copy_as_hexdump(&mut self, action: &CopyAsHexDump, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.copy_as_hexdump(action, window, cx);
            });
        }
    }

    fn on_action_copy_as_cpp_array(&mut self, action: &CopyAsCppArray, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.copy_as_cpp_array(action, window, cx);
            });
        }
    }

    fn on_action_copy_as_hex_stream(&mut self, action: &CopyAsHexStream, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.copy_as_hex_stream(action, window, cx);
            });
        }
    }

    fn on_action_copy_as_hex_spaces(&mut self, action: &CopyAsHexSpaces, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.copy_as_hex_spaces(action, window, cx);
            });
        }
    }

    fn on_action_copy_as_printable_text(&mut self, action: &CopyAsPrintableText, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.copy_as_printable_text(action, window, cx);
            });
        }
    }

    fn on_action_copy_as_base64(&mut self, action: &CopyAsBase64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.copy_as_base64(action, window, cx);
            });
        }
    }

    fn on_action_copy_as_escaped_string(&mut self, action: &CopyAsEscapedString, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.copy_as_escaped_string(action, window, cx);
            });
        }
    }

    fn on_action_copy_as_binary(&mut self, action: &CopyAsBinary, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.copy_as_binary(action, window, cx);
            });
        }
    }

    fn on_action_copy_as_rust_array(&mut self, action: &CopyAsRustArray, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.copy_as_rust_array(action, window, cx);
            });
        }
    }

    fn on_action_copy_as_json_array(&mut self, action: &CopyAsJsonArray, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.copy_as_json_array(action, window, cx);
            });
        }
    }

    fn on_action_bookmark_red(&mut self, action: &BookmarkRed, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.bookmark_red(action, window, cx);
            });
        }
    }

    fn on_action_bookmark_orange(&mut self, action: &BookmarkOrange, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.bookmark_orange(action, window, cx);
            });
        }
    }

    fn on_action_bookmark_yellow(&mut self, action: &BookmarkYellow, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.bookmark_yellow(action, window, cx);
            });
        }
    }

    fn on_action_bookmark_green(&mut self, action: &BookmarkGreen, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.bookmark_green(action, window, cx);
            });
        }
    }

    fn on_action_bookmark_cyan(&mut self, action: &BookmarkCyan, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.bookmark_cyan(action, window, cx);
            });
        }
    }

    fn on_action_bookmark_blue(&mut self, action: &BookmarkBlue, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.bookmark_blue(action, window, cx);
            });
        }
    }

    fn on_action_bookmark_purple(&mut self, action: &BookmarkPurple, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.bookmark_purple(action, window, cx);
            });
        }
    }

    fn on_action_bookmark_pink(&mut self, action: &BookmarkPink, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.bookmark_pink(action, window, cx);
            });
        }
    }

    fn on_action_clear_bookmark(&mut self, action: &ClearBookmark, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.clear_bookmark(action, window, cx);
            });
        }
    }

    fn on_action_clear_all_bookmarks(&mut self, action: &ClearAllBookmarks, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.clear_all_bookmarks(action, window, cx);
            });
        }
    }

    fn on_action_add_custom_break(&mut self, action: &AddCustomBreak, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.add_custom_break(action, window, cx);
            });
        }
    }

    fn on_action_remove_custom_break_backward(&mut self, action: &RemoveCustomBreakBackward, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.remove_custom_break_backward(action, window, cx);
            });
        }
    }

    fn on_action_remove_custom_break_forward(&mut self, action: &RemoveCustomBreakForward, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.remove_custom_break_forward(action, window, cx);
            });
        }
    }

    fn on_action_join_line(&mut self, action: &JoinLine, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.join_line(action, window, cx);
            });
        }
    }

    fn on_action_clear_all_custom_breaks(&mut self, action: &ClearAllCustomBreaks, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.clear_all_custom_breaks(action, window, cx);
            });
        }
    }

    fn on_action_set_encoding(&mut self, action: &SetEncoding, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, cx| {
                editor.set_encoding(action.encoding);
                cx.notify();
            });
        }
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.focus_hex_view(&FocusHexView, window, cx);
            });
        }
        cx.notify();
    }

    fn on_action_set_encoding_ascii(&mut self, _: &SetEncodingAscii, window: &mut Window, cx: &mut Context<Self>) {
        self.on_action_set_encoding(&SetEncoding { encoding: Encoding::Ascii }, window, cx);
    }

    fn on_action_set_encoding_utf8(&mut self, _: &SetEncodingUtf8, window: &mut Window, cx: &mut Context<Self>) {
        self.on_action_set_encoding(&SetEncoding { encoding: Encoding::Utf8 }, window, cx);
    }

    fn on_action_set_encoding_utf16le(&mut self, _: &SetEncodingUtf16Le, window: &mut Window, cx: &mut Context<Self>) {
        self.on_action_set_encoding(&SetEncoding { encoding: Encoding::Utf16Le }, window, cx);
    }

    fn on_action_set_encoding_utf16be(&mut self, _: &SetEncodingUtf16Be, window: &mut Window, cx: &mut Context<Self>) {
        self.on_action_set_encoding(&SetEncoding { encoding: Encoding::Utf16Be }, window, cx);
    }

    fn on_action_set_radix_hex(&mut self, _: &SetRadixHex, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, cx| {
                editor.set_radix(crate::core::radix::DisplayRadix::Hexadecimal);
                cx.notify();
            });
        }
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.focus_hex_view(&FocusHexView, window, cx);
            });
        }
        cx.notify();
    }

    fn on_action_set_radix_dec(&mut self, _: &SetRadixDec, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, cx| {
                editor.set_radix(crate::core::radix::DisplayRadix::Decimal);
                cx.notify();
            });
        }
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.focus_hex_view(&FocusHexView, window, cx);
            });
        }
        cx.notify();
    }

    fn on_action_set_radix_oct(&mut self, _: &SetRadixOct, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, cx| {
                editor.set_radix(crate::core::radix::DisplayRadix::Octal);
                cx.notify();
            });
        }
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.focus_hex_view(&FocusHexView, window, cx);
            });
        }
        cx.notify();
    }

    fn on_action_set_radix_bin(&mut self, _: &SetRadixBin, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, cx| {
                editor.set_radix(crate::core::radix::DisplayRadix::Binary);
                cx.notify();
            });
        }
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.focus_hex_view(&FocusHexView, window, cx);
            });
        }
        cx.notify();
    }

    fn on_action_set_group_size_1(&mut self, _: &SetGroupSize1, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, cx| {
                editor.set_group_size(crate::core::radix::ByteGroupSize::One);
                cx.notify();
            });
        }
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.focus_hex_view(&FocusHexView, window, cx);
            });
        }
        cx.notify();
    }

    fn on_action_set_group_size_2(&mut self, _: &SetGroupSize2, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, cx| {
                editor.set_group_size(crate::core::radix::ByteGroupSize::Two);
                cx.notify();
            });
        }
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.focus_hex_view(&FocusHexView, window, cx);
            });
        }
        cx.notify();
    }

    fn on_action_set_group_size_4(&mut self, _: &SetGroupSize4, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, cx| {
                editor.set_group_size(crate::core::radix::ByteGroupSize::Four);
                cx.notify();
            });
        }
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.focus_hex_view(&FocusHexView, window, cx);
            });
        }
        cx.notify();
    }

    fn on_action_set_group_size_8(&mut self, _: &SetGroupSize8, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, cx| {
                editor.set_group_size(crate::core::radix::ByteGroupSize::Eight);
                cx.notify();
            });
        }
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.focus_hex_view(&FocusHexView, window, cx);
            });
        }
        cx.notify();
    }

    fn on_action_set_byte_order_le(&mut self, _: &SetByteOrderLittleEndian, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, cx| {
                editor.set_is_big_endian(false);
                cx.notify();
            });
        }
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.focus_hex_view(&FocusHexView, window, cx);
            });
        }
        cx.notify();
    }

    fn on_action_set_byte_order_be(&mut self, _: &SetByteOrderBigEndian, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, cx| {
                editor.set_is_big_endian(true);
                cx.notify();
            });
        }
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.focus_hex_view(&FocusHexView, window, cx);
            });
        }
        cx.notify();
    }

    fn on_action_toggle_byte_order(&mut self, _: &ToggleByteOrder, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, cx| {
                editor.toggle_byte_order();
                cx.notify();
            });
        }
        if let Some(panel) = self.active_editor_panel(cx) {
            panel.update(cx, |panel, cx| {
                panel.focus_hex_view(&FocusHexView, window, cx);
            });
        }
        cx.notify();
    }

    fn on_action_open_file(&mut self, action: &OpenFile, window: &mut Window, cx: &mut Context<Self>) {
        let file_path = action.path.clone();
        let path = std::path::PathBuf::from(&file_path);
        let path = path.canonicalize().unwrap_or(path);

        // Check if path is already open in any group
        for group in self.pane_tree.read(cx).all_groups() {
            let tabs = group.read(cx).tabs.iter().enumerate().map(|(i, t)| (i, t.path(cx))).collect::<Vec<_>>();
            for (idx, tab_path) in tabs {
                let is_same_file = tab_path
                    .as_ref()
                    .is_some_and(|tab_path| tab_path.canonicalize().unwrap_or_else(|_| tab_path.clone()) == path);
                if is_same_file {
                    group.update(cx, |g, cx| {
                        g.activate_tab(idx, window, cx);
                    });
                    self.pane_tree.update(cx, |tree, cx| {
                        tree.set_active_group(group.read(cx).id, cx);
                    });
                    self.sync_active_editor(window, cx);
                    self.record_recent_file(path.clone(), cx);
                    cx.notify();
                    return;
                }
            }
        }

        let view = cx.entity();
        let recent_path = path.clone();
        cx.spawn_in(window, async move |_, window| {
            let editor_service_opt = window.update(|_, cx| AppState::global(cx).editor_service.clone()).ok();

            if let Some(editor_service) = editor_service_opt {
                match editor_service.open_file(std::path::PathBuf::from(&file_path)).await {
                    Ok(document) => {
                        window
                            .update(|window, cx| {
                                view.update(cx, |this, cx| {
                                    this.record_recent_file(recent_path.clone(), cx);
                                    this.open_editor_panel(document, window, cx);
                                });
                            })
                            .ok();
                    }
                    Err(e) => {
                        eprintln!("Failed to open file: {:?}", e);
                    }
                }
            }
        })
        .detach();
    }

    fn on_action_open_diff(&mut self, action: &OpenDiff, window: &mut Window, cx: &mut Context<Self>) {
        let left_path = action.left_path.clone();
        let right_path = action.right_path.clone();

        cx.spawn_in(window, async move |this, window| {
            let app = this.update(window, |_, cx| AppState::global(cx).clone()).expect("AppState global");

            if let Some(workspace) = this.upgrade() {
                let left_result = app.editor_service.open_file(std::path::PathBuf::from(left_path)).await;
                let right_result = app.editor_service.open_file(std::path::PathBuf::from(right_path)).await;

                if let (Ok(left_document), Ok(right_document)) = (left_result, right_result) {
                    let left_recent_path = left_document.read().ok().map(|document| document.path().to_path_buf());
                    let right_recent_path = right_document.read().ok().map(|document| document.path().to_path_buf());
                    let _ = workspace.update_in(window, |workspace_view, window, cx| {
                        if let Some(path) = left_recent_path {
                            workspace_view.record_recent_file(path, cx);
                        }
                        if let Some(path) = right_recent_path {
                            workspace_view.record_recent_file(path, cx);
                        }

                        let app = AppState::global(cx).clone();
                        let diff_result_task = app.editor_service.compute_diff(left_document.clone(), right_document.clone(), cx);

                        cx.spawn_in(window, async move |workspace, window| {
                            let diff_result = diff_result_task.await;

                            let _ = workspace.update_in(window, |workspace_view, window, cx| {
                                use crate::ui::panels::diff_panel::DiffPanel;
                                let diff_view = cx.new(|cx| {
                                    let mut view = DiffPanel::new(left_document.clone(), right_document.clone(), window, cx);
                                    view.set_diff_result(diff_result.clone(), cx);
                                    view
                                });

                                let content = TabContent::Diff(diff_view);
                                workspace_view.pane_tree.update(cx, |tree, cx| {
                                    tree.open_tab(content, window, cx);
                                });
                                workspace_view.sync_active_editor(window, cx);
                                cx.notify();
                            });
                        })
                        .detach();
                    });
                }
            }
        })
        .detach();
    }

    fn on_action_select_for_compare(&mut self, action: &SelectForCompare, _window: &mut Window, cx: &mut Context<Self>) {
        crate::app_state::PendingCompareState::set(Some(action.path.clone()), cx);
        self.left_panel.update(cx, |panel, cx| {
            panel.file_tree.update(cx, |file_tree, cx| {
                file_tree.pending_compare_path = Some(action.path.clone());
                cx.notify();
            });
        });
    }

    fn on_action_compare_with_active_file(&mut self, action: &CompareWithActiveFile, window: &mut Window, cx: &mut Context<Self>) {
        let active_path = self.active_editor(cx).and_then(|ed| {
            let doc = ed.read(cx).document.read().ok()?;
            Some(doc.path().to_path_buf())
        });
        if let Some(active_p) = active_path {
            let left_path = active_p.to_string_lossy().to_string();
            let right_path = action.path.clone();
            if left_path != right_path {
                self.on_action_open_diff(&OpenDiff { left_path, right_path }, window, cx);
            }
        }
    }

    fn on_action_compare_open_files(&mut self, _: &CompareOpenFiles, window: &mut Window, cx: &mut Context<Self>) {
        let mut open_paths = Vec::new();
        for group in self.pane_tree.read(cx).all_groups() {
            for tab in group.read(cx).tabs() {
                if let Some(p) = tab.path(cx)
                    && !open_paths.contains(&p)
                {
                    open_paths.push(p);
                }
            }
        }

        if open_paths.len() >= 2 {
            let active_path = self.active_editor(cx).and_then(|ed| {
                let doc = ed.read(cx).document.read().ok()?;
                Some(doc.path().to_path_buf())
            });

            let (left, right) = if let Some(active_p) = active_path {
                let other = open_paths.iter().find(|p| *p != &active_p).unwrap_or(&open_paths[0]);
                (active_p, other.clone())
            } else {
                (open_paths[0].clone(), open_paths[1].clone())
            };

            self.on_action_open_diff(
                &OpenDiff {
                    left_path: left.to_string_lossy().to_string(),
                    right_path: right.to_string_lossy().to_string(),
                },
                window,
                cx,
            );
        } else if open_paths.len() == 1 {
            let left_path = open_paths[0].clone();
            let prompt_path = cx.prompt_for_paths(gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("Select second file to compare with".into()),
            });

            let view = cx.entity();
            cx.spawn_in(window, async move |_, window| {
                if let Some(right_path) = prompt_path.await.ok().and_then(|r| r.ok()).flatten().and_then(|mut v| v.pop()) {
                    window
                        .update(|window, cx| {
                            view.update(cx, |this, cx| {
                                this.on_action_open_diff(
                                    &OpenDiff {
                                        left_path: left_path.to_string_lossy().to_string(),
                                        right_path: right_path.to_string_lossy().to_string(),
                                    },
                                    window,
                                    cx,
                                );
                            });
                        })
                        .ok();
                }
            })
            .detach();
        }
    }

    fn on_action_compare_visible_panes(&mut self, _: &CompareVisiblePanes, window: &mut Window, cx: &mut Context<Self>) {
        let groups = self.pane_tree.read(cx).all_groups();
        if groups.len() >= 2 {
            let g0_path = groups[0].read(cx).active_tab().and_then(|t| t.path(cx));
            let g1_path = groups[1].read(cx).active_tab().and_then(|t| t.path(cx));
            if let (Some(left), Some(right)) = (g0_path, g1_path)
                && left != right
            {
                self.on_action_open_diff(
                    &OpenDiff {
                        left_path: left.to_string_lossy().to_string(),
                        right_path: right.to_string_lossy().to_string(),
                    },
                    window,
                    cx,
                );
            }
        }
    }

    fn on_action_toggle_left_panel(&mut self, _: &ToggleLeftPanel, window: &mut Window, cx: &mut Context<Self>) {
        self.set_left_panel_visible(!self.is_left_panel_visible, window, cx);
    }

    fn on_action_toggle_search_panel(&mut self, _: &crate::actions::ToggleSearchPanel, window: &mut Window, cx: &mut Context<Self>) {
        self.select_activity(Activity::Search, window, cx);
    }

    fn on_action_show_files_tab(&mut self, _: &ShowFilesTab, window: &mut Window, cx: &mut Context<Self>) {
        self.select_activity(Activity::Files, window, cx);
    }

    fn on_action_show_strings_tab(&mut self, _: &ShowStringsTab, window: &mut Window, cx: &mut Context<Self>) {
        self.select_activity(Activity::Strings, window, cx);
    }

    fn on_action_show_structure_tab(&mut self, _: &ShowStructureTab, window: &mut Window, cx: &mut Context<Self>) {
        self.select_activity(Activity::Structure, window, cx);
    }

    fn on_action_show_checksum_tab(&mut self, _: &ShowChecksumTab, window: &mut Window, cx: &mut Context<Self>) {
        self.select_activity(Activity::Checksum, window, cx);
    }

    fn on_action_show_bookmarks_tab(&mut self, _: &ShowBookmarksTab, window: &mut Window, cx: &mut Context<Self>) {
        self.select_activity(Activity::Bookmarks, window, cx);
    }

    fn on_action_export_bookmarks(&mut self, _: &ExportBookmarks, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx) else { return };
        let (parent_dir, target_file_name) = {
            let doc_lock = editor.read(cx).document.read().ok();
            let parent = doc_lock
                .as_ref()
                .and_then(|d| d.path().parent().filter(|p| p.exists()).map(|p| p.to_path_buf()));
            let file_name = doc_lock.as_ref().and_then(|d| d.path().file_name().map(|n| n.to_string_lossy().into_owned()));
            (parent, file_name)
        };
        let parent_dir = parent_dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));
        let default_name = if let Some(name) = target_file_name
            && !name.is_empty()
            && name != "Untitled"
            && name != "untitled"
        {
            format!("{name}.bookmark.yaml")
        } else {
            "bookmarks.bookmark.yaml".to_string()
        };

        let prompt_path = cx.prompt_for_new_path(&parent_dir, Some(&default_name));

        let view = cx.entity().clone();
        cx.spawn_in(window, async move |_, window| {
            if let Some(mut path) = prompt_path.await.ok().and_then(|r| r.ok()).flatten() {
                if path.extension().is_none() {
                    path.set_extension("yaml");
                }

                window
                    .update(|_, cx| {
                        view.update(cx, |this, cx| {
                            if let Some(editor) = this.active_editor(cx)
                                && let Err(e) = editor.read(cx).export_bookmarks_to_file(&path)
                            {
                                eprintln!("Failed to export bookmarks: {}", e);
                            }
                        });
                    })
                    .ok();
            }
        })
        .detach();
    }

    fn on_action_import_bookmarks(&mut self, _: &ImportBookmarks, window: &mut Window, cx: &mut Context<Self>) {
        let prompt_path = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select bookmarks YAML file to import".into()),
        });

        let view = cx.entity().clone();
        cx.spawn_in(window, async move |_, window| {
            if let Some(path) = prompt_path.await.ok().and_then(|r| r.ok()).flatten().and_then(|mut v| v.pop()) {
                window
                    .update(|_, cx| {
                        view.update(cx, |this, cx| {
                            if let Some(editor) = this.active_editor(cx) {
                                let doc_path = editor.read(cx).document.read().ok().map(|d| d.path().to_path_buf());
                                editor.update(cx, |ed, cx| match ed.import_bookmarks_from_file(&path) {
                                    Ok(_) => {
                                        cx.notify();
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to import bookmarks: {}", e);
                                    }
                                });
                                if let Some(ref p) = doc_path {
                                    let service = crate::app_state::AppState::global(cx).editor_service.clone();
                                    service.notify_document_changed(p, cx);
                                }
                            }
                        });
                    })
                    .ok();
            }
        })
        .detach();
    }

    fn select_activity(&mut self, activity: Activity, window: &mut Window, cx: &mut Context<Self>) {
        let tab = match activity {
            Activity::Files => LeftPanelTab::Files,
            Activity::Search => LeftPanelTab::Search,
            Activity::Strings => LeftPanelTab::Strings,
            Activity::Structure => LeftPanelTab::Structure,
            Activity::Inspector => LeftPanelTab::Inspector,
            Activity::Map => LeftPanelTab::Map,
            Activity::Checksum => LeftPanelTab::Checksum,
            Activity::Bookmarks => LeftPanelTab::Bookmarks,
        };

        let current_tab = self.left_panel.read(cx).active_tab;

        if self.is_left_panel_visible && current_tab == tab {
            self.set_left_panel_visible(false, window, cx);
        } else {
            self.left_panel.update(cx, |p, cx| {
                p.set_tab(tab, cx);
            });
            self.set_left_panel_visible(true, window, cx);
            let focus_handle = self.left_panel.read(cx).focus_handle(cx);
            focus_handle.focus(window);
        }
    }

    fn on_action_load_structure_definition(&mut self, _: &LoadStructureDefinition, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_editor(cx).is_none() {
            return;
        }

        let path = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select a Kaitai Struct definition (.ksy, .yaml)".into()),
        });

        let view = cx.entity().clone();

        cx.spawn_in(window, async move |_, window| {
            if let Some(path) = path.await.ok().and_then(|r| r.ok()).flatten().and_then(|mut p| p.pop()) {
                window
                    .update(|window, cx| {
                        view.update(cx, |this, cx| {
                            this.load_structure_definition_from_path(path, window, cx);
                        });
                    })
                    .ok();
            }
        })
        .detach();
    }

    fn on_action_load_structure_definition_from_history(
        &mut self,
        action: &crate::actions::LoadStructureDefinitionFromHistory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_editor(cx).is_none() {
            return;
        }

        self.load_structure_definition_from_path(PathBuf::from(&action.path), window, cx);
    }

    fn load_structure_definition_from_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target_editor) = self.active_editor(cx) else { return };

        let view = cx.entity().clone();
        cx.spawn_in(window, async move |_, window| match std::fs::read_to_string(&path) {
            Ok(contents) => match serde_yaml::from_str::<crate::core::structure::KsyDefinition>(&contents) {
                Ok(ksy) => {
                    window
                        .update(|window, cx| {
                            view.update(cx, |this, cx| {
                                this.apply_loaded_structure_definition(target_editor, path, ksy, window, cx);
                            });
                        })
                        .ok();
                }
                Err(e) => {
                    eprintln!("Failed to parse KSY definition: {}", e);
                    let _ = window.update(|window, cx| {
                        window.push_notification(
                            gpui_component::notification::Notification::error(format!("Failed to parse structure definition: {e}")),
                            cx,
                        );
                    });
                }
            },
            Err(e) => {
                eprintln!("Failed to read KSY file at {:?}: {}", path, e);
                let _ = window.update(|window, cx| {
                    window.push_notification(
                        gpui_component::notification::Notification::error(format!("Failed to read structure file: {e}")),
                        cx,
                    );
                });
            }
        })
        .detach();
    }

    fn apply_loaded_structure_definition(
        &mut self,
        target_editor: Entity<Editor>,
        path: PathBuf,
        ksy: crate::core::structure::KsyDefinition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ksy = Arc::new(ksy);
        set_kaitai_definition_async(&target_editor, ksy, cx);

        self.recent_definition_history.record(path);
        self.publish_recent_history(cx);
        if self.active_editor(cx).is_some_and(|editor| editor.entity_id() == target_editor.entity_id()) {
            self.left_panel.update(cx, |panel, cx| {
                panel.set_editor(Some(target_editor), cx);
                panel.set_tab(crate::ui::panels::left_panel::LeftPanelTab::Structure, cx);
            });
            self.set_left_panel_visible(true, window, cx);
        }
    }

    fn on_action_remove_structure_definition_from_history(
        &mut self,
        action: &crate::actions::RemoveStructureDefinitionFromHistory,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = PathBuf::from(&action.path);
        if self.recent_definition_history.remove(&path) {
            self.publish_recent_history(cx);
        }
    }

    fn on_action_remove_file_from_history(&mut self, action: &crate::actions::RemoveFileFromHistory, _: &mut Window, cx: &mut Context<Self>) {
        let path = PathBuf::from(&action.path);
        if self.recent_file_history.remove(&path) {
            self.publish_recent_history(cx);
        }
    }

    fn on_action_clear_structure_definition(&mut self, _: &ClearStructureDefinition, _: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx) else {
            return;
        };
        let document_path = editor.read(cx).document.read().ok().map(|document| document.path().to_path_buf());
        editor.update(cx, |editor, cx| {
            editor.clear_structure_definition();
            cx.notify();
        });

        if let Some(path) = document_path {
            let service = crate::app_state::AppState::global(cx).editor_service.clone();
            service.notify_document_changed(&path, cx);
        }

        // The panels observe the editor entity. Clearing and re-binding the
        // same editor here is redundant and can re-enter the Structure Panel
        // while its action handler is still being dispatched.
        cx.notify();
    }

    fn on_action_toggle_inline_structure_view(&mut self, _: &crate::actions::ToggleInlineStructureView, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, cx| {
                editor.toggle_inline_structure_view();
                cx.notify();
            });
        }
    }

    fn on_action_open_folder(&mut self, _: &OpenFolder, window: &mut Window, cx: &mut Context<Self>) {
        let path = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select a folder".into()),
        });

        let left_panel = self.left_panel.clone();
        cx.spawn_in(window, async move |_, window| {
            if let Some(path) = path.await.ok().and_then(|r| r.ok()).flatten().and_then(|mut p| p.pop()) {
                window
                    .update(|_, cx| {
                        left_panel.update(cx, |p, cx| {
                            p.file_tree.update(cx, |ft, cx| {
                                ft.set_root_path(path, cx);
                            });
                        });
                    })
                    .ok();
            }
        })
        .detach();
    }

    fn on_action_close_folder(&mut self, _: &CloseFolder, _: &mut Window, cx: &mut Context<Self>) {
        self.left_panel.update(cx, |p, cx| {
            p.file_tree.update(cx, |ft, cx| {
                ft.close_folder(cx);
            });
        });
    }

    fn on_action_save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx) else {
            return;
        };
        let (document, state_id, is_read_only, path) = {
            let editor_read = editor.read(cx);
            let document = editor_read.document.clone();
            let document_read = document.read().expect("document read lock");
            (
                document.clone(),
                document_read.history.state_id(),
                document_read.is_read_only(),
                document_read.path().to_path_buf(),
            )
        };
        if is_read_only {
            return;
        }
        if !path.exists() {
            self.on_action_save_as(&crate::actions::SaveAs, window, cx);
            return;
        }
        let service = AppState::global(cx).editor_service.clone();
        let task = service.save_document(document.clone(), cx);
        let workspace = cx.entity().clone();

        cx.spawn(async move |_, cx| {
            let result = task.await;
            workspace
                .update(cx, |_, cx| {
                    match result {
                        Ok(()) => {
                            let should_mark_saved = document.read().map(|document| document.history.state_id() == state_id).unwrap_or(false);
                            if should_mark_saved {
                                document.write().expect("document write lock").mark_as_saved();
                            }
                            editor.update(cx, |_, cx| cx.notify());
                        }
                        Err(error) => {
                            eprintln!("Failed to save document: {error}");
                        }
                    }
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    fn on_action_toggle_read_only(&mut self, _: &ToggleReadOnly, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx) else {
            return;
        };
        let (document, is_read_only, is_dirty, state_id) = {
            let editor_read = editor.read(cx);
            let document = editor_read.document.clone();
            let document_read = document.read().expect("document read lock");
            (
                document.clone(),
                document_read.is_read_only(),
                document_read.is_dirty(),
                document_read.history.state_id(),
            )
        };

        if is_read_only {
            self.set_editor_read_only(&editor, false, cx);
            return;
        }

        if !is_dirty {
            self.set_editor_read_only(&editor, true, cx);
            return;
        }

        let prompt = window.prompt(
            gpui::PromptLevel::Warning,
            "Unsaved Changes",
            Some("Save changes before switching this file to read-only?"),
            &["Save and Make Read-only", "Cancel"],
            cx,
        );
        let workspace = cx.entity();
        let service = AppState::global(cx).editor_service.clone();

        cx.spawn_in(window, async move |_, window| {
            let Ok(choice) = prompt.await else {
                return;
            };
            if choice != 0 {
                return;
            }

            let Some(save_task) = window.update(|_, cx| service.save_document(document.clone(), cx)).ok() else {
                return;
            };
            let result = save_task.await;
            let _ = window.update(|_, cx| {
                workspace.update(cx, |workspace, cx| {
                    if let Err(error) = result {
                        eprintln!("Failed to save document before making it read-only: {error}");
                        return;
                    }

                    let unchanged_since_save = document.read().map(|document| document.history.state_id() == state_id).unwrap_or(false);
                    if unchanged_since_save {
                        document.write().expect("document write lock").mark_as_saved();
                        workspace.set_editor_read_only(&editor, true, cx);
                    }
                });
            });
        })
        .detach();
    }

    fn set_editor_read_only(&self, editor: &Entity<Editor>, read_only: bool, cx: &mut Context<Self>) {
        let (path, changed) = editor.update(cx, |editor, _| {
            let mut document = editor.document.write().expect("document write lock");
            let changed = document.is_read_only() != read_only;
            if changed {
                document.set_read_only(read_only);
            }
            (document.path().to_path_buf(), changed)
        });

        if changed {
            let service = AppState::global(cx).editor_service.clone();
            service.notify_document_changed(&path, cx);
            cx.notify();
        }
    }

    fn on_action_save_as(&mut self, _: &SaveAs, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx) else {
            return;
        };
        let (document, state_id, default_name, parent_dir) = {
            let editor_read = editor.read(cx);
            let document = editor_read.document.clone();
            let document_read = document.read().expect("document read lock");
            let default_name = document_read
                .path()
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "untitled.bin".to_string());
            let parent_dir = document_read
                .path()
                .parent()
                .filter(|p| p.exists())
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/")));
            (document.clone(), document_read.history.state_id(), default_name, parent_dir)
        };
        let prompt = cx.prompt_for_new_path(&parent_dir, Some(&default_name));
        let workspace = cx.entity().clone();
        let service = AppState::global(cx).editor_service.clone();

        cx.spawn_in(window, async move |_, window| {
            let Some(path) = prompt.await.ok().and_then(|result| result.ok()).flatten() else {
                return;
            };
            let task = window.update(|_, cx| service.save_document_to_path(document.clone(), path.clone(), cx)).ok();
            let Some(task) = task else {
                return;
            };
            let result = task.await;
            let _ = window.update(|_, cx| {
                workspace.update(cx, |this, cx| {
                    match result {
                        Ok(()) => {
                            let mut document_write = document.write().expect("document write lock");
                            document_write.set_path(path.clone());
                            if document_write.history.state_id() == state_id {
                                document_write.mark_as_saved();
                            }
                            drop(document_write);
                            service.notify_document_changed(&path, cx);
                            for group in this.pane_tree.read(cx).all_groups() {
                                group.update(cx, |_, cx| cx.notify());
                            }
                            editor.update(cx, |_, cx| cx.notify());
                        }
                        Err(error) => {
                            eprintln!("Failed to save document as: {error}");
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn on_action_close_active_panel(&mut self, _: &CloseActivePanel, window: &mut Window, cx: &mut Context<Self>) {
        self.pane_tree.update(cx, |tree, cx| {
            tree.close_active_tab(window, cx);
        });
        self.sync_active_editor(window, cx);
        cx.notify();
    }

    fn on_action_activate_next_tab(&mut self, _: &ActivateNextTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(group) = self.pane_tree.read(cx).active_group(cx) {
            group.update(cx, |g, cx| {
                g.activate_next_tab(window, cx);
            });
        }
    }

    fn on_action_activate_previous_tab(&mut self, _: &ActivatePreviousTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(group) = self.pane_tree.read(cx).active_group(cx) {
            group.update(cx, |g, cx| {
                g.activate_previous_tab(window, cx);
            });
        }
    }

    fn on_action_activate_tab(&mut self, action: &ActivateTab, window: &mut Window, cx: &mut Context<Self>) {
        if action.index > 0 {
            let zero_based = action.index - 1;
            if let Some(group) = self.pane_tree.read(cx).active_group(cx) {
                group.update(cx, |g, cx| {
                    g.activate_tab(zero_based, window, cx);
                });
            }
        }
    }

    fn on_action_close_other_tabs(&mut self, _: &CloseOtherTabs, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(group) = self.pane_tree.read(cx).active_group(cx) {
            let active_id = group.read(cx).active_tab().map(|t| t.id);
            if let Some(active_id) = active_id {
                let tab_ids: Vec<_> = group.read(cx).tabs.iter().map(|t| t.id).collect();
                for id in tab_ids {
                    if id != active_id {
                        group.update(cx, |g, cx| {
                            g.close_tab(id, window, cx);
                        });
                    }
                }
            }
        }
        self.sync_active_editor(window, cx);
        cx.notify();
    }

    fn on_action_close_tabs_to_right(&mut self, _: &CloseTabsToRight, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(group) = self.pane_tree.read(cx).active_group(cx) {
            let active_id = group.read(cx).active_tab().map(|t| t.id);
            if let Some(active_id) = active_id {
                group.update(cx, |g, cx| {
                    g.close_tabs_to_right(active_id, window, cx);
                });
            }
        }
        self.sync_active_editor(window, cx);
        cx.notify();
    }

    fn on_action_close_saved_tabs(&mut self, _: &CloseSavedTabs, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(group) = self.pane_tree.read(cx).active_group(cx) {
            group.update(cx, |g, cx| {
                g.close_saved_tabs(window, cx);
            });
        }
        self.sync_active_editor(window, cx);
        cx.notify();
    }

    fn on_action_close_all_tabs(&mut self, _: &CloseAllTabs, window: &mut Window, cx: &mut Context<Self>) {
        self.pane_tree = cx.new(|_| PaneTree::new());
        self.sync_active_editor(window, cx);
        cx.notify();
    }

    fn on_action_copy_path(&mut self, _: &CopyPath, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            let path = editor.read(cx).document.read().ok().map(|d| d.path().to_path_buf());
            if let Some(path) = path {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.to_string_lossy().to_string()));
            }
        }
    }

    fn on_action_copy_file_name(&mut self, _: &CopyFileName, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            let path = editor.read(cx).document.read().ok().map(|d| d.path().to_path_buf());
            if let Some(path) = path
                && let Some(name) = path.file_name()
            {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(name.to_string_lossy().to_string()));
            }
        }
    }

    fn on_action_reveal_in_explorer(&mut self, _: &RevealInExplorer, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            let path = editor.read(cx).document.read().ok().map(|d| d.path().to_path_buf());
            if let Some(path) = path {
                crate::ui::style::reveal_in_file_explorer(&path);
            }
        }
    }

    fn on_action_split_right(&mut self, _: &SplitRight, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(group) = self.pane_tree.read(cx).active_group(cx) {
            group.update(cx, |g, cx| {
                g.split_active_tab(SplitDirection::Horizontal, window, cx);
            });
        }
    }

    fn on_action_split_down(&mut self, _: &SplitDown, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(group) = self.pane_tree.read(cx).active_group(cx) {
            group.update(cx, |g, cx| {
                g.split_active_tab(SplitDirection::Vertical, window, cx);
            });
        }
    }

    fn on_action_open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings_panel(window, cx);
    }

    fn open_settings_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::ui::panels::settings_panel::SettingsPanel;

        // Check if settings is already open in any group
        for group in self.pane_tree.read(cx).all_groups() {
            for (idx, tab) in group.read(cx).tabs.iter().enumerate() {
                if let TabContent::Settings(_) = &tab.content {
                    group.update(cx, |g, cx| {
                        g.activate_tab(idx, window, cx);
                    });
                    self.pane_tree.update(cx, |tree, cx| {
                        tree.set_active_group(group.read(cx).id, cx);
                    });
                    self.sync_active_editor(window, cx);
                    cx.notify();
                    return;
                }
            }
        }

        let settings_panel = cx.new(|cx| SettingsPanel::new(window, cx));
        let content = TabContent::Settings(settings_panel);
        self.pane_tree.update(cx, |tree, cx| {
            tree.open_tab(content, window, cx);
        });
        self.sync_active_editor(window, cx);
        cx.notify();
    }

    fn on_action_open_visual_map(&mut self, _: &OpenVisualMap, window: &mut Window, cx: &mut Context<Self>) {
        self.select_activity(Activity::Map, window, cx);
    }

    /// Opens a new workspace window with the specified files and folder.
    /// This is the main public API for creating workspace windows.
    pub fn open_window(cx: &mut App, args: crate::CliArgs) -> Task<()> {
        let task = Self::new_local(cx);
        cx.spawn(async move |cx| {
            if let Ok(window) = task.await {
                let _ = window.update(cx, |root, window, cx| {
                    if let Ok(workspace) = root.view().clone().downcast::<Workspace>() {
                        workspace.update(cx, |workspace, cx| {
                            if let Some(folder_path) = args.folder_to_open.clone() {
                                workspace.left_panel.update(cx, |p, cx| {
                                    p.file_tree.update(cx, |ft, cx| {
                                        ft.set_root_path(folder_path, cx);
                                    });
                                });
                            }
                            if let Some((left_path, right_path)) = args.diff.clone() {
                                workspace.on_action_open_diff(
                                    &crate::actions::OpenDiff {
                                        left_path: left_path.to_string_lossy().to_string(),
                                        right_path: right_path.to_string_lossy().to_string(),
                                    },
                                    window,
                                    cx,
                                );
                            }
                        });

                        let view = workspace.clone();
                        let files = args.files_to_open.clone();
                        let ksy_to_load = args.ksy_to_load.clone();
                        let panel_name = args.panel.clone();

                        cx.spawn_in(window, async move |_, window| {
                            let editor_service_opt = window.update(|_, cx| AppState::global(cx).editor_service.clone()).ok();
                            if let Some(editor_service) = editor_service_opt {
                                for file_path in files {
                                    let recent_path = file_path.canonicalize().unwrap_or_else(|_| file_path.clone());
                                    if let Ok(document) = editor_service.open_file(file_path).await {
                                        let _ = window.update(|window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.record_recent_file(recent_path.clone(), cx);
                                                this.open_editor_panel(document, window, cx);
                                            });
                                        });
                                    }
                                }

                                if let Some(ksy_path) = ksy_to_load {
                                    let _ = window.update(|window, cx| {
                                        view.update(cx, |this, cx| {
                                            this.load_structure_definition_from_path(ksy_path, window, cx);
                                        });
                                    });
                                }

                                if let Some(panel_name) = panel_name {
                                    let tab = match panel_name.to_lowercase().as_str() {
                                        "files" => Some(LeftPanelTab::Files),
                                        "search" => Some(LeftPanelTab::Search),
                                        "strings" => Some(LeftPanelTab::Strings),
                                        "structure" => Some(LeftPanelTab::Structure),
                                        "inspector" => Some(LeftPanelTab::Inspector),
                                        "map" | "visual_map" => Some(LeftPanelTab::Map),
                                        "checksum" => Some(LeftPanelTab::Checksum),
                                        "bookmarks" => Some(LeftPanelTab::Bookmarks),
                                        _ => None,
                                    };
                                    if let Some(tab) = tab {
                                        let _ = window.update(|window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.left_panel.update(cx, |p, cx| {
                                                    p.set_tab(tab, cx);
                                                });
                                                this.set_left_panel_visible(true, window, cx);
                                            });
                                        });
                                    }
                                }

                                if args.sidebar == Some(false) || (args.diff.is_some() && args.sidebar != Some(true)) {
                                    let _ = window.update(|window, cx| {
                                        view.update(cx, |this, cx| {
                                            this.set_left_panel_visible(false, window, cx);
                                        });
                                    });
                                }
                            }
                        })
                        .detach();
                    }
                });
            }
        })
    }

    fn on_focus_changed(&self, cx: &mut Context<Self>) {
        self.left_panel.update(cx, |panel, cx| {
            panel.file_tree.update(cx, |_, cx| cx.notify());
            panel.strings_panel.update(cx, |_, cx| cx.notify());
            panel.struct_tree.update(cx, |_, cx| cx.notify());
            panel.data_inspector.update(cx, |_, cx| cx.notify());
            panel.visual_map.update(cx, |_, cx| cx.notify());
            panel.checksum_panel.update(cx, |_, cx| cx.notify());
        });

        for group in self.pane_tree.read(cx).all_groups() {
            group.update(cx, |_, cx| cx.notify());
        }
    }

    fn sync_activity_bar(&self, cx: &mut Context<Self>) {
        let is_visible = self.is_left_panel_visible;
        let active_tab = self.left_panel.read(cx).active_tab;
        self.activity_bar.update(cx, |activity_bar, cx| {
            if is_visible {
                match active_tab {
                    LeftPanelTab::Files => activity_bar.set_activity(Some(Activity::Files), cx),
                    LeftPanelTab::Search => activity_bar.set_activity(Some(Activity::Search), cx),
                    LeftPanelTab::Strings => activity_bar.set_activity(Some(Activity::Strings), cx),
                    LeftPanelTab::Structure => activity_bar.set_activity(Some(Activity::Structure), cx),
                    LeftPanelTab::Inspector => activity_bar.set_activity(Some(Activity::Inspector), cx),
                    LeftPanelTab::Map => activity_bar.set_activity(Some(Activity::Map), cx),
                    LeftPanelTab::Checksum => activity_bar.set_activity(Some(Activity::Checksum), cx),
                    LeftPanelTab::Bookmarks => activity_bar.set_activity(Some(Activity::Bookmarks), cx),
                }
            } else {
                activity_bar.set_activity(None, cx);
            }
        });
    }

    fn render_bottom_right_notifications(window: &mut Window, cx: &mut App) -> Option<impl IntoElement> {
        let root = window.root::<Root>()??;
        let items = root.read(cx).notification.read(cx).notifications();
        if items.is_empty() {
            return None;
        }
        let items = items.into_iter().rev().take(10).rev();

        Some(
            div()
                .absolute()
                .bottom(px(32.0))
                .right(px(16.0))
                .child(v_flex().id("notification-list-bottom-right").gap_3().children(items)),
        )
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("workspace")
            .on_action(cx.listener(Self::on_action_new_file))
            .on_action(cx.listener(Self::on_action_open_file))
            .on_action(cx.listener(Self::on_action_save))
            .on_action(cx.listener(Self::on_action_save_as))
            .on_action(cx.listener(Self::on_action_toggle_read_only))
            .on_action(cx.listener(Self::on_action_close_active_panel))
            .on_action(cx.listener(Self::on_action_open_file_dialog))
            .on_action(cx.listener(Self::on_action_quit))
            .on_action(cx.listener(Self::on_action_select_all))
            .on_action(cx.listener(Self::on_action_go_to_beginning))
            .on_action(cx.listener(Self::on_action_go_to_end))
            .on_action(cx.listener(Self::on_action_toggle_goto_address))
            .on_action(cx.listener(Self::on_action_toggle_search))
            .on_action(cx.listener(Self::on_action_toggle_search_panel))
            .on_action(cx.listener(Self::on_action_search_next))
            .on_action(cx.listener(Self::on_action_search_prev))
            .on_action(cx.listener(Self::on_action_copy))
            .on_action(cx.listener(Self::on_action_copy_as_hexdump))
            .on_action(cx.listener(Self::on_action_copy_as_cpp_array))
            .on_action(cx.listener(Self::on_action_copy_as_hex_stream))
            .on_action(cx.listener(Self::on_action_copy_as_hex_spaces))
            .on_action(cx.listener(Self::on_action_copy_as_printable_text))
            .on_action(cx.listener(Self::on_action_copy_as_base64))
            .on_action(cx.listener(Self::on_action_copy_as_escaped_string))
            .on_action(cx.listener(Self::on_action_copy_as_binary))
            .on_action(cx.listener(Self::on_action_copy_as_rust_array))
            .on_action(cx.listener(Self::on_action_copy_as_json_array))
            .on_action(cx.listener(Self::on_action_bookmark_red))
            .on_action(cx.listener(Self::on_action_bookmark_orange))
            .on_action(cx.listener(Self::on_action_bookmark_yellow))
            .on_action(cx.listener(Self::on_action_bookmark_green))
            .on_action(cx.listener(Self::on_action_bookmark_cyan))
            .on_action(cx.listener(Self::on_action_bookmark_blue))
            .on_action(cx.listener(Self::on_action_bookmark_purple))
            .on_action(cx.listener(Self::on_action_bookmark_pink))
            .on_action(cx.listener(Self::on_action_clear_bookmark))
            .on_action(cx.listener(Self::on_action_clear_all_bookmarks))
            .on_action(cx.listener(Self::on_action_add_custom_break))
            .on_action(cx.listener(Self::on_action_remove_custom_break_backward))
            .on_action(cx.listener(Self::on_action_remove_custom_break_forward))
            .on_action(cx.listener(Self::on_action_join_line))
            .on_action(cx.listener(Self::on_action_clear_all_custom_breaks))
            .on_action(cx.listener(Self::on_action_set_encoding))
            .on_action(cx.listener(Self::on_action_set_encoding_ascii))
            .on_action(cx.listener(Self::on_action_set_encoding_utf8))
            .on_action(cx.listener(Self::on_action_set_encoding_utf16le))
            .on_action(cx.listener(Self::on_action_set_encoding_utf16be))
            .on_action(cx.listener(Self::on_action_set_radix_hex))
            .on_action(cx.listener(Self::on_action_set_radix_dec))
            .on_action(cx.listener(Self::on_action_set_radix_oct))
            .on_action(cx.listener(Self::on_action_set_radix_bin))
            .on_action(cx.listener(Self::on_action_set_group_size_1))
            .on_action(cx.listener(Self::on_action_set_group_size_2))
            .on_action(cx.listener(Self::on_action_set_group_size_4))
            .on_action(cx.listener(Self::on_action_set_group_size_8))
            .on_action(cx.listener(Self::on_action_set_byte_order_le))
            .on_action(cx.listener(Self::on_action_set_byte_order_be))
            .on_action(cx.listener(Self::on_action_toggle_byte_order))
            .on_action(cx.listener(Self::on_action_open_diff))
            .on_action(cx.listener(Self::on_action_select_for_compare))
            .on_action(cx.listener(Self::on_action_compare_with_active_file))
            .on_action(cx.listener(Self::on_action_compare_open_files))
            .on_action(cx.listener(Self::on_action_compare_visible_panes))
            .on_action(cx.listener(Self::on_action_toggle_left_panel))
            .on_action(cx.listener(Self::on_action_open_settings))
            .on_action(cx.listener(Self::on_action_open_visual_map))
            .on_action(cx.listener(Self::on_action_show_files_tab))
            .on_action(cx.listener(Self::on_action_show_strings_tab))
            .on_action(cx.listener(Self::on_action_show_structure_tab))
            .on_action(cx.listener(Self::on_action_show_checksum_tab))
            .on_action(cx.listener(Self::on_action_show_bookmarks_tab))
            .on_action(cx.listener(Self::on_action_export_bookmarks))
            .on_action(cx.listener(Self::on_action_import_bookmarks))
            .on_action(cx.listener(Self::on_action_load_structure_definition))
            .on_action(cx.listener(Self::on_action_load_structure_definition_from_history))
            .on_action(cx.listener(Self::on_action_remove_structure_definition_from_history))
            .on_action(cx.listener(Self::on_action_remove_file_from_history))
            .on_action(cx.listener(Self::on_action_clear_structure_definition))
            .on_action(cx.listener(Self::on_action_toggle_inline_structure_view))
            .on_action(cx.listener(Self::on_action_open_folder))
            .on_action(cx.listener(Self::on_action_close_folder))
            .on_action(cx.listener(Self::on_action_activate_next_tab))
            .on_action(cx.listener(Self::on_action_activate_previous_tab))
            .on_action(cx.listener(Self::on_action_activate_tab))
            .on_action(cx.listener(Self::on_action_close_other_tabs))
            .on_action(cx.listener(Self::on_action_close_tabs_to_right))
            .on_action(cx.listener(Self::on_action_close_saved_tabs))
            .on_action(cx.listener(Self::on_action_close_all_tabs))
            .on_action(cx.listener(Self::on_action_copy_path))
            .on_action(cx.listener(Self::on_action_copy_file_name))
            .on_action(cx.listener(Self::on_action_reveal_in_explorer))
            .on_action(cx.listener(Self::on_action_split_right))
            .on_action(cx.listener(Self::on_action_split_down))
            .on_drop(cx.listener(move |this, external_paths: &gpui::ExternalPaths, window, cx| {
                for path in external_paths.paths() {
                    if path.is_file() {
                        this.left_panel.update(cx, |panel, cx| {
                            panel.sync_file_history(cx);
                        });
                        let action = crate::actions::OpenFile {
                            path: path.to_string_lossy().to_string(),
                        };
                        this.on_action_open_file(&action, window, cx);
                    } else if path.is_dir() {
                        this.left_panel.update(cx, |p, cx| {
                            p.set_tab(crate::ui::panels::left_panel::LeftPanelTab::Files, cx);
                            p.file_tree.update(cx, |ft, cx| {
                                ft.set_root_path(path.clone(), cx);
                            });
                        });
                        this.set_left_panel_visible(true, window, cx);
                    }
                }
                cx.notify();
            }))
            .relative()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(self.title_bar.clone())
            .child(
                div()
                    .track_focus(&self.focus_handle)
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.activity_bar.clone())
                    .child(
                        div().flex().flex_col().flex_1().size_full().min_w_0().min_h_0().overflow_hidden().child(
                            h_resizable("workspace-h-resize")
                                .child(
                                    resizable_panel()
                                        .visible(self.is_left_panel_visible)
                                        .size(px(250.))
                                        .child(div().size_full().min_w_0().min_h_0().overflow_hidden().child(self.left_panel.clone())),
                                )
                                .child(
                                    resizable_panel().child(div().relative().size_full().min_w_0().min_h_0().overflow_hidden().child(self.pane_tree.clone())),
                                ),
                        ),
                    ),
            )
            .child(self.status_bar.clone())
            .when_some(self.new_file_modal.clone(), |el, modal| {
                el.child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(gpui::rgba(0x00000080))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .child(modal),
                )
            })
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
            .children(Self::render_bottom_right_notifications(window, cx))
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

pub fn set_kaitai_definition_async(editor_entity: &Entity<Editor>, ksy: Arc<crate::core::structure::KsyDefinition>, cx: &mut App) {
    let cancel_token = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancel_token_clone = cancel_token.clone();

    let (doc_arc, doc_path, generation) = editor_entity.update(cx, |editor, cx| {
        editor.cancel_structure_parsing();
        editor.parse_cancel_token = Some(cancel_token.clone());
        editor.structure_parse_async = true;
        editor.structure_reparse_requested = false;
        *editor.ksy_definition.write().expect("ksy_definition write lock") = Some(ksy.clone());
        editor.is_parsing_structure = true;
        editor.parse_progress_offset = 0;
        let total = editor.document.read().expect("document read lock").buffer.len();
        let path = editor.document.read().ok().map(|d| d.path().to_path_buf());
        editor.parse_total_size = total;
        editor.begin_partial_parse_result(ksy.meta.id.clone());
        editor.invalidate_line_map();
        cx.notify();
        (editor.document.clone(), path, editor.parse_generation)
    });

    if let Some(ref path) = doc_path {
        let service = crate::app_state::AppState::global(cx).editor_service.clone();
        service.notify_document_changed(path, cx);
    }

    let mailbox = Arc::new(ParseUpdateMailbox {
        pending: Mutex::new(None),
        notify: tokio::sync::Notify::new(),
        closed: AtomicBool::new(false),
    });
    let producer_mailbox = mailbox.clone();

    let editor_entity = editor_entity.clone();
    let ksy_clone = (*ksy).clone();

    std::thread::Builder::new()
        .name("kaitai-parser".into())
        .spawn(move || {
            let buffer = {
                if let Ok(doc) = doc_arc.read() {
                    doc.buffer.clone()
                } else {
                    crate::core::buffer::Buffer::empty()
                }
            };

            let mut stream = crate::core::structure::KaitaiStream::new(buffer.data());
            let interpreter = crate::core::structure::KaitaiInterpreter::new(ksy_clone);
            interpreter.parse_with_progress_cancellable(&mut stream, Some(&cancel_token_clone), |progress| {
                if !cancel_token_clone.load(std::sync::atomic::Ordering::Relaxed) {
                    // The mailbox keeps field chunks but coalesces all stale
                    // progress metadata. Cloning the progress is shallow for
                    // the field payload (`Arc<[ParsedField]>`).
                    producer_mailbox.publish(progress.clone());
                }
            });
            producer_mailbox.close();
        })
        .expect("Failed to spawn kaitai parser thread");

    cx.spawn(async move |cx| {
        // Keep one foreground update small enough to leave the renderer
        // responsive even when a structure contains very cheap repeated
        // fields. The parser remains free to run ahead in the mailbox.
        const MAX_FIELDS_PER_UPDATE: usize = 1024;

        loop {
            let notified = mailbox.notify.notified();
            let Some(batch) = mailbox.take_batch(MAX_FIELDS_PER_UPDATE) else {
                if mailbox.closed.load(Ordering::Acquire) {
                    break;
                }
                notified.await;
                continue;
            };

            let mut batch = Some(batch);
            let delivery = editor_entity.update(cx, |editor, cx| {
                let batch = batch.take().expect("parse update batch must be present");
                if editor.parse_generation != generation {
                    return ParseUpdateDelivery::Stale(batch);
                }
                if !editor.is_parsing_structure && !batch.is_done {
                    return ParseUpdateDelivery::Stale(batch);
                }
                let is_done = batch.is_done;
                let has_more_fields = batch.has_more_fields;
                editor.parse_progress_offset = batch.parsed_offset;
                editor.parse_total_size = batch.total_bytes;
                editor.is_finalizing_structure = batch.is_finalizing;
                if let Some(res) = batch.parse_result {
                    editor.set_parse_result_arc(res);
                } else if !batch.fields.is_empty() {
                    editor.append_parse_chunks(batch.definition_id, batch.fields, batch.parsed_offset, batch.total_bytes);
                }
                if is_done {
                    editor.is_parsing_structure = false;
                    editor.is_finalizing_structure = false;
                    editor.parse_cancel_token = None;

                    if let Some(ref res) = editor.parse_result()
                        && let Some(err) = res.errors.first()
                    {
                        let msg = format!("Structure parse error at offset 0x{:08X}: {}", err.offset, err.message);
                        if let Some(window) = cx.active_window()
                            && let Some(window) = window.downcast::<Root>()
                        {
                            let _ = window.update(cx, |root, window, cx| {
                                let note = gpui_component::notification::Notification::error(msg);
                                root.notification.update(cx, |view, cx| view.push(note, window, cx));
                                cx.notify();
                            });
                        }
                    }
                }
                cx.notify();
                ParseUpdateDelivery::Applied {
                    should_continue: !is_done,
                    has_more_fields,
                }
            });

            let (should_continue, has_more_fields) = match delivery {
                Ok(ParseUpdateDelivery::Applied {
                    should_continue,
                    has_more_fields,
                }) => (should_continue, has_more_fields),
                Ok(ParseUpdateDelivery::Stale(stale_batch)) => {
                    let executor = cx.background_executor();
                    mailbox.close_and_discard(executor);
                    stale_batch.discard_on_background(executor);
                    break;
                }
                Err(_) => {
                    let executor = cx.background_executor();
                    mailbox.close_and_discard(executor);
                    if let Some(batch) = batch {
                        batch.discard_on_background(executor);
                    }
                    break;
                }
            };

            if !should_continue {
                break;
            }

            // Give GPUI a chance to render the newly received fields and
            // update the status bar before processing the next batch. If the
            // mailbox still contains chunks, the next notification is already
            // scheduled by `take_batch`.
            if has_more_fields || should_continue {
                cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
            }
        }
    })
    .detach();
}
