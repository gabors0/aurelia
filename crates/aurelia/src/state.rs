//! Process-wide state: who is signed in, to which server, and the clients
//! needed to talk to it.

use std::sync::Arc;

use gpui_kit::{App, Global};
use jellyfin::{Client, DeviceInfo, UserView};
use player::{MpvPlayer, Player};

use crate::session::{Session, SessionStore};

pub struct AppState {
    store: SessionStore,
    device: DeviceInfo,
    session: Option<Session>,
    client: Option<Client>,
    views: Vec<UserView>,
    player: Option<Arc<dyn Player>>,
}

impl Global for AppState {}

impl AppState {
    pub fn init(cx: &mut App) {
        let store = SessionStore::default_location();
        let device = DeviceInfo::new(
            "Aurelia",
            hostname(),
            store.device_id(),
            env!("CARGO_PKG_VERSION"),
        );
        let player = MpvPlayer::find().map(|p| Arc::new(p) as Arc<dyn Player>);
        cx.set_global(AppState {
            store,
            device,
            session: None,
            client: None,
            views: Vec::new(),
            player,
        });
    }

    pub fn global(cx: &App) -> &AppState {
        cx.global::<AppState>()
    }

    pub fn device(&self) -> &DeviceInfo {
        &self.device
    }

    pub fn store(&self) -> &SessionStore {
        &self.store
    }

    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    pub fn views(&self) -> &[UserView] {
        &self.views
    }

    pub fn player(&self) -> Option<Arc<dyn Player>> {
        self.player.clone()
    }

    /// The signed-in client. Views only exist while signed in.
    pub fn client(cx: &App) -> Client {
        cx.global::<AppState>()
            .client
            .clone()
            .expect("client requested while signed out")
    }

    /// An anonymous client for `server` (login screen).
    pub fn anonymous_client(&self, server: &str) -> jellyfin::Result<Client> {
        Client::new(server, self.device.clone())
    }

    pub fn sign_in(cx: &mut App, session: Session) -> jellyfin::Result<Client> {
        let state = cx.global_mut::<AppState>();
        let client = Client::new(&session.server, state.device.clone())?
            .with_session(session.token.clone(), session.user_id.clone());
        if let Err(err) = state.store.save(&session) {
            tracing::warn!("could not save session: {err}");
        }
        state.session = Some(session);
        state.client = Some(client.clone());
        Ok(client)
    }

    /// Restores a saved session without persisting anything.
    pub fn restore(cx: &mut App, session: Session) -> jellyfin::Result<Client> {
        let state = cx.global_mut::<AppState>();
        let client = Client::new(&session.server, state.device.clone())?
            .with_session(session.token.clone(), session.user_id.clone());
        state.session = Some(session);
        state.client = Some(client.clone());
        Ok(client)
    }

    pub fn set_views(cx: &mut App, views: Vec<UserView>) {
        cx.global_mut::<AppState>().views = views;
    }

    pub fn sign_out(cx: &mut App) {
        let state = cx.global_mut::<AppState>();
        state.store.clear();
        state.session = None;
        state.client = None;
        state.views.clear();
    }
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "Desktop".into())
}
