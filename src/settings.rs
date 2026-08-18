use crate::core::appearance::Appearance;
use crate::core::encoding::Encoding;
use crate::core::structure::{DefinitionHistory, FileHistory};
use gpui::App;
use gpui_component::theme::{Theme, ThemeMode};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

const APPLICATION_CONFIG_DIRECTORY: &str = "xvw";
const SETTINGS_FILE_NAME: &str = "settings.toml";

static SAVE_GENERATION: AtomicU64 = AtomicU64::new(0);
static SAVE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// User-configurable application settings persisted between launches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub appearance: Appearance,
    pub theme_mode: ThemeMode,
    pub default_encoding: Encoding,
    pub recent_definition_paths: Vec<PathBuf>,
    pub recent_file_paths: Vec<PathBuf>,
}

/// Application-wide recent-path histories shared by all workspace windows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecentHistoryState {
    pub definitions: DefinitionHistory,
    pub files: FileHistory,
}

impl gpui::Global for RecentHistoryState {}

impl RecentHistoryState {
    /// Creates the in-memory histories from persisted settings.
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            definitions: DefinitionHistory::from_paths(settings.recent_definition_paths.clone()),
            files: FileHistory::from_paths(settings.recent_file_paths.clone()),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: Appearance::default(),
            theme_mode: ThemeMode::Light,
            default_encoding: Encoding::default(),
            recent_definition_paths: Vec::new(),
            recent_file_paths: Vec::new(),
        }
    }
}

impl Settings {
    /// Loads settings from the platform configuration directory.
    ///
    /// Missing or invalid files are ignored and replaced with the defaults so
    /// a damaged preferences file cannot prevent the application from starting.
    pub fn load() -> Self {
        let Some(path) = settings_path() else {
            return Self::default();
        };

        match Self::load_from(&path) {
            Ok(settings) => settings,
            Err(SettingsError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                eprintln!("Failed to load settings from {}: {error}", path.display());
                Self::default()
            }
        }
    }

    /// Loads and validates settings from an explicit file path.
    pub fn load_from(path: &Path) -> Result<Self, SettingsError> {
        let contents = fs::read_to_string(path)?;
        let settings: Self = toml::from_str(&contents)?;
        Ok(settings.sanitized())
    }

    /// Creates a settings snapshot from the application's current globals.
    pub fn from_app(cx: &App) -> Self {
        let recent_history = cx.global::<RecentHistoryState>();
        Self {
            appearance: cx.global::<Appearance>().clone(),
            theme_mode: Theme::global(cx).mode,
            default_encoding: *cx.global::<Encoding>(),
            recent_definition_paths: recent_history.definitions.paths().to_vec(),
            recent_file_paths: recent_history.files.paths().to_vec(),
        }
    }

    /// Saves settings to an explicit file path.
    pub fn save_to(&self, path: &Path) -> Result<(), SettingsError> {
        let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty());
        if let Some(parent) = parent {
            fs::create_dir_all(parent)?;
        }

        let contents = toml::to_string_pretty(&self.clone().sanitized())?;
        fs::write(path, contents)?;
        Ok(())
    }

    /// Schedules a background save of these settings.
    ///
    /// Saving is serialized and superseded snapshots are skipped, so typing in
    /// a settings input cannot leave an older value on disk after a newer save.
    pub fn save_async(&self, cx: &App) {
        let Some(path) = settings_path() else {
            return;
        };

        let generation = SAVE_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
        let settings = self.clone();
        cx.background_executor()
            .spawn(async move {
                let lock = SAVE_LOCK.get_or_init(|| Mutex::new(()));
                let _guard = lock.lock().expect("settings save lock");
                if SAVE_GENERATION.load(Ordering::Acquire) != generation {
                    return;
                }

                if let Err(error) = settings.save_to(&path) {
                    eprintln!("Failed to save settings to {}: {error}", path.display());
                }
            })
            .detach();
    }

    fn sanitized(mut self) -> Self {
        let defaults = Appearance::default();
        if self.appearance.font_family.trim().is_empty() {
            self.appearance.font_family = defaults.font_family;
        }
        if !self.appearance.font_size.is_finite() || self.appearance.font_size <= 0.0 {
            self.appearance.font_size = defaults.font_size;
        }

        self.recent_definition_paths = DefinitionHistory::from_paths(self.recent_definition_paths).paths().to_vec();
        self.recent_file_paths = FileHistory::from_paths(self.recent_file_paths).paths().to_vec();
        self
    }
}

/// Returns the path used to persist user settings on this platform.
pub fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|directory| directory.join(APPLICATION_CONFIG_DIRECTORY).join(SETTINGS_FILE_NAME))
}

/// Saves the current application settings in the background.
pub fn save_current(cx: &App) {
    Settings::from_app(cx).save_async(cx);
}

/// Registers a final synchronous save so the latest edit is retained when the
/// application quits before a background save has completed.
pub fn register_quit_handler(cx: &App) {
    cx.on_app_quit(|cx| {
        let settings = Settings::from_app(cx);
        async move {
            let Some(path) = settings_path() else {
                return;
            };

            let lock = SAVE_LOCK.get_or_init(|| Mutex::new(()));
            let _guard = lock.lock().expect("settings save lock");
            if let Err(error) = settings.save_to(&path) {
                eprintln!("Failed to save settings to {}: {error}", path.display());
            }
        }
    })
    .detach();
}

/// Errors returned while reading or writing the settings file.
#[derive(Debug)]
pub enum SettingsError {
    Io(std::io::Error),
    Deserialize(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl Display for SettingsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Deserialize(error) => write!(formatter, "invalid TOML: {error}"),
            Self::Serialize(error) => write!(formatter, "serialization error: {error}"),
        }
    }
}

impl Error for SettingsError {}

impl From<std::io::Error> for SettingsError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<toml::de::Error> for SettingsError {
    fn from(error: toml::de::Error) -> Self {
        Self::Deserialize(error)
    }
}

impl From<toml::ser::Error> for SettingsError {
    fn from(error: toml::ser::Error) -> Self {
        Self::Serialize(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestSettingsFile {
        path: PathBuf,
    }

    impl TestSettingsFile {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock must be after the Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("xvw-settings-{label}-{}-{nonce}.toml", std::process::id()));
            Self { path }
        }
    }

    impl Drop for TestSettingsFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    #[test]
    fn settings_round_trip() {
        let file = TestSettingsFile::new("round-trip");
        let settings = Settings {
            appearance: Appearance {
                font_family: "Fira Code".into(),
                font_size: 18.0,
            },
            theme_mode: ThemeMode::Dark,
            default_encoding: Encoding::Utf16Le,
            recent_definition_paths: vec![PathBuf::from("definition.ksy")],
            recent_file_paths: vec![PathBuf::from("binary.bin")],
        };

        settings.save_to(&file.path).expect("save settings");

        assert_eq!(Settings::load_from(&file.path).expect("load settings"), settings);
    }

    #[test]
    fn missing_values_use_defaults() {
        let file = TestSettingsFile::new("defaults");
        fs::write(&file.path, "[appearance]\nfont_family = \"Fira Code\"\n").expect("write settings");

        let settings = Settings::load_from(&file.path).expect("load settings");

        assert_eq!(settings.appearance.font_family, "Fira Code");
        assert_eq!(settings.appearance.font_size, Appearance::default().font_size);
        assert_eq!(settings.theme_mode, ThemeMode::Light);
        assert_eq!(settings.default_encoding, Encoding::default());
    }

    #[test]
    fn invalid_appearance_values_are_replaced_with_defaults() {
        let file = TestSettingsFile::new("sanitize");
        fs::write(&file.path, "[appearance]\nfont_family = \"\"\nfont_size = 0.0\n").expect("write settings");

        let settings = Settings::load_from(&file.path).expect("load settings");

        assert_eq!(settings.appearance, Appearance::default());
    }
}
