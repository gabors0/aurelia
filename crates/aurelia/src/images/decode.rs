//! Pure image processing: decoding, ambient blur, accent colour, blurhash.

use std::sync::Arc;

use gpui_kit::{Hsla, RenderImage};
use image::RgbaImage;
use image::imageops::FilterType;

/// Widest ambient background we keep; it's blurred beyond recognition anyway.
const AMBIENT_WIDTH: u32 = 480;
const AMBIENT_SIGMA: f32 = 22.0;

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

/// Small, heavily blurred copy used as a page's ambient light. The layers
/// that show it veil it to suit the theme.
pub fn ambient(source: &RgbaImage) -> RgbaImage {
    let (w, h) = source.dimensions();
    let small = if w > AMBIENT_WIDTH {
        let height = ((h as u64 * AMBIENT_WIDTH as u64) / w as u64).max(1) as u32;
        image::imageops::resize(source, AMBIENT_WIDTH, height, FilterType::Triangle)
    } else {
        source.clone()
    };
    image::imageops::fast_blur(&small, AMBIENT_SIGMA)
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
/// visible ink has nothing to show its shape against the page.
const LOGO_REACH: f32 = 0.02;
/// Share of a logo's ink that must be lost that way before it's recoloured.
const LOGO_LOST_SHARE: f32 = 0.03;
/// Share of a faint shape that must be lost for the shape to be recoloured.
const SHAPE_LOST_SHARE: f32 = 0.6;
/// What lifted ink becomes on a dark page: the UI's text colour.
const LOGO_LIGHT: [f32; 3] = [
    0xF2 as f32 / 255.0,
    0xF3 as f32 / 255.0,
    0xF7 as f32 / 255.0,
];
/// How light lowered ink ends up on a light page.
const LOGO_DARK_LUMA: f32 = 0.13;

fn luma([r, g, b, _]: [u8; 4]) -> f32 {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) / 255.0
}

/// Saturation (0–1) and brightest channel (0–1).
fn chroma(pixel: [u8; 4]) -> (f32, f32) {
    let max = pixel[..3].iter().copied().max().unwrap_or(0) as f32 / 255.0;
    let min = pixel[..3].iter().copied().min().unwrap_or(0) as f32 / 255.0;
    let saturation = if max > 0.0 { (max - min) / max } else { 0.0 };
    (saturation, max)
}

/// Stands out on a dark page: light, or a bright saturated colour.
fn is_visible(pixel: [u8; 4]) -> bool {
    let (saturation, max) = chroma(pixel);
    luma(pixel) >= 0.4 || (max >= 0.6 && saturation >= 0.5)
}

/// Stands out on a light page: dark, or a strong saturated colour.
fn is_visible_on_light(pixel: [u8; 4]) -> bool {
    let (saturation, max) = chroma(pixel);
    luma(pixel) <= 0.6 || (max >= 0.35 && saturation >= 0.5 && luma(pixel) <= 0.72)
}

/// How a logo is made legible on one kind of page.
struct LogoRules {
    /// Ink that reads on this page.
    visible: fn([u8; 4]) -> bool,
    /// Ink that vanishes on this page unless something visible outlines it.
    faint: fn([u8; 4]) -> bool,
    recolour: fn([u8; 4]) -> [u8; 4],
    /// Judge a faint shape by its edge (is it outlined?) rather than by how
    /// much of it is far from visible ink. Faint lettering on a light page is
    /// the thick fill inside a thin outline, not the outline itself.
    by_edge: bool,
}

const ON_DARK: LogoRules = LogoRules {
    visible: is_visible,
    faint: |p| luma(p) < 0.25 && !is_visible(p),
    recolour: lift_pixel,
    by_edge: false,
};

const ON_LIGHT: LogoRules = LogoRules {
    visible: is_visible_on_light,
    faint: |p| luma(p) > 0.75 && !is_visible_on_light(p),
    recolour: lower_pixel,
    by_edge: true,
};

/// Share of a faint shape's edge that visible ink must line for the shape to
/// count as outlined.
const OUTLINED_SHARE: f32 = 0.5;

/// A logo made for light backgrounds, such as black lettering, vanishes on
/// the hero's dark fade. Dark shapes that nothing visible outlines are lifted
/// to a light tint of their colour. Dark outlines, shadows and details on
/// light ink are left alone.
pub fn legible_logo(logo: RgbaImage) -> RgbaImage {
    rework_logo(logo, &ON_DARK)
}

/// The light theme's counterpart of [`legible_logo`]: white lettering that
/// nothing dark outlines is lowered to a dark shade of its colour.
pub fn legible_logo_on_light(logo: RgbaImage) -> RgbaImage {
    rework_logo(logo, &ON_LIGHT)
}

fn rework_logo(mut logo: RgbaImage, rules: &LogoRules) -> RgbaImage {
    let (width, height) = (logo.width() as usize, logo.height() as usize);
    let pixels: Vec<[u8; 4]> = logo.pixels().map(|pixel| pixel.0).collect();
    let solid = |pixel: &[u8; 4]| pixel[3] > 127;
    let visible: Vec<bool> = pixels
        .iter()
        .map(|p| solid(p) && (rules.visible)(*p))
        .collect();
    let faint: Vec<bool> = pixels
        .iter()
        .map(|p| solid(p) && (rules.faint)(*p))
        .collect();
    let reach = ((width as f32 * LOGO_REACH).round() as usize).max(2);
    let near_visible = spread(&visible, width, height, reach);
    let lost: Vec<bool> = (0..pixels.len())
        .map(|i| faint[i] && !near_visible[i])
        .collect();

    let ink = pixels.iter().filter(|p| solid(p)).count();
    if ink == 0 {
        return logo;
    }
    let (labels, count) = shapes(&faint, width, height);
    let mut size = vec![0u32; count + 1];
    let mut lost_in = vec![0u32; count + 1];
    let (mut edge, mut edge_lined) = (vec![0u32; count + 1], vec![0u32; count + 1]);
    for (i, &label) in labels.iter().enumerate() {
        if label == 0 {
            continue;
        }
        let label = label as usize;
        size[label] += 1;
        lost_in[label] += lost[i] as u32;
        let (x, y) = (i % width, i / width);
        let outside = |nx: Option<usize>, ny: Option<usize>| match (nx, ny) {
            (Some(nx), Some(ny)) if nx < width && ny < height => labels[ny * width + nx] == 0,
            _ => true,
        };
        if outside(x.checked_sub(1), Some(y))
            || outside(Some(x + 1), Some(y))
            || outside(Some(x), y.checked_sub(1))
            || outside(Some(x), Some(y + 1))
        {
            edge[label] += 1;
            edge_lined[label] += near_visible[i] as u32;
        }
    }
    let is_lost = |label: usize| {
        if rules.by_edge {
            (edge_lined[label] as f32) < edge[label] as f32 * OUTLINED_SHARE
        } else {
            lost_in[label] as f32 >= size[label] as f32 * SHAPE_LOST_SHARE
        }
    };
    let lost_ink: u32 = if rules.by_edge {
        (1..=count).filter(|&l| is_lost(l)).map(|l| size[l]).sum()
    } else {
        lost.iter().filter(|&&lost| lost).count() as u32
    };
    if (lost_ink as f32) < ink as f32 * LOGO_LOST_SHARE {
        return logo;
    }

    // Recolour whole shapes, so a letter is never half one shade and half
    // the other.
    let recoloured: Vec<bool> = labels
        .iter()
        .map(|&label| label > 0 && is_lost(label as usize))
        .collect();
    // Take in the shapes' soft edges and the transparent pixels around them,
    // which texture filtering would otherwise blend in as a fringe.
    let recoloured = spread(&recoloured, width, height, 2);
    for (pixel, recolour) in logo.pixels_mut().zip(recoloured) {
        if recolour {
            pixel.0 = (rules.recolour)(pixel.0);
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

/// Light colours become a dark shade of themselves; dark ones are kept.
fn lower_pixel(pixel: [u8; 4]) -> [u8; 4] {
    let lightness = luma(pixel);
    let amount = ((lightness - 0.55) / 0.2).clamp(0.0, 1.0);
    if amount == 0.0 {
        return pixel;
    }
    let scale = LOGO_DARK_LUMA / lightness.max(0.01);
    let mut out = pixel;
    for channel in &mut out[..3] {
        let c = *channel as f32 / 255.0;
        *channel = ((c + (c * scale - c) * amount) * 255.0).round() as u8;
    }
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
    fn ambient_is_downscaled_and_keeps_its_brightness() {
        let source = solid(1920, 1080, [200, 180, 160, 255]);
        let out = ambient(&source);
        assert!(out.width() <= AMBIENT_WIDTH);
        assert_eq!(out.width() * 1080 / 1920, out.height());
        assert!((mean_luma(&out) - mean_luma(&source)).abs() < mean_luma(&source) * 0.03);
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
    fn on_light_white_lettering_is_lowered() {
        // A white word beside an orange bar.
        let mut logo = RgbaImage::new(400, 100);
        fill(&mut logo, 0..400, 0..20, ORANGE);
        fill(&mut logo, 40..200, 50..90, WHITE);
        let out = legible_logo_on_light(logo);
        assert!(luma_of(out.get_pixel(120, 70)) < 0.2, "word lowered");
        assert_eq!(out.get_pixel(10, 10).0, ORANGE, "colour untouched");
    }

    #[test]
    fn on_light_outlined_or_dark_logos_are_untouched() {
        // White letters in a black outline read on a light page too.
        let mut outlined = RgbaImage::new(400, 100);
        fill(&mut outlined, 20..380, 20..80, BLACK);
        fill(&mut outlined, 26..374, 26..74, WHITE);
        let before = outlined.clone();
        assert_eq!(legible_logo_on_light(outlined), before);
        let mut dark = RgbaImage::new(200, 60);
        fill(&mut dark, 40..160, 20..40, BLACK);
        let before = dark.clone();
        assert_eq!(legible_logo_on_light(dark), before);
    }

    #[test]
    fn on_light_lowering_keeps_a_hint_of_hue() {
        let mut logo = RgbaImage::new(200, 60);
        fill(&mut logo, 40..160, 20..40, [250, 240, 170, 255]);
        let pixel = *legible_logo_on_light(logo).get_pixel(100, 30);
        assert!(luma_of(&pixel) < 0.2, "lowered: {pixel:?}");
        assert!(pixel[0] > pixel[2], "still warm: {pixel:?}");
    }

    #[test]
    fn blurhash_decodes() {
        let image = blurhash_image("LEHV6nWB2yk8pyo0adR*.7kCMdnj", 32, 20).expect("valid hash");
        assert_eq!(image.dimensions(), (32, 20));
        assert!(blurhash_image("nonsense", 32, 20).is_none());
    }
}
