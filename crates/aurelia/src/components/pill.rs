use gpui_kit::prelude::*;
use gpui_kit::{ElementId, FontWeight, Hsla, SharedString, px};

use crate::components::button::focus_ring;
use crate::components::motion::{Pressable, pressable};
use crate::theme::Palette;

/// A selectable chip for sort and filter choices.
pub fn pill(id: impl Into<ElementId>, label: impl Into<SharedString>, active: bool) -> Pressable {
    let label = label.into();
    pressable(id).look(move |this, m| {
        let (bg, border, text): (Hsla, Hsla, Hsla) = if active {
            (Palette::inverse(), Palette::inverse(), Palette::bg().into())
        } else {
            (
                m.mix(Palette::glass(), Palette::glass_strong()),
                Palette::border().into(),
                m.mix(Palette::text_secondary(), Palette::text()),
            )
        };
        this.relative()
            .px_3()
            .h(px(32.))
            .flex()
            .items_center()
            .rounded_full()
            .cursor_pointer()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .border_1()
            .bg(bg)
            .border_color(border)
            .text_color(text)
            .child(focus_ring(m, px(16.)))
            .child(label)
    })
}
