//! Artwork-tinted light behind the content that follows a hero. It starts at
//! the page colour (matching the hero's bottom fade) and lets the blurred art
//! glow through further down, so there's no seam.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Div, ElementId, div, linear_color_stop, linear_gradient, px};

use crate::components::art::Art;
use crate::images::ImageRequest;
use crate::theme::Palette;

pub fn section(
    id: impl Into<ElementId>,
    request: Option<ImageRequest>,
    content: Vec<AnyElement>,
) -> Div {
    layered(vec![(id.into(), request)], content)
}

/// A section lit by several stacked artworks, bottom first: Home keeps the
/// outgoing slide's light under the incoming one so the change crossfades.
pub fn layered(layers: Vec<(ElementId, Option<ImageRequest>)>, content: Vec<AnyElement>) -> Div {
    div()
        .relative()
        .child(
            div()
                .absolute()
                .inset_0()
                .children(layers.into_iter().map(|(id, request)| {
                    div().absolute().inset_0().child(
                        Art::new(id, request)
                            .bare()
                            .size_full()
                            .opacity(Palette::ambient_opacity()),
                    )
                }))
                .child(div().absolute().inset_0().bg(Palette::ambient_veil()))
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
                            linear_color_stop(Palette::bg_alpha(0.), 1.),
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
                            linear_color_stop(Palette::bg_alpha(0.), 0.),
                            linear_color_stop(Palette::bg(), 1.),
                        )),
                ),
        )
        .child(div().relative().children(content))
}
