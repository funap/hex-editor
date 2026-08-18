use std::path::PathBuf;

/// The number of structure definitions kept in the recent history.
pub const MAX_RECENT_DEFINITIONS: usize = 5;
/// The number of binary files kept in the recent history.
pub const MAX_RECENT_FILES: usize = MAX_RECENT_DEFINITIONS;

/// Stores paths in most-recently-used order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecentPathHistory {
    paths: Vec<PathBuf>,
}

impl RecentPathHistory {
    /// Creates a history from paths ordered newest to oldest.
    pub fn from_paths(paths: Vec<PathBuf>) -> Self {
        let mut history = Self::default();
        for path in paths.into_iter().rev() {
            history.record(path);
        }
        history
    }

    /// Records a path, moving an existing entry to the front.
    pub fn record(&mut self, path: PathBuf) {
        let path = canonicalize_or_keep(path);
        self.paths.retain(|entry| entry != &path);
        self.paths.insert(0, path);
        self.paths.truncate(MAX_RECENT_FILES);
    }

    /// Removes a path and reports whether an entry was removed.
    pub fn remove(&mut self, path: &std::path::Path) -> bool {
        let path = canonicalize_or_keep(path.to_path_buf());
        let old_len = self.paths.len();
        self.paths.retain(|entry| entry != &path);
        self.paths.len() != old_len
    }

    /// Returns paths from newest to oldest.
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
}

/// Stores the most recently used Kaitai structure definition paths.
pub type DefinitionHistory = RecentPathHistory;

/// Stores the most recently opened binary file paths.
pub type FileHistory = RecentPathHistory;

fn canonicalize_or_keep(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_five_unique_paths_in_most_recent_order() {
        let mut history = DefinitionHistory::default();
        for index in 0..=MAX_RECENT_DEFINITIONS {
            history.record(PathBuf::from(format!("definition-{index}.ksy")));
        }

        assert_eq!(history.paths().len(), MAX_RECENT_DEFINITIONS);
        assert_eq!(history.paths().first(), Some(&PathBuf::from("definition-5.ksy")));
        assert!(!history.paths().contains(&PathBuf::from("definition-0.ksy")));
    }

    #[test]
    fn recording_an_existing_path_moves_it_to_the_front() {
        let mut history = DefinitionHistory::default();
        history.record(PathBuf::from("older.ksy"));
        history.record(PathBuf::from("newer.ksy"));
        history.record(PathBuf::from("older.ksy"));

        assert_eq!(history.paths(), &[PathBuf::from("older.ksy"), PathBuf::from("newer.ksy")]);
    }

    #[test]
    fn removes_a_path_from_history() {
        let mut history = FileHistory::from_paths(vec![PathBuf::from("new.bin"), PathBuf::from("old.bin")]);

        assert!(history.remove(std::path::Path::new("new.bin")));
        assert_eq!(history.paths(), &[PathBuf::from("old.bin")]);
        assert!(!history.remove(std::path::Path::new("missing.bin")));
    }
}
