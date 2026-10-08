//! Slowly drifting colour light behind the login screen. GPUI has no radial
//! gradients, so the blobs are tiny generated images scaled up on the GPU.

use std::f32::consts::TAU;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, App, Global, ImageSource, ObjectFit, RenderImage, Window, div,
    img, relative,
};
use image::{Rgba, RgbaImage};

use crate::images::decode::to_render_image;

struct Blobs(Vec<Arc<RenderImage>>);

impl Global for Blobs {}

/// Radial falloff from `rgb` at the centre to transparent at the edge.
fn blob(rgb: [u8; 3], size: u32) -> RgbaImage {
    let centre = size as f32 / 2.0;
    RgbaImage::from_fn(size, size, |x, y| {
        let dx = (x as f32 + 0.5 - centre) / centre;
        let dy = (y as f32 + 0.5 - centre) / centre;
        let d = (dx * dx + dy * dy).sqrt().min(1.0);
        let falloff = (1.0 - d).powf(1.8);
        Rgba([rgb[0], rgb[1], rgb[2], (falloff * 255.0) as u8])
    })
}

fn blobs(cx: &mut App) -> Vec<Arc<RenderImage>> {
    if !cx.has_global::<Blobs>() {
        let images = [[0xB6, 0x9C, 0xFF], [0x2F, 0xC6, 0xB5], [0x3B, 0x5B, 0xDB]]
            .into_iter()
            .map(|rgb| to_render_image(blob(rgb, 192)))
            .collect();
        cx.set_global(Blobs(images));
    }
    cx.global::<Blobs>().0.clone()
}

#[derive(IntoElement)]
pub struct Aurora;

impl RenderOnce for Aurora {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        // (x, y, size, phase, opacity), all relative to the window.
        let layout = [
            (0.08, -0.05, 0.85, 0.0, 0.55),
            (0.45, 0.30, 0.80, 0.33, 0.35),
            (-0.15, 0.40, 0.90, 0.66, 0.45),
        ];
        div().absolute().inset_0().overflow_hidden().children(
            blobs(cx).into_iter().zip(layout).enumerate().map(
                |(i, (image, (x, y, size, phase, opacity)))| {
                    img(ImageSource::Render(image))
                        .absolute()
                        .w(relative(size))
                        .h(relative(size * 1.3))
                        .object_fit(ObjectFit::Fill)
                        .opacity(opacity)
                        .with_animation(
                            ("aurora", i),
                            Animation::new(Duration::from_secs(48))
                                .repeat()
                                .with_max_fps(30.),
                            move |blob, t| {
                                let a = (t + phase) * TAU;
                                blob.left(relative(x + 0.07 * a.sin()))
                                    .top(relative(y + 0.05 * (a * 2.0).cos()))
                            },
                        )
                },
            ),
        )
    }
}
