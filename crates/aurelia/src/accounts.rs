//! Forgetting a saved account also ends its session on the server, so the
//! token left behind can't be used.

use gpui_kit::App;

use crate::runtime;
use crate::session::Session;
use crate::state::AppState;

pub fn remove(account: &Session, cx: &mut App) {
    if let Ok(client) = AppState::global(cx).client_for(account) {
        // Revoking must not hold anything up; the request outlives the handle.
        drop(runtime::api(cx, async move { client.logout().await }));
    }
    AppState::remove_account(cx, account);
}
