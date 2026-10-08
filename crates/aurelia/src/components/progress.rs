use gpui_kit::prelude::*;
use gpui_kit::{Div, Hsla, div, hsla, px, relative};

/// Thin watched-progress bar.
pub fn bar(fraction: f32, accent: Hsla) -> Div {
    div()
        .h(px(3.))
        .w_full()
        .rounded_full()
        .bg(hsla(0., 0., 1., 0.22))
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(accent)
                .w(relative(fraction.clamp(0.02, 1.0))),
        )
}
