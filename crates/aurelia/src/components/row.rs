//! A titled, horizontally scrolling shelf of cards.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, FontWeight, Pixels, ScrollHandle, SharedString, div, point, px};

use crate::components::key_nav;
use crate::theme::{FONT_DISPLAY, Palette};
use gpui_kit::TestSupportExt as _;

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
                .bg(Palette::control())
                .border_1()
                .border_color(Palette::control_border())
                .hover(|this| this.bg(Palette::control_hover()))
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
        // Cards rise into the space above them on hover; the padding inside
        // the shelf keeps them from being clipped.
        .gap_1p5()
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
                .child(key_nav::shelf(handle.clone()))
                .child(
                    div()
                        .id(id.clone())
                        .flex()
                        .gap_4()
                        .px(ROW_PADDING)
                        .pt(px(10.))
                        .pb_3()
                        .overflow_x_scroll()
                        // Without this GPUI turns vertical wheel motion into
                        // sideways scrolling here, hijacking page scrolls.
                        .restrict_scroll_to_axis()
                        .track_scroll(handle)
                        .children(cards)
                        .test_support(),
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
        .gap_4()
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

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Bounds, Context, IntoElement, ParentElement as _, Render, ScrollDelta,
        ScrollHandle, Styled as _, TestAppContext, TestSupportExt as _, Window, WindowBounds,
        WindowOptions, div, point, px, size,
    };

    use super::row;
    use crate::components::scroller::page;

    /// A page with a tall header and one shelf of cards wider than the window.
    struct Fixture {
        page: ScrollHandle,
        shelf: ScrollHandle,
    }

    impl Render for Fixture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let cards = (0..12)
                .map(|_| {
                    div()
                        .w(px(200.))
                        .h(px(120.))
                        .flex_shrink_0()
                        .into_any_element()
                })
                .collect();
            page("page", &self.page)
                .child(div().h(px(400.)))
                .child(row("shelf", "Shelf", &self.shelf, cards))
                .child(div().h(px(800.)))
                .test_support()
        }
    }

    fn open(cx: &mut TestAppContext) -> (gpui_kit::AnyWindowHandle, ScrollHandle, ScrollHandle) {
        cx.update(gpui_kit::init);
        let page = ScrollHandle::new();
        let shelf = ScrollHandle::new();
        let (handle, _) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(800.), px(600.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |_, cx| {
                    cx.new(|_| Fixture {
                        page: page.clone(),
                        shelf: shelf.clone(),
                    })
                },
            )
            .expect("open test window")
        });
        (handle, page, shelf)
    }

    #[gpui_kit::test]
    fn vertical_wheel_over_a_shelf_scrolls_only_the_page(cx: &mut TestAppContext) {
        let (window, page, shelf) = open(cx);
        cx.update_window(window, |_, window, cx| {
            window.scroll("shelf", ScrollDelta::Pixels(point(px(0.), px(-120.))), cx);
        })
        .unwrap();
        assert_eq!(shelf.offset().x, px(0.), "shelf must not move sideways");
        assert!(page.offset().y < px(0.), "page scrolls down");
    }

    #[gpui_kit::test]
    fn horizontal_wheel_over_a_shelf_scrolls_only_the_shelf(cx: &mut TestAppContext) {
        let (window, page, shelf) = open(cx);
        cx.update_window(window, |_, window, cx| {
            window.scroll("shelf", ScrollDelta::Pixels(point(px(-120.), px(0.))), cx);
        })
        .unwrap();
        assert!(shelf.offset().x < px(0.), "shelf scrolls sideways");
        assert_eq!(page.offset().y, px(0.), "page must not move vertically");
    }
}
