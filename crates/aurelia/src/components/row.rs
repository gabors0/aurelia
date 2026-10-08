//! A titled, horizontally scrolling shelf of cards.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, FontWeight, Pixels, ScrollHandle, SharedString, div, hsla, point, px, rgba,
};

use crate::theme::{FONT_DISPLAY, Palette};

pub const ROW_PADDING: Pixels = px(48.);

fn scroll_by(handle: &ScrollHandle, direction: f32) {
    let bounds = handle.bounds();
    let step = (bounds.size.width * 0.8).max(px(200.));
    let max = handle.max_offset().x;
    let current = handle.offset().x;
    let next = (current - step * direction).clamp(-max, px(0.));
    handle.set_offset(point(next, handle.offset().y));
}

fn arrow(
    id: SharedString,
    icon: IconName,
    group: SharedString,
    handle: ScrollHandle,
    direction: f32,
) -> impl IntoElement {
    div()
        .id(id)
        .absolute()
        .top_0()
        .bottom_0()
        .when(direction < 0., |this| this.left_0())
        .when(direction > 0., |this| this.right_0())
        .w(ROW_PADDING)
        .flex()
        .items_center()
        .justify_center()
        .opacity(0.)
        .group_hover(group, |style| style.opacity(1.))
        .cursor_pointer()
        .on_click(move |_, window, _| {
            scroll_by(&handle, direction);
            window.refresh();
        })
        .child(
            div()
                .size(px(40.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(0x0A0B10CC))
                .border_1()
                .border_color(hsla(0., 0., 1., 0.18))
                .hover(|this| this.bg(rgba(0x22232ECC)))
                .child(Icon::new(icon).size_5().text_color(Palette::text())),
        )
}

pub fn row(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    handle: &ScrollHandle,
    cards: Vec<AnyElement>,
) -> impl IntoElement {
    let id = id.into();
    let group: SharedString = format!("row-{id}").into();
    let can_left = handle.offset().x < px(-1.);
    let can_right = handle.offset().x > -handle.max_offset().x + px(1.);
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .px(ROW_PADDING)
                .font_family(FONT_DISPLAY)
                .font_weight(FontWeight::BOLD)
                .text_size(px(20.))
                .child(title.into()),
        )
        .child(
            div()
                .group(group.clone())
                .relative()
                .child(
                    div()
                        .id(id.clone())
                        .flex()
                        .gap_4()
                        .px(ROW_PADDING)
                        .pt_1()
                        .pb_3()
                        .overflow_x_scroll()
                        .track_scroll(handle)
                        .children(cards),
                )
                .when(can_left, |this| {
                    this.child(arrow(
                        format!("{id}-left").into(),
                        IconName::ChevronLeft,
                        group.clone(),
                        handle.clone(),
                        -1.,
                    ))
                })
                .when(can_right, |this| {
                    this.child(arrow(
                        format!("{id}-right").into(),
                        IconName::ChevronRight,
                        group.clone(),
                        handle.clone(),
                        1.,
                    ))
                }),
        )
}

/// Placeholder shelf while loading.
pub fn skeleton_row(width: Pixels, height: Pixels) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .px(ROW_PADDING)
        .child(
            div()
                .w(px(180.))
                .h(px(22.))
                .rounded(px(6.))
                .bg(Palette::glass()),
        )
        .child(
            div()
                .flex()
                .gap_4()
                .overflow_hidden()
                .children((0..8).map(move |_| {
                    div()
                        .flex_shrink_0()
                        .w(width)
                        .h(height)
                        .rounded(px(12.))
                        .bg(Palette::glass())
                })),
        )
}
