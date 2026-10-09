//! The signed-in window: floating navigation bar over the current page,
//! history, keyboard shortcuts and navigation, page transitions, and
//! app-wide error handling.

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{Sizable as _, TitleBar, WindowExt as _, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, App, Context, ElementId, EntityId, EventEmitter, FocusHandle,
    Focusable, FontWeight, Global, KeyBinding, MouseButton, NavigationDirection, Pixels,
    SharedString, SpringAnimation, WeakEntity, Window, actions, div, linear_color_stop,
    linear_gradient, px,
};

use crate::components::art::Art;
use crate::components::button::{focus_ring, icon_button};
use crate::components::key_nav::{self, Direction};
use crate::components::logo;
use crate::components::motion::{SPRING, mix, pressable};
use crate::images::ImageRequest;
use crate::loadable::describe;
use crate::nav::{Nav, Route};
use crate::state::AppState;
use crate::theme::Palette;
use crate::views::pages::Page;
use gpui_kit::assets::IconName as icon;

pub const NAV_HEIGHT: Pixels = px(64.);
/// Coming back to the window after this long refreshes the page.
const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(120);
/// How long a page takes to fade in.
const PAGE_FADE: Duration = Duration::from_millis(220);
/// How far a newly opened page rises as it fades in.
const PAGE_RISE: f32 = 12.;

/// Whether data loaded at `last` is old enough to reload on return.
fn is_stale(last: std::time::Instant, now: std::time::Instant) -> bool {
    now.duration_since(last) >= STALE_AFTER
}
const CONTEXT: &str = "Shell";

actions!(
    aurelia,
    [
        Back,
        Forward,
        Refresh,
        Quit,
        PlayCurrent,
        Search,
        OpenSettings
    ]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Back, Some(CONTEXT)),
        KeyBinding::new("alt-left", Back, Some(CONTEXT)),
        KeyBinding::new("alt-right", Forward, Some(CONTEXT)),
        KeyBinding::new("ctrl-r", Refresh, Some(CONTEXT)),
        KeyBinding::new("f5", Refresh, Some(CONTEXT)),
        KeyBinding::new("enter", PlayCurrent, Some(CONTEXT)),
        KeyBinding::new("ctrl-f", Search, Some(CONTEXT)),
        KeyBinding::new("ctrl-,", OpenSettings, Some(CONTEXT)),
        KeyBinding::new("ctrl-q", Quit, None),
    ]);
    key_nav::bind_keys(&[CONTEXT], cx);
    cx.on_action(|_: &Quit, cx| cx.quit());
}

pub enum ShellEvent {
    SignOut,
    SessionExpired,
    SwitchUser,
    AddAccount,
}

/// Lets pages reach the shell without threading a handle through every view.
struct ShellHandle(WeakEntity<Shell>);

impl Global for ShellHandle {}

fn with_shell(cx: &mut App, f: impl FnOnce(&mut Shell, &mut Context<Shell>)) {
    let Some(shell) = cx
        .try_global::<ShellHandle>()
        .and_then(|handle| handle.0.upgrade())
    else {
        return;
    };
    shell.update(cx, f);
}

pub fn navigate(route: Route, window: &mut Window, cx: &mut App) {
    with_shell(cx, |shell, cx| shell.navigate(route, window, cx));
}

/// Opens "Who's watching?".
pub fn switch_user(cx: &mut App) {
    with_shell(cx, |_, cx| cx.emit(ShellEvent::SwitchUser));
}

/// Opens sign-in for another account, keeping this one open behind it.
pub fn add_account(cx: &mut App) {
    with_shell(cx, |_, cx| cx.emit(ShellEvent::AddAccount));
}

/// Every page in the history, oldest first.
fn pages(cx: &App) -> Vec<Page> {
    match cx.try_global::<ShellHandle>().and_then(|h| h.0.upgrade()) {
        Some(shell) => shell.read(cx).nav.pages().cloned().collect(),
        None => Vec::new(),
    }
}

/// Reloads every page in the history (e.g. after playback changed progress).
pub fn refresh_all(window: &mut Window, cx: &mut App) {
    for page in pages(cx) {
        page.refresh(window, cx);
    }
}

/// Shows an item's new watched/favourite state wherever it's on screen.
///
/// Deferred: the caller may be one of those pages (a button's listener runs
/// inside its page's update), and GPUI can't update an entity re-entrantly.
pub fn patch_user_data(id: &str, data: &jellyfin::UserData, cx: &mut App) {
    let (id, data) = (id.to_string(), data.clone());
    cx.defer(move |cx| {
        for page in pages(cx) {
            page.patch_user_data(&id, &data, cx);
        }
    });
}

/// Shows a failed action to the user; an expired session signs out.
pub fn report_error(err: &jellyfin::Error, window: &mut Window, cx: &mut App) {
    if matches!(err, jellyfin::Error::Unauthorized) {
        session_expired(cx);
    } else {
        window.push_notification(Notification::error(describe(err)), cx);
    }
}

pub fn report_message(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    window.push_notification(Notification::error(message), cx);
}

pub fn notify(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    window.push_notification(Notification::info(message), cx);
}

/// Call when a page's load fails; returns the message to show inline.
pub fn load_failed(err: &jellyfin::Error, cx: &mut App) -> SharedString {
    if matches!(err, jellyfin::Error::Unauthorized) {
        session_expired(cx);
    }
    describe(err).into()
}

pub fn session_expired(cx: &mut App) {
    with_shell(cx, |_, cx| cx.emit(ShellEvent::SessionExpired));
}

/// Development: `AURELIA_ROUTE=item:<id>|series:<id>|library:<id>|
/// collection:<id>|person:<id>|genre:<name>|search:<query>|settings` opens a
/// page at startup (for screenshots).
fn dev_start_route() -> Option<Route> {
    let value = std::env::var("AURELIA_ROUTE").ok()?;
    if value == "settings" {
        return Some(Route::Settings);
    }
    let (kind, id) = value.split_once(':')?;
    let id = id.to_string();
    Some(match kind {
        "item" => Route::Item { id },
        "series" => Route::Series {
            id,
            season_id: None,
        },
        "library" => Route::Library {
            name: String::new(),
            id,
        },
        "collection" => Route::Collection { id },
        "person" => Route::Person { id },
        "genre" => Route::Genre { name: id },
        "search" => Route::Search,
        _ => return None,
    })
}

/// The query for `AURELIA_ROUTE=search:<query>`.
fn dev_search_query() -> Option<String> {
    let value = std::env::var("AURELIA_ROUTE").ok()?;
    let query = value.strip_prefix("search:")?;
    (!query.is_empty()).then(|| query.to_string())
}

/// How the page on screen arrived, for its entrance.
#[derive(Clone, Copy)]
struct Transition {
    serial: u64,
    /// Opened (rises as it fades in) rather than returned to (fades).
    opened: bool,
}

pub struct Shell {
    nav: Nav<Page>,
    focus: FocusHandle,
    /// Bumped on every refresh; keys the refresh icon's spin.
    refreshes: usize,
    last_refresh: std::time::Instant,
    transition: Transition,
    /// The nav target that had keyboard focus on each page left behind.
    focused_on: HashMap<EntityId, ElementId>,
    _activation: gpui_kit::Subscription,
}

impl EventEmitter<ShellEvent> for Shell {}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.set_global(ShellHandle(cx.weak_entity()));
        let home = Page::for_route(&Route::Home, window, cx);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        // Back at the window after a while: reload what's on screen so new
        // additions and progress from other devices show up.
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() && is_stale(this.last_refresh, std::time::Instant::now()) {
                this.reload(false, window, cx);
            }
        });
        let mut this = Self {
            nav: Nav::new(Route::Home, home),
            focus,
            refreshes: 0,
            last_refresh: std::time::Instant::now(),
            transition: Transition {
                serial: 0,
                opened: true,
            },
            focused_on: HashMap::new(),
            _activation: activation,
        };
        if let Some(route) = dev_start_route() {
            this.navigate(route, window, cx);
            if let (Page::Search(page), Some(query)) = (this.nav.page(), dev_search_query()) {
                page.update(cx, |page, cx| page.set_query(&query, window, cx));
            }
        }
        this
    }

    pub fn navigate(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        if self.nav.route() == &route {
            return;
        }
        self.remember_focus(window, cx);
        let page = Page::for_route(&route, window, cx);
        self.nav.push(route, page);
        self.arrive(true, None, window, cx);
    }

    /// Notes which target had focus on the page being left.
    fn remember_focus(&mut self, window: &Window, cx: &App) {
        let page = self.nav.page().view().entity_id();
        match key_nav::focused_key(window, cx) {
            Some(key) => {
                self.focused_on.insert(page, key);
            }
            None => {
                self.focused_on.remove(&page);
            }
        }
    }

    /// Shows the page now current: its entrance, its focus.
    fn arrive(
        &mut self,
        opened: bool,
        restore: Option<ElementId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.transition = Transition {
            serial: self.transition.serial + 1,
            opened,
        };
        self.focus_page(window, cx);
        key_nav::restore(restore, cx);
        // Forget pages that fell out of the history.
        let alive: Vec<EntityId> = self.nav.pages().map(|p| p.view().entity_id()).collect();
        self.focused_on.retain(|page, _| alive.contains(page));
        cx.notify();
    }

    /// Keys go to the page's own field (Search) or else the shell.
    fn focus_page(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self.nav.page().focus_handle(cx) {
            Some(handle) => window.focus(&handle, cx),
            None => window.focus(&self.focus, cx),
        }
    }

    fn back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        self.remember_focus(window, cx);
        if self.nav.back() {
            let restore = self
                .focused_on
                .get(&self.nav.page().view().entity_id())
                .cloned();
            self.arrive(false, restore, window, cx);
        } else if key_nav::has_focus(window, cx) {
            // Nowhere to go back to: Esc lets go of the focused card instead.
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn forward(&mut self, _: &Forward, window: &mut Window, cx: &mut Context<Self>) {
        self.remember_focus(window, cx);
        if self.nav.forward() {
            let restore = self
                .focused_on
                .get(&self.nav.page().view().entity_id())
                .cloned();
            self.arrive(false, restore, window, cx);
        }
    }

    fn search(&mut self, _: &Search, window: &mut Window, cx: &mut Context<Self>) {
        if self.nav.route() == &Route::Search {
            self.focus_page(window, cx);
        } else {
            self.navigate(Route::Search, window, cx);
        }
    }

    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Route::Settings, window, cx);
    }

    /// Arrow keys. With nothing focused, ←/→ on Home still change slides.
    fn step(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        let horizontal = matches!(direction, Direction::Left | Direction::Right);
        if horizontal && !key_nav::has_focus(window, cx) && matches!(self.nav.page(), Page::Home(_))
        {
            let delta = if direction == Direction::Left { -1 } else { 1 };
            self.nav.page().clone().step_hero(delta, cx);
            return;
        }
        key_nav::step(direction, window, cx);
    }

    fn play_current(&mut self, _: &PlayCurrent, window: &mut Window, cx: &mut Context<Self>) {
        self.nav.page().clone().primary_action(window, cx);
    }

    fn refresh(&mut self, _: &Refresh, window: &mut Window, cx: &mut Context<Self>) {
        self.reload(true, window, cx);
    }

    /// `reshuffle`: asked for by the user, so Home also re-rolls its hero.
    fn reload(&mut self, reshuffle: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.refreshes += 1;
        self.last_refresh = std::time::Instant::now();
        crate::images::ImageStore::retry_failed(cx);
        let page = self.nav.page().clone();
        if reshuffle {
            page.reshuffle(window, cx);
        } else {
            page.refresh(window, cx);
        }
        cx.notify();
    }

    fn render_search(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.nav.route() == &Route::Search;
        icon_button("search", icon::Search, active)
            .in_bar()
            .tooltip(|window, cx| {
                gpui_kit::component::tooltip::Tooltip::new("Search (Ctrl+F)").build(window, cx)
            })
            .on_click(cx.listener(|this, _, window, cx| this.search(&Search, window, cx)))
    }

    fn render_refresh(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let refreshes = self.refreshes;
        pressable("refresh")
            .in_bar()
            .tooltip(|window, cx| {
                gpui_kit::component::tooltip::Tooltip::new("Refresh (Ctrl+R)").build(window, cx)
            })
            .on_click(cx.listener(|this, _, window, cx| this.refresh(&Refresh, window, cx)))
            .look(move |this, m| {
                let glyph = gpui_kit::component::Icon::new(icon::RefreshCw)
                    .size_4()
                    .text_color(m.mix(Palette::text_secondary(), Palette::text()));
                let glyph = if refreshes > 0 {
                    glyph
                        .with_animation(
                            ("refresh-spin", refreshes),
                            Animation::new(Duration::from_millis(650))
                                .with_easing(gpui_kit::ease_in_out),
                            |glyph, t| glyph.rotate(gpui_kit::percentage(t)),
                        )
                        .into_any_element()
                } else {
                    glyph.into_any_element()
                };
                this.relative()
                    .size_8()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .bg(mix(
                        gpui_kit::transparent_black(),
                        Palette::glass_strong().into(),
                        m.amount(),
                    ))
                    .child(focus_ring(m, px(16.)))
                    .child(glyph)
            })
    }

    fn render_tab(
        &self,
        id: SharedString,
        label: SharedString,
        route: Route,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = match (self.nav.route(), &route) {
            (Route::Home, Route::Home) => true,
            (Route::Library { id: a, .. }, Route::Library { id: b, .. }) => a == b,
            _ => false,
        };
        pressable(id)
            .in_bar()
            .on_click(
                cx.listener(move |this, _, window, cx| this.navigate(route.clone(), window, cx)),
            )
            .look(move |this, m| {
                this.relative()
                    .px_4()
                    .py_1p5()
                    .rounded_full()
                    .cursor_pointer()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if active {
                        Palette::text().into()
                    } else {
                        m.mix(Palette::text_secondary(), Palette::text())
                    })
                    .bg(if active {
                        Palette::glass_strong().into()
                    } else {
                        mix(
                            gpui_kit::transparent_black(),
                            Palette::glass().into(),
                            m.amount(),
                        )
                    })
                    .child(focus_ring(m, px(16.)))
                    .child(label)
            })
    }

    fn render_nav(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = AppState::global(cx);
        let views: Vec<_> = state
            .views()
            .iter()
            .filter(|view| view.is_video_library() || view.is_collections())
            .cloned()
            .collect();
        let session = state.session().cloned();
        let avatar = session.as_ref().and_then(|s| {
            let tag = s.image_tag.as_deref()?;
            let client = state.client_for(s).ok()?;
            Some(ImageRequest::new(
                client.user_image_url(&s.user_id, tag, 96).to_string(),
            ))
        });
        let (user, server) = session
            .map(|s| (s.user_name, s.server_name))
            .unwrap_or_default();
        let can_go_back = self.nav.can_go_back();
        let scrolled = self.nav.page().scroll_offset(cx) < px(-24.);
        let _ = window;

        let mut tabs = h_flex().gap_1().child(self.render_tab(
            "tab-home".into(),
            "Home".into(),
            Route::Home,
            cx,
        ));
        for view in views {
            let route = Route::Library {
                id: view.id.clone(),
                name: view.name.clone(),
            };
            tabs = tabs.child(self.render_tab(
                format!("tab-{}", view.id).into(),
                view.name.clone().into(),
                route,
                cx,
            ));
        }

        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(NAV_HEIGHT)
            // Over the top of the page: a fade. Once the page scrolls under
            // it: an opaque bar, so text can't show through.
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(NAV_HEIGHT + px(24.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(Palette::bg_alpha(0.9), 0.),
                        linear_color_stop(Palette::bg_alpha(0.), 1.),
                    ))
                    .with_spring(
                        "nav-fade",
                        SpringAnimation::new(SPRING).to(!scrolled),
                        |this, phase| this.opacity(phase.0.clamp(0., 1.)),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(Palette::bar())
                    .border_b_1()
                    .border_color(Palette::border())
                    .with_spring(
                        "nav-solid",
                        SpringAnimation::new(SPRING).to(scrolled),
                        |this, phase| this.opacity(phase.0.clamp(0., 1.)),
                    ),
            )
            .child(
                TitleBar::new().h(NAV_HEIGHT).pl_5().child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .pr_3()
                        .child(
                            h_flex()
                                .gap_6()
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .when(can_go_back, |this| {
                                            this.child(
                                                controls(div()).child(
                                                    icon_button("back", icon::ChevronLeft, false)
                                                        .in_bar()
                                                        .tooltip(|window, cx| {
                                                            gpui_kit::component::tooltip::Tooltip::new(
                                                                "Back (Esc)",
                                                            )
                                                            .build(window, cx)
                                                        })
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.back(&Back, window, cx)
                                                            },
                                                        )),
                                                ),
                                            )
                                        })
                                        .child(logo::small()),
                                )
                                .child(controls(tabs)),
                        )
                        .child(
                            controls(h_flex())
                                .gap_2()
                                .child(self.render_search(cx))
                                .child(self.render_refresh(cx))
                                .child(account_button(user, server, avatar)),
                        ),
                ),
            )
    }
}

/// Keeps presses on the nav's controls to themselves. The title bar around
/// them starts a window move when the pointer moves while pressed; KWin then
/// takes the pointer, so the release never arrives and the click is lost.
/// Clicks still work: an element's own click handler sees the press first.
fn controls(element: gpui_kit::Div) -> gpui_kit::Div {
    element.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// The user's avatar (or initial) in a circle; opens the account menu.
fn account_button(user: String, server: String, avatar: Option<ImageRequest>) -> impl IntoElement {
    let initial: SharedString = user
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default()
        .into();
    let accent: gpui_kit::Hsla = Palette::accent().into();
    Button::new("account")
        .ghost()
        .small()
        // A button with content is sized and rounded like a text button;
        // make it the avatar's circle so hover and clicks match what you see.
        .size_8()
        .p_0()
        .rounded_full()
        .child(
            div()
                .relative()
                .size_7()
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(accent.opacity(0.25))
                .text_color(Palette::text())
                .font_weight(FontWeight::BOLD)
                .text_sm()
                .child(initial)
                .when_some(avatar, |this, avatar| {
                    this.child(
                        Art::new("account-avatar", Some(avatar))
                            .bare()
                            .radius(px(14.))
                            .absolute()
                            .inset_0(),
                    )
                }),
        )
        .dropdown_menu_with_anchor(gpui_kit::Anchor::TopRight, move |menu, _, _| {
            menu.label(format!("{user} · {server}"))
                .separator()
                .item(
                    PopupMenuItem::new("Switch user…")
                        .icon(icon::Users)
                        .on_click(|_, _, cx| switch_user(cx)),
                )
                .item(
                    PopupMenuItem::new("Settings")
                        .icon(icon::Settings)
                        .on_click(|_, window, cx| navigate(Route::Settings, window, cx)),
                )
                .separator()
                .item(
                    PopupMenuItem::new("Sign out")
                        .icon(icon::LogOut)
                        .on_click(|_, _, cx| with_shell(cx, |_, cx| cx.emit(ShellEvent::SignOut))),
                )
        })
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scope = key_nav::scope(
            self.focus.clone(),
            self.nav.page().scroll_handle(cx),
            NAV_HEIGHT,
        );
        let Transition { serial, opened } = self.transition;
        let page = div()
            .relative()
            .size_full()
            .child(self.nav.page().view())
            .with_animation(
                ElementId::NamedInteger("page".into(), serial),
                Animation::new(PAGE_FADE).with_easing(gpui_kit::ease_out_quint()),
                move |page, t| {
                    let page = page.opacity(t);
                    if opened {
                        page.top(px(PAGE_RISE * (1. - t)))
                    } else {
                        page
                    }
                },
            );

        div()
            .id("shell")
            .relative()
            .size_full()
            .track_focus(&self.focus)
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::back))
            .on_action(cx.listener(Self::forward))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::play_current))
            .on_action(cx.listener(Self::search))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(|this, _: &key_nav::NavUp, window, cx| {
                this.step(Direction::Up, window, cx)
            }))
            .on_action(cx.listener(|this, _: &key_nav::NavDown, window, cx| {
                this.step(Direction::Down, window, cx)
            }))
            .on_action(cx.listener(|this, _: &key_nav::NavLeft, window, cx| {
                this.step(Direction::Left, window, cx)
            }))
            .on_action(cx.listener(|this, _: &key_nav::NavRight, window, cx| {
                this.step(Direction::Right, window, cx)
            }))
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _, window, cx| this.back(&Back, window, cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|this, _, window, cx| this.forward(&Forward, window, cx)),
            )
            .child(scope)
            .child(page)
            .child(self.render_nav(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Bounds, Context, InteractiveElement as _, IntoElement, ParentElement as _,
        Render, StatefulInteractiveElement as _, Styled as _, TestAppContext, TestSupportExt as _,
        Window, WindowBounds, WindowOptions, div, px, size,
    };

    use super::{STALE_AFTER, account_button, controls, is_stale};

    struct Account;

    impl Render for Account {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                div()
                    .id("account-slot")
                    .test_support()
                    .absolute()
                    .child(account_button("user".into(), "server".into(), None)),
            )
        }
    }

    #[gpui_kit::test]
    fn account_button_is_the_avatar_circle(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(400.), px(200.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| Account),
            )
            .expect("open test window")
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let bounds = window.find("account-slot").bounds();
            assert_eq!(
                bounds.size,
                size(px(32.), px(32.)),
                "hit area hugs the avatar"
            );
        })
        .unwrap();
    }

    /// A title bar stand-in that counts presses reaching it, around a tab.
    struct Bar {
        presses: std::rc::Rc<std::cell::Cell<usize>>,
        clicks: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl Render for Bar {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let presses = self.presses.clone();
            let clicks = self.clicks.clone();
            div()
                .size_full()
                .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, _| {
                    presses.set(presses.get() + 1)
                })
                .child(
                    controls(div()).child(
                        div()
                            .id("tab")
                            .size(px(80.))
                            .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
                            .test_support(),
                    ),
                )
        }
    }

    #[gpui_kit::test]
    fn nav_controls_click_without_arming_a_window_drag(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let presses = std::rc::Rc::new(std::cell::Cell::new(0));
        let clicks = std::rc::Rc::new(std::cell::Cell::new(0));
        let (window, _) = cx.update(|cx| {
            let (presses, clicks) = (presses.clone(), clicks.clone());
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(400.), px(200.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| Bar { presses, clicks }),
            )
            .expect("open test window")
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("tab", cx);
        })
        .unwrap();
        assert_eq!(clicks.get(), 1, "the tab still gets its click");
        assert_eq!(presses.get(), 0, "the title bar never sees the press");
    }

    #[test]
    fn data_goes_stale_after_two_minutes() {
        let t0 = Instant::now();
        assert!(!is_stale(t0, t0 + Duration::from_secs(30)));
        assert!(!is_stale(t0, t0 + STALE_AFTER - Duration::from_millis(1)));
        assert!(is_stale(t0, t0 + STALE_AFTER));
    }
}
