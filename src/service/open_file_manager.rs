use crate::core::document::Document;
use crate::core::editor::Editor;
use crate::ui::panels::editor_panel::EditorPanel;
use gpui::{Context, Entity, EntityId, EventEmitter};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OpenEntryId(pub usize);

#[allow(dead_code)]
#[derive(Clone)]
pub struct OpenEntry {
    pub id: OpenEntryId,
    pub path: PathBuf,
    pub document: Arc<RwLock<Document>>,
    pub editor: Entity<Editor>,
    pub panel: Entity<EditorPanel>,
}

#[allow(dead_code)]
pub enum OpenFileEvent {
    Opened(OpenEntryId),
    Closed(OpenEntryId),
    Activated(OpenEntryId),
}

pub struct OpenFileManager {
    entries: Vec<OpenEntry>,
    active_id: Option<OpenEntryId>,
    next_id: usize,
}

#[allow(dead_code)]
impl OpenFileManager {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            active_id: None,
            next_id: 1,
        }
    }

    pub fn open(
        &mut self,
        path: PathBuf,
        document: Arc<RwLock<Document>>,
        editor: Entity<Editor>,
        panel: Entity<EditorPanel>,
        cx: &mut Context<Self>,
    ) -> OpenEntryId {
        let path = path.canonicalize().unwrap_or(path);

        let id = OpenEntryId(self.next_id);
        self.next_id += 1;

        let entry = OpenEntry {
            id,
            path,
            document,
            editor,
            panel,
        };

        self.entries.push(entry);
        self.active_id = Some(id);

        cx.emit(OpenFileEvent::Opened(id));
        cx.emit(OpenFileEvent::Activated(id));
        cx.notify();

        id
    }

    pub fn close(&mut self, id: OpenEntryId, cx: &mut Context<Self>) {
        if let Some(pos) = self.entries.iter().position(|e| e.id == id) {
            self.entries.remove(pos);

            cx.emit(OpenFileEvent::Closed(id));

            if self.active_id == Some(id) {
                if !self.entries.is_empty() {
                    let new_active_pos = pos.min(self.entries.len() - 1);
                    let new_active_id = self.entries[new_active_pos].id;
                    self.active_id = Some(new_active_id);
                    cx.emit(OpenFileEvent::Activated(new_active_id));
                } else {
                    self.active_id = None;
                }
            }
            cx.notify();
        }
    }

    pub fn close_panel(&mut self, panel_id: EntityId, cx: &mut Context<Self>) -> Option<PathBuf> {
        if let Some(pos) = self.entries.iter().position(|e| e.panel.entity_id() == panel_id) {
            let entry = self.entries.remove(pos);
            let id = entry.id;
            let path = entry.path;

            cx.emit(OpenFileEvent::Closed(id));

            if self.active_id == Some(id) {
                if !self.entries.is_empty() {
                    let new_active_pos = pos.min(self.entries.len() - 1);
                    let new_active_id = self.entries[new_active_pos].id;
                    self.active_id = Some(new_active_id);
                    cx.emit(OpenFileEvent::Activated(new_active_id));
                } else {
                    self.active_id = None;
                }
            }
            cx.notify();
            Some(path)
        } else {
            None
        }
    }

    pub fn activate(&mut self, id: OpenEntryId, cx: &mut Context<Self>) {
        if self.entries.iter().any(|e| e.id == id) && self.active_id != Some(id) {
            self.active_id = Some(id);
            cx.emit(OpenFileEvent::Activated(id));
            cx.notify();
        }
    }

    pub fn activate_panel(&mut self, panel_id: EntityId, cx: &mut Context<Self>) {
        if let Some(entry) = self.entries.iter().find(|e| e.panel.entity_id() == panel_id) {
            let id = entry.id;
            self.activate(id, cx);
        }
    }

    pub fn activate_editor(&mut self, editor_id: EntityId, cx: &mut Context<Self>) {
        if let Some(entry) = self.entries.iter().find(|e| e.editor.entity_id() == editor_id) {
            let id = entry.id;
            self.activate(id, cx);
        }
    }

    pub fn find_by_path(&self, path: &Path) -> Option<&OpenEntry> {
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.entries.iter().find(|e| e.path == canonical_path)
    }

    pub fn find_by_panel(&self, panel_id: EntityId) -> Option<&OpenEntry> {
        self.entries.iter().find(|e| e.panel.entity_id() == panel_id)
    }

    pub fn find_by_editor(&self, editor_id: EntityId) -> Option<&OpenEntry> {
        self.entries.iter().find(|e| e.editor.entity_id() == editor_id)
    }

    pub fn count_for_path(&self, path: &Path) -> usize {
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.entries.iter().filter(|e| e.path == canonical_path).count()
    }

    pub fn active_entry(&self) -> Option<&OpenEntry> {
        let active_id = self.active_id?;
        self.entries.iter().find(|e| e.id == active_id)
    }

    pub fn active_editor(&self) -> Option<Entity<Editor>> {
        self.active_entry().map(|e| e.editor.clone())
    }

    #[allow(dead_code)]
    pub fn entries(&self) -> &[OpenEntry] {
        &self.entries
    }

    pub fn active_index(&self) -> Option<usize> {
        let active_id = self.active_id?;
        self.entries.iter().position(|e| e.id == active_id)
    }

    pub fn activate_next(&mut self, cx: &mut Context<Self>) {
        if self.entries.is_empty() {
            return;
        }
        let current = self.active_index().unwrap_or(0);
        let next = (current + 1) % self.entries.len();
        let id = self.entries[next].id;
        self.activate(id, cx);
    }

    pub fn activate_previous(&mut self, cx: &mut Context<Self>) {
        if self.entries.is_empty() {
            return;
        }
        let current = self.active_index().unwrap_or(0);
        let prev = if current == 0 { self.entries.len() - 1 } else { current - 1 };
        let id = self.entries[prev].id;
        self.activate(id, cx);
    }

    pub fn activate_index(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.entries.is_empty() {
            return;
        }
        let target = index.min(self.entries.len() - 1);
        let id = self.entries[target].id;
        self.activate(id, cx);
    }

    pub fn close_others(&mut self, cx: &mut Context<Self>) -> Vec<PathBuf> {
        let active_id = self.active_id;
        let mut closed_paths = Vec::new();
        if let Some(active_id) = active_id {
            let (keep, remove): (Vec<_>, Vec<_>) = self.entries.drain(..).partition(|e| e.id == active_id);
            self.entries = keep;
            for entry in remove {
                closed_paths.push(entry.path);
                cx.emit(OpenFileEvent::Closed(entry.id));
            }
            cx.notify();
        }
        closed_paths
    }

    pub fn close_all(&mut self, cx: &mut Context<Self>) -> Vec<PathBuf> {
        let mut closed_paths = Vec::new();
        let old_entries = std::mem::take(&mut self.entries);
        self.active_id = None;
        for entry in old_entries {
            closed_paths.push(entry.path);
            cx.emit(OpenFileEvent::Closed(entry.id));
        }
        cx.notify();
        closed_paths
    }
}

impl EventEmitter<OpenFileEvent> for OpenFileManager {}
