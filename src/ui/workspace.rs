use gpui::prelude::*;
use gpui::*;

use crate::actions::*;

use crate::ui::components::activity_bar::{Activity, ActivityBar, ActivityBarEvent};
use crate::ui::components::file_tree_view::{FileTreeView, FileTreeViewEvent};
use crate::ui::components::title_bar::AppTitleBar;
use crate::ui::pane::{PaneTree, PaneTreeEvent, SplitDirection, TabContent};
use crate::ui::panels::editor_panel::EditorPanel;
use crate::ui::panels::left_panel::{LeftPanel, LeftPanelTab};

use crate::app_state::AppState;
use crate::core::editor::Editor;
use crate::ui::components::status_bar::StatusBar;
use gpui_component::Root;
use gpui_component::menu::AppMenuBar;
use gpui_component::resizable::{h_resizable, resizable_panel};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub struct Workspace {
    pub pane_tree: Entity<PaneTree>,
    pub title_bar: Entity<AppTitleBar>,
    pub status_bar: Entity<StatusBar>,
    pub left_panel: Entity<LeftPanel>,
    pub activity_bar: Entity<ActivityBar>,
    pub ksy_definition: Option<Arc<crate::core::structure::KsyDefinition>>,
    pub is_left_panel_visible: bool,
}

pub fn init(cx: &mut App) {
    cx.bind_keys(vec![
        KeyBinding::new("shift-escape", gpui_component::dock::ToggleZoom, None),
        KeyBinding::new("ctrl-w", crate::actions::CloseActivePanel, None),
        KeyBinding::new("cmd-w", crate::actions::CloseActivePanel, None),
    ]);

    cx.activate(true);
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pane_tree = cx.new(|_| PaneTree::new());

        cx.subscribe_in(&pane_tree, window, |this, _, event: &PaneTreeEvent, _window, cx| match event {
            PaneTreeEvent::ActiveEditorChanged => {
                this.sync_active_editor(cx);
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

        let file_tree = cx.new(|cx| FileTreeView::new("FILES", cx));
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
        cx.subscribe(&status_bar, |this, _, event, cx| match event {
            crate::ui::components::status_bar::StatusBarEvent::ToggleLeftPanel => {
                this.is_left_panel_visible = !this.is_left_panel_visible;
                cx.notify();
            }
        })
        .detach();

        let (handles, highlight_panel) = {
            let left_read = left_panel.read(cx);
            (
                [
                    file_tree.read(cx).focus_handle(cx),
                    left_read.struct_tree.read(cx).focus_handle(cx),
                    left_read.data_inspector.read(cx).focus_handle(cx),
                    left_read.visual_map.read(cx).focus_handle(cx),
                    left_read.checksum_panel.read(cx).focus_handle(cx),
                    left_read.highlight_panel.read(cx).focus_handle(cx),
                ],
                left_read.highlight_panel.clone(),
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
            &highlight_panel,
            window,
            |this, _, event: &crate::ui::components::highlight_panel::HighlightPanelEvent, window, cx| match event {
                crate::ui::components::highlight_panel::HighlightPanelEvent::Export => {
                    this.on_action_export_highlights(&crate::actions::ExportHighlights, window, cx);
                }
                crate::ui::components::highlight_panel::HighlightPanelEvent::Import => {
                    this.on_action_import_highlights(&crate::actions::ImportHighlights, window, cx);
                }
                crate::ui::components::highlight_panel::HighlightPanelEvent::NavigateTo { .. } => {}
            },
        )
        .detach();

        cx.subscribe(&left_panel, |_, _, event, cx| match event {
            FileTreeViewEvent::OpenFile(path) => {
                cx.dispatch_action(&crate::actions::OpenFile {
                    path: path.to_string_lossy().to_string(),
                });
            }
        })
        .detach();

        Self {
            pane_tree,
            title_bar,
            status_bar,
            left_panel,
            activity_bar,
            ksy_definition: None,
            is_left_panel_visible: true,
        }
    }

    pub fn active_editor(&self, cx: &App) -> Option<Entity<Editor>> {
        self.pane_tree.read(cx).active_editor(cx)
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
        let editor = cx.new(|_| Editor::new(document));

        if let Some(ksy) = &self.ksy_definition {
            set_kaitai_definition_async(&editor, ksy.clone(), cx);
        }

        let editor_panel = cx.new(|cx| EditorPanel::new(editor, window, cx));
        let content = TabContent::Editor(editor_panel);

        self.pane_tree.update(cx, |tree, cx| {
            tree.open_tab(content, window, cx);
        });

        self.sync_active_editor(cx);
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

    fn on_action_set_radix_hex(&mut self, _: &SetRadixHex, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.set_radix(crate::core::radix::DisplayRadix::Hexadecimal);
            });
        }
    }

    fn on_action_set_radix_dec(&mut self, _: &SetRadixDec, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.set_radix(crate::core::radix::DisplayRadix::Decimal);
            });
        }
    }

    fn on_action_set_radix_oct(&mut self, _: &SetRadixOct, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.set_radix(crate::core::radix::DisplayRadix::Octal);
            });
        }
    }

    fn on_action_set_radix_bin(&mut self, _: &SetRadixBin, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.set_radix(crate::core::radix::DisplayRadix::Binary);
            });
        }
    }

    fn on_action_set_group_size_1(&mut self, _: &SetGroupSize1, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.set_group_size(crate::core::radix::ByteGroupSize::One);
            });
        }
    }

    fn on_action_set_group_size_2(&mut self, _: &SetGroupSize2, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.set_group_size(crate::core::radix::ByteGroupSize::Two);
            });
        }
    }

    fn on_action_set_group_size_4(&mut self, _: &SetGroupSize4, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.set_group_size(crate::core::radix::ByteGroupSize::Four);
            });
        }
    }

    fn on_action_set_group_size_8(&mut self, _: &SetGroupSize8, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.set_group_size(crate::core::radix::ByteGroupSize::Eight);
            });
        }
    }

    fn on_action_set_byte_order_le(&mut self, _: &SetByteOrderLittleEndian, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.set_is_big_endian(false);
            });
        }
    }

    fn on_action_set_byte_order_be(&mut self, _: &SetByteOrderBigEndian, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.set_is_big_endian(true);
            });
        }
    }

    fn on_action_toggle_byte_order(&mut self, _: &ToggleByteOrder, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.active_editor(cx) {
            editor.update(cx, |editor, _cx| {
                editor.toggle_byte_order();
            });
        }
    }

    fn on_action_open_file(&mut self, action: &OpenFile, window: &mut Window, cx: &mut Context<Self>) {
        let file_path = action.path.clone();
        let path = std::path::PathBuf::from(&file_path);

        // Check if path is already open in any group
        for group in self.pane_tree.read(cx).all_groups() {
            let tabs = group.read(cx).tabs.iter().enumerate().map(|(i, t)| (i, t.path(cx))).collect::<Vec<_>>();
            for (idx, tab_path) in tabs {
                if tab_path.as_ref() == Some(&path) {
                    group.update(cx, |g, cx| {
                        g.activate_tab(idx, window, cx);
                    });
                    self.pane_tree.update(cx, |tree, cx| {
                        tree.set_active_group(group.read(cx).id, cx);
                    });
                    self.sync_active_editor(cx);
                    cx.notify();
                    return;
                }
            }
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

                                let content = TabContent::Diff(diff_view);
                                workspace_view.pane_tree.update(cx, |tree, cx| {
                                    tree.open_tab(content, window, cx);
                                });
                                workspace_view.sync_active_editor(cx);
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

    fn on_action_show_highlights_tab(&mut self, _: &ShowHighlightsTab, window: &mut Window, cx: &mut Context<Self>) {
        self.select_activity(Activity::Highlights, window, cx);
    }

    fn on_action_export_highlights(&mut self, _: &ExportHighlights, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx) else { return };
        let doc_path = editor.read(cx).document.read().ok().map(|d| d.path().to_path_buf());
        let prompt_path = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some("Select destination JSON file or directory for highlights".into()),
        });

        let view = cx.entity().clone();
        cx.spawn_in(window, async move |_, window| {
            if let Some(mut path) = prompt_path.await.ok().and_then(|r| r.ok()).flatten().and_then(|mut v| v.pop()) {
                if path.is_dir() {
                    let default_name = doc_path
                        .and_then(|p| p.file_name().map(|n| format!("{}.highlights.json", n.to_string_lossy())))
                        .unwrap_or_else(|| "highlights.json".to_string());
                    path = path.join(default_name);
                } else if path.extension().is_none() {
                    path.set_extension("json");
                }

                window
                    .update(|_, cx| {
                        view.update(cx, |this, cx| {
                            if let Some(editor) = this.active_editor(cx)
                                && let Err(e) = editor.read(cx).export_highlights_to_file(&path)
                            {
                                eprintln!("Failed to export highlights: {}", e);
                            }
                        });
                    })
                    .ok();
            }
        })
        .detach();
    }

    fn on_action_import_highlights(&mut self, _: &ImportHighlights, window: &mut Window, cx: &mut Context<Self>) {
        let prompt_path = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select highlights JSON file to import".into()),
        });

        let view = cx.entity().clone();
        cx.spawn_in(window, async move |_, window| {
            if let Some(path) = prompt_path.await.ok().and_then(|r| r.ok()).flatten().and_then(|mut v| v.pop()) {
                window
                    .update(|_, cx| {
                        view.update(cx, |this, cx| {
                            if let Some(editor) = this.active_editor(cx) {
                                editor.update(cx, |ed, cx| match ed.import_highlights_from_file(&path) {
                                    Ok(_) => {
                                        cx.notify();
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to import highlights: {}", e);
                                    }
                                });
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
            Activity::Structure => LeftPanelTab::Structure,
            Activity::Inspector => LeftPanelTab::Inspector,
            Activity::Map => LeftPanelTab::Map,
            Activity::Checksum => LeftPanelTab::Checksum,
            Activity::Highlights => LeftPanelTab::Highlights,
        };

        let current_tab = self.left_panel.read(cx).active_tab;

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

                                        let mut editors = Vec::new();
                                        for group in this.pane_tree.read(cx).all_groups() {
                                            for tab in &group.read(cx).tabs {
                                                if let Some(ed) = tab.content.editor(cx) {
                                                    editors.push(ed);
                                                }
                                            }
                                        }

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
        let mut editors = Vec::new();
        for group in self.pane_tree.read(cx).all_groups() {
            for tab in &group.read(cx).tabs {
                if let Some(ed) = tab.content.editor(cx) {
                    editors.push(ed);
                }
            }
        }
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
        self.pane_tree.update(cx, |tree, cx| {
            tree.close_active_tab(window, cx);
        });
        self.sync_active_editor(cx);
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
        self.sync_active_editor(cx);
        cx.notify();
    }

    fn on_action_close_all_tabs(&mut self, _: &CloseAllTabs, _window: &mut Window, cx: &mut Context<Self>) {
        self.pane_tree = cx.new(|_| PaneTree::new());
        self.sync_active_editor(cx);
        cx.notify();
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
                    self.sync_active_editor(cx);
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
        self.sync_active_editor(cx);
        cx.notify();
    }

    fn on_action_open_visual_map(&mut self, _: &OpenVisualMap, window: &mut Window, cx: &mut Context<Self>) {
        self.select_activity(Activity::Map, window, cx);
    }

    /// Opens a new workspace window with the specified files and folder.
    /// This is the main public API for creating workspace windows.
    pub fn open_window(cx: &mut App, initial_files: Vec<PathBuf>, initial_folder: Option<PathBuf>) -> Task<()> {
        let task = Self::new_local(cx);
        cx.spawn(async move |cx| {
            if let Ok(window) = task.await {
                if !initial_files.is_empty() {
                    for file_path in initial_files {
                        let _ = window.update(cx, |_, _window, cx| {
                            cx.dispatch_action(&crate::actions::OpenFile {
                                path: file_path.to_string_lossy().to_string(),
                            });
                        });
                    }
                }

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

    fn on_focus_changed(&self, cx: &mut Context<Self>) {
        self.left_panel.update(cx, |panel, cx| {
            panel.file_tree.update(cx, |_, cx| cx.notify());
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
                    LeftPanelTab::Structure => activity_bar.set_activity(Some(Activity::Structure), cx),
                    LeftPanelTab::Inspector => activity_bar.set_activity(Some(Activity::Inspector), cx),
                    LeftPanelTab::Map => activity_bar.set_activity(Some(Activity::Map), cx),
                    LeftPanelTab::Checksum => activity_bar.set_activity(Some(Activity::Checksum), cx),
                    LeftPanelTab::Highlights => activity_bar.set_activity(Some(Activity::Highlights), cx),
                }
            } else {
                activity_bar.set_activity(None, cx);
            }
        });
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .on_action(cx.listener(Self::on_action_toggle_left_panel))
            .on_action(cx.listener(Self::on_action_open_settings))
            .on_action(cx.listener(Self::on_action_open_visual_map))
            .on_action(cx.listener(Self::on_action_show_files_tab))
            .on_action(cx.listener(Self::on_action_show_structure_tab))
            .on_action(cx.listener(Self::on_action_show_checksum_tab))
            .on_action(cx.listener(Self::on_action_show_highlights_tab))
            .on_action(cx.listener(Self::on_action_export_highlights))
            .on_action(cx.listener(Self::on_action_import_highlights))
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
                        .child(resizable_panel().child(div().relative().size_full().child(self.pane_tree.clone()))),
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
    let ksy_clone = (*ksy).clone();

    std::thread::Builder::new()
        .name("kaitai-parser".into())
        .spawn(move || {
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
