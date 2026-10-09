//! The window's root: decides between "Who's watching?", signing in, the
//! main shell, and the offline screen.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable as _, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    Context, Entity, Focusable as _, FontWeight, SharedString, Subscription, Task, Window, div, px,
};
use jellyfin::Client;

use crate::components::glass::glass;
use crate::components::key_nav;
use crate::components::logo;
use crate::loadable::describe;
use crate::runtime;
use crate::session::Session;
use crate::settings;
use crate::state::AppState;
use crate::theme::{FONT_DISPLAY, Palette};
use crate::views::login::{LoginEvent, LoginOptions, LoginView};
use crate::views::profiles::{ProfilesEvent, ProfilesView};
use crate::views::shell::{Shell, ShellEvent};

const SESSION_EXPIRED: &str = "Your session has expired. Sign in again.";

enum Screen {
    Starting,
    Profiles(Entity<ProfilesView>),
    Login(Entity<LoginView>),
    Main(Entity<Shell>),
    Offline(SharedString),
}

pub struct AppRoot {
    screen: Screen,
    /// The open account's shell while the picker or sign-in is shown on top
    /// of it (switching users); going back returns to it untouched.
    shell: Option<Entity<Shell>>,
    _subscription: Option<Subscription>,
    _appearance: Subscription,
    _task: Option<Task<()>>,
}

impl AppRoot {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // A System theme follows the desktop as it switches.
        let appearance = cx.observe_window_appearance(window, |_, window, cx| {
            settings::system_appearance_changed(window.appearance(), cx);
        });
        settings::system_appearance_changed(window.appearance(), cx);
        crate::dev::type_keys(window, cx);
        let mut this = Self {
            screen: Screen::Starting,
            shell: None,
            _subscription: None,
            _appearance: appearance,
            _task: None,
        };
        let store = AppState::global(cx).store();
        let accounts = store.accounts();
        let active = store.load();
        let ask = settings::get(cx).profile_picker && accounts.len() >= 2;
        if ask || (active.is_none() && !accounts.is_empty()) {
            this.show_profiles(window, cx);
        } else if let Some(session) = active {
            this.open(session, window, cx);
        } else if let Some(credentials) = dev_credentials() {
            this.dev_sign_in(credentials, window, cx);
        } else {
            this.show_login(LoginOptions::default(), window, cx);
        }
        this
    }

    /// Opens a saved account (or one that just signed in).
    fn open(&mut self, session: Session, window: &mut Window, cx: &mut Context<Self>) {
        self.shell = None;
        let (server, user) = (session.server.clone(), session.user_name.clone());
        match AppState::sign_in(cx, session) {
            Ok(client) => self.connect(client, window, cx),
            Err(err) => self.show_login(
                LoginOptions {
                    notice: Some(describe(&err).into()),
                    server: Some(server),
                    user: Some(user),
                    can_go_back: self.has_accounts(cx),
                },
                window,
                cx,
            ),
        }
    }

    fn has_accounts(&self, cx: &Context<Self>) -> bool {
        !AppState::global(cx).store().accounts().is_empty()
    }

    /// Loads the user's libraries (and avatar), then opens the shell.
    fn connect(&mut self, client: Client, window: &mut Window, cx: &mut Context<Self>) {
        self.screen = Screen::Starting;
        let fetch = runtime::api(cx, async move {
            let (views, me) = tokio::join!(client.user_views(), client.me());
            Ok((views?, me.ok()))
        });
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = fetch.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok((views, me)) => {
                    if let Some(me) = me {
                        AppState::update_session(cx, |s| s.image_tag = me.primary_image_tag);
                    }
                    AppState::set_views(cx, views);
                    this.show_main(window, cx);
                }
                Err(jellyfin::Error::Unauthorized) => this.expired(window, cx),
                Err(err) => {
                    this.screen = Screen::Offline(describe(&err).into());
                    cx.notify();
                }
            })
            .ok();
        }));
        cx.notify();
    }

    /// The open account's token stopped working: forget it and ask that
    /// user to sign in again.
    fn expired(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let session = AppState::global(cx).session().cloned();
        AppState::sign_out(cx);
        self.shell = None;
        self.show_login(
            LoginOptions {
                notice: Some(SESSION_EXPIRED.into()),
                server: session.as_ref().map(|s| s.server.clone()),
                user: session.map(|s| s.user_name),
                can_go_back: self.has_accounts(cx),
            },
            window,
            cx,
        );
    }

    fn show_profiles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let can_cancel = self.shell.is_some();
        let profiles = cx.new(|cx| ProfilesView::new(can_cancel, window, cx));
        self._subscription = Some(cx.subscribe_in(
            &profiles,
            window,
            |this, _, event: &ProfilesEvent, window, cx| match event {
                ProfilesEvent::Open(session) => this.open(session.clone(), window, cx),
                ProfilesEvent::SignedIn(session) => this.open(session.clone(), window, cx),
                ProfilesEvent::SignIn { server, user } => this.show_login(
                    LoginOptions {
                        server: Some(server.clone()),
                        user: Some(user.clone()),
                        can_go_back: true,
                        ..LoginOptions::default()
                    },
                    window,
                    cx,
                ),
                ProfilesEvent::AddAccount => this.show_login(
                    LoginOptions {
                        can_go_back: true,
                        ..LoginOptions::default()
                    },
                    window,
                    cx,
                ),
                ProfilesEvent::Cancel => this.return_to_shell(window, cx),
            },
        ));
        self.screen = Screen::Profiles(profiles);
        cx.notify();
    }

    fn show_login(&mut self, options: LoginOptions, window: &mut Window, cx: &mut Context<Self>) {
        let login = cx.new(|cx| LoginView::new(options, window, cx));
        self._subscription = Some(cx.subscribe_in(
            &login,
            window,
            |this, _, event: &LoginEvent, window, cx| match event {
                LoginEvent::SignedIn(session) => this.open(session.clone(), window, cx),
                LoginEvent::Back => {
                    if this.shell.is_some() && !this.has_accounts_besides_shell(cx) {
                        this.return_to_shell(window, cx);
                    } else {
                        this.show_profiles(window, cx);
                    }
                }
            },
        ));
        self.screen = Screen::Login(login);
        cx.notify();
    }

    /// Whether going back from sign-in has a picker worth showing.
    fn has_accounts_besides_shell(&self, cx: &Context<Self>) -> bool {
        AppState::global(cx).store().accounts().len() > 1
    }

    fn return_to_shell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.shell.take() {
            Some(shell) => {
                self.subscribe_shell(&shell, window, cx);
                let focus = shell.focus_handle(cx);
                window.focus(&focus, cx);
                self.screen = Screen::Main(shell);
                cx.notify();
            }
            None => self.show_profiles(window, cx),
        }
    }

    fn show_main(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shell = cx.new(|cx| Shell::new(window, cx));
        self.subscribe_shell(&shell, window, cx);
        self.screen = Screen::Main(shell);
        cx.notify();
    }

    fn subscribe_shell(
        &mut self,
        shell: &Entity<Shell>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._subscription = Some(cx.subscribe_in(
            shell,
            window,
            |this, shell, event: &ShellEvent, window, cx| match event {
                ShellEvent::SignOut => this.sign_out(window, cx),
                ShellEvent::SessionExpired => this.expired(window, cx),
                ShellEvent::SwitchUser => {
                    this.shell = Some(shell.clone());
                    this.show_profiles(window, cx);
                }
                ShellEvent::AddAccount => {
                    this.shell = Some(shell.clone());
                    this.show_login(
                        LoginOptions {
                            can_go_back: true,
                            ..LoginOptions::default()
                        },
                        window,
                        cx,
                    );
                }
            },
        ));
    }

    fn sign_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Revoke the token server-side; signing out locally must not wait for
        // it. Dropping the handle leaves the request running.
        let client = AppState::client(cx);
        drop(runtime::api(cx, async move { client.logout().await }));
        AppState::sign_out(cx);
        self.shell = None;
        if self.has_accounts(cx) {
            self.show_profiles(window, cx);
        } else {
            self.show_login(LoginOptions::default(), window, cx);
        }
    }

    fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match AppState::global(cx).store().load() {
            Some(session) => self.open(session, window, cx),
            None => self.show_login(LoginOptions::default(), window, cx),
        }
    }

    fn dev_sign_in(
        &mut self,
        credentials: DevCredentials,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let device = AppState::new_device_id();
        let client = match AppState::global(cx).anonymous_client(&credentials.server, &device) {
            Ok(client) => client,
            Err(err) => {
                return self.show_login(
                    LoginOptions {
                        notice: Some(describe(&err).into()),
                        ..LoginOptions::default()
                    },
                    window,
                    cx,
                );
            }
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
                device_id: Some(device),
                image_tag: auth.user.primary_image_tag,
            })
        });
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = auth.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(session) => this.open(session, window, cx),
                Err(err) => this.show_login(
                    LoginOptions {
                        notice: Some(describe(&err).into()),
                        ..LoginOptions::default()
                    },
                    window,
                    cx,
                ),
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
                                        Button::new("switch")
                                            .ghost()
                                            .label("Switch user")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.show_profiles(window, cx)
                                            })),
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
            Screen::Profiles(profiles) => profiles.clone().into_any_element(),
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
            .child(key_nav::frame_start())
            .child(content)
            .child(key_nav::frame_end())
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
