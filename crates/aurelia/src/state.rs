//! Process-wide state: who is signed in, to which server, and the clients
//! needed to talk to it.

use std::sync::Arc;

use gpui_kit::{App, Global};
use jellyfin::{Client, DeviceInfo, UserView};
use player::{MpvPlayer, Player};

use crate::session::{Session, SessionStore};

pub struct AppState {
    store: SessionStore,
    /// This installation's device id: accounts saved before each had its own
    /// use it.
    device_id: String,
    session: Option<Session>,
    client: Option<Client>,
    views: Vec<UserView>,
    player: Option<Arc<dyn Player>>,
}

impl Global for AppState {}

impl AppState {
    pub fn init(cx: &mut App) {
        let store = SessionStore::default_location();
        let device_id = store.device_id();
        let player = MpvPlayer::find().map(|p| Arc::new(p) as Arc<dyn Player>);
        cx.set_global(AppState {
            store,
            device_id,
            session: None,
            client: None,
            views: Vec::new(),
            player,
        });
    }

    pub fn global(cx: &App) -> &AppState {
        cx.global::<AppState>()
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

    fn device(&self, device_id: Option<&str>) -> DeviceInfo {
        DeviceInfo::new(
            "Aurelia",
            hostname(),
            device_id.unwrap_or(&self.device_id),
            env!("CARGO_PKG_VERSION"),
        )
    }

    /// A device id for an account about to sign in. Chosen before sign-in
    /// starts: Quick Connect ties the token to the device that asked for it.
    pub fn new_device_id() -> String {
        uuid::Uuid::new_v4().simple().to_string()
    }

    /// An anonymous client for `server` (sign-in) as device `device_id`.
    pub fn anonymous_client(&self, server: &str, device_id: &str) -> jellyfin::Result<Client> {
        Client::new(server, self.device(Some(device_id)))
    }

    /// A signed-in client for any saved account.
    pub fn client_for(&self, session: &Session) -> jellyfin::Result<Client> {
        Ok(
            Client::new(&session.server, self.device(session.device_id.as_deref()))?
                .with_session(session.token.clone(), session.user_id.clone()),
        )
    }

    /// Makes `session` the signed-in account and remembers it as the one to
    /// open next time.
    pub fn sign_in(cx: &mut App, session: Session) -> jellyfin::Result<Client> {
        let state = cx.global_mut::<AppState>();
        let client = state.client_for(&session)?;
        if let Err(err) = state.store.save(&session) {
            tracing::warn!("could not save session: {err}");
        }
        state.session = Some(session);
        state.client = Some(client.clone());
        state.views.clear();
        Ok(client)
    }

    /// Updates the signed-in account's saved copy (e.g. a new avatar).
    pub fn update_session(cx: &mut App, change: impl FnOnce(&mut Session)) {
        let state = cx.global_mut::<AppState>();
        if let Some(session) = state.session.as_mut() {
            change(session);
            if let Err(err) = state.store.save(session) {
                tracing::warn!("could not save session: {err}");
            }
        }
    }

    pub fn set_views(cx: &mut App, views: Vec<UserView>) {
        cx.global_mut::<AppState>().views = views;
    }

    /// Forgets the signed-in account on this device.
    pub fn sign_out(cx: &mut App) {
        let state = cx.global_mut::<AppState>();
        if let Some(session) = state.session.take() {
            state.store.remove(&session);
        }
        state.client = None;
        state.views.clear();
    }

    /// Forgets a saved account (not necessarily the signed-in one).
    pub fn remove_account(cx: &mut App, session: &Session) {
        cx.global::<AppState>().store.remove(session);
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
