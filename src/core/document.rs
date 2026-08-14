#![allow(dead_code)]

use crate::core::buffer::Buffer;
use crate::core::highlight::HighlightItem;
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
    pub highlights: Arc<RwLock<Vec<HighlightItem>>>,
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
            highlights: Arc::new(RwLock::new(Vec::new())),
            ksy_definition: Arc::new(RwLock::new(None)),
            parse_result: Arc::new(RwLock::new(None)),
            custom_breaks: Arc::new(RwLock::new(BTreeSet::new())),
            custom_joins: Arc::new(RwLock::new(BTreeSet::new())),
            empty_lines: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns true if the document has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.history.version() != self.last_saved_version
    }

    /// Marks the document as saved, updating the last saved version.
    pub fn mark_as_saved(&mut self) {
        self.last_saved_version = self.history.version();
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
}
