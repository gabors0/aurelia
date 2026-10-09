//! Keyboard navigation: the arrow keys move focus between whatever is on
//! screen by position, like a TV interface, and Enter/Space press it.
//!
//! Every focusable `Pressable` records its bounds here each frame (the
//! registry is cleared by `frame_start`, drawn first in the window). Shelves
//! record their horizontal scroll handle and the shell records the page's
//! vertical one, so a target that gains focus can scroll itself into view.
//! Pressing happens through GPUI's own keyboard click: a focused element with
//! `on_click` is clicked by Enter/Space, so no handler is written twice.

use std::time::{Duration, Instant};

use gpui_kit::{
    App, Bounds, ElementId, FocusHandle, Global, IntoElement, KeyBinding, NoAction, Pixels,
    ScrollHandle, Styled as _, Window, canvas, point, px,
};

/// Key context of every nav target.
pub const TARGET_CONTEXT: &str = "NavTarget";

gpui_kit::actions!(aurelia, [NavUp, NavDown, NavLeft, NavRight]);

/// Arrow keys for views that host nav targets (the shell, the profile
/// picker), and Enter/Space for the targets themselves. Tab and Shift+Tab
/// come from gpui-base's root (`focus_next`/`focus_prev`).
pub fn bind_keys(contexts: &[&str], cx: &mut App) {
    for context in contexts {
        let context = Some(*context);
        cx.bind_keys([
            KeyBinding::new("up", NavUp, context),
            KeyBinding::new("down", NavDown, context),
            KeyBinding::new("left", NavLeft, context),
            KeyBinding::new("right", NavRight, context),
        ]);
    }
    // A focused target takes Enter/Space for itself (GPUI's keyboard click),
    // ahead of the shell's Enter-to-play.
    cx.bind_keys([
        KeyBinding::new("enter", NoAction, Some(TARGET_CONTEXT)),
        KeyBinding::new("space", NoAction, Some(TARGET_CONTEXT)),
    ]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// Separate areas: the floating nav bar and the page under it. Up/Down cross
/// from one to the other only when nothing is left in that direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Page,
    Bar,
}

struct Target {
    key: ElementId,
    focus: FocusHandle,
    bounds: Bounds<Pixels>,
    zone: Zone,
    primary: bool,
}

struct Shelf {
    handle: ScrollHandle,
    bounds: Bounds<Pixels>,
}

#[derive(Default)]
pub struct KeyNav {
    targets: Vec<Target>,
    shelves: Vec<Shelf>,
    page: Option<(ScrollHandle, Pixels)>,
    /// Where keys go when the focused target disappears (the shell).
    fallback: Option<FocusHandle>,
    /// The target that had focus this frame, and the one before.
    focused: Option<FocusHandle>,
    was_focused: Option<FocusHandle>,
    /// A target is taking focus back this frame (`restore`).
    restoring: bool,
    /// The focused target most recently scrolled into view.
    revealed: Option<FocusHandle>,
    /// A target to focus as soon as it's drawn (going back to a page), and
    /// how many more frames to keep trying: the first frame after a window
    /// opens is drawn again from scratch, with new focus handles.
    pending: Option<(ElementId, u8)>,
    /// Bumped by every scroll animation; older ones stop.
    scrolls: u64,
}

impl Global for KeyNav {}

fn nav_mut(cx: &mut App) -> Option<&mut KeyNav> {
    if cx.has_global::<KeyNav>() {
        Some(cx.global_mut::<KeyNav>())
    } else {
        None
    }
}

pub fn init(cx: &mut App) {
    cx.set_global(KeyNav::default());
}

/// Drawn first in the window: forgets last frame's targets.
pub fn frame_start() -> impl IntoElement {
    canvas(
        |_, _, cx| {
            if let Some(nav) = nav_mut(cx) {
                nav.targets.clear();
                nav.shelves.clear();
                nav.page = None;
                nav.fallback = None;
                nav.was_focused = nav.focused.take();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_0()
}

/// Drawn last in the window. If the focused target went away (its slide
/// changed, its shelf reloaded), keys would go nowhere: focus falls back to
/// the view hosting the targets.
pub fn frame_end() -> impl IntoElement {
    canvas(
        |_, window, cx| {
            let Some(nav) = nav_mut(cx) else {
                return;
            };
            let Some(fallback) = nav.fallback.clone() else {
                return;
            };
            // A target had focus last frame and wasn't drawn this one, and
            // focus is still on it (or on a handle already released).
            // Nothing focused (e.g. at launch), or a target had focus last
            // frame and wasn't drawn this one while focus is still on it.
            let vanished = nav.focused.is_none() && nav.was_focused.is_some();
            let was_focused = nav.was_focused.clone();
            let restoring = std::mem::take(&mut nav.restoring);
            let lost = !restoring
                && match window.focused(cx) {
                    None => true,
                    Some(focused) => vanished && was_focused.as_ref() == Some(&focused),
                };
            if lost {
                tracing::debug!(
                    nothing_focused = window.focused(cx).is_none(),
                    vanished,
                    "focus returns to its view"
                );
                window.defer(cx, move |window, cx| window.focus(&fallback, cx));
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_0()
}

/// Drawn first by a view that hosts targets (the shell, the profile picker):
/// its focus, for keys to fall back to, and its page's vertical scroll handle
/// with how far the nav bar covers its top.
pub fn scope(
    fallback: FocusHandle,
    page: Option<ScrollHandle>,
    covered: Pixels,
) -> impl IntoElement {
    canvas(
        move |_, _, cx| {
            if let Some(nav) = nav_mut(cx) {
                nav.fallback = Some(fallback);
                nav.page = page.map(|page| (page, covered));
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_0()
}

/// An invisible layer that records a shelf's visible bounds.
pub fn shelf(handle: ScrollHandle) -> impl IntoElement {
    canvas(
        move |bounds, _, cx| {
            if let Some(nav) = nav_mut(cx) {
                nav.shelves.push(Shelf {
                    handle: handle.clone(),
                    bounds,
                });
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

/// An invisible layer that records a target's bounds, and scrolls it into
/// view (or focuses it) when that's due.
pub fn target(key: ElementId, focus: FocusHandle, zone: Zone, primary: bool) -> impl IntoElement {
    canvas(
        move |bounds, window, cx| {
            let focused = focus.is_focused(window);
            let Some(nav) = nav_mut(cx) else {
                return;
            };
            let reveal = focused && nav.revealed.as_ref() != Some(&focus);
            if reveal {
                nav.revealed = Some(focus.clone());
            }
            if focused {
                nav.focused = Some(focus.clone());
            }
            let mut restore = false;
            if let Some((pending, tries)) = nav.pending.as_mut()
                && *pending == key
            {
                if focused || *tries == 0 {
                    nav.pending = None;
                } else {
                    *tries -= 1;
                    restore = true;
                    nav.restoring = true;
                }
            }
            nav.targets.push(Target {
                key,
                focus: focus.clone(),
                bounds,
                zone,
                primary,
            });
            if reveal {
                window.defer(cx, reveal_focused);
            }
            if restore {
                window.defer(cx, move |window, cx| window.focus(&focus, cx));
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

/// The key of the focused target, to restore with `restore` later.
pub fn focused_key(window: &Window, cx: &App) -> Option<ElementId> {
    let nav = cx.try_global::<KeyNav>()?;
    nav.targets
        .iter()
        .find(|t| t.focus.is_focused(window))
        .map(|t| t.key.clone())
}

/// Whether a nav target has focus.
pub fn has_focus(window: &Window, cx: &App) -> bool {
    focused_key(window, cx).is_some()
}

/// Focuses the target with `key` once it's drawn.
pub fn restore(key: Option<ElementId>, cx: &mut App) {
    if let Some(nav) = nav_mut(cx) {
        nav.pending = key.map(|key| (key, 10));
        nav.revealed = None;
    }
}

/// Moves focus one step in `direction`. With nothing focused, focuses the
/// page's primary target (Play) if it's in view, else the first one.
pub fn step(direction: Direction, window: &mut Window, cx: &mut App) {
    let Some(nav) = cx.try_global::<KeyNav>() else {
        return;
    };
    let rects: Vec<Rect> = nav.targets.iter().map(|t| Rect::from(t.bounds)).collect();
    let next = match nav.targets.iter().position(|t| t.focus.is_focused(window)) {
        Some(current) => {
            let zone = nav.targets[current].zone;
            let in_zone = |same: bool| {
                rects
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != current && (nav.targets[*i].zone == zone) == same)
                    .map(|(i, r)| (i, *r))
                    .collect::<Vec<_>>()
            };
            pick(rects[current], direction, &in_zone(true)).or_else(|| {
                matches!(direction, Direction::Up | Direction::Down)
                    .then(|| pick(rects[current], direction, &in_zone(false)))
                    .flatten()
            })
        }
        None => {
            let viewport = nav
                .page
                .as_ref()
                .map(|(handle, covered)| {
                    let mut bounds = Rect::from(handle.bounds());
                    bounds.top = bounds.top.max(f32::from(*covered));
                    bounds
                })
                .unwrap_or_else(|| {
                    Rect::from(Bounds::new(point(px(0.), px(0.)), window.viewport_size()))
                });
            let visible: Vec<(usize, Rect)> = rects
                .iter()
                .enumerate()
                .filter(|(i, r)| nav.targets[*i].zone == Zone::Page && r.inside(&viewport))
                .map(|(i, r)| (i, *r))
                .collect();
            visible
                .iter()
                .rev()
                .find(|(i, _)| nav.targets[*i].primary)
                .map(|(i, _)| *i)
                .or_else(|| first(&visible))
        }
    };
    tracing::debug!(
        targets = nav.targets.len(),
        focused = ?nav.targets.iter().position(|t| t.focus.is_focused(window)),
        next = ?next.map(|i| &nav.targets[i].key),
        "key nav {direction:?}"
    );
    if let Some(index) = next {
        let focus = nav.targets[index].focus.clone();
        window.focus(&focus, cx);
        window.refresh();
    }
}

/// Scrolls the focused target into view: within its shelf, then the page.
fn reveal_focused(window: &mut Window, cx: &mut App) {
    let Some(nav) = cx.try_global::<KeyNav>() else {
        return;
    };
    let Some(target) = nav.targets.iter().find(|t| t.focus.is_focused(window)) else {
        return;
    };
    if target.zone != Zone::Page {
        return;
    }
    let rect = Rect::from(target.bounds);
    let mut moves: Vec<(ScrollHandle, bool, f32)> = Vec::new();
    let center_y = (rect.top + rect.bottom) / 2.;
    if let Some(shelf) = nav.shelves.iter().find(|s| {
        let b = Rect::from(s.bounds);
        b.top <= center_y && center_y <= b.bottom
    }) {
        let b = Rect::from(shelf.bounds);
        let pad = f32::from(crate::components::row::ROW_PADDING);
        let shift = shift_into(rect.left, rect.right, b.left + pad, b.right - pad);
        if shift != 0. {
            let max = f32::from(shelf.handle.max_offset().x);
            let to = (f32::from(shelf.handle.offset().x) + shift).clamp(-max, 0.);
            moves.push((shelf.handle.clone(), true, to));
        }
    }
    if let Some((page, covered)) = &nav.page {
        let b = Rect::from(page.bounds());
        let top = (b.top + 12.).max(f32::from(*covered) + 16.);
        let shift = shift_into(rect.top, rect.bottom, top, b.bottom - 96.);
        if shift != 0. {
            let max = f32::from(page.max_offset().y);
            let to = (f32::from(page.offset().y) + shift).clamp(-max, 0.);
            moves.push((page.clone(), false, to));
        }
    }
    if !moves.is_empty() {
        scroll(moves, window, cx);
    }
}

/// How far to move a span so `[start, end]` sits within `[min, max]`;
/// positive moves it down/right.
fn shift_into(start: f32, end: f32, min: f32, max: f32) -> f32 {
    if start < min {
        min - start
    } else if end > max {
        // Never push the start out to show the end.
        (max - end).max(min - start)
    } else {
        0.
    }
}

const SCROLL_TIME: Duration = Duration::from_millis(220);

/// Eases each handle to its new offset (`true`: horizontal); jumps when
/// motion is reduced.
fn scroll(moves: Vec<(ScrollHandle, bool, f32)>, window: &mut Window, cx: &mut App) {
    let set = |handle: &ScrollHandle, horizontal: bool, value: f32| {
        let offset = handle.offset();
        handle.set_offset(if horizontal {
            point(px(value), offset.y)
        } else {
            point(offset.x, px(value))
        });
    };
    if cx.reduce_motion() {
        for (handle, horizontal, to) in &moves {
            set(handle, *horizontal, *to);
        }
        window.refresh();
        return;
    }
    let nav = cx.global_mut::<KeyNav>();
    nav.scrolls += 1;
    let generation = nav.scrolls;
    let starts: Vec<f32> = moves
        .iter()
        .map(|(handle, horizontal, _)| {
            let offset = handle.offset();
            f32::from(if *horizontal { offset.x } else { offset.y })
        })
        .collect();
    let started = Instant::now();
    window
        .spawn(cx, async move |cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(8))
                    .await;
                let t = (started.elapsed().as_secs_f32() / SCROLL_TIME.as_secs_f32()).min(1.);
                let eased = 1. - (1. - t).powi(3);
                let alive = cx
                    .update(|window, cx| {
                        if cx.global::<KeyNav>().scrolls != generation {
                            return false;
                        }
                        for ((handle, horizontal, to), from) in moves.iter().zip(&starts) {
                            set(handle, *horizontal, from + (to - from) * eased);
                        }
                        window.refresh();
                        true
                    })
                    .unwrap_or(false);
                if !alive || t >= 1. {
                    break;
                }
            }
        })
        .detach();
}

/// A rectangle in window coordinates, as plain numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Rect {
    pub fn new(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self {
            left,
            top,
            right: left + width,
            bottom: top + height,
        }
    }

    fn center_x(&self) -> f32 {
        (self.left + self.right) / 2.
    }

    fn center_y(&self) -> f32 {
        (self.top + self.bottom) / 2.
    }

    /// Mostly within `outer` (at least half of it on both axes).
    fn inside(&self, outer: &Rect) -> bool {
        let overlap = |a0: f32, a1: f32, b0: f32, b1: f32| (a1.min(b1) - a0.max(b0)).max(0.);
        let w = overlap(self.left, self.right, outer.left, outer.right);
        let h = overlap(self.top, self.bottom, outer.top, outer.bottom);
        w * 2. >= self.right - self.left && h * 2. >= self.bottom - self.top
    }
}

impl From<Bounds<Pixels>> for Rect {
    fn from(bounds: Bounds<Pixels>) -> Self {
        Rect::new(
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        )
    }
}

/// Things this close to the next row's top still count as that row.
const ROW_BAND: f32 = 32.;
/// Edges may overlap this much and still count as beside/below.
const SLACK: f32 = 8.;

/// The candidate to move to from `from` in `direction`, by index.
///
/// Left/Right stay in the same row (vertical overlap) and take the nearest
/// edge. Up/Down take the nearest row in that direction, then the candidate
/// that overlaps `from` horizontally (or is nearest to it).
pub fn pick(from: Rect, direction: Direction, candidates: &[(usize, Rect)]) -> Option<usize> {
    let gap = |c: &Rect| match direction {
        Direction::Down => c.top - from.bottom,
        Direction::Up => from.top - c.bottom,
        Direction::Right => c.left - from.right,
        Direction::Left => from.left - c.right,
    };
    // Reversed: of equally good candidates, the one drawn last (on top) wins.
    let ahead = candidates.iter().rev().filter(|(_, c)| match direction {
        Direction::Down => c.top >= from.bottom - SLACK && c.center_y() > from.center_y(),
        Direction::Up => c.bottom <= from.top + SLACK && c.center_y() < from.center_y(),
        Direction::Right => c.left >= from.right - SLACK && c.center_x() > from.center_x(),
        Direction::Left => c.right <= from.left + SLACK && c.center_x() < from.center_x(),
    });
    match direction {
        Direction::Left | Direction::Right => ahead
            .filter(|(_, c)| c.top < from.bottom - 1. && c.bottom > from.top + 1.)
            .min_by(|(_, a), (_, b)| {
                let key = |c: &Rect| (gap(c).max(0.), (c.center_y() - from.center_y()).abs());
                key(a).partial_cmp(&key(b)).unwrap()
            })
            .map(|(i, _)| *i),
        Direction::Up | Direction::Down => {
            let ahead: Vec<&(usize, Rect)> = ahead.collect();
            let nearest = ahead
                .iter()
                .map(|(_, c)| gap(c).max(0.))
                .fold(f32::INFINITY, f32::min);
            ahead
                .into_iter()
                .filter(|(_, c)| gap(c).max(0.) <= nearest + ROW_BAND)
                .min_by(|(_, a), (_, b)| {
                    let key = |c: &Rect| {
                        let apart = (c.left.max(from.left) - c.right.min(from.right)).max(0.);
                        (apart, (c.center_x() - from.center_x()).abs())
                    };
                    key(a).partial_cmp(&key(b)).unwrap()
                })
                .map(|(i, _)| *i)
        }
    }
}

/// The top-left candidate (reading order: the top row, then leftmost).
fn first(candidates: &[(usize, Rect)]) -> Option<usize> {
    let top = candidates
        .iter()
        .map(|(_, r)| r.top)
        .fold(f32::INFINITY, f32::min);
    candidates
        .iter()
        .filter(|(_, r)| r.top <= top + ROW_BAND)
        .min_by(|(_, a), (_, b)| a.left.partial_cmp(&b.left).unwrap())
        .map(|(i, _)| *i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Context, InteractiveElement as _, IntoElement, KeyDownEvent, KeyUpEvent,
        Keystroke, ParentElement as _, PlatformInput, Render, StatefulInteractiveElement as _,
        TestAppContext, TestSupportExt as _, WindowBounds, WindowOptions, div, size,
    };

    use crate::components::motion::pressable;

    /// A row of three pressables in a view that hosts keyboard navigation.
    struct Row {
        focus: FocusHandle,
        pressed: Rc<RefCell<Vec<usize>>>,
    }

    impl Render for Row {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let tiles = (0..3).map(|i| {
                let pressed = self.pressed.clone();
                div().id(("slot", i)).test_support().child(
                    pressable(("tile", i))
                        .on_click(move |_, _, _| pressed.borrow_mut().push(i))
                        .look(|this, _| this.size(px(100.))),
                )
            });
            div()
                .size_full()
                .track_focus(&self.focus)
                .key_context("NavTest")
                .on_action(|_: &NavRight, window, cx| step(Direction::Right, window, cx))
                .on_action(|_: &NavLeft, window, cx| step(Direction::Left, window, cx))
                .child(frame_start())
                .child(scope(self.focus.clone(), None, px(0.)))
                .child(div().flex().gap_4().p_4().children(tiles))
                .child(frame_end())
        }
    }

    fn press(window: &mut Window, key: &str, cx: &mut App) {
        let keystroke = Keystroke::parse(key).unwrap();
        window.dispatch_event(
            PlatformInput::KeyDown(KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            }),
            cx,
        );
        window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
        window.render_frame(cx);
    }

    #[gpui_kit::test]
    fn arrows_move_focus_and_enter_presses(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            init(cx);
            bind_keys(&["NavTest"], cx);
        });
        let pressed = Rc::new(RefCell::new(Vec::new()));
        let (window, row) = cx.update(|cx| {
            let pressed = pressed.clone();
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(600.), px(300.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |_, cx| {
                    cx.new(|cx| Row {
                        focus: cx.focus_handle(),
                        pressed,
                    })
                },
            )
            .expect("open test window")
        });
        cx.update_window(window, |_, window, cx| {
            let focus = row.read(cx).focus.clone();
            window.focus(&focus, cx);
            window.render_frame(cx);
            // Nothing focused yet: the first press focuses the first tile,
            // then each press moves one along, stopping at the end.
            press(window, "right", cx);
            assert_eq!(focused_key(window, cx), Some(("tile", 0usize).into()));
            press(window, "right", cx);
            press(window, "right", cx);
            press(window, "right", cx);
            assert_eq!(focused_key(window, cx), Some(("tile", 2usize).into()));
            press(window, "left", cx);
            assert_eq!(focused_key(window, cx), Some(("tile", 1usize).into()));
            press(window, "enter", cx);
        })
        .unwrap();
        assert_eq!(*pressed.borrow(), [1], "Enter clicked the focused tile");
    }

    /// Two shelves: four posters, then three wider stills further down.
    fn layout() -> Vec<(usize, Rect)> {
        let mut out = Vec::new();
        for i in 0..4 {
            out.push((i, Rect::new(48. + i as f32 * 190., 100., 172., 300.)));
        }
        for i in 0..3 {
            out.push((4 + i, Rect::new(48. + i as f32 * 330., 460., 316., 220.)));
        }
        out
    }

    fn from(index: usize) -> (Rect, Vec<(usize, Rect)>) {
        let all = layout();
        let rect = all[index].1;
        (rect, all.into_iter().filter(|(i, _)| *i != index).collect())
    }

    #[test]
    fn left_and_right_stay_in_the_row_without_wrapping() {
        let (rect, others) = from(1);
        assert_eq!(pick(rect, Direction::Right, &others), Some(2));
        assert_eq!(pick(rect, Direction::Left, &others), Some(0));
        let (rect, others) = from(3);
        assert_eq!(pick(rect, Direction::Right, &others), None);
        let (rect, others) = from(4);
        assert_eq!(pick(rect, Direction::Left, &others), None);
    }

    #[test]
    fn down_goes_to_the_next_row_under_the_card() {
        // Poster 2 spans 428–600; still 5 spans 378–694.
        let (rect, others) = from(2);
        assert_eq!(pick(rect, Direction::Down, &others), Some(5));
        let (rect, others) = from(0);
        assert_eq!(pick(rect, Direction::Down, &others), Some(4));
        let (rect, others) = from(6);
        assert_eq!(pick(rect, Direction::Down, &others), None);
    }

    #[test]
    fn up_prefers_the_overlapping_card_then_the_nearest() {
        // Still 5 overlaps posters 2 and 3 (428–600, 618–790): 2 overlaps more
        // of its centre.
        let (rect, others) = from(5);
        assert_eq!(pick(rect, Direction::Up, &others), Some(2));
        // A card past the end of the row above still finds the last one.
        let far = Rect::new(1200., 460., 316., 220.);
        assert_eq!(pick(far, Direction::Up, &layout()), Some(3));
    }

    #[test]
    fn rows_are_banded_so_ragged_tops_still_count() {
        // A pill row with one slightly lower chip, then a card row.
        let candidates = vec![
            (0, Rect::new(48., 100., 80., 32.)),
            (1, Rect::new(300., 110., 80., 32.)),
            (2, Rect::new(48., 300., 172., 260.)),
        ];
        let start = Rect::new(320., 20., 60., 40.);
        assert_eq!(pick(start, Direction::Down, &candidates), Some(1));
    }

    #[test]
    fn reading_order_starts_top_left() {
        assert_eq!(first(&layout()), Some(0));
        assert_eq!(first(&[]), None);
    }

    #[test]
    fn shifts_bring_spans_into_range() {
        assert_eq!(shift_into(10., 50., 0., 100.), 0.);
        assert_eq!(shift_into(-30., 10., 0., 100.), 30.);
        assert_eq!(shift_into(80., 140., 0., 100.), -40.);
        // Taller than the range: line up the start.
        assert_eq!(shift_into(20., 300., 0., 100.), -20.);
    }

    #[test]
    fn mostly_visible_counts_as_inside() {
        let viewport = Rect::new(0., 0., 1000., 800.);
        assert!(Rect::new(10., 10., 100., 100.).inside(&viewport));
        assert!(Rect::new(950., 10., 100., 100.).inside(&viewport));
        assert!(!Rect::new(980., 10., 100., 100.).inside(&viewport));
        assert!(!Rect::new(10., -90., 100., 100.).inside(&viewport));
    }
}
