//! The window's root: decides between signing in, the main shell, and the
//! offline screen.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable as _, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AppContext as _, Context, Entity, FontWeight, SharedString, Subscription, Task, Window, div, px,
};
use jellyfin::Client;

use crate::components::glass::glass;
use crate::components::logo;
use crate::loadable::describe;
use crate::runtime;
use crate::session::Session;
use crate::state::AppState;
use crate::theme::{FONT_DISPLAY, Palette};
use crate::views::login::{LoginEvent, LoginView};
use crate::views::shell::{Shell, ShellEvent};

const SESSION_EXPIRED: &str = "Your session has expired. Sign in again.";

enum Screen {
    Starting,
    Login(Entity<LoginView>),
    Main(Entity<Shell>),
    Offline(SharedString),
}

pub struct AppRoot {
    screen: Screen,
    _subscription: Option<Subscription>,
    _task: Option<Task<()>>,
}

impl AppRoot {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            screen: Screen::Starting,
            _subscription: None,
            _task: None,
        };
        if let Some(session) = AppState::global(cx).store().load() {
            this.resume(session, window, cx);
        } else if let Some(credentials) = dev_credentials() {
            this.dev_sign_in(credentials, window, cx);
        } else {
            this.show_login(None, window, cx);
        }
        this
    }

    fn resume(&mut self, session: Session, window: &mut Window, cx: &mut Context<Self>) {
        match AppState::restore(cx, session) {
            Ok(client) => self.connect(client, window, cx),
            Err(err) => self.show_login(Some(describe(&err).into()), window, cx),
        }
    }

    /// Loads the user's libraries, then opens the shell.
    fn connect(&mut self, client: Client, window: &mut Window, cx: &mut Context<Self>) {
        self.screen = Screen::Starting;
        let fetch = runtime::api(cx, async move { client.user_views().await });
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = fetch.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(views) => {
                    AppState::set_views(cx, views);
                    this.show_main(window, cx);
                }
                Err(jellyfin::Error::Unauthorized) => {
                    AppState::sign_out(cx);
                    this.show_login(Some(SESSION_EXPIRED.into()), window, cx);
                }
                Err(err) => {
                    this.screen = Screen::Offline(describe(&err).into());
                    cx.notify();
                }
            })
            .ok();
        }));
        cx.notify();
    }

    fn show_login(
        &mut self,
        notice: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let login = cx.new(|cx| LoginView::new(notice, window, cx));
        self._subscription = Some(cx.subscribe_in(
            &login,
            window,
            |this, _, event: &LoginEvent, window, cx| match event {
                LoginEvent::SignedIn(session) => this.signed_in(session.clone(), window, cx),
            },
        ));
        self.screen = Screen::Login(login);
        cx.notify();
    }

    fn signed_in(&mut self, session: Session, window: &mut Window, cx: &mut Context<Self>) {
        match AppState::sign_in(cx, session) {
            Ok(client) => self.connect(client, window, cx),
            Err(err) => self.show_login(Some(describe(&err).into()), window, cx),
        }
    }

    fn show_main(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shell = cx.new(|cx| Shell::new(window, cx));
        self._subscription = Some(cx.subscribe_in(
            &shell,
            window,
            |this, _, event: &ShellEvent, window, cx| match event {
                ShellEvent::SignOut => this.sign_out(window, cx),
                ShellEvent::SessionExpired => {
                    AppState::sign_out(cx);
                    this.show_login(Some(SESSION_EXPIRED.into()), window, cx);
                }
            },
        ));
        self.screen = Screen::Main(shell);
        cx.notify();
    }

    fn sign_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Revoke the token server-side; signing out locally must not wait for it.
        let client = AppState::client(cx);
        runtime::api(cx, async move { client.logout().await });
        AppState::sign_out(cx);
        self.show_login(None, window, cx);
    }

    fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match AppState::global(cx).store().load() {
            Some(session) => self.resume(session, window, cx),
            None => self.show_login(None, window, cx),
        }
    }

    fn dev_sign_in(
        &mut self,
        credentials: DevCredentials,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let client = match AppState::global(cx).anonymous_client(&credentials.server) {
            Ok(client) => client,
            Err(err) => return self.show_login(Some(describe(&err).into()), window, cx),
        };
        let auth = runtime::api(cx, async move {
            let info = client.public_info().await?;
            let auth = client
                .authenticate_by_name(&credentials.user, &credentials.password)
                .await?;
            Ok(Session {
                server: client.base_url().to_string(),
                server_name: info.server_name,
                user_id: auth.user.id,
                user_name: auth.user.name,
                token: auth.access_token,
            })
        });
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = auth.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(session) => this.signed_in(session, window, cx),
                Err(err) => this.show_login(Some(describe(&err).into()), window, cx),
            })
            .ok();
        }));
    }

    fn render_starting(&self) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(TitleBar::new().h(px(52.)))
            .child(
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_6()
                    .child(logo::mark(px(64.)))
                    .child(
                        Spinner::new()
                            .large()
                            .color(Palette::text_tertiary().into()),
                    ),
            )
    }

    fn render_offline(&self, message: SharedString, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(TitleBar::new().h(px(52.)))
            .child(
                v_flex().flex_1().items_center().justify_center().child(
                    glass().w(px(440.)).p_8().child(
                        v_flex()
                            .gap_4()
                            .child(
                                div()
                                    .font_family(FONT_DISPLAY)
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(px(24.))
                                    .child("Can't reach your server"),
                            )
                            .child(div().text_color(Palette::text_secondary()).child(message))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("retry").primary().label("Try again").on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.retry(window, cx)
                                            }),
                                        ),
                                    )
                                    .child(
                                        Button::new("sign-out").ghost().label("Sign out").on_click(
                                            cx.listener(|this, _, window, cx| {
                                                AppState::sign_out(cx);
                                                this.show_login(None, window, cx)
                                            }),
                                        ),
                                    ),
                            ),
                    ),
                ),
            )
    }
}

impl Render for AppRoot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.screen {
            Screen::Starting => self.render_starting().into_any_element(),
            Screen::Login(login) => login.clone().into_any_element(),
            Screen::Main(shell) => shell.clone().into_any_element(),
            Screen::Offline(message) => {
                let message = message.clone();
                self.render_offline(message, cx).into_any_element()
            }
        };
        div()
            .size_full()
            .bg(Palette::bg())
            .text_color(Palette::text())
            .font_family(crate::theme::FONT)
            .child(content)
    }
}

struct DevCredentials {
    server: String,
    user: String,
    password: String,
}

/// `AURELIA_SERVER` + `AURELIA_USER` (+ optional `AURELIA_PASSWORD`) sign in
/// automatically — for development and screenshots.
fn dev_credentials() -> Option<DevCredentials> {
    Some(DevCredentials {
        server: std::env::var("AURELIA_SERVER").ok()?,
        user: std::env::var("AURELIA_USER").ok()?,
        password: std::env::var("AURELIA_PASSWORD").unwrap_or_default(),
    })
}
