//! The signed-in window: floating navigation bar over the current page,
//! history, keyboard shortcuts, and app-wide error handling.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{Sizable as _, TitleBar, WindowExt as _, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, Focusable, FontWeight, Global, KeyBinding,
    MouseButton, NavigationDirection, Pixels, SharedString, WeakEntity, Window, actions, div,
    linear_color_stop, linear_gradient, px, rgba,
};

use crate::components::logo;
use crate::loadable::describe;
use crate::nav::{Nav, Route};
use crate::state::AppState;
use crate::theme::Palette;
use crate::views::pages::Page;
use gpui_kit::assets::IconName as icon;

pub const NAV_HEIGHT: Pixels = px(64.);
const CONTEXT: &str = "Shell";

actions!(aurelia, [Back, Forward, Refresh, Quit]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Back, Some(CONTEXT)),
        KeyBinding::new("alt-left", Back, Some(CONTEXT)),
        KeyBinding::new("alt-right", Forward, Some(CONTEXT)),
        KeyBinding::new("ctrl-r", Refresh, Some(CONTEXT)),
        KeyBinding::new("f5", Refresh, Some(CONTEXT)),
        KeyBinding::new("ctrl-q", Quit, None),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
}

pub enum ShellEvent {
    SignOut,
    SessionExpired,
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

/// Reloads every page in the history (e.g. after playback changed progress).
pub fn refresh_all(window: &mut Window, cx: &mut App) {
    let pages: Vec<Page> = match cx.try_global::<ShellHandle>().and_then(|h| h.0.upgrade()) {
        Some(shell) => shell.read(cx).nav.pages().cloned().collect(),
        None => return,
    };
    for page in pages {
        page.refresh(window, cx);
    }
}

/// Shows a failed action to the user; an expired session signs out.
pub fn report_error(err: &jellyfin::Error, window: &mut Window, cx: &mut App) {
    if matches!(err, jellyfin::Error::Unauthorized) {
        session_expired(cx);
    } else {
        window.push_notification(Notification::error(describe(err)), cx);
    }
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

/// Development: `AURELIA_ROUTE=item:<id>|series:<id>|library:<id>` opens a
/// page at startup (for screenshots).
fn dev_start_route() -> Option<Route> {
    let value = std::env::var("AURELIA_ROUTE").ok()?;
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
        _ => return None,
    })
}

pub struct Shell {
    nav: Nav<Page>,
    focus: FocusHandle,
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
        let mut this = Self {
            nav: Nav::new(Route::Home, home),
            focus,
        };
        if let Some(route) = dev_start_route() {
            this.navigate(route, window, cx);
        }
        this
    }

    pub fn navigate(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        if self.nav.route() == &route {
            return;
        }
        let page = Page::for_route(&route, window, cx);
        self.nav.push(route, page);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        if self.nav.back() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn forward(&mut self, _: &Forward, window: &mut Window, cx: &mut Context<Self>) {
        if self.nav.forward() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn refresh(&mut self, _: &Refresh, window: &mut Window, cx: &mut Context<Self>) {
        crate::images::ImageStore::retry_failed(cx);
        self.nav.page().clone().refresh(window, cx);
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
        div()
            .id(id)
            .px_4()
            .py_1p5()
            .rounded_full()
            .cursor_pointer()
            .font_weight(FontWeight::MEDIUM)
            .text_color(if active {
                Palette::text()
            } else {
                Palette::text_secondary()
            })
            .when(active, |this| this.bg(Palette::glass_strong()))
            .hover(|this| this.text_color(Palette::text()).bg(Palette::glass()))
            .on_click(
                cx.listener(move |this, _, window, cx| this.navigate(route.clone(), window, cx)),
            )
            .child(label)
    }

    fn render_nav(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = AppState::global(cx);
        let views: Vec<_> = state
            .views()
            .iter()
            .filter(|view| view.is_video_library())
            .cloned()
            .collect();
        let (user, server) = state
            .session()
            .map(|s| (s.user_name.clone(), s.server_name.clone()))
            .unwrap_or_default();
        let initial: SharedString = user
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default()
            .into();
        let can_go_back = self.nav.can_go_back();
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
            .h(NAV_HEIGHT + px(24.))
            .bg(linear_gradient(
                180.,
                linear_color_stop(rgba(0x0A0B10E6), 0.),
                linear_color_stop(rgba(0x0A0B1000), 1.),
            ))
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
                                                Button::new("back")
                                                    .ghost()
                                                    .small()
                                                    .icon(icon::ChevronLeft)
                                                    .tooltip("Back (Esc)")
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.back(&Back, window, cx)
                                                        },
                                                    )),
                                            )
                                        })
                                        .child(logo::small()),
                                )
                                .child(tabs),
                        )
                        .child(
                            Button::new("account")
                                .ghost()
                                .small()
                                .child(
                                    div()
                                        .size_7()
                                        .rounded_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .bg(rgba(0xB69CFF40))
                                        .text_color(Palette::text())
                                        .font_weight(FontWeight::BOLD)
                                        .text_sm()
                                        .child(initial),
                                )
                                .dropdown_menu_with_anchor(
                                    gpui_kit::Anchor::TopRight,
                                    move |menu, _, _| {
                                        menu.label(format!("{user} · {server}")).separator().item(
                                            PopupMenuItem::new("Sign out")
                                                .icon(icon::LogOut)
                                                .on_click(|_, _, cx| {
                                                    with_shell(cx, |_, cx| {
                                                        cx.emit(ShellEvent::SignOut)
                                                    })
                                                }),
                                        )
                                    },
                                ),
                        ),
                ),
            )
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("shell")
            .relative()
            .size_full()
            .track_focus(&self.focus)
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::back))
            .on_action(cx.listener(Self::forward))
            .on_action(cx.listener(Self::refresh))
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _, window, cx| this.back(&Back, window, cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|this, _, window, cx| this.forward(&Forward, window, cx)),
            )
            .child(div().size_full().child(self.nav.page().view()))
            .child(self.render_nav(window, cx))
    }
}
