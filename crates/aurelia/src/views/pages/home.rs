use gpui_kit::prelude::*;
use gpui_kit::{Context, Window, div};

use crate::theme::Palette;

pub struct HomePage;

impl HomePage {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }

    pub fn refresh(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}
}

impl Render for HomePage {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .pt_20()
            .px_12()
            .text_color(Palette::text_secondary())
            .child("Home")
    }
}
