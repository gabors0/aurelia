use gpui_kit::prelude::*;
use gpui_kit::{Div, ElementId, FontWeight, SharedString, Stateful, div, hsla, px};

use crate::theme::Palette;

/// A selectable chip for sort and filter choices.
pub fn pill(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .h(px(32.))
        .flex()
        .items_center()
        .rounded_full()
        .cursor_pointer()
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .border_1()
        .when(active, |this| {
            this.bg(hsla(0., 0., 1., 0.92))
                .border_color(hsla(0., 0., 1., 0.92))
                .text_color(Palette::bg())
        })
        .when(!active, |this| {
            this.bg(Palette::glass())
                .border_color(Palette::border())
                .text_color(Palette::text_secondary())
                .hover(|this| this.text_color(Palette::text()).bg(Palette::glass_strong()))
        })
        .child(label.into())
}
