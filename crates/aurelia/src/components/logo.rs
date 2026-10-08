use gpui_kit::prelude::*;
use gpui_kit::{Pixels, div, linear_color_stop, linear_gradient, px, rgb, rgba};

/// The Aurelia mark: a glowing moon-jellyfish bell.
pub fn mark(size: Pixels) -> impl IntoElement {
    div()
        .size(size)
        .rounded_full()
        .p(size * 0.18)
        .bg(rgba(0xB69CFF33))
        .border_1()
        .border_color(rgba(0xB69CFF66))
        .child(div().size_full().rounded_full().bg(linear_gradient(
            150.,
            linear_color_stop(rgb(0xE6DCFF), 0.),
            linear_color_stop(rgb(0x8D6BFF), 1.),
        )))
}

pub fn wordmark(size: Pixels) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(size * 0.35)
        .child(mark(size))
        .child(
            div()
                .font_family(crate::theme::FONT_DISPLAY)
                .font_weight(gpui_kit::FontWeight::BOLD)
                .text_size(size * 0.62)
                .child("Aurelia"),
        )
}

pub fn small() -> impl IntoElement {
    wordmark(px(22.))
}
