#![allow(dead_code)]

use crate::core::editor::Editor;
use crate::core::selection::Selection;

/// A trait representing an executable and undoable command.
pub trait Command: Send + Sync {
    fn execute(&mut self, editor: &mut Editor);
    fn undo(&mut self, editor: &mut Editor);

    /// Returns true when executing the command did not change the document.
    fn is_noop(&self) -> bool {
        false
    }
}

/// A snapshot of the transient cursor state surrounding an edit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CursorState {
    pub cursor_offset: usize,
    pub selection: Selection,
}

/// Replaces an arbitrary range of bytes and restores cursor state on undo.
///
/// An empty range is an insertion and an empty replacement is a deletion, so
/// one command type covers all ordinary buffer edits.
pub struct ReplaceRangeCommand {
    pub position: usize,
    pub old: Vec<u8>,
    pub new: Vec<u8>,
    pub before: CursorState,
    pub after: CursorState,
    noop: bool,
}

impl ReplaceRangeCommand {
    pub fn new(position: usize, old: Vec<u8>, new: Vec<u8>, before: CursorState, after: CursorState) -> Self {
        let noop = old == new;
        Self {
            position,
            old,
            new,
            before,
            after,
            noop,
        }
    }
}

impl Command for ReplaceRangeCommand {
    fn execute(&mut self, editor: &mut Editor) {
        if self.noop {
            return;
        }
        if let Ok(mut document) = editor.document.write() {
            document
                .buffer
                .replace_range(self.position..self.position.saturating_add(self.old.len()), &self.new);
        }
        editor.adjust_after_edit(self.position, self.old.len(), self.new.len());
        editor.restore_cursor_state(self.after);
    }

    fn undo(&mut self, editor: &mut Editor) {
        if self.noop {
            return;
        }
        if let Ok(mut document) = editor.document.write() {
            document
                .buffer
                .replace_range(self.position..self.position.saturating_add(self.new.len()), &self.old);
        }
        editor.adjust_after_edit(self.position, self.new.len(), self.old.len());
        editor.restore_cursor_state(self.before);
    }

    fn is_noop(&self) -> bool {
        self.noop
    }
}

/// Command to insert a single character.
pub struct InsertCharCommand {
    pub position: usize,
    pub c: u8,
    inserted: bool,
}

impl InsertCharCommand {
    pub fn new(position: usize, c: u8) -> Self {
        Self { position, c, inserted: false }
    }
}

impl Command for InsertCharCommand {
    fn execute(&mut self, editor: &mut Editor) {
        let mut inserted = false;
        if let Ok(mut document) = editor.document.write()
            && self.position <= document.buffer.len()
        {
            document.buffer.insert(self.position, self.c);
            inserted = true;
        }
        self.inserted = inserted;
        if inserted {
            editor.adjust_after_edit(self.position, 0, 1);
            editor.set_cursor_offset(self.position + 1);
        }
    }

    fn undo(&mut self, editor: &mut Editor) {
        let mut removed = false;
        if let Ok(mut document) = editor.document.write()
            && self.position < document.buffer.len()
        {
            removed = document.buffer.remove(self.position).is_some();
        }
        if removed {
            editor.adjust_after_edit(self.position, 1, 0);
            editor.set_cursor_offset(self.position);
        }
    }

    fn is_noop(&self) -> bool {
        !self.inserted
    }
}

/// Command to delete a single character at a specific position.
pub struct DeleteCharCommand {
    pub position: usize,
    pub deleted_char: Option<u8>,
}

impl Command for DeleteCharCommand {
    fn execute(&mut self, editor: &mut Editor) {
        let mut deleted = false;
        if let Ok(mut document) = editor.document.write() {
            if self.deleted_char.is_none() {
                self.deleted_char = document.buffer.remove(self.position);
                deleted = self.deleted_char.is_some();
            } else {
                deleted = document.buffer.remove(self.position).is_some();
            }
        }
        if deleted {
            editor.adjust_after_edit(self.position, 1, 0);
        }
    }

    fn undo(&mut self, editor: &mut Editor) {
        let mut inserted = false;
        if let Some(c) = self.deleted_char
            && let Ok(mut document) = editor.document.write()
            && self.position <= document.buffer.len()
        {
            document.buffer.insert(self.position, c);
            inserted = true;
        }
        if inserted {
            editor.adjust_after_edit(self.position, 0, 1);
        }
    }

    fn is_noop(&self) -> bool {
        self.deleted_char.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::{DeleteCharCommand, InsertCharCommand};
    use crate::core::buffer::Buffer;
    use crate::core::document::Document;
    use crate::core::editor::Editor;
    use std::path::PathBuf;
    use std::sync::{Arc, RwLock};

    fn editor_with_content(content: &[u8]) -> Editor {
        let document = Document::new(PathBuf::from("command-test.bin"), Buffer::new(content.to_vec()));
        Editor::new(Arc::new(RwLock::new(document)))
    }

    fn bytes(editor: &Editor) -> Vec<u8> {
        editor.document.read().expect("document read lock").buffer.data().to_vec()
    }

    #[test]
    fn insert_command_round_trips_through_undo_and_redo() {
        let mut editor = editor_with_content(b"ac");

        editor.execute_command(Box::new(InsertCharCommand::new(1, b'b')));
        assert_eq!(bytes(&editor), b"abc");
        assert_eq!(editor.cursor_offset, 2);
        assert!(editor.document.read().unwrap().is_dirty());

        editor.undo();
        assert_eq!(bytes(&editor), b"ac");
        assert_eq!(editor.cursor_offset, 1);

        editor.redo();
        assert_eq!(bytes(&editor), b"abc");
        assert_eq!(editor.cursor_offset, 2);
    }

    #[test]
    fn delete_command_restores_the_deleted_byte() {
        let mut editor = editor_with_content(b"abc");

        editor.execute_command(Box::new(DeleteCharCommand {
            position: 1,
            deleted_char: None,
        }));
        assert_eq!(bytes(&editor), b"ac");

        editor.undo();
        assert_eq!(bytes(&editor), b"abc");

        editor.redo();
        assert_eq!(bytes(&editor), b"ac");
    }

    #[test]
    fn deleting_out_of_bounds_is_a_no_op() {
        let mut editor = editor_with_content(b"abc");

        editor.execute_command(Box::new(DeleteCharCommand {
            position: 3,
            deleted_char: None,
        }));

        assert_eq!(bytes(&editor), b"abc");
        editor.undo();
        assert_eq!(bytes(&editor), b"abc");
        editor.redo();
        assert_eq!(bytes(&editor), b"abc");
    }

    #[test]
    fn inserting_out_of_bounds_is_a_no_op() {
        let mut editor = editor_with_content(b"abc");

        assert!(!editor.execute_command(Box::new(InsertCharCommand::new(4, b'x'))));
        assert_eq!(bytes(&editor), b"abc");
        assert!(!editor.can_undo());
        assert!(!editor.document.read().unwrap().is_dirty());
    }
}
