//! Full-bleed artwork with the item's title treatment, shared by Home's
//! carousel and the detail pages.

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ElementId, FontWeight, ObjectFit, Pixels, div, linear_color_stop, linear_gradient,
    px, rgba,
};
use jellyfin::{BaseItem, Client};

use crate::components::art::Art;
use crate::images::{ImageRequest, ImageState, ImageStore};
use crate::theme::{FONT_DISPLAY, Palette};

pub fn backdrop_request(client: &Client, item: &BaseItem) -> Option<ImageRequest> {
    item.backdrop_image()
        .map(|image| ImageRequest::for_image(client, &image, 1920))
}

/// Backdrop with the fades that let text sit on top of it.
pub fn backdrop(id: impl Into<ElementId>, request: Option<ImageRequest>) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .child(Art::new(id, request).bare().size_full())
        // Left fade for the title block.
        .child(div().absolute().inset_0().bg(linear_gradient(
            90.,
            linear_color_stop(rgba(0x0A0B10F0), 0.),
            linear_color_stop(rgba(0x0A0B1000), 0.62),
        )))
        // Bottom fade into the page.
        .child(div().absolute().inset_0().bg(linear_gradient(
            180.,
            linear_color_stop(rgba(0x0A0B1000), 0.45),
            linear_color_stop(Palette::bg(), 1.),
        )))
        // Top fade under the nav bar.
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(160.))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(rgba(0x0A0B10B0), 0.),
                    linear_color_stop(rgba(0x0A0B1000), 1.),
                )),
        )
}

/// A title logo sized to its own aspect ratio and pinned left.
#[derive(IntoElement)]
struct Logo {
    id: ElementId,
    request: ImageRequest,
    max_height: Pixels,
    fallback: gpui_kit::SharedString,
}

impl RenderOnce for Logo {
    fn render(self, window: &mut gpui_kit::Window, cx: &mut gpui_kit::App) -> impl IntoElement {
        let max_width = self.max_height * 4.;
        match ImageStore::get(&self.request, window, cx) {
            ImageState::Ready { image, .. } => {
                let size = image.size(0);
                let aspect = size.width.0 as f32 / size.height.0.max(1) as f32;
                let (mut width, mut height) = (self.max_height * aspect, self.max_height);
                if width > max_width {
                    height = max_width / aspect;
                    width = max_width;
                }
                div()
                    .w(width)
                    .h(height)
                    .child(
                        Art::new(self.id, Some(self.request))
                            .bare()
                            .fit(ObjectFit::Contain)
                            .size_full(),
                    )
                    .into_any_element()
            }
            ImageState::Loading { .. } => div().h(self.max_height).into_any_element(),
            ImageState::Failed { .. } => big_title(self.fallback),
        }
    }
}

fn big_title(name: gpui_kit::SharedString) -> AnyElement {
    div()
        .max_w(px(760.))
        .font_family(FONT_DISPLAY)
        .font_weight(FontWeight::EXTRA_BOLD)
        .text_size(px(56.))
        .line_height(px(60.))
        .line_clamp(2)
        .text_ellipsis()
        .child(name)
        .into_any_element()
}

/// The title logo if the item (or its series) has one, else a big title.
pub fn title(
    client: &Client,
    item: &BaseItem,
    id: impl Into<ElementId>,
    max_height: Pixels,
) -> AnyElement {
    let name = match (&item.series_name, item.kind) {
        (Some(series), jellyfin::ItemKind::Episode) => series.clone(),
        _ => item.name.clone(),
    };
    match item.logo_image() {
        Some(logo) => Logo {
            id: id.into(),
            request: ImageRequest::for_image(client, &logo, 800),
            max_height,
            fallback: name.into(),
        }
        .into_any_element(),
        None => big_title(name.into()),
    }
}
