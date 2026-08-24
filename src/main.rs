#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![recursion_limit = "2048"]

use gpui::{App, Application};
use std::path::PathBuf;

mod actions;
mod app_state;
mod assets;
mod core;
mod service;
mod settings;
mod theme;
mod ui;

use crate::assets::Assets;
use ui::workspace::Workspace;

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("initialize tokio runtime");
    let _guard = rt.enter();

    let (files_to_open, folder_to_open) = parse_cli_args();

    let app = Application::new().with_assets(Assets);

    app.run(move |cx| {
        init_app_state(cx);
        setup_menus(cx);
        setup_keybindings(cx);

        Workspace::open_window(cx, files_to_open, folder_to_open).detach();
    });
}

/// Parses command line arguments to determine files and folder to open at launch.
fn parse_cli_args() -> (Vec<PathBuf>, Option<PathBuf>) {
    let mut files_to_open = Vec::new();
    let mut folder_to_open = None;

    for arg in std::env::args_os().skip(1) {
        let path = PathBuf::from(arg);
        if path.is_file() {
            files_to_open.push(path);
        } else if path.is_dir() {
            // Use the last directory as the folder to open
            folder_to_open = Some(path);
        } else {
            eprintln!("Warning: Path does not exist: {}", path.display());
        }
    }

    (files_to_open, folder_to_open)
}

/// Initializes core application state, themes, and UI components.
fn init_app_state(cx: &mut App) {
    let settings = settings::Settings::load();

    app_state::AppState::init(cx);
    cx.set_global(settings.appearance.clone());
    cx.set_global(settings.default_encoding);
    cx.set_global(settings::RecentHistoryState::from_settings(&settings));

    gpui_component::init(cx);
    theme::init(cx);
    theme::set_mode(settings.theme_mode, None, cx);
    settings::register_quit_handler(cx);
    ui::workspace::init(cx);
    ui::components::new_file_modal::init(cx);
    ui::components::file_tree_view::init(cx);
    ui::components::goto_offset_bar::init(cx);
    ui::components::search_bar::init(cx);
    ui::components::search_panel::init(cx);
    ui::components::strings_panel::init(cx);
    ui::components::struct_tree_view::init(cx);
    ui::components::bookmark_panel::init(cx);
    ui::panels::editor_panel::init(cx);
    ui::panels::diff_panel::init(cx);
}

/// Registers the application top menu bar items.
fn setup_menus(cx: &mut App) {
    cx.set_menus(vec![
        gpui::Menu {
            name: "File".into(),
            items: vec![
                gpui::MenuItem::action("New File...", crate::actions::NewFile),
                gpui::MenuItem::action("Open File...", crate::actions::OpenFileDialog),
                gpui::MenuItem::action("Open Folder...", crate::actions::OpenFolder),
                gpui::MenuItem::action("Close Folder", crate::actions::CloseFolder),
                gpui::MenuItem::separator(),
                gpui::MenuItem::action("Save", crate::actions::Save),
                gpui::MenuItem::action("Save As...", crate::actions::SaveAs),
                gpui::MenuItem::action("Toggle Read-only", crate::actions::ToggleReadOnly),
                gpui::MenuItem::separator(),
                gpui::MenuItem::action("Close Tab", crate::actions::CloseActivePanel),
                gpui::MenuItem::action("Close Other Tabs", crate::actions::CloseOtherTabs),
                gpui::MenuItem::action("Close Tabs to Right", crate::actions::CloseTabsToRight),
                gpui::MenuItem::action("Close Saved Tabs", crate::actions::CloseSavedTabs),
                gpui::MenuItem::action("Close All Tabs", crate::actions::CloseAllTabs),
                gpui::MenuItem::separator(),
                gpui::MenuItem::action("Copy Path", crate::actions::CopyPath),
                gpui::MenuItem::action("Copy File Name", crate::actions::CopyFileName),
                gpui::MenuItem::action("Reveal in File Manager", crate::actions::RevealInExplorer),
                gpui::MenuItem::separator(),
                gpui::MenuItem::action("Settings", crate::actions::OpenSettings),
                gpui::MenuItem::separator(),
                gpui::MenuItem::action("Quit", crate::actions::Quit),
            ],
        },
        gpui::Menu {
            name: "Edit".into(),
            items: vec![
                gpui::MenuItem::action("Undo", crate::actions::Undo),
                gpui::MenuItem::action("Redo", crate::actions::Redo),
                gpui::MenuItem::separator(),
                gpui::MenuItem::action("Cut", crate::actions::Cut),
                gpui::MenuItem::action("Copy", crate::actions::Copy),
                gpui::MenuItem::action("Paste", crate::actions::Paste),
                gpui::MenuItem::action("Toggle Insert Mode", crate::actions::ToggleInsertMode),
                gpui::MenuItem::action("Toggle Read-only", crate::actions::ToggleReadOnly),
                gpui::MenuItem::separator(),
                gpui::MenuItem::submenu(gpui::Menu {
                    name: "Copy As".into(),
                    items: vec![
                        gpui::MenuItem::action("as Hex Dump", crate::actions::CopyAsHexDump),
                        gpui::MenuItem::action("as Hex with Spaces", crate::actions::CopyAsHexSpaces),
                        gpui::MenuItem::action("as Hex Stream", crate::actions::CopyAsHexStream),
                        gpui::MenuItem::action("as Printable Text", crate::actions::CopyAsPrintableText),
                        gpui::MenuItem::action("as Escaped String", crate::actions::CopyAsEscapedString),
                        gpui::MenuItem::action("as Base64", crate::actions::CopyAsBase64),
                        gpui::MenuItem::action("as Binary", crate::actions::CopyAsBinary),
                        gpui::MenuItem::action("as C++ Array", crate::actions::CopyAsCppArray),
                        gpui::MenuItem::action("as Rust Array", crate::actions::CopyAsRustArray),
                        gpui::MenuItem::action("as JSON Array", crate::actions::CopyAsJsonArray),
                    ],
                }),
                gpui::MenuItem::action("Select All", crate::actions::SelectAll),
                gpui::MenuItem::separator(),
                gpui::MenuItem::action("Find", crate::actions::ToggleSearch),
                gpui::MenuItem::action("Find in File (Scan All)", crate::actions::ToggleSearchPanel),
                gpui::MenuItem::action("Find Next", crate::actions::SearchNext),
                gpui::MenuItem::action("Find Previous", crate::actions::SearchPrev),
                gpui::MenuItem::separator(),
                gpui::MenuItem::submenu(gpui::Menu {
                    name: "Bookmark".into(),
                    items: vec![
                        gpui::MenuItem::action("Red", crate::actions::BookmarkRed),
                        gpui::MenuItem::action("Orange", crate::actions::BookmarkOrange),
                        gpui::MenuItem::action("Yellow", crate::actions::BookmarkYellow),
                        gpui::MenuItem::action("Green", crate::actions::BookmarkGreen),
                        gpui::MenuItem::action("Cyan", crate::actions::BookmarkCyan),
                        gpui::MenuItem::action("Blue", crate::actions::BookmarkBlue),
                        gpui::MenuItem::action("Purple", crate::actions::BookmarkPurple),
                        gpui::MenuItem::action("Pink", crate::actions::BookmarkPink),
                        gpui::MenuItem::separator(),
                        gpui::MenuItem::action("Clear Bookmark", crate::actions::ClearBookmark),
                        gpui::MenuItem::action("Clear All Bookmarks", crate::actions::ClearAllBookmarks),
                        gpui::MenuItem::separator(),
                        gpui::MenuItem::action("Import Bookmarks...", crate::actions::ImportBookmarks),
                        gpui::MenuItem::action("Export Bookmarks...", crate::actions::ExportBookmarks),
                    ],
                }),
            ],
        },
        gpui::Menu {
            name: "View".into(),
            items: vec![
                gpui::MenuItem::action("Toggle Left Panel", crate::actions::ToggleLeftPanel),
                gpui::MenuItem::submenu(gpui::Menu {
                    name: "Panels".into(),
                    items: vec![
                        gpui::MenuItem::action("Files", crate::actions::ShowFilesTab),
                        gpui::MenuItem::action("Strings", crate::actions::ShowStringsTab),
                        gpui::MenuItem::action("Structure", crate::actions::ShowStructureTab),
                        gpui::MenuItem::action("Bookmarks", crate::actions::ShowBookmarksTab),
                        gpui::MenuItem::action("Checksum", crate::actions::ShowChecksumTab),
                        gpui::MenuItem::action("2D Visual Map", crate::actions::OpenVisualMap),
                    ],
                }),
                gpui::MenuItem::separator(),
                gpui::MenuItem::submenu(gpui::Menu {
                    name: "Radix".into(),
                    items: vec![
                        gpui::MenuItem::action("Hexadecimal (16)", crate::actions::SetRadixHex),
                        gpui::MenuItem::action("Decimal (10)", crate::actions::SetRadixDec),
                        gpui::MenuItem::action("Octal (8)", crate::actions::SetRadixOct),
                        gpui::MenuItem::action("Binary (2)", crate::actions::SetRadixBin),
                    ],
                }),
                gpui::MenuItem::submenu(gpui::Menu {
                    name: "Grouping".into(),
                    items: vec![
                        gpui::MenuItem::action("1 Byte (8-bit)", crate::actions::SetGroupSize1),
                        gpui::MenuItem::action("2 Bytes (16-bit)", crate::actions::SetGroupSize2),
                        gpui::MenuItem::action("4 Bytes (32-bit)", crate::actions::SetGroupSize4),
                        gpui::MenuItem::action("8 Bytes (64-bit)", crate::actions::SetGroupSize8),
                    ],
                }),
                gpui::MenuItem::submenu(gpui::Menu {
                    name: "Byte Order".into(),
                    items: vec![
                        gpui::MenuItem::action("Little Endian", crate::actions::SetByteOrderLittleEndian),
                        gpui::MenuItem::action("Big Endian", crate::actions::SetByteOrderBigEndian),
                        gpui::MenuItem::separator(),
                        gpui::MenuItem::action("Toggle Byte Order", crate::actions::ToggleByteOrder),
                    ],
                }),
                gpui::MenuItem::submenu(gpui::Menu {
                    name: "Encoding".into(),
                    items: crate::core::encoding::Encoding::categories()
                        .iter()
                        .map(|(category, encodings)| {
                            gpui::MenuItem::submenu(gpui::Menu {
                                name: category.label().into(),
                                items: encodings
                                    .iter()
                                    .copied()
                                    .map(|encoding| gpui::MenuItem::action(encoding.label(), crate::actions::SetEncoding { encoding }))
                                    .collect(),
                            })
                        })
                        .collect(),
                }),
                gpui::MenuItem::separator(),
                gpui::MenuItem::submenu(gpui::Menu {
                    name: "Custom Line Breaks".into(),
                    items: vec![
                        gpui::MenuItem::action("Break Line", crate::actions::AddCustomBreak),
                        gpui::MenuItem::action("Join Lines", crate::actions::JoinLine),
                        gpui::MenuItem::separator(),
                        gpui::MenuItem::action("Remove Break Backward", crate::actions::RemoveCustomBreakBackward),
                        gpui::MenuItem::action("Remove Break Forward", crate::actions::RemoveCustomBreakForward),
                        gpui::MenuItem::separator(),
                        gpui::MenuItem::action("Reset Custom Breaks", crate::actions::ClearAllCustomBreaks),
                    ],
                }),
            ],
        },
        gpui::Menu {
            name: "Go".into(),
            items: vec![
                gpui::MenuItem::action("Go to Address...", crate::actions::ToggleGoToAddress),
                gpui::MenuItem::separator(),
                gpui::MenuItem::action("Go to Beginning", crate::actions::GoToBeginning),
                gpui::MenuItem::action("Go to End", crate::actions::GoToEnd),
                gpui::MenuItem::separator(),
                gpui::MenuItem::action("Next Difference", crate::actions::NextDifference),
                gpui::MenuItem::action("Previous Difference", crate::actions::PrevDifference),
            ],
        },
        gpui::Menu {
            name: "Analysis".into(),
            items: vec![
                gpui::MenuItem::submenu(gpui::Menu {
                    name: "Structure (Kaitai Struct)".into(),
                    items: vec![
                        gpui::MenuItem::action("Load Definition...", crate::actions::LoadStructureDefinition),
                        gpui::MenuItem::action("Clear Definition", crate::actions::ClearStructureDefinition),
                        gpui::MenuItem::separator(),
                        gpui::MenuItem::action("Toggle Inline Structure View", crate::actions::ToggleInlineStructureView),
                        gpui::MenuItem::separator(),
                        gpui::MenuItem::action("Expand All", crate::actions::ExpandAllStructure),
                        gpui::MenuItem::action("Collapse All", crate::actions::CollapseAllStructure),
                    ],
                }),
                gpui::MenuItem::separator(),
                gpui::MenuItem::action("2D Visual Map", crate::actions::OpenVisualMap),
                gpui::MenuItem::action("Checksum Calculation", crate::actions::ShowChecksumTab),
                gpui::MenuItem::separator(),
                gpui::MenuItem::submenu(gpui::Menu {
                    name: "Compare / Diff".into(),
                    items: vec![
                        gpui::MenuItem::action("Compare Open Files...", crate::actions::CompareOpenFiles),
                        gpui::MenuItem::action("Compare Visible Split Panes", crate::actions::CompareVisiblePanes),
                        gpui::MenuItem::separator(),
                        gpui::MenuItem::action("Swap Diff Files", crate::actions::SwapDiffFiles),
                        gpui::MenuItem::action("Refresh Diff", crate::actions::RefreshDiff),
                        gpui::MenuItem::separator(),
                        gpui::MenuItem::action("Next Difference", crate::actions::NextDifference),
                        gpui::MenuItem::action("Previous Difference", crate::actions::PrevDifference),
                    ],
                }),
            ],
        },
        gpui::Menu {
            name: "Window".into(),
            items: vec![
                gpui::MenuItem::action("Split Right", crate::actions::SplitRight),
                gpui::MenuItem::action("Split Down", crate::actions::SplitDown),
                gpui::MenuItem::separator(),
                gpui::MenuItem::action("Next Tab", crate::actions::ActivateNextTab),
                gpui::MenuItem::action("Previous Tab", crate::actions::ActivatePreviousTab),
            ],
        },
    ]);
}

/// Registers global keybindings for window and document actions.
fn setup_keybindings(cx: &mut App) {
    cx.bind_keys([
        // File / Folder dialogs
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-n", crate::actions::NewFile, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-n", crate::actions::NewFile, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-o", crate::actions::OpenFileDialog, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-o", crate::actions::OpenFileDialog, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-shift-o", crate::actions::OpenFolder, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-shift-o", crate::actions::OpenFolder, None),
        // Panels & Views
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-b", crate::actions::ToggleLeftPanel, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-b", crate::actions::ToggleLeftPanel, None),
        // Tab switching
        gpui::KeyBinding::new("ctrl-tab", crate::actions::ActivateNextTab, None),
        gpui::KeyBinding::new("ctrl-shift-tab", crate::actions::ActivatePreviousTab, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("alt-cmd-right", crate::actions::ActivateNextTab, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("alt-ctrl-right", crate::actions::ActivateNextTab, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("alt-cmd-left", crate::actions::ActivatePreviousTab, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("alt-ctrl-left", crate::actions::ActivatePreviousTab, None),
        // Direct Tab Selection (1..9)
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-1", crate::actions::ActivateTab { index: 1 }, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-1", crate::actions::ActivateTab { index: 1 }, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-2", crate::actions::ActivateTab { index: 2 }, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-2", crate::actions::ActivateTab { index: 2 }, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-3", crate::actions::ActivateTab { index: 3 }, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-3", crate::actions::ActivateTab { index: 3 }, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-4", crate::actions::ActivateTab { index: 4 }, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-4", crate::actions::ActivateTab { index: 4 }, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-5", crate::actions::ActivateTab { index: 5 }, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-5", crate::actions::ActivateTab { index: 5 }, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-6", crate::actions::ActivateTab { index: 6 }, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-6", crate::actions::ActivateTab { index: 6 }, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-7", crate::actions::ActivateTab { index: 7 }, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-7", crate::actions::ActivateTab { index: 7 }, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-8", crate::actions::ActivateTab { index: 8 }, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-8", crate::actions::ActivateTab { index: 8 }, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-9", crate::actions::ActivateTab { index: 9 }, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-9", crate::actions::ActivateTab { index: 9 }, None),
        // Close & Quit
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-w", crate::actions::CloseActivePanel, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-w", crate::actions::CloseActivePanel, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-f4", crate::actions::CloseActivePanel, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-q", crate::actions::Quit, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-q", crate::actions::Quit, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("alt-f4", crate::actions::Quit, None),
        // Search & Navigation
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-f", crate::actions::ToggleSearch, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-f", crate::actions::ToggleSearch, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-shift-f", crate::actions::ToggleSearchPanel, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-shift-f", crate::actions::ToggleSearchPanel, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-g", crate::actions::SearchNext, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("f3", crate::actions::SearchNext, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-shift-g", crate::actions::SearchPrev, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("shift-f3", crate::actions::SearchPrev, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-shift-g", crate::actions::SearchPrev, None),
        gpui::KeyBinding::new("ctrl-g", crate::actions::ToggleGoToAddress, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-l", crate::actions::ToggleGoToAddress, None),
        // Clipboard & Selection
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-a", crate::actions::SelectAll, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-a", crate::actions::SelectAll, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-c", crate::actions::Copy, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-c", crate::actions::Copy, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-shift-c", crate::actions::CopyAsHexDump, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-shift-c", crate::actions::CopyAsHexDump, None),
        // Editing
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-s", crate::actions::Save, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-s", crate::actions::Save, None),
        // Cursor movement
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-home", crate::actions::GoToBeginning, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-home", crate::actions::GoToBeginning, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-end", crate::actions::GoToEnd, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-end", crate::actions::GoToEnd, None),
        // Settings
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-,", crate::actions::OpenSettings, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-,", crate::actions::OpenSettings, None),
        // Structure & Splitting
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-shift-s", crate::actions::LoadStructureDefinition, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-shift-s", crate::actions::LoadStructureDefinition, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-shift-v", crate::actions::ToggleInlineStructureView, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-shift-v", crate::actions::ToggleInlineStructureView, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-\\", crate::actions::SplitRight, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-\\", crate::actions::SplitRight, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-shift-d", crate::actions::SplitDown, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-shift-d", crate::actions::SplitDown, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-shift-backspace", crate::actions::ClearAllCustomBreaks, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-shift-backspace", crate::actions::ClearAllCustomBreaks, None),
        // Compare / Diff
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("alt-cmd-d", crate::actions::CompareOpenFiles, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("alt-ctrl-d", crate::actions::CompareOpenFiles, None),
    ]);
}
