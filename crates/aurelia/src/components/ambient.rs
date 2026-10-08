//! Artwork-tinted light behind the content that follows a hero. It starts at
//! the page colour (matching the hero's bottom fade) and lets the blurred art
//! glow through further down, so there's no seam.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Div, ElementId, div, linear_color_stop, linear_gradient, px, rgba};

use crate::components::art::Art;
use crate::images::ImageRequest;
use crate::theme::Palette;

pub fn section(
    id: impl Into<ElementId>,
    request: Option<ImageRequest>,
    content: Vec<AnyElement>,
) -> Div {
    div()
        .relative()
        .child(
            div()
                .absolute()
                .inset_0()
                .child(Art::new(id, request).bare().size_full().opacity(0.8))
                .child(div().absolute().inset_0().bg(rgba(0x0A0B1080)))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(320.))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(Palette::bg(), 0.),
                            linear_color_stop(rgba(0x0A0B1000), 1.),
                        )),
                )
                .child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(px(240.))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(rgba(0x0A0B1000), 0.),
                            linear_color_stop(Palette::bg(), 1.),
                        )),
                ),
        )
        .child(div().relative().children(content))
}
