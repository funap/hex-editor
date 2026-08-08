#![allow(dead_code)]

use crate::core::editor::Editor;

/// A trait representing an executable and undoable command.
pub trait Command: Send + Sync {
    fn execute(&mut self, editor: &mut Editor);
    fn undo(&mut self, editor: &mut Editor);
}

/// Command to insert a single character.
pub struct InsertCharCommand {
    pub position: usize,
    pub c: u8,
}

impl InsertCharCommand {
    pub fn new(position: usize, c: u8) -> Self {
        Self { position, c }
    }
}

impl Command for InsertCharCommand {
    fn execute(&mut self, editor: &mut Editor) {
        if let Ok(mut document) = editor.document.write() {
            document.buffer.insert(self.position, self.c);
        }
        editor.set_cursor_offset(self.position + 1);
    }

    fn undo(&mut self, editor: &mut Editor) {
        if let Ok(mut document) = editor.document.write() {
            document.buffer.remove(self.position);
        }
        editor.set_cursor_offset(self.position);
    }
}

/// Command to delete a single character at a specific position.
pub struct DeleteCharCommand {
    pub position: usize,
    pub deleted_char: Option<u8>,
}

impl Command for DeleteCharCommand {
    fn execute(&mut self, editor: &mut Editor) {
        if let Ok(mut document) = editor.document.write() {
            self.deleted_char = document.buffer.remove(self.position);
        }
    }

    fn undo(&mut self, editor: &mut Editor) {
        if let Some(c) = self.deleted_char
            && let Ok(mut document) = editor.document.write()
        {
            document.buffer.insert(self.position, c);
        }
    }
}
