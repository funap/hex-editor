use gpui::prelude::*;
use gpui::*;
use gpui_component::ActiveTheme;

use crate::actions::*;

use crate::ui::components::activity_bar::{Activity, ActivityBar, ActivityBarEvent};
use crate::ui::components::file_tree_view::{FileTreeView, FileTreeViewEvent};
use crate::ui::components::title_bar::AppTitleBar;
use crate::ui::panels::editor_panel::EditorPanel;
use crate::ui::panels::left_panel::{LeftPanel, LeftPanelTab};

use crate::app_state::AppState;
use crate::core::editor::Editor;
use crate::service::open_file_manager::{OpenFileEvent, OpenFileManager};
use crate::ui::components::status_bar::StatusBar;
use gpui_component::Root;
use gpui_component::dock::{DockArea, DockItem, DockPlacement, PanelView};
use gpui_component::menu::AppMenuBar;
use gpui_component::resizable::{h_resizable, resizable_panel};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub struct Workspace {
    pub dock_area: Entity<DockArea>,
    pub title_bar: Entity<AppTitleBar>,
    pub status_bar: Entity<StatusBar>,
    pub open_file_manager: Entity<OpenFileManager>,
    pub active_panel: Option<Arc<dyn PanelView>>,
    pub left_panel: Entity<LeftPanel>,
    pub activity_bar: Entity<ActivityBar>,
    pub ksy_definition: Option<Arc<crate::core::structure::KsyDefinition>>,
    pub is_left_panel_visible: bool,
}

const MAIN_DOCK_AREA_ID: &str = "main_dock_area";
const MAIN_DOCK_AREA_VERSION: usize = 1;

pub fn init(cx: &mut App) {
    cx.bind_keys(vec![
        KeyBinding::new("shift-escape", gpui_component::dock::ToggleZoom, None),
        KeyBinding::new("ctrl-w", crate::actions::CloseActivePanel, None),
    ]);

    cx.activate(true);
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let dock_area = cx.new(|cx| DockArea::new(MAIN_DOCK_AREA_ID, Some(MAIN_DOCK_AREA_VERSION), window, cx));
        let weak_dock_area = dock_area.downgrade();

        cx.observe(&dock_area, |_, _, cx| cx.notify()).detach();

        let app_menu_bar = AppMenuBar::new(window, cx);
        let title_bar = cx.new(|_cx| AppTitleBar { app_menu_bar });

        cx.subscribe_in(&title_bar, window, |this, _, event, window, cx| match event {
            crate::ui::components::title_bar::AppTitleBarEvent::OpenSettings => {
                this.open_settings_panel(window, cx);
            }
        })
        .detach();

        let file_tree = cx.new(|cx| FileTreeView::new("FILES", cx));
        let left_panel = cx.new(|cx| LeftPanel::new(file_tree.clone(), cx));
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
        cx.subscribe(&status_bar, |this, _, event, cx| match event {
            crate::ui::components::status_bar::StatusBarEvent::ToggleLeftPanel => {
                this.is_left_panel_visible = !this.is_left_panel_visible;
                cx.notify();
            }
        })
        .detach();

        let left_read = left_panel.read(cx);
        let handles = [
            file_tree.read(cx).focus_handle(cx),
            left_read.struct_tree.read(cx).focus_handle(cx),
            left_read.data_inspector.read(cx).focus_handle(cx),
            left_read.visual_map.read(cx).focus_handle(cx),
            left_read.checksum_panel.read(cx).focus_handle(cx),
        ];

        for handle in handles {
            cx.on_focus_in(&handle, window, |this, _, cx| {
                this.on_focus_changed(cx);
                cx.notify();
            })
            .detach();
        }

        cx.subscribe(&left_panel, |_, _, event, cx| match event {
            FileTreeViewEvent::OpenFile(path) => {
                cx.dispatch_action(&crate::actions::OpenFile {
                    path: path.to_string_lossy().to_string(),
                });
            }
        })
        .detach();

        let open_file_manager = cx.new(|_| OpenFileManager::new());
        cx.subscribe(&open_file_manager, |this, _, event, cx| match event {
            OpenFileEvent::Opened(_) => {}
            OpenFileEvent::Closed(_) => {}
            OpenFileEvent::Activated(_) => {
                this.sync_active_editor(cx);
            }
        })
        .detach();

        Self::reset_default_layout(weak_dock_area, window, cx);
        Self {
            dock_area,
            title_bar,
            status_bar,
            open_file_manager,
            active_panel: None,
            left_panel,
            activity_bar,
            ksy_definition: None,
            is_left_panel_visible: true,
        }
    }

    pub fn active_editor(&self, cx: &App) -> Option<Entity<Editor>> {
        self.open_file_manager.read(cx).active_editor()
    }

    fn sync_active_editor(&self, cx: &mut Context<Self>) {
        let active_editor = self.active_editor(cx);
        self.status_bar.update(cx, |status_bar, _| {
            status_bar.set_active_editor(active_editor.clone());
        });
        self.left_panel.update(cx, |panel, cx| {
            panel.set_editor(active_editor, cx);
        });
        self.on_focus_changed(cx);
    }

    fn reset_default_layout(dock_area: WeakEntity<DockArea>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dock_area_entity) = dock_area.upgrade() {
            dock_area_entity.update(cx, |dock_area_view, cx| {
                // Center dock starts empty
                dock_area_view.set_center(DockItem::split(Axis::Vertical, vec![], &dock_area, window, cx), window, cx);
            });
        }
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
                cx.new(|cx| Root::new(view, window, cx))
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
        let path = document.read().expect("document read lock").path().to_path_buf();
        let editor = cx.new(|_| Editor::new(document.clone()));

        if let Some(ksy) = &self.ksy_definition {
            set_kaitai_definition_async(&editor, ksy.clone(), cx);
        }

        let editor_panel = cx.new(|cx| EditorPanel::new(editor.clone(), window, cx));

        let open_file_manager = self.open_file_manager.clone();
        let id = open_file_manager.update(cx, |manager, cx| manager.open(path, document, editor.clone(), editor_panel.clone(), cx));

        cx.on_focus_in(&editor_panel.read(cx).focus_handle(cx), window, {
            let editor_panel = editor_panel.clone();
            let open_file_manager = open_file_manager.clone();
            move |this, _window, cx| {
                open_file_manager.update(cx, |manager, cx| {
                    manager.activate(id, cx);
                });
                this.active_panel = Some(Arc::new(editor_panel.clone()));
                this.sync_active_editor(cx);
                cx.notify();
            }
        })
        .detach();
        let panel = Arc::new(editor_panel);
        self.add_panel_to_center_dock(panel, window, cx);
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
            println!("OpenFileDialog prompt returned");
            if let Some(path) = path.await.ok().and_then(|r| r.ok()).flatten().and_then(|mut v| v.pop()) {
                println!("Selected path: {:?}", path);
                window
                    .update(|window, cx| {
                        println!("Directly calling OpenFile handler for {:?}", path);
                        view.update(cx, |this, cx| {
                            let action = crate::actions::OpenFile {
                                path: path.to_string_lossy().to_string(),
                            };
                            this.on_action_open_file(&action, window, cx);
                        });
                    })
                    .ok();
            } else {
                println!("No path selected or error occurred");
            }
        })
        .detach();
    }

    fn on_action_quit(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
    }

    fn on_action_select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.select_all();
            });
        }
    }

    fn on_action_go_to_beginning(&mut self, _: &GoToBeginning, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.go_to_beginning();
            });
        }
    }

    fn on_action_go_to_end(&mut self, _: &GoToEnd, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.go_to_end();
            });
        }
    }

    fn on_action_set_encoding_ascii(&mut self, _: &SetEncodingAscii, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.set_encoding(crate::core::encoding::Encoding::Ascii);
            });
        }
    }

    fn on_action_set_encoding_utf8(&mut self, _: &SetEncodingUtf8, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.set_encoding(crate::core::encoding::Encoding::Utf8);
            });
        }
    }

    fn on_action_set_encoding_utf16le(&mut self, _: &SetEncodingUtf16Le, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.set_encoding(crate::core::encoding::Encoding::Utf16Le);
            });
        }
    }

    fn on_action_set_encoding_utf16be(&mut self, _: &SetEncodingUtf16Be, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.set_encoding(crate::core::encoding::Encoding::Utf16Be);
            });
        }
    }

    fn on_action_open_file(&mut self, action: &OpenFile, window: &mut Window, cx: &mut Context<Self>) {
        let file_path = action.path.clone();
        let path = std::path::PathBuf::from(&file_path);

        if let Some(entry) = self.open_file_manager.read(cx).find_by_path(&path) {
            entry.panel.read(cx).focus_handle(cx).focus(window);
            return;
        }

        let view = cx.entity();
        cx.spawn_in(window, async move |_, window| {
            let editor_service_opt = window.update(|_, cx| AppState::global(cx).editor_service.clone()).ok();

            if let Some(editor_service) = editor_service_opt {
                match editor_service.open_file(std::path::PathBuf::from(&file_path)).await {
                    Ok(document) => {
                        window
                            .update(|window, cx| {
                                view.update(cx, |this, cx| {
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
                    let _ = workspace.update_in(window, |_, window, cx| {
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

                                let diff_view_clone = diff_view.clone();
                                cx.on_focus_in(&diff_view.read(cx).focus_handle(cx), window, move |this, _, cx| {
                                    this.active_panel = Some(Arc::new(diff_view_clone.clone()));
                                    this.sync_active_editor(cx);
                                    cx.notify();
                                })
                                .detach();

                                let panel = Arc::new(diff_view);
                                workspace_view.add_panel_to_center_dock(panel, window, cx);
                            });
                        })
                        .detach();
                    });
                }
            }
        })
        .detach();
    }

    fn on_action_toggle_left_panel(&mut self, _: &ToggleLeftPanel, _: &mut Window, cx: &mut Context<Self>) {
        self.is_left_panel_visible = !self.is_left_panel_visible;
        self.sync_activity_bar(cx);
        cx.notify();
    }

    fn on_action_show_files_tab(&mut self, _: &ShowFilesTab, window: &mut Window, cx: &mut Context<Self>) {
        self.select_activity(Activity::Files, window, cx);
    }

    fn on_action_show_structure_tab(&mut self, _: &ShowStructureTab, window: &mut Window, cx: &mut Context<Self>) {
        self.select_activity(Activity::Structure, window, cx);
    }

    fn on_action_show_checksum_tab(&mut self, _: &ShowChecksumTab, window: &mut Window, cx: &mut Context<Self>) {
        self.select_activity(Activity::Checksum, window, cx);
    }

    fn select_activity(&mut self, activity: Activity, window: &mut Window, cx: &mut Context<Self>) {
        let tab = match activity {
            Activity::Files => LeftPanelTab::Files,
            Activity::Structure => LeftPanelTab::Structure,
            Activity::Inspector => LeftPanelTab::Inspector,
            Activity::Map => LeftPanelTab::Map,
            Activity::Checksum => LeftPanelTab::Checksum,
        };

        let current_tab = self.left_panel.read(cx).active_tab;

        // If the same tab is already active and the panel is visible, hide it.
        // Otherwise, switch to the tab and ensure it's visible.
        if self.is_left_panel_visible && current_tab == tab {
            self.is_left_panel_visible = false;
        } else {
            self.is_left_panel_visible = true;
            self.left_panel.update(cx, |p, cx| {
                p.set_tab(tab, cx);
            });
            let focus_handle = self.left_panel.read(cx).focus_handle(cx);
            focus_handle.focus(window);
        }

        // Ensure the activity bar reflects the new state immediately
        self.sync_activity_bar(cx);
        cx.notify();
    }

    fn on_action_load_structure_definition(&mut self, _: &LoadStructureDefinition, window: &mut Window, cx: &mut Context<Self>) {
        let path = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select a Kaitai Struct definition (.ksy, .yaml)".into()),
        });

        let view = cx.entity().clone();

        cx.spawn_in(window, async move |_, window| {
            if let Some(path) = path.await.ok().and_then(|r| r.ok()).flatten().and_then(|mut p| p.pop()) {
                match std::fs::read_to_string(&path) {
                    Ok(contents) => match serde_yaml::from_str::<crate::core::structure::KsyDefinition>(&contents) {
                        Ok(ksy) => {
                            window
                                .update(|_window, cx| {
                                    view.update(cx, |this, cx| {
                                        let ksy_arc = Arc::new(ksy);
                                        this.ksy_definition = Some(ksy_arc.clone());

                                        let editors: Vec<_> = this.open_file_manager.read(cx).entries().iter().map(|e| e.editor.clone()).collect();
                                        for editor_entity in editors {
                                            set_kaitai_definition_async(&editor_entity, ksy_arc.clone(), cx);
                                        }

                                        let active_editor = this.active_editor(cx);
                                        this.left_panel.update(cx, |p, cx| {
                                            p.set_editor(active_editor, cx);
                                            p.set_tab(crate::ui::panels::left_panel::LeftPanelTab::Structure, cx);
                                        });
                                        this.is_left_panel_visible = true;
                                        cx.notify();
                                    });
                                })
                                .ok();
                        }
                        Err(e) => {
                            eprintln!("Failed to parse KSY definition: {}", e);
                        }
                    },
                    Err(e) => {
                        eprintln!("Failed to read KSY file at {:?}: {}", path, e);
                    }
                }
            }
        })
        .detach();
    }

    fn on_action_clear_structure_definition(&mut self, _: &ClearStructureDefinition, _: &mut Window, cx: &mut Context<Self>) {
        self.ksy_definition = None;
        let editors: Vec<_> = self.open_file_manager.read(cx).entries().iter().map(|e| e.editor.clone()).collect();
        for editor_entity in editors {
            editor_entity.update(cx, |editor, cx| {
                editor.clear_structure_definition();
                cx.notify();
            });
        }
        let active_editor = self.active_editor(cx);
        self.left_panel.update(cx, |p, cx| {
            p.set_editor(active_editor, cx);
        });
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

    fn on_action_close_active_panel(&mut self, _: &CloseActivePanel, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_panel.take() {
            let is_editor = panel.panel_name(cx) == "EditorPanel";
            let mut closed_path = None;
            if is_editor && let Ok(editor_panel) = panel.view().downcast::<EditorPanel>() {
                let path = editor_panel.read(cx).path(cx);
                closed_path = Some(path);
            }

            self.dock_area.update(cx, |dock_area, cx| {
                dock_area.remove_panel_from_all_docks(panel.clone(), window, cx);
            });

            if let Some(path) = closed_path {
                let open_file_manager = self.open_file_manager.clone();
                let entry_id = open_file_manager.read(cx).find_by_path(&path).map(|e| e.id);
                if let Some(id) = entry_id {
                    open_file_manager.update(cx, |manager, cx| {
                        manager.close(id, cx);
                    });
                }
                let editor_service = AppState::global(cx).editor_service.clone();
                editor_service.close_file(&path);
            }

            if is_editor {
                if let Some(entry) = self.open_file_manager.read(cx).active_entry() {
                    let next_panel = Arc::new(entry.panel.clone());
                    next_panel.read(cx).focus_handle(cx).focus(window);
                    self.active_panel = Some(next_panel.clone());
                    self.add_panel_to_center_dock(next_panel, window, cx);
                } else {
                    self.active_panel = None;
                    let weak_dock_area = self.dock_area.downgrade();
                    Self::reset_default_layout(weak_dock_area, window, cx);
                }
            }

            self.sync_active_editor(cx);
            cx.notify();
        }
    }

    fn on_action_activate_next_tab(&mut self, _: &ActivateNextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.open_file_manager.update(cx, |manager, cx| {
            manager.activate_next(cx);
        });
        if let Some(entry) = self.open_file_manager.read(cx).active_entry() {
            let panel = Arc::new(entry.panel.clone());
            entry.panel.read(cx).focus_handle(cx).focus(window);
            self.active_panel = Some(panel.clone());
            self.add_panel_to_center_dock(panel, window, cx);
            self.sync_active_editor(cx);
        }
    }

    fn on_action_activate_previous_tab(&mut self, _: &ActivatePreviousTab, window: &mut Window, cx: &mut Context<Self>) {
        self.open_file_manager.update(cx, |manager, cx| {
            manager.activate_previous(cx);
        });
        if let Some(entry) = self.open_file_manager.read(cx).active_entry() {
            let panel = Arc::new(entry.panel.clone());
            entry.panel.read(cx).focus_handle(cx).focus(window);
            self.active_panel = Some(panel.clone());
            self.add_panel_to_center_dock(panel, window, cx);
            self.sync_active_editor(cx);
        }
    }

    fn on_action_activate_tab(&mut self, action: &ActivateTab, window: &mut Window, cx: &mut Context<Self>) {
        if action.index > 0 {
            let zero_based = action.index - 1;
            self.open_file_manager.update(cx, |manager, cx| {
                manager.activate_index(zero_based, cx);
            });
            if let Some(entry) = self.open_file_manager.read(cx).active_entry() {
                let panel = Arc::new(entry.panel.clone());
                entry.panel.read(cx).focus_handle(cx).focus(window);
                self.active_panel = Some(panel.clone());
                self.add_panel_to_center_dock(panel, window, cx);
                self.sync_active_editor(cx);
            }
        }
    }

    fn on_action_close_other_tabs(&mut self, _: &CloseOtherTabs, window: &mut Window, cx: &mut Context<Self>) {
        let active_panel = self.active_panel.clone();
        let closed_paths = self.open_file_manager.update(cx, |manager, cx| manager.close_others(cx));
        let editor_service = AppState::global(cx).editor_service.clone();
        for path in closed_paths {
            editor_service.close_file(&path);
        }

        if let Some(panel) = active_panel {
            self.dock_area.update(cx, |dock_area, cx| {
                dock_area.set_center(DockItem::panel(panel), window, cx);
            });
        }
        self.sync_active_editor(cx);
        cx.notify();
    }

    fn on_action_close_all_tabs(&mut self, _: &CloseAllTabs, window: &mut Window, cx: &mut Context<Self>) {
        let closed_paths = self.open_file_manager.update(cx, |manager, cx| manager.close_all(cx));
        let editor_service = AppState::global(cx).editor_service.clone();
        for path in closed_paths {
            editor_service.close_file(&path);
        }
        self.active_panel = None;
        let weak_dock_area = self.dock_area.downgrade();
        Self::reset_default_layout(weak_dock_area, window, cx);
        self.sync_active_editor(cx);
        cx.notify();
    }

    fn on_action_split_right(&mut self, _: &SplitRight, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_panel.clone() {
            self.dock_area.update(cx, |dock_area, cx| {
                dock_area.add_panel(panel, DockPlacement::Right, None, window, cx);
            });
        }
    }

    fn on_action_split_down(&mut self, _: &SplitDown, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.active_panel.clone() {
            self.dock_area.update(cx, |dock_area, cx| {
                dock_area.add_panel(panel, DockPlacement::Bottom, None, window, cx);
            });
        }
    }

    fn on_action_open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings_panel(window, cx);
    }

    fn open_settings_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::ui::panels::settings_panel::SettingsPanel;

        let dock_area = self.dock_area.read(cx);
        let existing_panel = Self::check_has_settings_panel(dock_area.items());

        if let Some(panel) = existing_panel {
            let focus_handle = panel.read(cx).focus_handle(cx);
            focus_handle.focus(window);
            return;
        }

        let settings_panel = cx.new(|cx| SettingsPanel::new(window, cx));
        let settings_panel_clone = settings_panel.clone();
        cx.on_focus_in(&settings_panel.read(cx).focus_handle(cx), window, {
            let settings_panel_clone = settings_panel_clone.clone();
            move |this, _, cx| {
                this.active_panel = Some(Arc::new(settings_panel_clone.clone()));
                this.sync_active_editor(cx);
                cx.notify();
            }
        })
        .detach();
        let panel = Arc::new(settings_panel);
        self.add_panel_to_center_dock(panel, window, cx);
    }

    fn check_has_settings_panel(dock_item: &DockItem) -> Option<Entity<crate::ui::panels::settings_panel::SettingsPanel>> {
        match dock_item {
            DockItem::Tabs { items, .. } => {
                for item in items {
                    if let Ok(panel) = item.view().downcast::<crate::ui::panels::settings_panel::SettingsPanel>() {
                        return Some(panel);
                    }
                }
            }
            DockItem::Split { items, .. } => {
                for item in items {
                    if let Some(panel) = Self::check_has_settings_panel(item) {
                        return Some(panel);
                    }
                }
            }
            _ => {}
        }
        None
    }

    fn on_action_open_visual_map(&mut self, _: &OpenVisualMap, window: &mut Window, cx: &mut Context<Self>) {
        self.select_activity(Activity::Map, window, cx);
    }

    fn add_panel_to_center_dock(&self, panel: Arc<dyn PanelView>, window: &mut Window, cx: &mut Context<Self>) {
        self.dock_area.update(cx, |dock_area, cx| {
            dock_area.set_center(DockItem::panel(panel), window, cx);
        });
    }

    /// Opens a new workspace window with the specified files and folder.
    /// This is the main public API for creating workspace windows.
    pub fn open_window(cx: &mut App, initial_files: Vec<PathBuf>, initial_folder: Option<PathBuf>) -> Task<()> {
        let task = Self::new_local(cx);
        cx.spawn(async move |cx| {
            if let Ok(window) = task.await {
                // Open all initial files if provided
                if !initial_files.is_empty() {
                    for file_path in initial_files {
                        let _ = window.update(cx, |_, _window, cx| {
                            cx.dispatch_action(&crate::actions::OpenFile {
                                path: file_path.to_string_lossy().to_string(),
                            });
                        });
                    }
                }

                // Set initial folder if provided
                if let Some(folder_path) = initial_folder {
                    let _ = window.update(cx, |_root, _window, cx| {
                        cx.dispatch_action(&SetFileTreeFolder {
                            path: folder_path.to_string_lossy().to_string(),
                        });
                    });
                }
            }
        })
    }

    fn check_has_panels(&self, cx: &App) -> bool {
        self.active_panel.is_some() || !self.open_file_manager.read(cx).entries().is_empty()
    }

    fn on_focus_changed(&self, cx: &mut Context<Self>) {
        self.left_panel.update(cx, |panel, cx| {
            panel.file_tree.update(cx, |_, cx| cx.notify());
            panel.struct_tree.update(cx, |_, cx| cx.notify());
            panel.data_inspector.update(cx, |_, cx| cx.notify());
            panel.visual_map.update(cx, |_, cx| cx.notify());
            panel.checksum_panel.update(cx, |_, cx| cx.notify());
        });

        // Clone the item to release the immutable borrow on cx
        let item = self.dock_area.read(cx).items().clone();
        Self::notify_panels_recursive(&item, cx);
    }

    fn notify_panels_recursive(item: &gpui_component::dock::DockItem, cx: &mut Context<Self>) {
        match item {
            gpui_component::dock::DockItem::Tabs { items, .. } => {
                for panel in items {
                    if let Ok(p) = panel.view().downcast::<EditorPanel>() {
                        p.update(cx, |_, cx| cx.notify());
                    } else if let Ok(p) = panel.view().downcast::<crate::ui::panels::diff_panel::DiffPanel>() {
                        p.update(cx, |_, cx| cx.notify());
                    } else if let Ok(p) = panel.view().downcast::<crate::ui::panels::settings_panel::SettingsPanel>() {
                        p.update(cx, |_, cx| cx.notify());
                    }
                }
            }
            gpui_component::dock::DockItem::Split { items, .. } => {
                for sub_item in items {
                    Self::notify_panels_recursive(sub_item, cx);
                }
            }
            gpui_component::dock::DockItem::Panel { view, .. } => {
                if let Ok(p) = view.view().downcast::<EditorPanel>() {
                    p.update(cx, |_, cx| cx.notify());
                } else if let Ok(p) = view.view().downcast::<crate::ui::panels::diff_panel::DiffPanel>() {
                    p.update(cx, |_, cx| cx.notify());
                } else if let Ok(p) = view.view().downcast::<crate::ui::panels::settings_panel::SettingsPanel>() {
                    p.update(cx, |_, cx| cx.notify());
                }
            }
            _ => {}
        }
    }

    fn sync_activity_bar(&self, cx: &mut Context<Self>) {
        let is_visible = self.is_left_panel_visible;
        let active_tab = self.left_panel.read(cx).active_tab;
        self.activity_bar.update(cx, |activity_bar, cx| {
            if is_visible {
                match active_tab {
                    LeftPanelTab::Files => activity_bar.set_activity(Some(Activity::Files), cx),
                    LeftPanelTab::Structure => activity_bar.set_activity(Some(Activity::Structure), cx),
                    LeftPanelTab::Inspector => activity_bar.set_activity(Some(Activity::Inspector), cx),
                    LeftPanelTab::Map => activity_bar.set_activity(Some(Activity::Map), cx),
                    LeftPanelTab::Checksum => activity_bar.set_activity(Some(Activity::Checksum), cx),
                }
            } else {
                activity_bar.set_activity(None, cx);
            }
        });
    }

    fn tab_items(&self, cx: &App) -> Vec<crate::ui::components::tab_bar::TabItemInfo> {
        let manager = self.open_file_manager.read(cx);
        let active_id = manager.active_entry().map(|e| e.id);
        manager
            .entries()
            .iter()
            .map(|entry| {
                let doc = entry.document.read().expect("document read lock");
                let title = entry
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Untitled".to_string());
                let is_dirty = doc.is_dirty();
                let is_active = Some(entry.id) == active_id;
                crate::ui::components::tab_bar::TabItemInfo {
                    id: entry.id.0,
                    title,
                    is_dirty,
                    is_active,
                    path: Some(entry.path.clone()),
                }
            })
            .collect()
    }

    #[allow(dead_code)]
    fn get_tab_items(&self, cx: &App) -> Vec<crate::ui::components::tab_bar::TabItemInfo> {
        self.tab_items(cx)
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab_items = self.tab_items(cx);
        let has_tabs = !tab_items.is_empty();

        div()
            .id("workspace")
            .on_action(cx.listener(Self::on_action_open_file))
            .on_action(cx.listener(Self::on_action_close_active_panel))
            .on_action(cx.listener(Self::on_action_open_file_dialog))
            .on_action(cx.listener(Self::on_action_quit))
            .on_action(cx.listener(Self::on_action_select_all))
            .on_action(cx.listener(Self::on_action_go_to_beginning))
            .on_action(cx.listener(Self::on_action_go_to_end))
            .on_action(cx.listener(Self::on_action_set_encoding_ascii))
            .on_action(cx.listener(Self::on_action_set_encoding_utf8))
            .on_action(cx.listener(Self::on_action_set_encoding_utf16le))
            .on_action(cx.listener(Self::on_action_set_encoding_utf16be))
            .on_action(cx.listener(Self::on_action_open_diff))
            .on_action(cx.listener(Self::on_action_toggle_left_panel))
            .on_action(cx.listener(Self::on_action_open_settings))
            .on_action(cx.listener(Self::on_action_open_visual_map))
            .on_action(cx.listener(Self::on_action_show_files_tab))
            .on_action(cx.listener(Self::on_action_show_structure_tab))
            .on_action(cx.listener(Self::on_action_show_checksum_tab))
            .on_action(cx.listener(Self::on_action_load_structure_definition))
            .on_action(cx.listener(Self::on_action_clear_structure_definition))
            .on_action(cx.listener(Self::on_action_toggle_inline_structure_view))
            .on_action(cx.listener(Self::on_action_open_folder))
            .on_action(cx.listener(Self::on_action_close_folder))
            .on_action(cx.listener(Self::on_action_activate_next_tab))
            .on_action(cx.listener(Self::on_action_activate_previous_tab))
            .on_action(cx.listener(Self::on_action_activate_tab))
            .on_action(cx.listener(Self::on_action_close_other_tabs))
            .on_action(cx.listener(Self::on_action_close_all_tabs))
            .on_action(cx.listener(Self::on_action_split_right))
            .on_action(cx.listener(Self::on_action_split_down))
            .on_drop(cx.listener(move |this, external_paths: &gpui::ExternalPaths, window, cx| {
                for path in external_paths.paths() {
                    if path.is_file() {
                        let action = crate::actions::OpenFile {
                            path: path.to_string_lossy().to_string(),
                        };
                        this.on_action_open_file(&action, window, cx);
                    } else if path.is_dir() {
                        this.is_left_panel_visible = true;
                        this.left_panel.update(cx, |p, cx| {
                            p.set_tab(crate::ui::panels::left_panel::LeftPanelTab::Files, cx);
                            p.file_tree.update(cx, |ft, cx| {
                                ft.set_root_path(path.clone(), cx);
                            });
                        });
                        this.sync_activity_bar(cx);
                    }
                }
                cx.notify();
            }))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .child(self.title_bar.clone())
            .child(
                div().flex().flex_row().flex_1().child(self.activity_bar.clone()).child(
                    h_resizable("workspace-h-resize")
                        .child(
                            resizable_panel()
                                .visible(self.is_left_panel_visible)
                                .size(px(250.))
                                .child(self.left_panel.clone()),
                        )
                        .child(
                            resizable_panel().child(
                                div()
                                    .relative()
                                    .size_full()
                                    .flex()
                                    .flex_col()
                                    .when(has_tabs, |el| {
                                        el.child(crate::ui::components::tab_bar::render_zed_tab_bar(&tab_items, window, cx))
                                    })
                                    .child(self.dock_area.clone())
                                    .when(!self.check_has_panels(cx), |this| {
                                        this.child(
                                            div()
                                                .absolute()
                                                .top_0()
                                                .left_0()
                                                .size_full()
                                                .flex()
                                                .justify_center()
                                                .items_center()
                                                .bg(cx.theme().background)
                                                .child(div().text_xl().text_color(cx.theme().muted_foreground).child("Nothing is open")),
                                        )
                                    }),
                            ),
                        ),
                ),
            )
            .child(self.status_bar.clone())
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}

pub fn set_kaitai_definition_async(editor_entity: &Entity<Editor>, ksy: Arc<crate::core::structure::KsyDefinition>, cx: &mut App) {
    let cancel_token = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancel_token_clone = cancel_token.clone();

    let (doc_arc, generation) = editor_entity.update(cx, |editor, cx| {
        editor.cancel_structure_parsing();
        editor.parse_cancel_token = Some(cancel_token.clone());
        editor.ksy_definition = Some(ksy.clone());
        editor.is_parsing_structure = true;
        editor.parse_progress_offset = 0;
        let total = editor.document.read().expect("document read lock").buffer.len();
        editor.parse_total_size = total;
        editor.parse_generation += 1;
        editor.parse_result = None;
        editor.invalidate_line_map();
        cx.notify();
        (editor.document.clone(), editor.parse_generation)
    });

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<crate::core::structure::types::ParseProgress>();

    let editor_entity = editor_entity.clone();

    // Dedicated background OS thread for parsing: guarantees UI thread never freezes
    let ksy_clone = (*ksy).clone();
    std::thread::Builder::new()
        .name("kaitai-parser".into())
        .spawn(move || {
            // Read document buffer in background thread (no UI thread copy)
            let bytes = {
                if let Ok(doc) = doc_arc.read() {
                    doc.buffer.data().to_vec()
                } else {
                    Vec::new()
                }
            };

            let mut stream = crate::core::structure::KaitaiStream::new(&bytes);
            let interpreter = crate::core::structure::KaitaiInterpreter::new(ksy_clone);
            interpreter.parse_with_progress_cancellable(&mut stream, Some(&cancel_token_clone), |progress| {
                if !cancel_token_clone.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = tx.send(progress.clone());
                }
            });
        })
        .expect("Failed to spawn kaitai parser thread");

    // UI task to consume progress updates
    cx.spawn(async move |cx| {
        while let Some(mut progress) = rx.recv().await {
            while let Ok(newer) = rx.try_recv() {
                progress = newer;
            }
            let is_done = progress.is_done;
            let parse_res = progress.parse_result;

            let should_continue = editor_entity.update(cx, |editor, cx| {
                if editor.parse_generation != generation {
                    return false;
                }
                if !editor.is_parsing_structure && !is_done {
                    // Canceled via stop button; exit UI task immediately
                    return false;
                }
                editor.parse_progress_offset = progress.parsed_offset;
                editor.parse_total_size = progress.total_bytes;
                if let Some(res) = parse_res {
                    editor.set_parse_result_arc(res);
                }
                if is_done {
                    editor.is_parsing_structure = false;
                    editor.parse_cancel_token = None;
                }
                cx.notify();
                !is_done
            });

            if should_continue.is_err() || !should_continue.unwrap_or(false) {
                break;
            }
        }
    })
    .detach();
}
