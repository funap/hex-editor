use crate::core::goto::{GotoParseError, GotoRadix, ParsedGotoOffset, parse_goto_offset};
use crate::ui::icon::IconName;
use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon,
    button::{Button, ButtonVariants},
    input::{self, Input, InputState},
};

#[derive(Clone, PartialEq, Action)]
pub struct GotoJump;

#[derive(Clone, PartialEq, Action)]
pub struct GotoJumpExtend;

#[derive(Clone, PartialEq, Action)]
pub struct GotoDismiss;

pub enum GotoBarEvent {
    Jump { offset: usize, extend_selection: bool },
    Dismiss,
}

pub struct GotoOffsetBar {
    input: Entity<InputState>,
    radix: GotoRadix,
    current_cursor: usize,
    total_size: usize,
    parsed_result: Option<Result<ParsedGotoOffset, GotoParseError>>,
}

impl EventEmitter<GotoBarEvent> for GotoOffsetBar {}

impl GotoOffsetBar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Address (e.g. 0x100, 256, +0x20, 50%)..."));

        // Subscribe to input changes
        cx.subscribe(&input, |this, input, event: &input::InputEvent, cx| {
            if let input::InputEvent::Change = event {
                let query = input.read(cx).value().to_string();
                this.update_parsed_result(&query, cx);
            }
        })
        .detach();

        Self {
            input,
            radix: GotoRadix::Hex,
            current_cursor: 0,
            total_size: 0,
            parsed_result: None,
        }
    }

    pub fn set_context_info(&mut self, current_cursor: usize, total_size: usize, cx: &mut Context<Self>) {
        self.current_cursor = current_cursor;
        self.total_size = total_size;
        let query = self.input.read(cx).value().to_string();
        self.update_parsed_result(&query, cx);
    }

    fn update_parsed_result(&mut self, query: &str, cx: &mut Context<Self>) {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            self.parsed_result = None;
        } else {
            self.parsed_result = Some(parse_goto_offset(trimmed, self.current_cursor, self.total_size, self.radix));
        }
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.focus(window, cx);
        });
    }

    pub fn set_radix(&mut self, radix: GotoRadix, cx: &mut Context<Self>) {
        self.radix = radix;
        let query = self.input.read(cx).value().to_string();
        self.update_parsed_result(&query, cx);
    }

    pub fn execute_jump(&mut self, extend_selection: bool, cx: &mut Context<Self>) {
        let query = self.input.read(cx).value().to_string();
        if let Ok(parsed) = parse_goto_offset(&query, self.current_cursor, self.total_size, self.radix) {
            cx.emit(GotoBarEvent::Jump {
                offset: parsed.target_offset,
                extend_selection,
            });
        }
    }
}

impl Render for GotoOffsetBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        let preview_info = match &self.parsed_result {
            None => {
                let text = format!("Pos: 0x{:X} / Size: 0x{:X}", self.current_cursor, self.total_size);
                div().text_sm().text_color(theme.muted_foreground).child(text)
            }
            Some(Ok(parsed)) => {
                let text = format!("Target: 0x{:X} ({} dec)", parsed.target_offset, parsed.target_offset);
                let warning = if parsed.is_out_of_bounds {
                    format!(" ⚠ (max 0x{:X})", self.total_size.saturating_sub(1))
                } else {
                    String::new()
                };
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_sm()
                    .text_color(if parsed.is_out_of_bounds { theme.yellow } else { theme.accent })
                    .child(text)
                    .when(!warning.is_empty(), |el| el.child(div().text_color(theme.yellow).child(warning)))
            }
            Some(Err(err)) => div().text_sm().text_color(theme.red).child(format!("{}", err)),
        };

        div()
            .flex()
            .items_center()
            .gap_2()
            .p_2()
            .bg(theme.background)
            .border_b_1()
            .border_color(theme.border)
            .key_context("GotoOffsetBar")
            .on_action(cx.listener(|this, _: &GotoJump, _, cx| {
                this.execute_jump(false, cx);
            }))
            .on_action(cx.listener(|this, _: &GotoJumpExtend, _, cx| {
                this.execute_jump(true, cx);
            }))
            .on_action(cx.listener(|_, _: &GotoDismiss, _, cx| {
                cx.emit(GotoBarEvent::Dismiss);
            }))
            .child(
                div()
                    .flex()
                    .child(
                        if self.radix == GotoRadix::Hex {
                            Button::new("hex_radix").label("Hex").primary()
                        } else {
                            Button::new("hex_radix").label("Hex").ghost()
                        }
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_radix(GotoRadix::Hex, cx);
                        })),
                    )
                    .child(
                        if self.radix == GotoRadix::Dec {
                            Button::new("dec_radix").label("Dec").primary()
                        } else {
                            Button::new("dec_radix").label("Dec").ghost()
                        }
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_radix(GotoRadix::Dec, cx);
                        })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .child(Input::new(&self.input).prefix(Icon::new(IconName::Hash).size_3p5()).cleanable(true)),
            )
            .child(preview_info)
            .child(Button::new("go_jump").label("Go").primary().on_click(cx.listener(|this, _, _, cx| {
                this.execute_jump(false, cx);
            })))
            .child(Button::new("close").ghost().icon(IconName::Close).on_click(cx.listener(|_, _, _, cx| {
                cx.emit(GotoBarEvent::Dismiss);
            })))
    }
}

pub fn init(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("enter", GotoJump, Some("GotoOffsetBar")),
        gpui::KeyBinding::new("shift-enter", GotoJumpExtend, Some("GotoOffsetBar")),
        gpui::KeyBinding::new("escape", GotoDismiss, Some("GotoOffsetBar")),
    ]);
}
