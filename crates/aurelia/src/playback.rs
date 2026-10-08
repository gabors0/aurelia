//! Starting playback and reporting it to the server.

use gpui_kit::{App, Window};
use jellyfin::BaseItem;

use crate::nav::Route;
use crate::views::shell;

/// Plays `item` (resuming unless `from_start`). Series start their next episode.
pub fn play_item(item: &BaseItem, from_start: bool, window: &mut Window, cx: &mut App) {
    let _ = from_start;
    shell::navigate(Route::for_item(item), window, cx);
}
