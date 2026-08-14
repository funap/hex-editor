use crate::ui::icon::IconName;
use gpui::{Context, Entity, EventEmitter, IntoElement, ParentElement, Render, Styled, Window, div};
use gpui_component::button::ButtonVariants;
use gpui_component::{TitleBar, button::Button, menu::AppMenuBar};

pub enum AppTitleBarEvent {
    OpenSettings,
}

pub struct AppTitleBar {
    pub app_menu_bar: Entity<AppMenuBar>,
}

impl EventEmitter<AppTitleBarEvent> for AppTitleBar {}

impl Render for AppTitleBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        TitleBar::new().child(div().flex().items_center().child(self.app_menu_bar.clone())).child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap_2()
                .child(Button::new("settings").ghost().icon(IconName::Settings).on_click(cx.listener(|_, _, _, cx| {
                    cx.emit(AppTitleBarEvent::OpenSettings);
                })))
                .child(Button::new("help").ghost().icon(IconName::Info)),
        )
    }
}
