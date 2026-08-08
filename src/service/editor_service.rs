use crate::core::buffer::Buffer;
use crate::core::document::Document;
use crate::core::editor::Editor;
use crate::core::search::{self, SearchOptions};
use gpui::{App, Entity, Task};
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// A service for managing file buffers and editor workflows.
/// It caches open files to avoid redundant reads and ensures thread-safe access.
#[allow(dead_code)]
#[derive(Clone)]
pub struct EditorService {
    documents: Arc<RwLock<HashMap<PathBuf, Arc<RwLock<Document>>>>>,
}

#[allow(dead_code)]
impl EditorService {
    /// Creates a new, empty EditorService.
    pub fn new() -> Self {
        Self {
            documents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Opens a file asynchronously.
    /// If the file is already in the cache, it returns the cached document.
    /// Otherwise, it reads the file from disk, adds it to the cache, and returns it.
    /// This operation is thread-safe.
    pub async fn open_file(&self, path: PathBuf) -> anyhow::Result<Arc<RwLock<Document>>> {
        let path = path.canonicalize().unwrap_or(path);
        // First, check if the document is already in the cache with a read lock.
        if let Some(document) = self.documents.read().expect("documents read lock").get(&path) {
            return Ok(document.clone());
        }

        // If not in the cache, read the file using memory mapping without holding any lock.
        let path_clone = path.clone();
        let buffer = tokio::task::spawn_blocking(move || -> anyhow::Result<Buffer> {
            let file = std::fs::File::open(&path_clone)?;
            let mmap = unsafe { memmap2::MmapOptions::new().map(&file)? };
            Ok(Buffer::from_mmap(mmap))
        })
        .await??;
        let new_document = Arc::new(RwLock::new(Document::new(path.clone(), buffer)));

        // Acquire a write lock to insert the new document into the cache.
        let mut documents = self.documents.write().expect("documents write lock");

        // Before inserting, check again if another thread has inserted it in the meantime.
        if let Some(document) = documents.get(&path) {
            return Ok(document.clone());
        }

        documents.insert(path, new_document.clone());
        Ok(new_document)
    }

    /// Closes a file by removing it from the document cache.
    pub fn close_file(&self, path: &std::path::Path) {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let mut documents = self.documents.write().expect("documents write lock");
        documents.remove(&path);
    }

    /// Searches for a query in the given buffer based on the search options.
    /// Returns a Task that executes the search in the background.
    pub fn search(&self, buffer: Arc<Buffer>, query: String, options: crate::core::search::SearchOptions, cx: &gpui::App) -> gpui::Task<Vec<usize>> {
        cx.background_executor().spawn(async move {
            if query.is_empty() {
                return Vec::new();
            }

            match options.mode {
                crate::core::search::SearchMode::Text => {
                    let pattern: Vec<crate::core::search::PatternByte> =
                        query.as_bytes().iter().map(|&b| crate::core::search::PatternByte::new_exact(b)).collect();
                    search::find_occurrences(buffer.data(), &pattern, options.limit, options.range.clone())
                }
                crate::core::search::SearchMode::Hex => {
                    if let Some(pattern) = crate::core::search::parse_hex_pattern(&query) {
                        search::find_occurrences(buffer.data(), &pattern, options.limit, options.range.clone())
                    } else {
                        Vec::new()
                    }
                }
            }
        })
    }

    /// Performs a search and updates the provided Editor entity with the results.
    pub fn perform_search(
        &self,
        editor: gpui::Entity<crate::core::editor::Editor>,
        query: String,
        options: crate::core::search::SearchOptions,
        generation: usize,
        is_full: bool,
        cx: &gpui::App,
    ) -> gpui::Task<()> {
        let buffer_data = {
            let editor_read = editor.read(cx);
            let document = editor_read.document.read().expect("document read lock");
            // Since `Buffer` cloning is O(1) (internally uses Arc<Vec<u8>> or Arc<Mmap>),
            // cloning the buffer here is extremely cheap and creates a consistent snapshot
            // for the background search thread.
            Arc::new(document.buffer.clone())
        };

        let search_task = self.search(buffer_data, query, options, cx);
        let editor_weak = editor.downgrade();

        cx.spawn(move |cx: &mut gpui::AsyncApp| {
            let mut cx = cx.clone();
            async move {
                let results = search_task.await;
                if let Some(editor) = editor_weak.upgrade() {
                    editor
                        .update(&mut cx, |editor, cx| {
                            editor.set_search_results(results, generation, is_full);
                            cx.notify();
                        })
                        .ok();
                }
            }
        })
    }

    /// Performs an incremental search: immediate viewport search followed by background full search.
    pub fn incremental_search(
        &self,
        editor: Entity<Editor>,
        query: String,
        mode: crate::core::search::SearchMode,
        viewport_range: Range<usize>,
        cx: &App,
    ) -> (Task<()>, Task<()>) {
        // Read the generation ID on the main thread from editor
        let generation = editor.read(cx).search_state.generation;

        // 1. Immediate viewport search
        let viewport_options = SearchOptions {
            mode,
            limit: crate::core::search::SearchLimit::Unlimited,
            range: Some(viewport_range),
        };
        let viewport_task = self.perform_search(editor.clone(), query.clone(), viewport_options, generation, false, cx);

        // 2. Background full search
        let full_options = SearchOptions {
            mode,
            limit: crate::core::search::SearchLimit::Unlimited,
            range: None,
        };
        let full_task = self.perform_search(editor, query, full_options, generation, true, cx);

        (viewport_task, full_task)
    }

    pub fn compute_diff(&self, left: Arc<RwLock<Document>>, right: Arc<RwLock<Document>>, cx: &gpui::App) -> gpui::Task<crate::core::diff::DiffResult> {
        cx.background_executor().spawn(async move {
            let left_doc = left.read().expect("left document read lock");
            let right_doc = right.read().expect("right document read lock");
            let left_data = left_doc.buffer.data();
            let right_data = right_doc.buffer.data();
            crate::core::diff::compute_simple_diff(left_data, right_data)
        })
    }
}
impl Default for EditorService {
    fn default() -> Self {
        Self::new()
    }
}
