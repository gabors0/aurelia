//! Slowly drifting colour light behind the login screen. GPUI has no radial
//! gradients, so each light is a small generated image scaled up on the GPU.
//!
//! The images are much larger than the window, with the glow in their middle,
//! so their edges never come on screen: some GPU/compositor setups draw a
//! faint seam along the edge of a stretched texture.

use std::f32::consts::TAU;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, App, Global, ImageSource, ObjectFit, RenderImage, Window, div,
    img,
};
use image::{Rgba, RgbaImage};

use crate::images::decode::to_render_image;

/// Image side as a multiple of the window width.
const SPAN: f32 = 2.4;
const IMAGE_SIZE: u32 = 320;

struct Light {
    /// Centre, as fractions of the window width and height.
    x: f32,
    y: f32,
    /// Glow radius as a fraction of the window width.
    radius: f32,
    phase: f32,
    rgb: [u8; 3],
    opacity: f32,
}

const LIGHTS: [Light; 3] = [
    Light {
        x: 0.50,
        y: 0.42,
        radius: 0.42,
        phase: 0.0,
        rgb: [0xB6, 0x9C, 0xFF],
        opacity: 0.55,
    },
    Light {
        x: 0.86,
        y: 0.80,
        radius: 0.40,
        phase: 0.33,
        rgb: [0x2F, 0xC6, 0xB5],
        opacity: 0.35,
    },
    Light {
        x: 0.28,
        y: 0.96,
        radius: 0.44,
        phase: 0.66,
        rgb: [0x3B, 0x5B, 0xDB],
        opacity: 0.45,
    },
];

/// Where light `index` sits at animation time `t` (0–1), for a window with
/// the given aspect ratio (width / height): (left, top, width, height), with
/// horizontal values as fractions of the window width and vertical ones as
/// fractions of its height.
fn light_rect(index: usize, t: f32, aspect: f32) -> (f32, f32, f32, f32) {
    let light = &LIGHTS[index];
    let a = (t + light.phase) * TAU;
    let centre_x = light.x + 0.07 * a.sin();
    let centre_y = light.y + 0.05 * (a * 2.0).cos();
    let (width, height) = (SPAN, SPAN * aspect);
    (
        centre_x - width / 2.0,
        centre_y - height / 2.0,
        width,
        height,
    )
}

/// Radial falloff from `rgb` at the centre to fully transparent at
/// `radius` (a fraction of the half-size); everything beyond is empty.
fn glow_image(rgb: [u8; 3], size: u32, radius: f32) -> RgbaImage {
    let centre = size as f32 / 2.0;
    RgbaImage::from_fn(size, size, |x, y| {
        let dx = (x as f32 + 0.5 - centre) / centre;
        let dy = (y as f32 + 0.5 - centre) / centre;
        let d = (dx * dx + dy * dy).sqrt() / radius;
        if d >= 1.0 {
            return Rgba([0, 0, 0, 0]);
        }
        let falloff = (1.0 - d).powf(1.8);
        Rgba([rgb[0], rgb[1], rgb[2], (falloff * 255.0) as u8])
    })
}

struct Glows(Vec<Arc<RenderImage>>);

impl Global for Glows {}

fn glows(cx: &mut App) -> Vec<Arc<RenderImage>> {
    if !cx.has_global::<Glows>() {
        let images = LIGHTS
            .iter()
            .map(|light| {
                let radius = light.radius / (SPAN / 2.0);
                to_render_image(glow_image(light.rgb, IMAGE_SIZE, radius))
            })
            .collect();
        cx.set_global(Glows(images));
    }
    cx.global::<Glows>().0.clone()
}

#[derive(IntoElement)]
pub struct Aurora;

impl RenderOnce for Aurora {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let viewport = window.viewport_size();
        let aspect = f32::from(viewport.width) / f32::from(viewport.height).max(1.0);
        div().absolute().inset_0().overflow_hidden().children(
            glows(cx)
                .into_iter()
                .enumerate()
                .map(move |(index, image)| {
                    img(ImageSource::Render(image))
                        .absolute()
                        .object_fit(ObjectFit::Fill)
                        .opacity(LIGHTS[index].opacity)
                        .with_animation(
                            ("aurora", index),
                            Animation::new(Duration::from_secs(48))
                                .repeat()
                                .with_max_fps(30.),
                            move |light, t| {
                                let (left, top, width, height) = light_rect(index, t, aspect);
                                light
                                    .left(viewport.width * left)
                                    .top(viewport.height * top)
                                    .w(viewport.width * width)
                                    .h(viewport.height * height)
                            },
                        )
                }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_edges_stay_off_screen() {
        // From the minimum window size (960×600) to ultrawide.
        for aspect in [960.0 / 600.0, 16.0 / 9.0, 21.0 / 9.0] {
            for index in 0..LIGHTS.len() {
                for step in 0..=100 {
                    let (left, top, width, height) = light_rect(index, step as f32 / 100.0, aspect);
                    assert!(
                        left < 0.0 && left + width > 1.0,
                        "light {index} x edge on screen"
                    );
                    assert!(
                        top < 0.0 && top + height > 1.0,
                        "light {index} y edge on screen"
                    );
                }
            }
        }
    }

    #[test]
    fn glow_fades_to_nothing_well_inside_the_image() {
        let image = glow_image([255, 0, 0], 64, 0.4);
        assert!(image.get_pixel(32, 32)[3] > 200, "bright centre");
        for edge in [0, 63] {
            for i in 0..64 {
                assert_eq!(image.get_pixel(edge, i)[3], 0);
                assert_eq!(image.get_pixel(i, edge)[3], 0);
            }
        }
    }
}
