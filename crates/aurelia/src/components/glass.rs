use gpui_kit::prelude::*;
use gpui_kit::{BoxShadow, Div, div, hsla, point, px};

use crate::theme::Palette;

/// Translucent panel with a hairline border, floating over artwork.
pub fn glass() -> Div {
    div()
        .bg(Palette::glass())
        .border_1()
        .border_color(Palette::border())
        .rounded(px(18.))
        .shadow(vec![BoxShadow {
            color: hsla(0., 0., 0., 0.45),
            offset: point(px(0.), px(24.)),
            blur_radius: px(64.),
            spread_radius: px(-12.),
            inset: false,
        }])
}
