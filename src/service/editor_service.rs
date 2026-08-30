use crate::core::buffer::Buffer;
use crate::core::document::Document;
use crate::core::editor::Editor;
use crate::core::search::{self, SearchOptions};
use gpui::{App, Entity, EntityId, Task, WeakEntity};
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// A service for managing file buffers and editor workflows.
/// It caches open files to avoid redundant reads and ensures thread-safe access.
#[allow(dead_code)]
#[derive(Clone)]
pub struct EditorService {
    documents: Arc<RwLock<HashMap<PathBuf, Arc<RwLock<Document>>>>>,
    editors: Arc<RwLock<HashMap<PathBuf, Vec<WeakEntity<Editor>>>>>,
}

#[allow(dead_code)]
impl EditorService {
    /// Creates a new, empty EditorService.
    pub fn new() -> Self {
        Self {
            documents: Arc::new(RwLock::new(HashMap::new())),
            editors: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Registers an editor weak entity for a given document path to receive live change notifications.
    pub fn register_editor(&self, path: PathBuf, editor: WeakEntity<Editor>) {
        let path = path.canonicalize().unwrap_or(path);
        let mut editors = self.editors.write().expect("editors write lock");
        let list = editors.entry(path).or_default();
        list.retain(|w| w.upgrade().is_some());
        list.push(editor);
    }

    /// Releases one editor's ownership of a cached document.
    ///
    /// The cache remains available while another editor still references the
    /// same path. Once the last editor is released, the document cache entry
    /// is evicted so a future open starts with a fresh document state.
    pub fn release_editor(&self, path: &Path, editor_id: EntityId) {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let should_evict = {
            let mut editors = self.editors.write().expect("editors write lock");
            let remove_entry = if let Some(list) = editors.get_mut(&path) {
                list.retain(|editor| editor.entity_id() != editor_id && editor.upgrade().is_some());
                list.is_empty()
            } else {
                true
            };
            if remove_entry {
                editors.remove(&path);
            }
            remove_entry
        };

        if should_evict {
            self.documents.write().expect("documents write lock").remove(&path);
        }
    }

    /// Notifies all active editors viewing the document at `path` to invalidate layout and repaint.
    pub fn notify_document_changed(&self, path: &Path, cx: &mut App) {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let editors_to_notify: Vec<Entity<Editor>> = {
            let mut editors = self.editors.write().expect("editors write lock");
            if let Some(list) = editors.get_mut(&path) {
                list.retain(|w| w.upgrade().is_some());
                list.iter().filter_map(|w| w.upgrade()).collect()
            } else {
                Vec::new()
            }
        };

        for editor in editors_to_notify {
            editor.update(cx, |ed, cx| {
                ed.invalidate_line_map();
                cx.notify();
            });
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
        let (buffer, address_map) = tokio::task::spawn_blocking(move || -> anyhow::Result<(Buffer, crate::core::hex_import::AddressMap)> {
            let is_likely_hex_mot = path_clone
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| {
                    let e = ext.to_ascii_lowercase();
                    matches!(e.as_str(), "mot" | "srec" | "s19" | "s28" | "s37" | "s" | "hex" | "ihex" | "ihx")
                })
                .unwrap_or(false);

            if is_likely_hex_mot
                && let Ok(content) = std::fs::read_to_string(&path_clone)
                && let Ok(import_result) = crate::core::hex_import::parse_hex_or_mot(&content)
            {
                let buf = Buffer::new(import_result.data);
                return Ok((buf, import_result.address_map));
            }

            let file = std::fs::File::open(&path_clone)?;
            // SAFETY: Memory mapping the opened file is safe as long as the file is not
            // concurrently truncated or modified outside this process. Buffer encapsulates
            // read-only access to this memory mapping.
            let mmap = unsafe { memmap2::MmapOptions::new().map(&file)? };
            Ok((Buffer::from_mmap(mmap), crate::core::hex_import::AddressMap::default()))
        })
        .await??;
        let mut doc = Document::new_read_only(path.clone(), buffer);
        doc = doc.with_address_map(address_map);
        let new_document = Arc::new(RwLock::new(doc));

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

    /// Writes the current document snapshot to its path on a background
    /// executor. The document lock is released before any filesystem await.
    pub fn save_document(&self, document: Arc<RwLock<Document>>, cx: &App) -> Task<anyhow::Result<()>> {
        let (path, contents, address_map) = {
            let document = document.read().expect("document read lock");
            if document.is_read_only() {
                return cx.background_executor().spawn(async { Err(anyhow::anyhow!("document is read-only")) });
            }
            (document.path().to_path_buf(), document.buffer.data().to_vec(), document.address_map.clone())
        };
        cx.background_executor().spawn(async move {
            let bytes_to_write = if crate::core::hex_import::is_mot_extension(&path) {
                crate::core::hex_import::export_motorola_srec(&contents, &address_map).into_bytes()
            } else if crate::core::hex_import::is_hex_extension(&path) {
                crate::core::hex_import::export_intel_hex(&contents, &address_map).into_bytes()
            } else {
                crate::core::hex_import::export_raw_binary(&contents, &address_map, 0x00)
            };
            std::fs::write(path, bytes_to_write)?;
            Ok(())
        })
    }

    /// Writes a document snapshot to an explicit path on a background
    /// executor. This is used by Save As workflows.
    pub fn save_document_to_path(&self, document: Arc<RwLock<Document>>, path: PathBuf, cx: &App) -> Task<anyhow::Result<()>> {
        let (contents, address_map) = {
            let document = document.read().expect("document read lock");
            (document.buffer.data().to_vec(), document.address_map.clone())
        };
        cx.background_executor().spawn(async move {
            let bytes_to_write = if crate::core::hex_import::is_mot_extension(&path) {
                crate::core::hex_import::export_motorola_srec(&contents, &address_map).into_bytes()
            } else if crate::core::hex_import::is_hex_extension(&path) {
                crate::core::hex_import::export_intel_hex(&contents, &address_map).into_bytes()
            } else {
                crate::core::hex_import::export_raw_binary(&contents, &address_map, 0x00)
            };
            std::fs::write(path, bytes_to_write)?;
            Ok(())
        })
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
                    if let Some(pattern) = crate::core::search::parse_text_pattern(&query, options.encoding) {
                        search::find_occurrences(buffer.data(), &pattern, options.limit, options.range.clone())
                    } else {
                        Vec::new()
                    }
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
        // Read the generation ID and encoding on the main thread from editor
        let (generation, encoding) = {
            let ed = editor.read(cx);
            (ed.search_state.generation, ed.encoding)
        };

        // 1. Immediate viewport search
        let viewport_options = SearchOptions {
            mode,
            encoding,
            limit: crate::core::search::SearchLimit::Unlimited,
            range: Some(viewport_range),
        };
        let viewport_task = self.perform_search(editor.clone(), query.clone(), viewport_options, generation, false, cx);

        // 2. Background full search
        let full_options = SearchOptions {
            mode,
            encoding,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestFile(PathBuf);

    impl TestFile {
        fn create(label: &str, contents: &[u8]) -> Self {
            let path = temporary_path(label);
            std::fs::write(&path, contents).expect("write test file");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn temporary_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("xvw-editor-service-{label}-{}-{nonce}.bin", std::process::id()))
    }

    #[test]
    fn test_editor_service_register_empty() {
        let service = EditorService::new();
        let path = PathBuf::from("test_doc.bin");
        assert!(service.editors.read().unwrap().get(&path).is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn open_file_caches_and_reopens_documents() {
        let file = TestFile::create("cache", &[0x10, 0x20, 0x30]);

        let service = EditorService::new();
        let first = service.open_file(file.path().to_path_buf()).await.expect("open test file");
        assert_eq!(first.read().unwrap().buffer.data(), &[0x10, 0x20, 0x30]);
        assert!(first.read().unwrap().is_read_only());

        let second = service.open_file(file.path().to_path_buf()).await.expect("open cached file");
        assert!(Arc::ptr_eq(&first, &second), "opening a cached path must reuse its document");

        service.close_file(file.path());
        let reopened = service.open_file(file.path().to_path_buf()).await.expect("reopen test file");
        assert!(!Arc::ptr_eq(&first, &reopened), "closing a file must evict its cached document");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn open_file_returns_an_error_for_missing_path() {
        let path = temporary_path("missing");
        let service = EditorService::new();

        let result = service.open_file(path).await;

        assert!(result.is_err());
    }

    #[test]
    fn test_save_document_with_address_map_to_bin_and_mot() {
        let data = vec![0x11, 0x22, 0x33, 0x44];
        let map = crate::core::hex_import::AddressMap::from_segments(vec![
            crate::core::hex_import::MemorySegment {
                buffer_offset: 0,
                address: 4,
                length: 2,
            },
            crate::core::hex_import::MemorySegment {
                buffer_offset: 2,
                address: 8,
                length: 2,
            },
        ]);

        let doc = Document::new(PathBuf::from("test.mot"), crate::core::buffer::Buffer::new(data)).with_address_map(map);

        let bin_path = PathBuf::from("output.bin");
        let mot_path = PathBuf::from("output.mot");

        assert!(!crate::core::hex_import::is_mot_extension(&bin_path));
        assert!(crate::core::hex_import::is_mot_extension(&mot_path));

        let bin_bytes = crate::core::hex_import::export_raw_binary(doc.buffer.data(), &doc.address_map, 0x00);
        assert_eq!(bin_bytes.len(), 10);
        assert_eq!(bin_bytes, vec![0, 0, 0, 0, 0x11, 0x22, 0, 0, 0x33, 0x44]);

        let mot_string = crate::core::hex_import::export_motorola_srec(doc.buffer.data(), &doc.address_map);
        assert!(mot_string.contains("S1") || mot_string.contains("S2") || mot_string.contains("S3"));
    }
}
