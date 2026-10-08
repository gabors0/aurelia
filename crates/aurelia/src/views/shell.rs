//! The signed-in window: navigation bar, pages, shortcuts.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{TitleBar, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Context, EventEmitter, Window, div, px};

use crate::components::logo;
use crate::state::AppState;
use crate::theme::Palette;

pub enum ShellEvent {
    SignOut,
    SessionExpired,
}

pub struct Shell;

impl EventEmitter<ShellEvent> for Shell {}

impl Shell {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let user = AppState::global(cx)
            .session()
            .map(|s| s.user_name.clone())
            .unwrap_or_default();
        v_flex()
            .size_full()
            .child(
                TitleBar::new().h(px(52.)).child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .pr_4()
                        .child(logo::small())
                        .child(
                            Button::new("sign-out")
                                .ghost()
                                .label(format!("Sign out {user}"))
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(ShellEvent::SignOut))),
                        ),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .text_color(Palette::text_secondary())
                    .p_8()
                    .child(format!("{} libraries", AppState::global(cx).views().len())),
            )
    }
}
