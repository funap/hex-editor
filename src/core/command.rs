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
}
