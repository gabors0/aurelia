//! Cinematic buttons: a solid accent pill for the main action and glass
//! buttons for the rest.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::*;
use gpui_kit::{Div, ElementId, FontWeight, Hsla, SharedString, Stateful, div, hsla, px};

use crate::theme::Palette;

/// Readable text colour on top of `background`.
pub fn on_color(background: Hsla) -> Hsla {
    if background.l > 0.55 {
        hsla(0., 0., 0.06, 1.)
    } else {
        hsla(0., 0., 1., 1.)
    }
}

pub fn play_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    accent: Hsla,
) -> Stateful<Div> {
    let foreground = on_color(accent);
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .h(px(48.))
        .pl_5()
        .pr_6()
        .rounded_full()
        .cursor_pointer()
        .bg(accent)
        .text_color(foreground)
        .font_weight(FontWeight::SEMIBOLD)
        .shadow(vec![gpui_kit::BoxShadow {
            color: accent.opacity(0.35),
            offset: gpui_kit::point(px(0.), px(10.)),
            blur_radius: px(30.),
            spread_radius: px(-6.),
            inset: false,
        }])
        .hover(move |this| this.bg(accent.blend(hsla(0., 0., 1., 0.15))))
        .active(move |this| this.bg(accent.blend(hsla(0., 0., 0., 0.12))))
        .child(Icon::new(IconName::Play).size_5().text_color(foreground))
        .child(label.into())
}

pub fn glass_button(
    id: impl Into<ElementId>,
    icon: Option<IconName>,
    label: impl Into<SharedString>,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .h(px(48.))
        .px_5()
        .rounded_full()
        .cursor_pointer()
        .bg(Palette::glass_strong())
        .border_1()
        .border_color(Palette::border())
        .text_color(Palette::text())
        .font_weight(FontWeight::MEDIUM)
        .hover(|this| this.bg(hsla(0., 0., 1., 0.16)))
        .active(|this| this.bg(hsla(0., 0., 1., 0.22)))
        .when_some(icon, |this, icon| this.child(Icon::new(icon).size_5()))
        .child(label.into())
}

/// Circular toggle (watched, favourite).
pub fn round_button(
    id: impl Into<ElementId>,
    icon: IconName,
    active: bool,
    accent: Hsla,
) -> Stateful<Div> {
    div()
        .id(id)
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
        .bg(if active {
            accent.opacity(0.22)
        } else {
            Palette::glass_strong().into()
        })
        .text_color(if active {
            accent
        } else {
            Palette::text().into()
        })
        .hover(|this| this.bg(hsla(0., 0., 1., 0.16)))
        .child(Icon::new(icon).size_5())
}
