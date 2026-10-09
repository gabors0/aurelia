//! "Who's watching?": the accounts saved on this device, the other users the
//! saved accounts' servers list publicly, and a way to add another account.

use std::collections::HashSet;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Icon, Sizable as _, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, ElementId, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla,
    Pixels, SharedString, Task, Window, div, hsla, px,
};
use jellyfin::User;

use crate::components::art::Art;
use crate::components::aurora::Aurora;
use crate::components::button::focus_ring;
use crate::components::key_nav::{self, Direction};
use crate::components::logo;
use crate::components::motion::{Motion, pressable};
use crate::components::poster_card::LIFT;
use crate::images::ImageRequest;
use crate::loadable::describe;
use crate::runtime;
use crate::session::Session;
use crate::state::AppState;
use crate::theme::{FONT_DISPLAY, Palette};

const CONTEXT: &str = "Profiles";
const SIZE: f32 = 168.;

gpui_kit::actions!(aurelia, [CancelProfiles]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([gpui_kit::KeyBinding::new(
        "escape",
        CancelProfiles,
        Some(CONTEXT),
    )]);
    key_nav::bind_keys(&[CONTEXT], cx);
}

pub enum ProfilesEvent {
    /// A saved account was chosen.
    Open(Session),
    /// A server user signed in without a password.
    SignedIn(Session),
    /// A server user who needs to sign in (password, or Quick Connect).
    SignIn {
        server: String,
        user: String,
    },
    AddAccount,
    /// Back to the account already open.
    Cancel,
}

/// A user a saved account's server lists, who isn't saved here.
#[derive(Clone)]
struct ServerUser {
    server: String,
    server_name: String,
    user: User,
}

pub struct ProfilesView {
    focus: FocusHandle,
    accounts: Vec<Session>,
    others: Vec<ServerUser>,
    /// Signing in as this server user (id).
    busy: Option<String>,
    error: Option<SharedString>,
    can_cancel: bool,
    _load: Task<()>,
    _sign_in: Option<Task<()>>,
}

impl EventEmitter<ProfilesEvent> for ProfilesView {}

impl Focusable for ProfilesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ProfilesView {
    pub fn new(can_cancel: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let accounts = AppState::global(cx).store().accounts();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        // Enter right away opens the account used last.
        key_nav::restore(accounts.first().map(tile_id), cx);
        let load = Self::load_others(&accounts, cx);
        Self {
            focus,
            accounts,
            others: Vec::new(),
            busy: None,
            error: None,
            can_cancel,
            _load: load,
            _sign_in: None,
        }
    }

    /// Asks each saved account's server who else can sign in there.
    fn load_others(accounts: &[Session], cx: &mut Context<Self>) -> Task<()> {
        let mut servers: Vec<(String, String)> = Vec::new();
        for account in accounts {
            if !servers.iter().any(|(s, _)| *s == account.server) {
                servers.push((account.server.clone(), account.server_name.clone()));
            }
        }
        let saved: HashSet<(String, String)> = accounts
            .iter()
            .map(|a| (a.server.clone(), a.user_id.clone()))
            .collect();
        let device = AppState::new_device_id();
        let clients: Vec<_> = servers
            .into_iter()
            .filter_map(|(server, name)| {
                let client = AppState::global(cx)
                    .anonymous_client(&server, &device)
                    .ok()?;
                Some((server, name, client))
            })
            .collect();
        let fetch = runtime::run(cx, async move {
            let mut found = Vec::new();
            for (server, server_name, client) in clients {
                // A server that's down or hides its users just adds nobody.
                let Ok(users) = client.public_users().await else {
                    continue;
                };
                for user in users {
                    if !saved.contains(&(server.clone(), user.id.clone())) {
                        found.push(ServerUser {
                            server: server.clone(),
                            server_name: server_name.clone(),
                            user,
                        });
                    }
                }
            }
            found
        });
        cx.spawn(async move |this, cx| {
            let others = fetch.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                this.others = others;
                cx.notify();
            })
            .ok();
        })
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.accounts = AppState::global(cx).store().accounts();
        self._load = Self::load_others(&self.accounts, cx);
        cx.notify();
    }

    fn remove(&mut self, account: &Session, cx: &mut Context<Self>) {
        crate::accounts::remove(account, cx);
        self.reload(cx);
    }

    fn choose_server_user(&mut self, other: ServerUser, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        if other.user.has_password {
            cx.emit(ProfilesEvent::SignIn {
                server: other.server,
                user: other.user.name,
            });
            return;
        }
        // No password: sign straight in.
        let device = AppState::new_device_id();
        let client = match AppState::global(cx).anonymous_client(&other.server, &device) {
            Ok(client) => client,
            Err(err) => {
                self.error = Some(describe(&err).into());
                cx.notify();
                return;
            }
        };
        self.busy = Some(other.user.id.clone());
        self.error = None;
        let name = other.user.name.clone();
        let auth_client = client.clone();
        let auth = runtime::api(cx, async move {
            auth_client.authenticate_by_name(&name, "").await
        });
        self._sign_in = Some(cx.spawn(async move |this, cx| {
            let result = auth.await;
            this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(auth) => cx.emit(ProfilesEvent::SignedIn(Session {
                        server: client.base_url().to_string(),
                        server_name: other.server_name,
                        user_id: auth.user.id,
                        user_name: auth.user.name,
                        token: auth.access_token,
                        device_id: Some(device),
                        image_tag: auth.user.primary_image_tag,
                    })),
                    // It wants a password after all.
                    Err(jellyfin::Error::Unauthorized) => cx.emit(ProfilesEvent::SignIn {
                        server: other.server,
                        user: other.user.name,
                    }),
                    Err(err) => this.error = Some(describe(&err).into()),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn step(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        key_nav::step(direction, window, cx);
    }

    fn render_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let state = AppState::global(cx);
        let several_servers = self
            .accounts
            .iter()
            .map(|a| a.server.as_str())
            .chain(self.others.iter().map(|o| o.server.as_str()))
            .collect::<HashSet<_>>()
            .len()
            > 1;
        let mut tiles = Vec::new();
        for (i, account) in self.accounts.iter().enumerate() {
            let id = tile_id(account);
            let open = account.clone();
            let removable = account.clone();
            let page = cx.weak_entity();
            let picture = avatar_request(
                state,
                &account.server,
                &account.user_id,
                account.image_tag.as_deref(),
            );
            let detail = several_servers.then(|| account.server_name.clone());
            let name = account.user_name.clone();
            let tile = pressable(id.clone())
                .when(i == 0, |this| this.primary())
                .on_click(
                    cx.listener(move |_, _, _, cx| cx.emit(ProfilesEvent::Open(open.clone()))),
                )
                .look(move |this, m| tile_look(this, m, id, name, detail, picture, None, false))
                .context_menu(move |menu, _, _| {
                    let page = page.clone();
                    let account = removable.clone();
                    menu.item(
                        PopupMenuItem::new("Remove from this device")
                            .icon(IconName::Trash)
                            .on_click(move |_, _, cx| {
                                page.update(cx, |page, cx| page.remove(&account, cx)).ok();
                            }),
                    )
                });
            tiles.push(tile.into_any_element());
        }
        for other in &self.others {
            let id: ElementId =
                SharedString::from(format!("user-{}-{}", other.server, other.user.id)).into();
            let picture = avatar_request(
                state,
                &other.server,
                &other.user.id,
                other.user.primary_image_tag.as_deref(),
            );
            let detail = several_servers.then(|| other.server_name.clone());
            let busy = self.busy.as_deref() == Some(other.user.id.as_str());
            let name = other.user.name.clone();
            let locked = other.user.has_password.then_some(IconName::Lock);
            let chosen = other.clone();
            tiles.push(
                pressable(id.clone())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.choose_server_user(chosen.clone(), cx)
                    }))
                    .look(move |this, m| {
                        tile_look(this, m, id, name, detail, picture, locked, busy)
                    })
                    .into_any_element(),
            );
        }
        tiles.push(
            pressable("add-account")
                .on_click(cx.listener(|_, _, _, cx| cx.emit(ProfilesEvent::AddAccount)))
                .look(|this, m| {
                    this.flex()
                        .flex_col()
                        .items_center()
                        .gap_3()
                        .w(px(SIZE + 24.))
                        .cursor_pointer()
                        .child(
                            div()
                                .relative()
                                .top(px(m.lerp(0., -LIFT)))
                                .size(px(SIZE))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .border_2()
                                .border_dashed()
                                .border_color(m.mix(Palette::border(), Palette::text_secondary()))
                                .bg(m.mix(Palette::glass(), Palette::glass_strong()))
                                .child(focus_ring(m, px(SIZE / 2.)))
                                .child(
                                    Icon::new(IconName::Plus).size(px(52.)).text_color(
                                        m.mix(Palette::text_secondary(), Palette::text()),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(20.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(m.mix(Palette::text_secondary(), Palette::text()))
                                .child("Add account"),
                        )
                })
                .into_any_element(),
        );
        tiles
    }
}

/// The element id of a saved account's tile.
fn tile_id(account: &Session) -> ElementId {
    SharedString::from(format!("account-{}-{}", account.server, account.user_id)).into()
}

fn avatar_request(
    state: &AppState,
    server: &str,
    user_id: &str,
    tag: Option<&str>,
) -> Option<ImageRequest> {
    let tag = tag?;
    let client = state.anonymous_client(server, "avatar").ok()?;
    Some(ImageRequest::new(
        client.user_image_url(user_id, tag, 320).to_string(),
    ))
}

/// A steady colour per name, for avatars without a picture.
fn name_colour(name: &str) -> Hsla {
    let hash = name
        .bytes()
        .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    hsla((hash % 360) as f32 / 360., 0.45, 0.48, 1.)
}

/// A round avatar: the user's picture, or their initial on a colour.
pub fn avatar(
    id: impl Into<ElementId>,
    account: &Session,
    state: &AppState,
    size: Pixels,
) -> AnyElement {
    let picture = avatar_request(
        state,
        &account.server,
        &account.user_id,
        account.image_tag.as_deref(),
    );
    circle(id.into(), &account.user_name, picture, size).into_any_element()
}

fn circle(
    id: ElementId,
    name: &str,
    picture: Option<ImageRequest>,
    size: Pixels,
) -> impl IntoElement {
    let initial = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    div()
        .relative()
        .size(size)
        .flex_shrink_0()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(name_colour(name))
        .text_color(gpui_kit::white())
        .font_family(FONT_DISPLAY)
        .font_weight(FontWeight::BOLD)
        .text_size(size * 0.42)
        .child(initial)
        .when_some(picture, |this, picture| {
            this.child(
                Art::new(id, Some(picture))
                    .bare()
                    .radius(size / 2.)
                    .absolute()
                    .inset_0(),
            )
        })
}

#[allow(clippy::too_many_arguments)]
fn tile_look(
    this: gpui_kit::Stateful<gpui_kit::Div>,
    m: Motion,
    id: ElementId,
    name: String,
    detail: Option<String>,
    picture: Option<ImageRequest>,
    badge: Option<IconName>,
    busy: bool,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let accent: Hsla = Palette::accent().into();
    this.flex()
        .flex_col()
        .items_center()
        .gap_3()
        .w(px(SIZE + 24.))
        .cursor_pointer()
        .child(
            div()
                .relative()
                .top(px(m.lerp(0., -LIFT)))
                .size(px(SIZE))
                .rounded_full()
                .shadow(vec![gpui_kit::BoxShadow {
                    color: Palette::shadow(m.lerp(0.3, 0.5)),
                    offset: gpui_kit::point(px(0.), px(m.lerp(8., 16.))),
                    blur_radius: px(m.lerp(24., 34.)),
                    spread_radius: px(-8.),
                    inset: false,
                }])
                .child(circle(
                    ElementId::from((id, "avatar")),
                    &name,
                    picture,
                    px(SIZE),
                ))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded_full()
                        .border_2()
                        .border_color(accent.opacity(m.amount())),
                )
                .when_some(badge, |this, badge| {
                    this.child(
                        div()
                            .absolute()
                            .bottom_1()
                            .right_1()
                            .size_8()
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(Palette::surface())
                            .border_1()
                            .border_color(Palette::border())
                            .child(
                                Icon::new(badge)
                                    .size_4()
                                    .text_color(Palette::text_secondary()),
                            ),
                    )
                })
                .when(busy, |this| {
                    this.child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(hsla(0., 0., 0., 0.45))
                            .child(Spinner::new().large()),
                    )
                }),
        )
        .child(
            v_flex()
                .items_center()
                .child(
                    div()
                        .text_size(px(20.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(m.mix(Palette::text_caption(), Palette::text()))
                        .max_w(px(SIZE + 24.))
                        .truncate()
                        .child(name),
                )
                .when_some(detail, |this, detail| {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(Palette::text_tertiary())
                            .max_w(px(SIZE + 24.))
                            .truncate()
                            .child(detail),
                    )
                }),
        )
}

impl Render for ProfilesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tiles = self.render_tiles(cx);
        let can_cancel = self.can_cancel;
        div()
            .id("profiles")
            .relative()
            .size_full()
            .bg(Palette::bg())
            .text_color(Palette::text())
            .track_focus(&self.focus)
            .key_context(CONTEXT)
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
            .on_action(cx.listener(move |_, _: &CancelProfiles, _, cx| {
                if can_cancel {
                    cx.emit(ProfilesEvent::Cancel);
                }
            }))
            .child(key_nav::scope(self.focus.clone(), None, px(0.)))
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
                            .gap_10()
                            .pb_16()
                            .px_8()
                            .child(logo::wordmark(px(40.)))
                            .child(
                                div()
                                    .font_family(FONT_DISPLAY)
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(px(52.))
                                    .child("Who's watching?"),
                            )
                            .child(
                                h_flex()
                                    .flex_wrap()
                                    .justify_center()
                                    .items_start()
                                    .gap_x_12()
                                    .gap_y_8()
                                    .max_w(px(1080.))
                                    .children(tiles),
                            )
                            .when_some(self.error.clone(), |this, error| {
                                this.child(div().text_color(Palette::danger()).child(error))
                            })
                            .when(can_cancel, |this| {
                                this.child(
                                    Button::new("profiles-cancel")
                                        .ghost()
                                        .large()
                                        .label("Cancel")
                                        .on_click(cx.listener(|_, _, _, cx| {
                                            cx.emit(ProfilesEvent::Cancel)
                                        })),
                                )
                            }),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_colours_are_steady_per_name() {
        assert_eq!(name_colour("gabor"), name_colour("gabor"));
        assert_ne!(name_colour("gabor"), name_colour("kids"));
    }
}
