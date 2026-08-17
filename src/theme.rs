use gpui::{App, SharedString, Window};
use gpui_component::ThemeRegistry;
use gpui_component::theme::{Theme, ThemeMode};
use std::path::PathBuf;

const LIGHT_THEME_NAME: &str = "Ayu Light";
const DARK_THEME_NAME: &str = "Ayu Dark";

pub fn init(cx: &mut App) {
    // Load and watch themes from ./themes directory
    if let Err(err) = ThemeRegistry::watch_dir(PathBuf::from("./themes"), cx, move |cx| {
        let mode = Theme::global(cx).mode;
        apply_ayu_themes(mode, None, cx);
    }) {
        eprintln!("Failed to watch themes directory: {}", err);
    }
}

/// Changes the active theme mode while retaining the application's Ayu theme pair.
pub fn set_mode(mode: ThemeMode, window: Option<&mut Window>, cx: &mut App) {
    apply_ayu_themes(mode, window, cx);
}

fn apply_ayu_themes(mode: ThemeMode, window: Option<&mut Window>, cx: &mut App) {
    let light_name = SharedString::from(LIGHT_THEME_NAME);
    let dark_name = SharedString::from(DARK_THEME_NAME);
    let (light_theme, dark_theme) = {
        let registry = ThemeRegistry::global(cx);
        (registry.themes().get(&light_name).cloned(), registry.themes().get(&dark_name).cloned())
    };

    {
        let theme = Theme::global_mut(cx);
        if let Some(light_theme) = light_theme {
            theme.light_theme = light_theme;
        }
        if let Some(dark_theme) = dark_theme {
            theme.dark_theme = dark_theme;
        }
    }

    Theme::change(mode, window, cx);
    cx.refresh_windows();
}
