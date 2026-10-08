//! Artwork with a blurhash placeholder that fades to the real image.

use std::time::Duration;

use gpui_kit::component::StyledExt as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, App, ElementId, ImageSource, ObjectFit, Pixels, SharedString,
    StyleRefinement, Window, div, img, linear_color_stop, linear_gradient, px,
};

use crate::images::{ImageRequest, ImageState, ImageStore};
use crate::theme::{FONT_DISPLAY, Palette};

#[derive(IntoElement)]
pub struct Art {
    id: ElementId,
    request: Option<ImageRequest>,
    fit: ObjectFit,
    /// Shown large on the gradient when there's no artwork at all.
    title: Option<SharedString>,
    radius: Pixels,
    bare: bool,
    style: StyleRefinement,
}

impl Art {
    pub fn new(id: impl Into<ElementId>, request: Option<ImageRequest>) -> Self {
        Self {
            id: id.into(),
            request,
            fit: ObjectFit::Cover,
            title: None,
            radius: px(0.),
            bare: false,
            style: StyleRefinement::default(),
        }
    }

    /// Corner radius. GPUI clips images to their own corners, not their
    /// parent's, so set it here rather than with `.rounded_*()`.
    pub fn radius(mut self, radius: Pixels) -> Self {
        self.radius = radius;
        self
    }

    pub fn fit(mut self, fit: ObjectFit) -> Self {
        self.fit = fit;
        self
    }

    /// No surface colour or placeholder: for logos and overlays.
    pub fn bare(mut self) -> Self {
        self.bare = true;
        self
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }
}

impl Styled for Art {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Art {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self
            .request
            .as_ref()
            .map(|request| ImageStore::get(request, window, cx));

        let (placeholder, ready, failed) = match state {
            Some(ImageState::Ready { image, .. }) => (None, Some(image), false),
            Some(ImageState::Loading { placeholder }) => (placeholder, None, false),
            Some(ImageState::Failed { placeholder }) => (placeholder, None, true),
            None => (None, None, true),
        };

        let base = div()
            .id(self.id.clone())
            .relative()
            .overflow_hidden()
            .when(!self.bare, |this| this.bg(Palette::surface()))
            .rounded(self.radius)
            .refine_style(&self.style);
        let radius = self.radius;

        let backdrop = match placeholder {
            Some(placeholder) => img(ImageSource::Render(placeholder))
                .absolute()
                .inset_0()
                .size_full()
                .rounded(radius)
                .object_fit(ObjectFit::Fill)
                .into_any_element(),
            None => div()
                .absolute()
                .inset_0()
                .rounded(radius)
                .bg(linear_gradient(
                    160.,
                    linear_color_stop(gpui_kit::rgb(0x23202F), 0.),
                    linear_color_stop(gpui_kit::rgb(0x12131A), 1.),
                ))
                .when_some(self.title.filter(|_| failed), |this, title| {
                    this.flex().items_center().justify_center().p_3().child(
                        div()
                            .font_family(FONT_DISPLAY)
                            .font_weight(gpui_kit::FontWeight::BOLD)
                            .text_center()
                            .text_color(Palette::text_secondary())
                            .child(title),
                    )
                })
                .into_any_element(),
        };

        base.when(!self.bare, |this| this.child(backdrop))
            .when_some(ready, |this, image| {
                this.child(
                    img(ImageSource::Render(image))
                        .absolute()
                        .inset_0()
                        .size_full()
                        .rounded(radius)
                        .object_fit(self.fit)
                        .with_animation(
                            ElementId::NamedChild(std::sync::Arc::new(self.id), "fade".into()),
                            Animation::new(Duration::from_millis(260))
                                .with_easing(gpui_kit::ease_in_out),
                            |image, t| image.opacity(t),
                        ),
                )
            })
    }
}
