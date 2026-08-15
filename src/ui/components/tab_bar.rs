use crate::ui::icon::IconName;
use gpui::prelude::*;
use gpui::*;
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{ActiveTheme, Icon};
use std::path::PathBuf;

use crate::actions::{ActivateTab, CloseActivePanel};

#[allow(dead_code)]
pub struct TabItemInfo {
    pub id: usize,
    pub title: String,
    pub is_dirty: bool,
    pub is_active: bool,
    #[allow(dead_code)]
    pub path: Option<PathBuf>,
}

#[allow(dead_code)]
pub fn render_zed_tab_bar(tabs: &[TabItemInfo], _window: &mut Window, cx: &mut App) -> impl IntoElement {
    let theme = cx.theme();
    let tab_bar_bg = theme.tab_bar;

    div()
        .id("zed-tab-bar")
        .flex()
        .flex_row()
        .items_center()
        .w_full()
        .h(px(34.0))
        .bg(tab_bar_bg)
        .border_b_1()
        .border_color(theme.border)
        .overflow_x_scroll()
        .children(tabs.iter().enumerate().map(|(idx, tab)| {
            let tab_id = tab.id;
            let is_active = tab.is_active;
            let is_dirty = tab.is_dirty;
            let title = tab.title.clone();
            let one_based_index = idx + 1;

            div()
                .id(ElementId::NamedInteger("zed-tab".into(), tab_id as u64))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .h_full()
                .min_w(px(100.0))
                .max_w(px(220.0))
                .cursor_pointer()
                .border_r_1()
                .border_color(theme.border)
                .when(is_active, |s| {
                    s.bg(theme.background).text_color(theme.foreground).font_weight(gpui::FontWeight::MEDIUM)
                })
                .when(!is_active, |s| {
                    s.bg(tab_bar_bg)
                        .text_color(theme.muted_foreground)
                        .hover(|style| style.bg(theme.accent.opacity(0.12)))
                })
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    window.dispatch_action(Box::new(ActivateTab { index: one_based_index }), cx);
                })
                .on_mouse_down(MouseButton::Middle, move |_, window, cx| {
                    window.dispatch_action(Box::new(CloseActivePanel), cx);
                })
                .context_menu({
                    let can_close_others = tabs.len() > 1;
                    let can_close_right = idx + 1 < tabs.len();
                    let tab_path = tab.path.clone();
                    move |menu, _window, _cx| {
                        let mut menu = menu
                            .menu_with_icon("Close", IconName::Close, Box::new(crate::actions::CloseActivePanel))
                            .menu_with_icon_and_disabled("Close Others", IconName::Close, Box::new(crate::actions::CloseOtherTabs), !can_close_others)
                            .menu_with_icon_and_disabled(
                                "Close to the Right",
                                IconName::ChevronRight,
                                Box::new(crate::actions::CloseTabsToRight),
                                !can_close_right,
                            )
                            .menu("Close All", Box::new(crate::actions::CloseAllTabs))
                            .separator()
                            .menu_with_icon("Split Right", IconName::PanelRight, Box::new(crate::actions::SplitRight))
                            .menu_with_icon("Split Down", IconName::PanelBottom, Box::new(crate::actions::SplitDown));

                        if tab_path.is_some() {
                            menu = menu
                                .separator()
                                .menu("Copy Path", Box::new(crate::actions::CopyPath))
                                .menu("Copy File Name", Box::new(crate::actions::CopyFileName))
                                .menu("Reveal in File Explorer", Box::new(crate::actions::RevealInExplorer));
                        }
                        menu
                    }
                })
                .child(
                    Icon::new(IconName::File)
                        .size(px(14.0))
                        .text_color(if is_active { theme.accent } else { theme.muted_foreground }),
                )
                .child(div().flex_1().truncate().text_sm().child(title))
                .child(
                    div()
                        .id(ElementId::NamedInteger("tab-close-area".into(), tab_id as u64))
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(18.0))
                        .h(px(18.0))
                        .rounded_sm()
                        .hover(|style| style.bg(theme.accent.opacity(0.2)).text_color(theme.accent_foreground))
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            window.dispatch_action(Box::new(ActivateTab { index: one_based_index }), cx);
                            window.dispatch_action(Box::new(CloseActivePanel), cx);
                        })
                        .child(if is_dirty && !is_active {
                            div().w(px(6.0)).h(px(6.0)).rounded_full().bg(theme.accent).into_any_element()
                        } else {
                            Icon::new(IconName::Close).size(px(12.0)).text_color(theme.muted_foreground).into_any_element()
                        }),
                )
        }))
}
