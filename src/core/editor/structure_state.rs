use crate::core::structure::KsyDefinition;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Encapsulates Kaitai Struct parsing and inline visualization state for [`Editor`].
#[derive(Clone, Debug)]
pub struct EditorStructureState {
    pub is_parsing: bool,
    /// True after byte parsing reaches the end and display indexes are being finalized.
    pub is_finalizing: bool,
    pub progress_offset: usize,
    pub total_size: usize,
    pub generation: usize,
    pub cancel_token: Option<Arc<AtomicBool>>,
    /// Enables background reparsing after document edits.
    pub is_async: bool,
    /// Set after an edit until the UI starts the debounced background parse.
    pub reparse_requested: bool,
    pub collapsed_struct_ids: HashSet<String>,
    pub show_inline_structure_view: bool,
}

impl Default for EditorStructureState {
    fn default() -> Self {
        Self {
            is_parsing: false,
            is_finalizing: false,
            progress_offset: 0,
            total_size: 0,
            generation: 0,
            cancel_token: None,
            is_async: false,
            reparse_requested: false,
            collapsed_struct_ids: HashSet::new(),
            show_inline_structure_view: true,
        }
    }
}

impl EditorStructureState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&mut self) {
        if let Some(token) = self.cancel_token.take() {
            token.store(true, Ordering::SeqCst);
        }
        self.generation = self.generation.wrapping_add(1);
        self.reparse_requested = false;
        self.is_parsing = false;
        self.is_finalizing = false;
    }

    pub fn reset_progress(&mut self) {
        self.progress_offset = 0;
        self.total_size = 0;
        self.is_parsing = false;
        self.is_finalizing = false;
    }

    pub fn toggle_collapsed(&mut self, struct_id: &str) {
        if self.collapsed_struct_ids.contains(struct_id) {
            self.collapsed_struct_ids.remove(struct_id);
        } else {
            self.collapsed_struct_ids.insert(struct_id.to_string());
        }
    }

    pub fn is_collapsed(&self, struct_id: &str) -> bool {
        self.collapsed_struct_ids.contains(struct_id)
    }

    pub fn toggle_inline_view(&mut self) {
        self.show_inline_structure_view = !self.show_inline_structure_view;
    }

    pub fn pending_reparse(&self, ksy: Option<&Arc<KsyDefinition>>) -> Option<(Arc<KsyDefinition>, usize)> {
        if self.reparse_requested {
            ksy.cloned().map(|k| (k, self.generation))
        } else {
            None
        }
    }

    pub fn take_reparse_request(&mut self, generation: usize, ksy: Option<&Arc<KsyDefinition>>) -> Option<Arc<KsyDefinition>> {
        if !self.reparse_requested || self.generation != generation {
            return None;
        }
        self.reparse_requested = false;
        ksy.cloned()
    }

    pub fn start_async_parse(&mut self, total_size: usize, cancel_token: Arc<AtomicBool>) {
        self.cancel();
        self.is_async = true;
        self.reparse_requested = false;
        self.is_parsing = true;
        self.is_finalizing = false;
        self.progress_offset = 0;
        self.total_size = total_size;
        self.cancel_token = Some(cancel_token);
    }

    pub fn finish_parse(&mut self) {
        self.is_parsing = false;
        self.is_finalizing = false;
    }
}
