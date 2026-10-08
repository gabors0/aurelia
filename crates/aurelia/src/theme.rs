//! Aurelia's visual language: colour tokens, type, and the bridge into
//! gpui-component's theme so stock components match our look.

use std::borrow::Cow;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, Rgba, px, rgb, rgba};

pub const FONT: &str = "Inter";
pub const FONT_DISPLAY: &str = "Inter Display";

/// Fixed palette. Accent is dynamic per page (taken from artwork); this is the
/// fallback when artwork has no usable colour.
pub struct Palette;

impl Palette {
    pub fn bg() -> Rgba {
        rgb(0x0A0B10)
    }
    /// Slightly raised surface (cards without art, inputs).
    pub fn surface() -> Rgba {
        rgb(0x14161E)
    }
    pub fn glass() -> Rgba {
        rgba(0xFFFFFF0F)
    }
    pub fn glass_strong() -> Rgba {
        rgba(0xFFFFFF1A)
    }
    pub fn border() -> Rgba {
        rgba(0xFFFFFF17)
    }
    pub fn text() -> Rgba {
        rgb(0xF2F3F7)
    }
    pub fn text_secondary() -> Rgba {
        rgb(0xA3A7B5)
    }
    pub fn text_tertiary() -> Rgba {
        rgb(0x6B7080)
    }
    pub fn accent() -> Rgba {
        rgb(0xB69CFF)
    }
    pub fn danger() -> Rgba {
        rgb(0xFF6B81)
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

/// Registers bundled fonts and dresses gpui-component in Aurelia's colours.
/// Must run after `gpui_kit::init`.
pub fn init(cx: &mut App) {
    let fonts = FONTS.iter().map(|bytes| Cow::Borrowed(*bytes)).collect();
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        tracing::warn!("failed to register bundled fonts: {err:#}");
    }

    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |theme| {
        theme.font_family = FONT.into();
        theme.font_size = px(14.);
        theme.radius = px(10.);
        theme.radius_lg = px(14.);
        theme.shadow = true;

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
        c.primary_foreground = Palette::bg().into();
        c.button_primary = c.primary;
        c.button_primary_hover = c.primary_hover;
        c.button_primary_active = c.primary_active;
        c.button_primary_foreground = c.primary_foreground;
        c.secondary = Palette::glass_strong().into();
        c.secondary_hover = Hsla::white().opacity(0.16);
        c.secondary_active = Hsla::white().opacity(0.2);
        c.secondary_foreground = Palette::text().into();
        c.button_secondary = c.secondary;
        c.button_secondary_hover = c.secondary_hover;
        c.button_secondary_active = c.secondary_active;
        c.button_secondary_foreground = c.secondary_foreground;
        c.popover = Palette::surface().into();
        c.popover_foreground = Palette::text().into();
        c.list_hover = Hsla::white().opacity(0.06);
        c.list_active = accent.opacity(0.18);
        c.list_active_border = accent.opacity(0.5);
        c.accent = Hsla::white().opacity(0.08);
        c.accent_foreground = Palette::text().into();
        c.title_bar = Hsla::transparent_black();
        c.title_bar_border = Hsla::transparent_black();
        c.skeleton = Hsla::white().opacity(0.06);
        c.progress_bar = accent;
        c.scrollbar = Hsla::transparent_black();
        c.scrollbar_thumb = Hsla::white().opacity(0.18);
        c.scrollbar_thumb_hover = Hsla::white().opacity(0.3);
        c.overlay = Hsla::black().opacity(0.6);
        c.window_border = Palette::border().into();
    });
}
