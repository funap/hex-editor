use gpui::prelude::*;
use gpui::*;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::core::document::Document;
use crate::core::editor::Editor;
use crate::ui::panels::diff_panel::DiffPanel;
use crate::ui::panels::editor_panel::EditorPanel;
use crate::ui::panels::settings_panel::SettingsPanel;
use crate::ui::panels::visual_map_panel::VisualMapPanel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabDrag {
    pub from_group_id: usize,
    pub tab_id: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropPlacement {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

#[allow(dead_code)]
#[derive(Clone)]
pub enum TabContent {
    Editor(Entity<EditorPanel>),
    Diff(Entity<DiffPanel>),
    Settings(Entity<SettingsPanel>),
    VisualMap(Entity<VisualMapPanel>),
}

#[allow(dead_code)]
impl TabContent {
    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self {
            TabContent::Editor(p) => p.read(cx).focus_handle(cx),
            TabContent::Diff(p) => p.read(cx).focus_handle(cx),
            TabContent::Settings(p) => p.read(cx).focus_handle(cx),
            TabContent::VisualMap(p) => p.read(cx).focus_handle(cx),
        }
    }

    pub fn editor(&self, cx: &App) -> Option<Entity<Editor>> {
        match self {
            TabContent::Editor(p) => Some(p.read(cx).editor()),
            _ => None,
        }
    }

    pub fn editor_panel(&self) -> Option<Entity<EditorPanel>> {
        match self {
            TabContent::Editor(p) => Some(p.clone()),
            _ => None,
        }
    }

    pub fn document(&self, cx: &App) -> Option<Arc<RwLock<Document>>> {
        match self {
            TabContent::Editor(p) => Some(p.read(cx).editor().read(cx).document.clone()),
            _ => None,
        }
    }

    pub fn path(&self, cx: &App) -> Option<PathBuf> {
        match self {
            TabContent::Editor(p) => Some(p.read(cx).path(cx)),
            TabContent::Diff(_) => None,
            TabContent::Settings(_) => None,
            TabContent::VisualMap(_) => None,
        }
    }

    pub fn title(&self, cx: &App) -> String {
        match self {
            TabContent::Editor(p) => {
                let path = p.read(cx).path(cx);
                path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Untitled".to_string())
            }
            TabContent::Diff(_) => "Diff".to_string(),
            TabContent::Settings(_) => "Settings".to_string(),
            TabContent::VisualMap(_) => "Visual Map".to_string(),
        }
    }

    pub fn is_dirty(&self, cx: &App) -> bool {
        match self {
            TabContent::Editor(p) => p.read(cx).editor().read(cx).document.read().map(|d| d.is_dirty()).unwrap_or(false),
            _ => false,
        }
    }

    pub fn render(&self) -> AnyElement {
        match self {
            TabContent::Editor(p) => p.clone().into_any_element(),
            TabContent::Diff(p) => p.clone().into_any_element(),
            TabContent::Settings(p) => p.clone().into_any_element(),
            TabContent::VisualMap(p) => p.clone().into_any_element(),
        }
    }
}

pub struct TabItem {
    pub id: usize,
    pub content: TabContent,
}

impl TabItem {
    pub fn new(id: usize, content: TabContent) -> Self {
        Self { id, content }
    }

    pub fn title(&self, cx: &App) -> String {
        self.content.title(cx)
    }

    pub fn path(&self, cx: &App) -> Option<PathBuf> {
        self.content.path(cx)
    }

    pub fn is_dirty(&self, cx: &App) -> bool {
        self.content.is_dirty(cx)
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.content.focus_handle(cx)
    }
}
