//! The Aurelia mark: a moon jellyfish (*Aurelia aurita*) seen from above —
//! a glowing bell with its four horseshoe-shaped gonads.

use std::sync::{Arc, LazyLock};

use gpui_kit::prelude::*;
use gpui_kit::{Image, ImageFormat, ImageSource, ObjectFit, Pixels, div, img, px};

static MARK: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Svg,
        include_bytes!("../../assets/logo.svg").to_vec(),
    ))
});

pub fn mark(size: Pixels) -> impl IntoElement {
    img(ImageSource::Image(MARK.clone()))
        .size(size)
        .flex_shrink_0()
        .object_fit(ObjectFit::Contain)
}

pub fn wordmark(size: Pixels) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(size * 0.32)
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
    wordmark(px(26.))
}
