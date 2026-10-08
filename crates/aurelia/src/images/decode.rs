//! Pure image processing: decoding, ambient blur, accent colour, blurhash.

use std::sync::Arc;

use gpui_kit::{Hsla, RenderImage};
use image::RgbaImage;
use image::imageops::FilterType;

/// Widest ambient background we keep; it's blurred beyond recognition anyway.
const AMBIENT_WIDTH: u32 = 480;
const AMBIENT_SIGMA: f32 = 22.0;
const AMBIENT_BRIGHTNESS: f32 = 0.55;

pub fn decode(bytes: &[u8]) -> Option<RgbaImage> {
    image::load_from_memory(bytes)
        .ok()
        .map(|image| image.into_rgba8())
}

/// GPUI wants BGRA.
pub fn to_render_image(mut image: RgbaImage) -> Arc<RenderImage> {
    for pixel in image.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![image::Frame::new(image)]))
}

/// Small, heavily blurred and darkened copy used as a page's ambient light.
pub fn ambient(source: &RgbaImage) -> RgbaImage {
    let (w, h) = source.dimensions();
    let small = if w > AMBIENT_WIDTH {
        let height = ((h as u64 * AMBIENT_WIDTH as u64) / w as u64).max(1) as u32;
        image::imageops::resize(source, AMBIENT_WIDTH, height, FilterType::Triangle)
    } else {
        source.clone()
    };
    let mut blurred = image::imageops::fast_blur(&small, AMBIENT_SIGMA);
    for pixel in blurred.pixels_mut() {
        for channel in &mut pixel.0[..3] {
            *channel = (*channel as f32 * AMBIENT_BRIGHTNESS) as u8;
        }
    }
    blurred
}

/// A vivid colour from the artwork, tuned to read well on a dark UI.
/// `None` for greyscale or near-black art.
pub fn accent(source: &RgbaImage) -> Option<Hsla> {
    let thumb = image::imageops::resize(source, 48, 48, FilterType::Triangle);
    let (mut x, mut y, mut weight_sum, mut saturation_sum) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for pixel in thumb.pixels() {
        let [r, g, b, _] = pixel.0.map(|c| c as f32 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        if max < 0.15 || max - min < 0.08 {
            continue;
        }
        let chroma = max - min;
        let hue = if max == r {
            ((g - b) / chroma).rem_euclid(6.0)
        } else if max == g {
            (b - r) / chroma + 2.0
        } else {
            (r - g) / chroma + 4.0
        } / 6.0;
        let saturation = chroma / max;
        // Vivid, bright pixels dominate; muddy ones barely count.
        let weight = saturation * saturation * max;
        let angle = hue * std::f32::consts::TAU;
        x += weight * angle.cos();
        y += weight * angle.sin();
        weight_sum += weight;
        saturation_sum += weight * saturation;
    }
    let pixels = (thumb.width() * thumb.height()) as f32;
    if weight_sum / pixels < 0.01 {
        return None;
    }
    let hue = (y.atan2(x) / std::f32::consts::TAU).rem_euclid(1.0);
    let saturation = (saturation_sum / weight_sum).clamp(0.55, 0.85);
    Some(gpui_kit::hsla(hue, saturation, 0.70, 1.0))
}

pub fn blurhash_image(hash: &str, width: u32, height: u32) -> Option<RgbaImage> {
    let pixels = blurhash::decode(hash, width, height, 1.0).ok()?;
    RgbaImage::from_raw(width, height, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(rgba))
    }

    fn mean_luma(image: &RgbaImage) -> f32 {
        let sum: f32 = image
            .pixels()
            .map(|p| 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32)
            .sum();
        sum / (image.width() * image.height()) as f32
    }

    #[test]
    fn accent_of_solid_red_is_reddish() {
        let accent = accent(&solid(64, 64, [220, 30, 40, 255])).expect("vivid");
        assert!(accent.h < 0.05 || accent.h > 0.95, "hue {}", accent.h);
        assert!(accent.s >= 0.5);
        assert!(accent.l > 0.55, "light enough for a dark UI: {}", accent.l);
    }

    #[test]
    fn accent_of_gray_falls_back() {
        assert_eq!(accent(&solid(64, 64, [128, 128, 128, 255])), None);
        assert_eq!(accent(&solid(64, 64, [4, 2, 6, 255])), None, "near black");
    }

    #[test]
    fn accent_prefers_the_vivid_part() {
        // Mostly grey poster with a saturated blue band.
        let mut image = solid(64, 64, [90, 90, 90, 255]);
        for y in 0..16 {
            for x in 0..64 {
                image.put_pixel(x, y, Rgba([30, 80, 230, 255]));
            }
        }
        let accent = accent(&image).expect("blue band");
        assert!((0.55..0.72).contains(&accent.h), "hue {}", accent.h);
    }

    #[test]
    fn ambient_is_downscaled_and_darker() {
        let source = solid(1920, 1080, [200, 180, 160, 255]);
        let out = ambient(&source);
        assert!(out.width() <= AMBIENT_WIDTH);
        assert_eq!(out.width() * 1080 / 1920, out.height());
        assert!(mean_luma(&out) < mean_luma(&source) * 0.7);
    }

    #[test]
    fn bgra_conversion_swaps_channels() {
        let image = RgbaImage::from_pixel(1, 1, Rgba([1, 2, 3, 4]));
        let render = to_render_image(image);
        assert_eq!(render.as_bytes(0), Some(&[3u8, 2, 1, 4][..]));
    }

    #[test]
    fn decodes_png_and_rejects_garbage() {
        let mut bytes = Vec::new();
        solid(3, 2, [9, 9, 9, 255])
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        assert_eq!(decode(&bytes).map(|i| i.dimensions()), Some((3, 2)));
        assert!(decode(b"<html>nope</html>").is_none());
    }

    #[test]
    fn blurhash_decodes() {
        let image = blurhash_image("LEHV6nWB2yk8pyo0adR*.7kCMdnj", 32, 20).expect("valid hash");
        assert_eq!(image.dimensions(), (32, 20));
        assert!(blurhash_image("nonsense", 32, 20).is_none());
    }
}
