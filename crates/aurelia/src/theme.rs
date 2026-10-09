//! Aurelia's visual language: colour tokens for the dark and light themes,
//! type, and the bridge into gpui-component's theme so stock components
//! match our look.

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, Rgba, hsla, px, rgb, rgba};

pub const FONT: &str = "Inter";
pub const FONT_DISPLAY: &str = "Inter Display";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    Dark,
    Light,
}

/// The palette in use. Process-wide, so tokens can be read anywhere without
/// a context; `apply` changes it and refreshes the windows.
static LIGHT: AtomicBool = AtomicBool::new(false);

pub fn appearance() -> Appearance {
    if LIGHT.load(Ordering::Relaxed) {
        Appearance::Light
    } else {
        Appearance::Dark
    }
}

pub fn is_light() -> bool {
    appearance() == Appearance::Light
}

fn pick<T>(dark: T, light: T) -> T {
    if is_light() { light } else { dark }
}

/// Colour tokens. Accent is dynamic per page (taken from artwork); this is the
/// fallback when artwork has no usable colour.
pub struct Palette;

impl Palette {
    pub fn bg() -> Rgba {
        pick(rgb(0x0A0B10), rgb(0xF4F4F7))
    }
    /// The page colour at `alpha`, for fades over artwork.
    pub fn bg_alpha(alpha: f32) -> Hsla {
        Hsla::from(Self::bg()).opacity(alpha)
    }
    /// Slightly raised surface (cards without art, inputs, menus).
    pub fn surface() -> Rgba {
        pick(rgb(0x14161E), rgb(0xFFFFFF))
    }
    /// Translucent fill for chips, skeletons and quiet buttons.
    pub fn glass() -> Rgba {
        pick(rgba(0xFFFFFF0F), rgba(0x10122A0B))
    }
    pub fn glass_strong() -> Rgba {
        pick(rgba(0xFFFFFF1A), rgba(0x10122A14))
    }
    /// `glass_strong` under the pointer.
    pub fn glass_hover() -> Rgba {
        pick(rgba(0xFFFFFF29), rgba(0x10122A1F))
    }
    pub fn border() -> Rgba {
        pick(rgba(0xFFFFFF17), rgba(0x10122A1A))
    }
    pub fn text() -> Rgba {
        pick(rgb(0xF2F3F7), rgb(0x14151B))
    }
    /// Long-form text over artwork (overviews, biographies).
    pub fn text_body() -> Rgba {
        pick(rgb(0xDBDBDB), rgb(0x2B2D36))
    }
    /// Card captions: a touch softer than `text` until hovered.
    pub fn text_caption() -> Rgba {
        pick(rgb(0xE6E6E6), rgb(0x22242C))
    }
    pub fn text_secondary() -> Rgba {
        pick(rgb(0xA3A7B5), rgb(0x50545F))
    }
    pub fn text_tertiary() -> Rgba {
        pick(rgb(0x6B7080), rgb(0x80838F))
    }
    pub fn accent() -> Rgba {
        pick(rgb(0xB69CFF), rgb(0x6A4BD6))
    }
    pub fn danger() -> Rgba {
        pick(rgb(0xFF6B81), rgb(0xD12F4E))
    }
    /// The nav bar once the page scrolls under it: opaque, so text can't
    /// show through.
    pub fn bar() -> Rgba {
        pick(rgb(0x0C0D13), rgb(0xFBFBFC))
    }
    /// A floating panel over moving light (sign-in, profiles).
    pub fn panel() -> Rgba {
        pick(rgba(0x0F1018C0), rgba(0xFFFFFFD0))
    }
    /// Round controls floating over the page (shelf and slideshow arrows).
    pub fn control() -> Rgba {
        pick(rgba(0x0A0B10CC), rgba(0xFFFFFFE6))
    }
    pub fn control_hover() -> Rgba {
        pick(rgba(0x22232ECC), rgb(0xFFFFFF))
    }
    pub fn control_border() -> Hsla {
        pick(hsla(0., 0., 1., 0.18), hsla(0.65, 0.4, 0.1, 0.12))
    }
    /// A selected chip: the page's text colour as a solid fill.
    pub fn inverse() -> Hsla {
        Hsla::from(Self::text()).opacity(pick(0.92, 1.0))
    }
    /// Drop shadows are softer on a light page.
    pub fn shadow(dark_alpha: f32) -> Hsla {
        hsla(0., 0., 0., dark_alpha * pick(1.0, 0.45))
    }
    /// The gradient behind a card that has no artwork.
    pub fn art_fallback() -> (Rgba, Rgba) {
        pick(
            (rgb(0x23202F), rgb(0x12131A)),
            (rgb(0xE6E3EE), rgb(0xD7D8E0)),
        )
    }
    /// Veil over a page's ambient light, so the art only tints the page.
    pub fn ambient_veil() -> Hsla {
        Self::bg_alpha(pick(0.5, 0.6))
    }
    /// How strongly the blurred ambient art shows before the veil.
    pub fn ambient_opacity() -> f32 {
        pick(0.44, 0.32)
    }

    /// An accent taken from artwork, adjusted to keep its contrast on this
    /// theme's background.
    pub fn tune_accent(accent: Hsla) -> Hsla {
        tune_accent(accent, appearance())
    }
}

/// Artwork accents are extracted light (for a dark page); a light page needs
/// them darker. Dark and fully saturated turns garish, so it calms down too.
pub fn tune_accent(accent: Hsla, appearance: Appearance) -> Hsla {
    match appearance {
        Appearance::Dark => accent,
        Appearance::Light => hsla(accent.h, (accent.s * 0.8).clamp(0.4, 0.66), 0.42, accent.a),
    }
}

const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../assets/fonts/InterDisplay-SemiBold.ttf"),
    include_bytes!("../assets/fonts/InterDisplay-Bold.ttf"),
    include_bytes!("../assets/fonts/InterDisplay-ExtraBold.ttf"),
];

/// Registers bundled fonts and dresses gpui-component in Aurelia's dark
/// colours. Must run after `gpui_kit::init`; `settings::init` then applies
/// the chosen theme.
pub fn init(cx: &mut App) {
    let fonts = FONTS.iter().map(|bytes| Cow::Borrowed(*bytes)).collect();
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        tracing::warn!("failed to register bundled fonts: {err:#}");
    }
    apply(Appearance::Dark, cx);
}

/// Switches the palette and restyles gpui-component to match.
pub fn apply(appearance: Appearance, cx: &mut App) {
    LIGHT.store(appearance == Appearance::Light, Ordering::Relaxed);
    let light = appearance == Appearance::Light;
    Theme::change(
        if light {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        },
        None,
        cx,
    );
    Theme::update(cx, |theme| {
        theme.font_family = FONT.into();
        theme.font_size = px(14.);
        theme.radius = px(10.);
        theme.radius_lg = px(14.);
        theme.shadow = true;

        // Washes over the current surface: white on dark, ink on light.
        let wash = |alpha: f32| {
            if light {
                hsla(0.65, 0.4, 0.1, alpha)
            } else {
                Hsla::white().opacity(alpha)
            }
        };
        let accent: Hsla = Palette::accent().into();
        let c = &mut theme.colors;
        c.background = Palette::bg().into();
        c.foreground = Palette::text().into();
        c.muted = Palette::surface().into();
        c.muted_foreground = Palette::text_secondary().into();
        c.border = Palette::border().into();
        c.input = Palette::border().into();
        c.ring = accent.opacity(0.6);
        c.caret = accent;
        c.selection = accent.opacity(0.3);
        c.primary = accent;
        c.primary_hover = accent.blend(Hsla::white().opacity(0.12));
        c.primary_active = accent.blend(Hsla::black().opacity(0.12));
        c.primary_foreground = crate::components::button::on_color(accent);
        c.button_primary = c.primary;
        c.button_primary_hover = c.primary_hover;
        c.button_primary_active = c.primary_active;
        c.button_primary_foreground = c.primary_foreground;
        c.secondary = Palette::glass_strong().into();
        c.secondary_hover = wash(0.16);
        c.secondary_active = wash(0.2);
        c.secondary_foreground = Palette::text().into();
        c.button_secondary = c.secondary;
        c.button_secondary_hover = c.secondary_hover;
        c.button_secondary_active = c.secondary_active;
        c.button_secondary_foreground = c.secondary_foreground;
        c.popover = Palette::surface().into();
        c.popover_foreground = Palette::text().into();
        c.list_hover = wash(0.06);
        c.list_active = accent.opacity(0.18);
        c.list_active_border = accent.opacity(0.5);
        c.accent = wash(0.08);
        c.accent_foreground = Palette::text().into();
        // The title bar fades from a mix of this and the background; a
        // transparent black would darken the top of a light window.
        c.title_bar = Palette::bg_alpha(0.);
        c.title_bar_border = Hsla::transparent_black();
        c.skeleton = wash(0.06);
        c.progress_bar = accent;
        c.scrollbar = Hsla::transparent_black();
        c.scrollbar_thumb = wash(0.18);
        c.scrollbar_thumb_hover = wash(0.3);
        c.overlay = Hsla::black().opacity(if light { 0.35 } else { 0.6 });
        c.window_border = Palette::border().into();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_accents_are_darker_but_keep_their_hue() {
        let art = hsla(0.6, 0.7, 0.7, 1.0);
        assert_eq!(tune_accent(art, Appearance::Dark), art);
        let light = tune_accent(art, Appearance::Light);
        assert_eq!(light.h, art.h);
        assert!(light.l < 0.5, "dark enough for white text and light pages");
        assert!(light.s <= art.s, "no more vivid than the art");
    }
}
