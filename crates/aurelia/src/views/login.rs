//! Sign-in: pick a server, then a password or Quick Connect.

use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Disableable as _, Sizable as _, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    Context, Entity, EventEmitter, FontWeight, SharedString, Subscription, Task, Window, div, px,
};
use jellyfin::{Client, PublicSystemInfo};

use crate::components::aurora::Aurora;
use crate::components::glass::glass;
use crate::components::logo;
use crate::loadable::describe;
use crate::runtime;
use crate::session::Session;
use crate::state::AppState;
use crate::theme::{FONT_DISPLAY, Palette};

pub const DEFAULT_SERVER: &str = "https://jellyfin.gs0.me";

pub enum LoginEvent {
    SignedIn(Session),
    /// Back to the profile picker or the open account.
    Back,
}

/// How sign-in opens.
#[derive(Default)]
pub struct LoginOptions {
    /// Shown above the form (e.g. "Your session has expired").
    pub notice: Option<SharedString>,
    /// A known server: probed right away.
    pub server: Option<String>,
    pub user: Option<String>,
    /// Offer a way back (to the picker or the open account).
    pub can_go_back: bool,
}

enum Step {
    Server,
    Credentials {
        client: Client,
        info: PublicSystemInfo,
        quick_connect: bool,
    },
    QuickConnect {
        client: Client,
        info: PublicSystemInfo,
        code: SharedString,
        _poll: Task<()>,
    },
}

pub struct LoginView {
    step: Step,
    /// The device id the new account signs in as (see `Session::device_id`).
    device_id: String,
    can_go_back: bool,
    server: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    busy: bool,
    error: Option<SharedString>,
    notice: Option<SharedString>,
    _task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LoginEvent> for LoginView {}

impl LoginView {
    pub fn new(options: LoginOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let from_env = options
            .server
            .clone()
            .or_else(|| std::env::var("AURELIA_SERVER").ok());
        let last = from_env
            .clone()
            .or_else(|| AppState::global(cx).store().last_server());
        let server_value = last
            .map(|s| s.trim_end_matches('/').to_string())
            .unwrap_or_else(|| DEFAULT_SERVER.to_string());
        let server = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("jellyfin.example.com")
                .default_value(server_value)
        });
        let user = options.user.clone().unwrap_or_default();
        let username = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Username")
                .default_value(user)
        });
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password")
                .masked(true)
        });

        let subscriptions = vec![
            cx.subscribe_in(&server, window, Self::on_input),
            cx.subscribe_in(&username, window, Self::on_input),
            cx.subscribe_in(&password, window, Self::on_input),
        ];
        server.update(cx, |input, cx| input.focus(window, cx));

        let mut this = Self {
            step: Step::Server,
            device_id: AppState::new_device_id(),
            can_go_back: options.can_go_back,
            server,
            username,
            password,
            busy: false,
            error: None,
            notice: options.notice,
            _task: None,
            _subscriptions: subscriptions,
        };
        // A known server (or one given in the environment) is probed right away.
        if from_env.is_some() {
            this.probe_server(window, cx);
        }
        this
    }

    fn on_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            // A message about what was typed before is stale once you edit.
            InputEvent::Change if self.error.is_some() => {
                self.error = None;
                cx.notify();
            }
            // Enter only acts for the fields of the step on screen; a stray
            // Enter in the (hidden) server field must not submit sign-in.
            InputEvent::PressEnter { .. } => {
                let on_screen = match self.step {
                    Step::Server => input == &self.server,
                    Step::Credentials { .. } => input == &self.username || input == &self.password,
                    Step::QuickConnect { .. } => false,
                };
                if on_screen {
                    self.submit(window, cx);
                }
            }
            _ => {}
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        match &self.step {
            Step::Server => self.probe_server(window, cx),
            Step::Credentials { .. } => self.sign_in(window, cx),
            Step::QuickConnect { .. } => {}
        }
    }

    fn probe_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let address = self.server.read(cx).value().to_string();
        let client = match AppState::global(cx).anonymous_client(&address, &self.device_id) {
            Ok(client) => client,
            Err(err) => {
                self.error = Some(describe(&err).into());
                cx.notify();
                return;
            }
        };
        self.busy = true;
        self.error = None;
        let probe_client = client.clone();
        let probe = runtime::api(cx, async move {
            let info = probe_client.public_info().await?;
            let quick_connect = probe_client.quick_connect_enabled().await.unwrap_or(false);
            Ok((info, quick_connect))
        });
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = probe.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok((info, quick_connect)) => {
                        this.step = Step::Credentials {
                            client,
                            info,
                            quick_connect,
                        };
                        // Carry on typing: focus the first empty field.
                        let target = if this.username.read(cx).value().is_empty() {
                            this.username.clone()
                        } else {
                            this.password.clone()
                        };
                        target.update(cx, |input, cx| input.focus(window, cx));
                    }
                    Err(err) => this.error = Some(describe(&err).into()),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Step::Credentials { client, info, .. } = &self.step else {
            return;
        };
        let username = self.username.read(cx).value().trim().to_string();
        let password = self.password.read(cx).value().to_string();
        if username.is_empty() {
            self.error = Some("Enter your username.".into());
            self.username
                .update(cx, |input, cx| input.focus(window, cx));
            cx.notify();
            return;
        }
        self.busy = true;
        self.error = None;
        let (client, info) = (client.clone(), info.clone());
        let auth_client = client.clone();
        let auth = runtime::api(cx, async move {
            auth_client.authenticate_by_name(&username, &password).await
        });
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = auth.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(auth) => cx.emit(LoginEvent::SignedIn(session_from(&client, &info, auth))),
                    Err(jellyfin::Error::Unauthorized) => {
                        this.error = Some("That username and password didn't work.".into())
                    }
                    Err(err) => this.error = Some(describe(&err).into()),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn start_quick_connect(&mut self, cx: &mut Context<Self>) {
        let Step::Credentials { client, info, .. } = &self.step else {
            return;
        };
        let (client, info) = (client.clone(), info.clone());
        self.busy = true;
        self.error = None;
        let initiate_client = client.clone();
        let initiate = runtime::api(
            cx,
            async move { initiate_client.quick_connect_initiate().await },
        );
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = initiate.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(state) => {
                        let poll =
                            this.poll_quick_connect(client.clone(), info.clone(), state.secret, cx);
                        this.step = Step::QuickConnect {
                            client,
                            info,
                            code: state.code.into(),
                            _poll: poll,
                        };
                    }
                    Err(err) => this.error = Some(describe(&err).into()),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn poll_quick_connect(
        &mut self,
        client: Client,
        info: PublicSystemInfo,
        secret: String,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                let poll_client = client.clone();
                let poll_secret = secret.clone();
                let state = cx.update(|cx| {
                    runtime::api(cx, async move {
                        poll_client.quick_connect_state(&poll_secret).await
                    })
                });
                match state.await {
                    Ok(state) if state.authenticated => break,
                    Ok(_) => continue,
                    Err(err) => {
                        this.update(cx, |this, cx| {
                            this.error = Some(describe(&err).into());
                            this.cancel_quick_connect(cx);
                        })
                        .ok();
                        return;
                    }
                }
            }
            let auth_client = client.clone();
            let auth = cx.update(|cx| {
                runtime::api(cx, async move {
                    auth_client.authenticate_with_quick_connect(&secret).await
                })
            });
            let result = auth.await;
            this.update(cx, |this, cx| match result {
                Ok(auth) => cx.emit(LoginEvent::SignedIn(session_from(&client, &info, auth))),
                Err(err) => {
                    this.error = Some(describe(&err).into());
                    this.cancel_quick_connect(cx);
                }
            })
            .ok();
        })
    }

    fn cancel_quick_connect(&mut self, cx: &mut Context<Self>) {
        if let Step::QuickConnect { client, info, .. } = &self.step {
            self.step = Step::Credentials {
                client: client.clone(),
                info: info.clone(),
                quick_connect: true,
            };
            cx.notify();
        }
    }

    fn change_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.step = Step::Server;
        self.error = None;
        self.server.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn render_server_step(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_4()
            .child(heading("Connect to your server"))
            .child(
                v_flex()
                    .gap_2()
                    .child(label("Server address"))
                    .child(Input::new(&self.server).large()),
            )
            .child(
                Button::new("continue")
                    .primary()
                    .large()
                    .w_full()
                    .label("Continue")
                    .loading(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            )
    }

    fn render_credentials_step(
        &self,
        info: &PublicSystemInfo,
        quick_connect: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .gap_4()
            .child(server_pill(info, cx))
            .child(heading("Sign in"))
            .child(
                v_flex()
                    .gap_2()
                    .child(Input::new(&self.username).large())
                    .child(Input::new(&self.password).large().mask_toggle()),
            )
            .child(
                Button::new("sign-in")
                    .primary()
                    .large()
                    .w_full()
                    .label("Sign in")
                    .loading(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            )
            .when(quick_connect, |this| {
                this.child(
                    Button::new("quick-connect")
                        .ghost()
                        .large()
                        .w_full()
                        .icon(gpui_kit::assets::IconName::Smartphone)
                        .label("Use Quick Connect")
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, _, cx| this.start_quick_connect(cx))),
                )
            })
    }

    fn render_quick_connect_step(
        &self,
        info: &PublicSystemInfo,
        code: &SharedString,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .gap_4()
            .child(server_pill(info, cx))
            .child(heading("Quick Connect"))
            .child(div().text_color(Palette::text_secondary()).child(
                "Enter this code in Quick Connect on a device where you're already signed in.",
            ))
            .child(
                div()
                    .py_4()
                    .rounded(px(14.))
                    .bg(Palette::glass())
                    .border_1()
                    .border_color(Palette::border())
                    .text_center()
                    .font_family(FONT_DISPLAY)
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(40.))
                    .child(spaced(code)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_center()
                    .text_color(Palette::text_secondary())
                    .child(Spinner::new().small())
                    .child("Waiting for approval…"),
            )
            .child(
                Button::new("cancel-qc")
                    .ghost()
                    .large()
                    .w_full()
                    .label("Cancel")
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_quick_connect(cx))),
            )
    }
}

fn session_from(client: &Client, info: &PublicSystemInfo, auth: jellyfin::AuthResult) -> Session {
    Session {
        server: client.base_url().to_string(),
        server_name: info.server_name.clone(),
        user_id: auth.user.id,
        user_name: auth.user.name,
        token: auth.access_token,
        device_id: Some(client.device().device_id().to_string()),
        image_tag: auth.user.primary_image_tag,
    }
}

fn heading(text: &'static str) -> impl IntoElement {
    div()
        .font_family(FONT_DISPLAY)
        .font_weight(FontWeight::BOLD)
        .text_size(px(24.))
        .child(text)
}

fn label(text: &'static str) -> impl IntoElement {
    div()
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_color(Palette::text_secondary())
        .child(text)
}

fn spaced(code: &SharedString) -> String {
    code.chars()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

fn server_pill(info: &PublicSystemInfo, cx: &mut Context<LoginView>) -> impl IntoElement {
    h_flex()
        .justify_between()
        .gap_2()
        .px_3()
        .py_2()
        .rounded(px(12.))
        .bg(Palette::glass())
        .child(
            h_flex()
                .gap_2()
                .child(div().size_2().rounded_full().bg(gpui_kit::rgb(0x4ADE80)))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(info.server_name.clone()),
                )
                .child(
                    div()
                        .text_color(Palette::text_tertiary())
                        .child(format!("Jellyfin {}", info.version)),
                ),
        )
        .child(
            Button::new("change-server")
                .ghost()
                .small()
                .label("Change")
                .on_click(cx.listener(|this, _, window, cx| this.change_server(window, cx))),
        )
}

impl Render for LoginView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.step {
            Step::Server => self.render_server_step(cx).into_any_element(),
            Step::Credentials {
                info,
                quick_connect,
                ..
            } => {
                let (info, quick_connect) = (info.clone(), *quick_connect);
                self.render_credentials_step(&info, quick_connect, cx)
                    .into_any_element()
            }
            Step::QuickConnect { info, code, .. } => {
                let (info, code) = (info.clone(), code.clone());
                self.render_quick_connect_step(&info, &code, cx)
                    .into_any_element()
            }
        };

        div()
            .relative()
            .size_full()
            .bg(Palette::bg())
            .text_color(Palette::text())
            .child(Aurora)
            .child(
                v_flex()
                    .absolute()
                    .inset_0()
                    .child(TitleBar::new().h(px(52.)))
                    .child(
                        v_flex()
                            .flex_1()
                            .items_center()
                            .justify_center()
                            .gap_8()
                            .pb_16()
                            .child(
                                v_flex()
                                    .items_center()
                                    .gap_3()
                                    .child(logo::wordmark(px(56.)))
                                    .child(
                                        div()
                                            .text_color(Palette::text_secondary())
                                            .child("Your Jellyfin library, in cinema mode."),
                                    ),
                            )
                            .child(
                                glass().w(px(420.)).p_8().bg(Palette::panel()).child(
                                    v_flex()
                                        .gap_4()
                                        .when_some(self.notice.clone(), |this, notice| {
                                            this.child(
                                                div()
                                                    .text_sm()
                                                    .px_3()
                                                    .py_2()
                                                    .rounded(px(10.))
                                                    .bg(gpui_kit::Hsla::from(Palette::accent())
                                                        .opacity(0.14))
                                                    .child(notice),
                                            )
                                        })
                                        .child(body)
                                        .when_some(self.error.clone(), |this, error| {
                                            this.child(
                                                div()
                                                    .text_sm()
                                                    .text_color(Palette::danger())
                                                    .child(error),
                                            )
                                        }),
                                ),
                            )
                            .when(self.can_go_back, |this| {
                                this.child(
                                    Button::new("login-back")
                                        .ghost()
                                        .icon(gpui_kit::assets::IconName::ChevronLeft)
                                        .label("Back")
                                        .on_click(
                                            cx.listener(|_, _, _, cx| cx.emit(LoginEvent::Back)),
                                        ),
                                )
                            }),
                    ),
            )
    }
}
