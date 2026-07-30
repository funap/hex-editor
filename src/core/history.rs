#![allow(dead_code)]

use crate::core::command::Command;

/// Manages the history of commands for undo/redo functionality.
#[derive(Default)]
pub struct History {
    undo_stack: Vec<Box<dyn Command>>,
    redo_stack: Vec<Box<dyn Command>>,
}

impl History {
    /// Creates a new empty history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Pushes a new command onto the undo stack and clears the redo stack.
    /// This should be called when a new command is executed.
    pub fn push(&mut self, command: Box<dyn Command>) {
        self.undo_stack.push(command);
        self.redo_stack.clear();
    }

    /// Pops the last command from the undo stack.
    pub fn pop_undo(&mut self) -> Option<Box<dyn Command>> {
        self.undo_stack.pop()
    }

    /// Pushes a command onto the redo stack.
    pub fn push_redo(&mut self, command: Box<dyn Command>) {
        self.redo_stack.push(command);
    }

    /// Pops the last command from the redo stack.
    pub fn pop_redo(&mut self) -> Option<Box<dyn Command>> {
        self.redo_stack.pop()
    }

    /// Pushes a command back onto the undo stack (used during redo).
    pub fn push_undo(&mut self, command: Box<dyn Command>) {
        self.undo_stack.push(command);
    }

    /// Clears the history.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Returns the current version of the history (length of the undo stack).
    pub fn version(&self) -> usize {
        self.undo_stack.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyCommand;
    impl Command for DummyCommand {
        fn execute(&mut self, _: &mut crate::core::editor::Editor) {}
        fn undo(&mut self, _: &mut crate::core::editor::Editor) {}
    }

    #[test]
    fn test_history_push_pop() {
        let mut history = History::new();
        assert_eq!(history.version(), 0);

        history.push(Box::new(DummyCommand));
        assert_eq!(history.version(), 1);

        history.push_redo(Box::new(DummyCommand));
        assert!(history.pop_redo().is_some());

        history.push(Box::new(DummyCommand));
        assert!(history.pop_redo().is_none()); // push clears redo stack

        assert!(history.pop_undo().is_some());
        assert_eq!(history.version(), 1);

        history.clear();
        assert_eq!(history.version(), 0);
    }
}
