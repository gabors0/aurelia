//! Cinematic buttons: a solid accent pill for the main action and glass
//! buttons for the rest. All ease between states and take keyboard focus.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::*;
use gpui_kit::{
    BoxShadow, ElementId, FontWeight, Hsla, Pixels, SharedString, div, hsla, point, px,
};

use crate::components::motion::{Motion, Pressable, mix, pressable};
use crate::theme::Palette;

/// Readable text colour on top of `background`: white or near-black,
/// whichever contrasts more.
pub fn on_color(background: Hsla) -> Hsla {
    let rgb = gpui_kit::Rgba::from(background);
    let linear = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * linear(rgb.r) + 0.7152 * linear(rgb.g) + 0.0722 * linear(rgb.b);
    let on_white = 1.05 / (luminance + 0.05);
    let on_black = (luminance + 0.05) / 0.05;
    if on_black > on_white {
        hsla(0., 0., 0.06, 1.)
    } else {
        hsla(0., 0., 1., 1.)
    }
}

/// The keyboard focus ring: an outline just outside an element with corner
/// `radius`, shown while it has keyboard focus. Takes no layout space.
pub fn focus_ring(motion: Motion, radius: Pixels) -> impl IntoElement {
    let gap = px(4.);
    let color = Hsla::from(Palette::text()).opacity(if motion.focused {
        0.9 * motion.amount()
    } else {
        0.
    });
    div()
        .absolute()
        .top(-gap)
        .left(-gap)
        .right(-gap)
        .bottom(-gap)
        .rounded(radius + gap)
        .border_2()
        .border_color(color)
}

pub fn play_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    accent: Hsla,
) -> Pressable {
    let label = label.into();
    let foreground = on_color(accent);
    let hover = accent.blend(hsla(0., 0., 1., 0.15));
    pressable(id).primary().look(move |this, m| {
        this.relative()
            .flex()
            .items_center()
            .gap_2()
            .h(px(48.))
            .pl_5()
            .pr_6()
            .rounded_full()
            .cursor_pointer()
            .bg(mix(accent, hover, m.amount()))
            .text_color(foreground)
            .font_weight(FontWeight::SEMIBOLD)
            .shadow(vec![BoxShadow {
                color: accent.opacity(0.3 + 0.2 * m.amount()),
                offset: point(px(0.), px(10.)),
                blur_radius: px(30. + 8. * m.amount()),
                spread_radius: px(-6.),
                inset: false,
            }])
            .active(move |this| this.bg(accent.blend(hsla(0., 0., 0., 0.12))))
            .child(focus_ring(m, px(24.)))
            .child(Icon::new(IconName::Play).size_5().text_color(foreground))
            .child(label)
    })
}

pub fn glass_button(
    id: impl Into<ElementId>,
    icon: Option<IconName>,
    label: impl Into<SharedString>,
) -> Pressable {
    let label = label.into();
    pressable(id).look(move |this, m| {
        this.relative()
            .flex()
            .items_center()
            .gap_2()
            .h(px(48.))
            .px_5()
            .rounded_full()
            .cursor_pointer()
            .bg(m.mix(Palette::glass_strong(), Palette::glass_hover()))
            .border_1()
            .border_color(Palette::border())
            .text_color(Palette::text())
            .font_weight(FontWeight::MEDIUM)
            .active(|this| this.bg(Palette::glass_hover()))
            .child(focus_ring(m, px(24.)))
            .when_some(icon, |this, icon| this.child(Icon::new(icon).size_5()))
            .child(label)
    })
}

/// Circular toggle (watched, favourite).
pub fn round_button(
    id: impl Into<ElementId>,
    icon: IconName,
    active: bool,
    accent: Hsla,
) -> Pressable {
    pressable(id).look(move |this, m| {
        let (rest, hover) = if active {
            (accent.opacity(0.22), accent.opacity(0.32))
        } else {
            (
                Palette::glass_strong().into(),
                Palette::glass_hover().into(),
            )
        };
        this.relative()
            .size(px(48.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .border_1()
            .border_color(if active {
                accent.opacity(0.6)
            } else {
                Palette::border().into()
            })
            .bg(mix(rest, hover, m.amount()))
            .text_color(if active {
                accent
            } else {
                Palette::text().into()
            })
            .child(focus_ring(m, px(24.)))
            .child(Icon::new(icon).size_5())
    })
}

/// A small round icon button for bars (back, search, refresh).
pub fn icon_button(id: impl Into<ElementId>, icon: IconName, selected: bool) -> Pressable {
    pressable(id).look(move |this, m| {
        this.relative()
            .size_8()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .bg(mix(
                if selected {
                    Palette::glass_strong().into()
                } else {
                    gpui_kit::transparent_black()
                },
                Palette::glass_strong().into(),
                m.amount(),
            ))
            .child(focus_ring(m, px(16.)))
            .child(Icon::new(icon).size_4().text_color(if selected {
                Palette::text().into()
            } else {
                m.mix(Palette::text_secondary(), Palette::text())
            }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_on_colour_follows_contrast() {
        // Light lavender (the dark theme's accent) takes dark text.
        assert!(on_color(gpui_kit::rgb(0xB69CFF).into()).l < 0.5);
        // A mid violet that HSL calls "light" (l ≈ 0.57) still needs white.
        assert!(on_color(gpui_kit::rgb(0x6A4BD6).into()).l > 0.5);
        assert!(on_color(hsla(0., 0., 0.95, 1.)).l < 0.5);
        assert!(on_color(hsla(0.6, 0.6, 0.2, 1.)).l > 0.5);
    }
}
