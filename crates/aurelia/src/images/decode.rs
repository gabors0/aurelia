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

/// Logo ink farther than this (as a share of the logo's width) from any
/// visible ink has nothing to show its shape on a dark page.
const LOGO_REACH: f32 = 0.02;
/// Share of a logo's ink that must be lost that way before it's recoloured.
const LOGO_LOST_SHARE: f32 = 0.03;
/// Share of a dark shape that must be lost for the shape to be lifted.
const SHAPE_LOST_SHARE: f32 = 0.6;
/// What lifted ink becomes: the UI's text colour.
const LOGO_LIGHT: [f32; 3] = [
    0xF2 as f32 / 255.0,
    0xF3 as f32 / 255.0,
    0xF7 as f32 / 255.0,
];

fn luma([r, g, b, _]: [u8; 4]) -> f32 {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) / 255.0
}

/// Stands out on a dark page: light, or a bright saturated colour.
fn is_visible(pixel: [u8; 4]) -> bool {
    let max = pixel[..3].iter().copied().max().unwrap_or(0) as f32 / 255.0;
    let min = pixel[..3].iter().copied().min().unwrap_or(0) as f32 / 255.0;
    luma(pixel) >= 0.4 || (max >= 0.6 && (max - min) / max >= 0.5)
}

/// A logo made for light backgrounds, such as black lettering, vanishes on
/// the hero's dark fade. Dark shapes that nothing visible outlines are lifted
/// to a light tint of their colour. Dark outlines, shadows and details on
/// light ink are left alone.
pub fn legible_logo(mut logo: RgbaImage) -> RgbaImage {
    let (width, height) = (logo.width() as usize, logo.height() as usize);
    let pixels: Vec<[u8; 4]> = logo.pixels().map(|pixel| pixel.0).collect();
    let solid = |pixel: &[u8; 4]| pixel[3] > 127;
    let visible: Vec<bool> = pixels.iter().map(|p| solid(p) && is_visible(*p)).collect();
    let dark: Vec<bool> = pixels
        .iter()
        .map(|p| solid(p) && luma(*p) < 0.25 && !is_visible(*p))
        .collect();
    let reach = ((width as f32 * LOGO_REACH).round() as usize).max(2);
    let near_visible = spread(&visible, width, height, reach);
    let lost: Vec<bool> = (0..pixels.len())
        .map(|i| dark[i] && !near_visible[i])
        .collect();

    let ink = pixels.iter().filter(|p| solid(p)).count();
    let lost_ink = lost.iter().filter(|&&lost| lost).count();
    if ink == 0 || (lost_ink as f32) < ink as f32 * LOGO_LOST_SHARE {
        return logo;
    }

    // Lift whole shapes, so a letter is never half light and half dark.
    let (labels, count) = shapes(&dark, width, height);
    let (mut size, mut lost_in) = (vec![0u32; count + 1], vec![0u32; count + 1]);
    for (i, &label) in labels.iter().enumerate() {
        size[label as usize] += 1;
        lost_in[label as usize] += lost[i] as u32;
    }
    let lifted: Vec<bool> = labels
        .iter()
        .map(|&label| {
            label > 0
                && lost_in[label as usize] as f32 >= size[label as usize] as f32 * SHAPE_LOST_SHARE
        })
        .collect();
    // Take in the shapes' soft edges and the transparent pixels around them,
    // which texture filtering would otherwise blend in as a dark fringe.
    let lifted = spread(&lifted, width, height, 2);
    for (pixel, lift) in logo.pixels_mut().zip(lifted) {
        if lift {
            pixel.0 = lift_pixel(pixel.0);
        }
    }
    logo
}

/// Dark colours become a light tint of themselves; light ones are kept.
fn lift_pixel(pixel: [u8; 4]) -> [u8; 4] {
    let amount = ((0.45 - luma(pixel)) / 0.2).clamp(0.0, 1.0);
    if amount == 0.0 {
        return pixel;
    }
    let [r, g, b, a] = pixel;
    let rgb = [r, g, b].map(|c| c as f32 / 255.0);
    let max = rgb[0].max(rgb[1]).max(rgb[2]);
    let min = rgb[0].min(rgb[1]).min(rgb[2]);
    // How much of the hue survives: none for greys, some for strong colours.
    let hue = if max > 0.0 {
        0.3 * (max - min) / max
    } else {
        0.0
    };
    let mut out = [0u8; 4];
    for (channel, (c, light)) in rgb.iter().zip(LOGO_LIGHT).enumerate() {
        let tint = if max > 0.0 {
            1.0 - hue + hue * c / max
        } else {
            1.0
        };
        out[channel] = ((c + (light * tint - c) * amount) * 255.0).round() as u8;
    }
    out[3] = a;
    out
}

/// Marks every pixel within `reach` (in both axes) of a marked one.
fn spread(mask: &[bool], width: usize, height: usize, reach: usize) -> Vec<bool> {
    let mut prefix = vec![0u32; width.max(height) + 1];
    let mut rows = vec![false; mask.len()];
    for y in 0..height {
        let line = y * width..(y + 1) * width;
        spread_line(&mask[line.clone()], &mut rows[line], reach, &mut prefix);
    }
    let mut out = vec![false; mask.len()];
    let (mut column, mut spread_column) = (vec![false; height], vec![false; height]);
    for x in 0..width {
        for y in 0..height {
            column[y] = rows[y * width + x];
        }
        spread_line(&column, &mut spread_column, reach, &mut prefix);
        for y in 0..height {
            out[y * width + x] = spread_column[y];
        }
    }
    out
}

fn spread_line(line: &[bool], out: &mut [bool], reach: usize, prefix: &mut [u32]) {
    for (i, &set) in line.iter().enumerate() {
        prefix[i + 1] = prefix[i] + set as u32;
    }
    for (i, out) in out.iter_mut().enumerate() {
        let (lo, hi) = (i.saturating_sub(reach), (i + reach + 1).min(line.len()));
        *out = prefix[hi] > prefix[lo];
    }
}

/// Labels the 8-connected shapes in `mask` from 1; 0 is background.
fn shapes(mask: &[bool], width: usize, height: usize) -> (Vec<u32>, usize) {
    let mut labels = vec![0u32; mask.len()];
    let mut count = 0;
    let mut stack = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || labels[start] != 0 {
            continue;
        }
        count += 1;
        labels[start] = count as u32;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % width, i / width);
            for ny in y.saturating_sub(1)..(y + 2).min(height) {
                for nx in x.saturating_sub(1)..(x + 2).min(width) {
                    let n = ny * width + nx;
                    if mask[n] && labels[n] == 0 {
                        labels[n] = count as u32;
                        stack.push(n);
                    }
                }
            }
        }
    }
    (labels, count)
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

    const BLACK: [u8; 4] = [0, 0, 0, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const ORANGE: [u8; 4] = [232, 78, 15, 255];

    fn fill(
        image: &mut RgbaImage,
        xs: std::ops::Range<u32>,
        ys: std::ops::Range<u32>,
        rgba: [u8; 4],
    ) {
        for y in ys {
            for x in xs.clone() {
                image.put_pixel(x, y, Rgba(rgba));
            }
        }
    }

    fn luma_of(pixel: &Rgba<u8>) -> f32 {
        (0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32) / 255.0
    }

    #[test]
    fn logo_dark_text_on_its_own_is_lifted() {
        // A black word beside an orange bar, like The Grand Tour's logo.
        let mut logo = RgbaImage::new(400, 100);
        fill(&mut logo, 0..400, 0..20, ORANGE);
        fill(&mut logo, 40..200, 50..90, BLACK);
        let out = legible_logo(logo);
        assert!(luma_of(out.get_pixel(120, 70)) > 0.8, "word lifted");
        assert_eq!(out.get_pixel(120, 70)[3], 255);
        assert_eq!(out.get_pixel(10, 10).0, ORANGE, "colour untouched");
    }

    #[test]
    fn logo_outlined_light_text_is_untouched() {
        // White letters in a black outline already read on a dark page.
        let mut logo = RgbaImage::new(400, 100);
        fill(&mut logo, 20..380, 20..80, BLACK);
        fill(&mut logo, 26..374, 26..74, WHITE);
        let before = logo.clone();
        assert_eq!(legible_logo(logo), before);
    }

    #[test]
    fn logo_dark_detail_on_light_ink_stays_dark() {
        // A lone black block, and black lettering on a white badge.
        let mut logo = RgbaImage::new(400, 100);
        fill(&mut logo, 0..160, 0..100, BLACK);
        fill(&mut logo, 240..400, 0..100, WHITE);
        fill(&mut logo, 300..340, 30..70, BLACK);
        let out = legible_logo(logo);
        assert!(luma_of(out.get_pixel(80, 50)) > 0.8);
        assert_eq!(out.get_pixel(320, 50).0, BLACK);
    }

    #[test]
    fn logo_vivid_dark_rim_is_not_lonely() {
        // A saturated red badge with a dark rim, like Azumanga Daioh's.
        let mut logo = RgbaImage::new(400, 100);
        fill(&mut logo, 0..400, 0..100, BLACK);
        fill(&mut logo, 6..394, 6..94, [205, 20, 20, 255]);
        let before = logo.clone();
        assert_eq!(legible_logo(logo), before);
    }

    #[test]
    fn logo_lift_covers_soft_edges() {
        let mut logo = RgbaImage::new(200, 60);
        fill(&mut logo, 40..160, 20..40, BLACK);
        fill(&mut logo, 39..40, 20..40, [0, 0, 0, 90]);
        let out = legible_logo(logo);
        let edge = out.get_pixel(39, 30);
        assert_eq!(edge[3], 90, "alpha kept");
        assert!(luma_of(edge) > 0.8, "no dark fringe");
        assert!(
            luma_of(out.get_pixel(38, 30)) > 0.8,
            "transparent neighbours too"
        );
    }

    #[test]
    fn logo_lift_keeps_a_hint_of_hue() {
        let mut logo = RgbaImage::new(200, 60);
        fill(&mut logo, 40..160, 20..40, [20, 30, 110, 255]);
        let pixel = *legible_logo(logo).get_pixel(100, 30);
        assert!(luma_of(&pixel) > 0.75, "lifted: {pixel:?}");
        assert!(pixel[2] > pixel[0] + 20, "still blue: {pixel:?}");
    }

    #[test]
    fn logo_of_only_light_ink_is_untouched() {
        let mut logo = RgbaImage::new(200, 60);
        fill(&mut logo, 40..160, 20..40, WHITE);
        let before = logo.clone();
        assert_eq!(legible_logo(logo), before);
        let empty = RgbaImage::new(8, 8);
        assert_eq!(legible_logo(empty.clone()), empty);
    }

    #[test]
    fn blurhash_decodes() {
        let image = blurhash_image("LEHV6nWB2yk8pyo0adR*.7kCMdnj", 32, 20).expect("valid hash");
        assert_eq!(image.dimensions(), (32, 20));
        assert!(blurhash_image("nonsense", 32, 20).is_none());
    }
}
