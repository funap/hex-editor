use crate::service::editor_service::EditorService;
use gpui::{App, BorrowAppContext, Global};

/// Application-wide editing mode shared by every open document view.
#[derive(Clone, Copy, Debug, Default)]
pub struct InsertModeState {
    pub enabled: bool,
}

impl Global for InsertModeState {}

impl InsertModeState {
    /// Returns whether the application is currently in Insert Mode.
    pub fn is_enabled(cx: &App) -> bool {
        cx.global::<Self>().enabled
    }

    /// Toggles Insert Mode and returns its new state.
    pub fn toggle(cx: &mut App) -> bool {
        let mut enabled = false;
        cx.update_global::<Self, _>(|state, _| {
            state.enabled = !state.enabled;
            enabled = state.enabled;
        });
        enabled
    }
}

#[allow(dead_code)]
#[derive(Clone)]
pub struct AppState {
    pub editor_service: EditorService,
}

impl Global for AppState {}

impl AppState {
    pub fn init(cx: &mut App) {
        let state = Self {
            editor_service: EditorService::new(),
        };
        cx.set_global::<AppState>(state);
        cx.set_global(InsertModeState::default());
    }

    pub fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    #[allow(dead_code)]
    pub fn global_mut(cx: &mut App) -> &mut Self {
        cx.global_mut::<Self>()
    }
}
