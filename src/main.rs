#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![recursion_limit = "256"]

use gpui::Application;
use gpui_component_assets::Assets;

mod actions;
mod app_state;
mod core;
mod service;
mod theme;
mod ui;

use crate::core::appearance::Appearance;
use ui::workspace::Workspace;

impl gpui::Global for Appearance {}

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("initialize tokio runtime");
    let _guard = rt.enter();

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();

    let app = Application::new().with_assets(Assets);

    app.run(move |cx| {
        app_state::AppState::init(cx);
        cx.set_global(Appearance::default());

        gpui_component::init(cx);
        theme::init(cx);
        ui::workspace::init(cx);
        ui::components::file_tree_view::init(cx);
        ui::components::search_bar::init(cx);
        ui::components::struct_tree_view::init(cx);
        ui::panels::editor_panel::init(cx);
        ui::panels::diff_panel::init(cx);

        cx.set_menus(vec![
            gpui::Menu {
                name: "File".into(),
                items: vec![
                    gpui::MenuItem::action("Open File...", crate::actions::OpenFileDialog),
                    gpui::MenuItem::action("Open Folder...", crate::actions::OpenFolder),
                    gpui::MenuItem::action("Close Folder", crate::actions::CloseFolder),
                    gpui::MenuItem::separator(),
                    gpui::MenuItem::action("Settings", crate::actions::OpenSettings),
                    gpui::MenuItem::separator(),
                    gpui::MenuItem::action("Quit", crate::actions::Quit),
                ],
            },
            gpui::Menu {
                name: "Edit".into(),
                items: vec![
                    gpui::MenuItem::action("Select All", crate::actions::SelectAll),
                    gpui::MenuItem::separator(),
                    gpui::MenuItem::action("Copy", crate::actions::Copy),
                    gpui::MenuItem::submenu(gpui::Menu {
                        name: "Copy As".into(),
                        items: vec![
                            gpui::MenuItem::action("as Hex Dump", crate::actions::CopyAsHexDump),
                            gpui::MenuItem::action("as C++ Array", crate::actions::CopyAsCppArray),
                            gpui::MenuItem::action("as Hex Stream", crate::actions::CopyAsHexStream),
                            gpui::MenuItem::action("as Hex with Spaces", crate::actions::CopyAsHexSpaces),
                            gpui::MenuItem::action("as Printable Text", crate::actions::CopyAsPrintableText),
                            gpui::MenuItem::action("as Base64", crate::actions::CopyAsBase64),
                            gpui::MenuItem::action("as Escaped String", crate::actions::CopyAsEscapedString),
                            gpui::MenuItem::action("as Binary", crate::actions::CopyAsBinary),
                            gpui::MenuItem::action("as Rust Array", crate::actions::CopyAsRustArray),
                            gpui::MenuItem::action("as JSON Array", crate::actions::CopyAsJsonArray),
                        ],
                    }),
                    gpui::MenuItem::separator(),
                    gpui::MenuItem::submenu(gpui::Menu {
                        name: "Highlight".into(),
                        items: vec![
                            gpui::MenuItem::action("Red", crate::actions::HighlightRed),
                            gpui::MenuItem::action("Orange", crate::actions::HighlightOrange),
                            gpui::MenuItem::action("Yellow", crate::actions::HighlightYellow),
                            gpui::MenuItem::action("Green", crate::actions::HighlightGreen),
                            gpui::MenuItem::action("Cyan", crate::actions::HighlightCyan),
                            gpui::MenuItem::action("Blue", crate::actions::HighlightBlue),
                            gpui::MenuItem::action("Purple", crate::actions::HighlightPurple),
                            gpui::MenuItem::action("Pink", crate::actions::HighlightPink),
                            gpui::MenuItem::separator(),
                            gpui::MenuItem::action("Clear Highlight", crate::actions::ClearHighlight),
                            gpui::MenuItem::action("Clear All Highlights", crate::actions::ClearAllHighlights),
                        ],
                    }),
                    gpui::MenuItem::separator(),
                    gpui::MenuItem::action("Find", crate::actions::ToggleSearch),
                    gpui::MenuItem::action("Find Next", crate::actions::SearchNext),
                    gpui::MenuItem::action("Find Previous", crate::actions::SearchPrev),
                ],
            },
            gpui::Menu {
                name: "Go".into(),
                items: vec![
                    gpui::MenuItem::action("Go to Beginning", crate::actions::GoToBeginning),
                    gpui::MenuItem::action("Go to End", crate::actions::GoToEnd),
                ],
            },
            gpui::Menu {
                name: "View".into(),
                items: vec![
                    gpui::MenuItem::action("Toggle Left Panel", crate::actions::ToggleLeftPanel),
                    gpui::MenuItem::separator(),
                    gpui::MenuItem::action("2D Visual Map", crate::actions::OpenVisualMap),
                    gpui::MenuItem::separator(),
                    gpui::MenuItem::submenu(gpui::Menu {
                        name: "Encoding".into(),
                        items: vec![
                            gpui::MenuItem::action("ASCII", crate::actions::SetEncodingAscii),
                            gpui::MenuItem::action("UTF-8", crate::actions::SetEncodingUtf8),
                            gpui::MenuItem::action("UTF-16 LE", crate::actions::SetEncodingUtf16Le),
                            gpui::MenuItem::action("UTF-16 BE", crate::actions::SetEncodingUtf16Be),
                        ],
                    }),
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
                        ],
                    }),
                ],
            },
            gpui::Menu {
                name: "Structure".into(),
                items: vec![
                    gpui::MenuItem::action("Load Definition...", crate::actions::LoadStructureDefinition),
                    gpui::MenuItem::action("Clear Definition", crate::actions::ClearStructureDefinition),
                    gpui::MenuItem::separator(),
                    gpui::MenuItem::action("Toggle Inline Structure View", crate::actions::ToggleInlineStructureView),
                ],
            },
            gpui::Menu {
                name: "Layout".into(),
                items: vec![
                    gpui::MenuItem::action("Break Line", crate::actions::AddCustomBreak),
                    gpui::MenuItem::action("Join Lines", crate::actions::JoinLine),
                    gpui::MenuItem::separator(),
                    gpui::MenuItem::action("Remove Break Backward", crate::actions::RemoveCustomBreakBackward),
                    gpui::MenuItem::action("Remove Break Forward", crate::actions::RemoveCustomBreakForward),
                    gpui::MenuItem::separator(),
                    gpui::MenuItem::action("Reset Layout", crate::actions::ClearAllCustomBreaks),
                ],
            },
            gpui::Menu {
                name: "Window".into(),
                items: vec![
                    gpui::MenuItem::action("Close Tab", crate::actions::CloseActivePanel),
                    gpui::MenuItem::action("Close Other Tabs", crate::actions::CloseOtherTabs),
                    gpui::MenuItem::action("Close All Tabs", crate::actions::CloseAllTabs),
                    gpui::MenuItem::separator(),
                    gpui::MenuItem::action("Next Tab", crate::actions::ActivateNextTab),
                    gpui::MenuItem::action("Previous Tab", crate::actions::ActivatePreviousTab),
                    gpui::MenuItem::separator(),
                    gpui::MenuItem::action("Split Right", crate::actions::SplitRight),
                    gpui::MenuItem::action("Split Down", crate::actions::SplitDown),
                ],
            },
        ]);

        cx.bind_keys([
            gpui::KeyBinding::new("cmd-o", crate::actions::OpenFileDialog, None),
            gpui::KeyBinding::new("cmd-shift-o", crate::actions::OpenFolder, None),
            gpui::KeyBinding::new("cmd-b", crate::actions::ToggleLeftPanel, None),
            gpui::KeyBinding::new("ctrl-tab", crate::actions::ActivateNextTab, None),
            gpui::KeyBinding::new("ctrl-shift-tab", crate::actions::ActivatePreviousTab, None),
            gpui::KeyBinding::new("alt-cmd-right", crate::actions::ActivateNextTab, None),
            gpui::KeyBinding::new("alt-cmd-left", crate::actions::ActivatePreviousTab, None),
            gpui::KeyBinding::new("cmd-1", crate::actions::ActivateTab { index: 1 }, None),
            gpui::KeyBinding::new("cmd-2", crate::actions::ActivateTab { index: 2 }, None),
            gpui::KeyBinding::new("cmd-3", crate::actions::ActivateTab { index: 3 }, None),
            gpui::KeyBinding::new("cmd-4", crate::actions::ActivateTab { index: 4 }, None),
            gpui::KeyBinding::new("cmd-5", crate::actions::ActivateTab { index: 5 }, None),
            gpui::KeyBinding::new("cmd-6", crate::actions::ActivateTab { index: 6 }, None),
            gpui::KeyBinding::new("cmd-7", crate::actions::ActivateTab { index: 7 }, None),
            gpui::KeyBinding::new("cmd-8", crate::actions::ActivateTab { index: 8 }, None),
            gpui::KeyBinding::new("cmd-9", crate::actions::ActivateTab { index: 9 }, None),
            gpui::KeyBinding::new("cmd-w", crate::actions::CloseActivePanel, None),
            gpui::KeyBinding::new("ctrl-w", crate::actions::CloseActivePanel, None),
            gpui::KeyBinding::new("cmd-q", crate::actions::Quit, None),
            gpui::KeyBinding::new("cmd-f", crate::actions::ToggleSearch, None),
            gpui::KeyBinding::new("cmd-g", crate::actions::SearchNext, None),
            gpui::KeyBinding::new("cmd-shift-g", crate::actions::SearchPrev, None),
            gpui::KeyBinding::new("cmd-a", crate::actions::SelectAll, None),
            gpui::KeyBinding::new("cmd-c", crate::actions::Copy, None),
            gpui::KeyBinding::new("ctrl-c", crate::actions::Copy, None),
            gpui::KeyBinding::new("cmd-shift-c", crate::actions::CopyAsHexDump, None),
            gpui::KeyBinding::new("cmd-home", crate::actions::GoToBeginning, None),
            gpui::KeyBinding::new("cmd-end", crate::actions::GoToEnd, None),
            gpui::KeyBinding::new("cmd-,", crate::actions::OpenSettings, None),
            gpui::KeyBinding::new("cmd-shift-s", crate::actions::LoadStructureDefinition, None),
            gpui::KeyBinding::new("cmd-shift-v", crate::actions::ToggleInlineStructureView, None),
            gpui::KeyBinding::new("cmd-\\", crate::actions::SplitRight, None),
            gpui::KeyBinding::new("cmd-shift-d", crate::actions::SplitDown, None),
            gpui::KeyBinding::new("cmd-shift-backspace", crate::actions::ClearAllCustomBreaks, None),
        ]);

        // Parse command line arguments (skip the first one which is the program name)
        let mut files_to_open = Vec::new();
        let mut folder_to_open = None;

        for arg in args.iter().skip(1) {
            let path = std::path::PathBuf::from(arg);
            if path.is_file() {
                files_to_open.push(path);
            } else if path.is_dir() {
                // Use the last directory as the folder to open
                folder_to_open = Some(path);
            } else {
                eprintln!("Warning: Path does not exist: {}", path.display());
            }
        }

        Workspace::open_window(cx, files_to_open, folder_to_open).detach();
    });
}
