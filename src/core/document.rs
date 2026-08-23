#![allow(dead_code)]

use crate::core::bookmark::BookmarkItem;
use crate::core::buffer::Buffer;
use crate::core::history::History;
use crate::core::structure::{KsyDefinition, ParseResult};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Represents a document processing unit that bundles a file buffer and its edit history.
pub struct Document {
    pub path: PathBuf,
    pub buffer: Buffer,
    pub history: History,
    pub last_saved_version: usize,
    read_only: bool,
    pub bookmarks: Arc<RwLock<Vec<BookmarkItem>>>,
    pub ksy_definition: Arc<RwLock<Option<Arc<KsyDefinition>>>>,
    pub parse_result: Arc<RwLock<Option<Arc<ParseResult>>>>,
    pub custom_breaks: Arc<RwLock<BTreeSet<usize>>>,
    pub custom_joins: Arc<RwLock<BTreeSet<usize>>>,
    pub empty_lines: Arc<RwLock<BTreeMap<usize, usize>>>,
}

impl Document {
    pub fn new(path: PathBuf, buffer: Buffer) -> Self {
        Self {
            path,
            buffer,
            history: History::new(),
            last_saved_version: 0,
            read_only: false,
            bookmarks: Arc::new(RwLock::new(Vec::new())),
            ksy_definition: Arc::new(RwLock::new(None)),
            parse_result: Arc::new(RwLock::new(None)),
            custom_breaks: Arc::new(RwLock::new(BTreeSet::new())),
            custom_joins: Arc::new(RwLock::new(BTreeSet::new())),
            empty_lines: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    /// Creates a document that starts in read-only mode.
    pub fn new_read_only(path: PathBuf, buffer: Buffer) -> Self {
        let mut document = Self::new(path, buffer);
        document.read_only = true;
        document
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Changes the file path used by subsequent save operations.
    pub fn set_path(&mut self, path: PathBuf) {
        self.path = path;
    }

    /// Returns true if the document has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.history.state_id() != self.last_saved_version
    }

    /// Returns whether editing and normal saves are currently disabled.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Changes whether the document accepts edits and normal saves.
    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }

    /// Toggles read-only mode and returns the new state.
    pub fn toggle_read_only(&mut self) -> bool {
        self.read_only = !self.read_only;
        self.read_only
    }

    /// Marks the document as saved, updating the last saved version.
    pub fn mark_as_saved(&mut self) {
        self.last_saved_version = self.history.state_id();
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        let old_definition = self.ksy_definition.write().ok().and_then(|mut definition| definition.take());
        let old_parse_result = self.parse_result.write().ok().and_then(|mut result| result.take());
        if old_definition.is_some() || old_parse_result.is_some() {
            std::thread::spawn(move || {
                drop((old_definition, old_parse_result));
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::command::Command;
    use crate::core::editor::Editor;

    struct MockCommand;

    impl Command for MockCommand {
        fn execute(&mut self, _editor: &mut Editor) {}
        fn undo(&mut self, _editor: &mut Editor) {}
    }

    #[test]
    fn test_is_dirty() {
        let mut doc = Document::new(PathBuf::from("test"), Buffer::empty());

        // Initially clean
        assert!(!doc.is_dirty());

        // Simulate modification (push command to history)
        doc.history.push(Box::new(MockCommand));
        assert!(doc.is_dirty());

        // Simulate save
        doc.mark_as_saved();
        assert!(!doc.is_dirty());

        // Modify again
        doc.history.push(Box::new(MockCommand));
        assert!(doc.is_dirty());

        // Undo
        // Note: history.pop_undo() returns the command and removes it from undo stack
        doc.history.pop_undo();
        assert!(!doc.is_dirty());
    }

    #[test]
    fn test_read_only_state() {
        let mut doc = Document::new_read_only(PathBuf::from("test"), Buffer::empty());
        assert!(doc.is_read_only());

        doc.set_read_only(false);
        assert!(!doc.is_read_only());
        assert!(doc.toggle_read_only());
        assert!(doc.is_read_only());
    }
}
