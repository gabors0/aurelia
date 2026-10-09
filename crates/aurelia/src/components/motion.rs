//! Motion: a spring between an element's resting look and its hovered (or
//! keyboard-focused) look, and the keyboard focus that goes with it.
//!
//! `Pressable` wraps a `Stateful<Div>` and passes `on_click`, `tooltip`,
//! styling and children straight through to it, so it drops in wherever a
//! clickable div was built. Its look is drawn by a closure that receives the
//! spring's position, so colours, offsets and shadows move instead of
//! switching. With reduced motion GPUI snaps springs to their end.

use std::sync::Arc;

use gpui_kit::{
    AnimationExt as _, AnyElement, App, Div, ElementId, Hsla, InteractiveElement, Interactivity,
    IntoElement, ParentElement, RenderOnce, SharedString, SpringAnimation, SpringConfig, Stateful,
    StatefulInteractiveElement, StyleRefinement, Styled, Window, div,
};

use crate::components::key_nav::{self, TARGET_CONTEXT, Zone};

/// Quick and barely overshooting: hover feels immediate but not mechanical.
pub const SPRING: SpringConfig = SpringConfig::new(320., 30., 1.);

/// Where an element is between resting and active (hovered or focused).
#[derive(Debug, Clone, Copy)]
pub struct Motion {
    /// The spring's position: 0 resting, 1 active (it may overshoot a bit).
    pub t: f32,
    pub hovered: bool,
    /// Has keyboard focus (and the keyboard was used last).
    pub focused: bool,
}

impl Motion {
    /// `t`, kept within 0–1 for colours and opacities.
    pub fn amount(&self) -> f32 {
        self.t.clamp(0., 1.)
    }

    /// Blends from `rest` to `active`.
    pub fn mix(&self, rest: impl Into<Hsla>, active: impl Into<Hsla>) -> Hsla {
        mix(rest.into(), active.into(), self.amount())
    }

    /// Interpolates a number (overshoot included, for movement).
    pub fn lerp(&self, rest: f32, active: f32) -> f32 {
        rest + (active - rest) * self.t
    }
}

/// Linear blend in RGB, so a fade to transparent doesn't pass through other
/// hues.
pub fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b) = (gpui_kit::Rgba::from(a), gpui_kit::Rgba::from(b));
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    // Transparent ends take the other end's colour, so they fade, not darken.
    let (ar, ag, ab) = if a.a == 0. {
        (b.r, b.g, b.b)
    } else {
        (a.r, a.g, a.b)
    };
    let (br, bg, bb) = if b.a == 0. {
        (a.r, a.g, a.b)
    } else {
        (b.r, b.g, b.b)
    };
    gpui_kit::Rgba {
        r: lerp(ar, br),
        g: lerp(ag, bg),
        b: lerp(ab, bb),
        a: lerp(a.a, b.a),
    }
    .into()
}

type Look = Box<dyn FnOnce(Stateful<Div>, Motion) -> Stateful<Div>>;

/// A clickable surface with a sprung look and, by default, keyboard focus.
#[derive(IntoElement)]
pub struct Pressable {
    id: ElementId,
    base: Stateful<Div>,
    nav: Option<Zone>,
    primary: bool,
    look: Option<Look>,
}

/// A new pressable `div` with `id`.
pub fn pressable(id: impl Into<ElementId>) -> Pressable {
    let id = id.into();
    Pressable::new(id.clone(), div().id(id))
}

impl Pressable {
    /// Wraps `base`, whose element id must be `id`.
    pub fn new(id: ElementId, base: Stateful<Div>) -> Self {
        Self {
            id,
            base,
            nav: Some(Zone::Page),
            primary: false,
            look: None,
        }
    }

    /// Draws the element's look for a spring position.
    pub fn look(
        mut self,
        look: impl FnOnce(Stateful<Div>, Motion) -> Stateful<Div> + 'static,
    ) -> Self {
        self.look = Some(Box::new(look));
        self
    }

    /// Not reachable with the arrow keys (decoration, or reached otherwise).
    pub fn unfocusable(mut self) -> Self {
        self.nav = None;
        self
    }

    /// Part of the nav bar rather than the page.
    pub fn in_bar(mut self) -> Self {
        self.nav = Some(Zone::Bar);
        self
    }

    /// The page's main action: the first arrow press focuses it.
    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }

    fn child_id(&self, name: &'static str) -> ElementId {
        ElementId::NamedChild(Arc::new(self.id.clone()), SharedString::new_static(name))
    }
}

impl InteractiveElement for Pressable {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Pressable {}

impl Styled for Pressable {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl ParentElement for Pressable {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(elements);
    }
}

impl RenderOnce for Pressable {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let hover = window.use_keyed_state(self.child_id("hover"), cx, |_, _| false);
        let hovered = *hover.read(cx);
        let mut element = self.base.on_hover(move |now, _, cx| {
            hover.update(cx, |hovered, cx| {
                if *hovered != *now {
                    *hovered = *now;
                    cx.notify();
                }
            });
        });

        let mut focused = false;
        if let Some(zone) = self.nav {
            let focus = window
                .use_keyed_state(
                    ElementId::NamedChild(Arc::new(self.id.clone()), "focus".into()),
                    cx,
                    |_, cx| cx.focus_handle(),
                )
                .read(cx)
                .clone();
            focused = focus.is_focused(window) && window.last_input_was_keyboard();
            element = element
                .track_focus(&focus)
                .key_context(TARGET_CONTEXT)
                .child(key_nav::target(self.id.clone(), focus, zone, self.primary));
        }

        let Some(look) = self.look else {
            return element.into_any_element();
        };
        let active = hovered || focused;
        let spring_id = ElementId::NamedChild(Arc::new(self.id), "spring".into());
        element
            .with_spring(
                spring_id,
                SpringAnimation::new(SPRING).to(active),
                move |element, phase| {
                    look(
                        element,
                        Motion {
                            t: phase.0,
                            hovered,
                            focused,
                        },
                    )
                },
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Bounds, Context, Render, TestAppContext, TestSupportExt as _,
        WindowBounds, WindowOptions, hsla, px, size,
    };

    struct Card {
        seen: Rc<Cell<(f32, bool)>>,
    }

    impl Render for Card {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let seen = self.seen.clone();
            // Away from the corner, where the test pointer starts.
            div()
                .size_full()
                .p(px(120.))
                .child(
                    div()
                        .id("slot")
                        .test_support()
                        .child(pressable("card").unfocusable().look(move |this, m| {
                            seen.set((m.t, m.hovered));
                            this.size(px(120.))
                        })),
                )
        }
    }

    #[gpui_kit::test]
    fn hover_springs_to_the_active_look(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let seen = Rc::new(Cell::new((0., false)));
        let (window, _) = cx.update(|cx| {
            let seen = seen.clone();
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(400.), px(300.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| Card { seen }),
            )
            .expect("open test window")
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(seen.get(), (0., false), "rests until pointed at");
            window.hover("slot", cx);
            window.render_frame(cx);
        })
        .unwrap();
        assert!(seen.get().1, "knows it's hovered");
        // The spring runs on the clock: let it settle, then draw again.
        std::thread::sleep(std::time::Duration::from_millis(600));
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .unwrap();
        let (t, _) = seen.get();
        assert!((t - 1.).abs() < 0.02, "settled at the active look: {t}");
    }

    #[test]
    fn mixing_to_transparent_fades_without_changing_colour() {
        let red = hsla(0., 1., 0.5, 1.);
        let half = mix(red, gpui_kit::transparent_black(), 0.5);
        let rgb = gpui_kit::Rgba::from(half);
        assert!((rgb.a - 0.5).abs() < 0.01);
        assert!(rgb.r > 0.99 && rgb.g < 0.01, "still red, just fainter");
    }

    #[test]
    fn motion_clamps_colours_but_not_movement() {
        let overshoot = Motion {
            t: 1.05,
            hovered: true,
            focused: false,
        };
        assert_eq!(overshoot.amount(), 1.);
        assert!(overshoot.lerp(0., -6.) < -6.);
    }
}
