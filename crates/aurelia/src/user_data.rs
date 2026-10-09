//! Marking things watched or favourite, from any page or menu.
//!
//! The change shows at once: the item is patched in every page in the
//! history. Then the server is asked; its answer replaces the guess (or the
//! old state comes back with an error), and every page reloads so shelves that
//! depend on it (Continue Watching, Next Up, Favourites) follow.

use std::future::Future;

use gpui_kit::{App, Window};
use jellyfin::{BaseItem, Client, Ticks, UserData};

use crate::runtime;
use crate::state::AppState;
use crate::views::shell;

/// `data` as it will be once the item is marked (un)watched.
pub fn with_played(data: Option<&UserData>, played: bool) -> UserData {
    let mut data = data.cloned().unwrap_or_default();
    data.played = played;
    if played {
        data.playback_position_ticks = Ticks::ZERO;
        data.played_percentage = None;
        if data.unplayed_item_count.is_some() {
            data.unplayed_item_count = Some(0);
        }
    }
    data
}

/// `data` as it will be once the item is (un)favourited.
pub fn with_favorite(data: Option<&UserData>, favorite: bool) -> UserData {
    let mut data = data.cloned().unwrap_or_default();
    data.is_favorite = favorite;
    data
}

/// Replaces the user data of every copy of item `id`; true if any changed.
pub fn patch<'a>(
    items: impl IntoIterator<Item = &'a mut BaseItem>,
    id: &str,
    data: &UserData,
) -> bool {
    let mut changed = false;
    for item in items {
        if item.id == id && item.user_data.as_ref() != Some(data) {
            item.user_data = Some(data.clone());
            changed = true;
        }
    }
    changed
}

pub fn set_played(item: &BaseItem, played: bool, window: &mut Window, cx: &mut App) {
    let optimistic = with_played(item.user_data.as_ref(), played);
    change(item, optimistic, window, cx, move |client, id| async move {
        client.set_played(&id, played).await
    });
}

pub fn set_favorite(item: &BaseItem, favorite: bool, window: &mut Window, cx: &mut App) {
    let optimistic = with_favorite(item.user_data.as_ref(), favorite);
    change(item, optimistic, window, cx, move |client, id| async move {
        client.set_favorite(&id, favorite).await
    });
}

fn change<F, Fut>(item: &BaseItem, optimistic: UserData, window: &mut Window, cx: &mut App, send: F)
where
    F: FnOnce(Client, String) -> Fut,
    Fut: Future<Output = jellyfin::Result<UserData>> + Send + 'static,
{
    let id = item.id.clone();
    let previous = item.user_data.clone().unwrap_or_default();
    shell::patch_user_data(&id, &optimistic, cx);
    let request = runtime::api(cx, send(AppState::client(cx), id.clone()));
    window
        .spawn(cx, async move |cx| {
            let result = request.await;
            cx.update(|window, cx| match result {
                Ok(data) => {
                    shell::patch_user_data(&id, &data, cx);
                    shell::refresh_all(window, cx);
                }
                Err(err) => {
                    shell::patch_user_data(&id, &previous, cx);
                    shell::report_error(&err, window, cx);
                }
            })
            .ok();
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, data: Option<UserData>) -> BaseItem {
        BaseItem {
            id: id.into(),
            user_data: data,
            ..Default::default()
        }
    }

    #[test]
    fn marking_watched_clears_progress() {
        let data = UserData {
            played_percentage: Some(40.),
            playback_position_ticks: Ticks(1_000),
            unplayed_item_count: Some(3),
            ..Default::default()
        };
        let watched = with_played(Some(&data), true);
        assert!(watched.played);
        assert_eq!(watched.playback_position_ticks, Ticks::ZERO);
        assert_eq!(watched.played_percentage, None);
        assert_eq!(watched.unplayed_item_count, Some(0));

        let unwatched = with_played(Some(&watched), false);
        assert!(!unwatched.played);
        assert_eq!(unwatched.unplayed_item_count, Some(0), "server fills it in");
    }

    #[test]
    fn favourite_keeps_the_rest() {
        let data = UserData {
            played: true,
            ..Default::default()
        };
        let favourite = with_favorite(Some(&data), true);
        assert!(favourite.is_favorite && favourite.played);
        assert!(with_favorite(None, true).is_favorite);
    }

    #[test]
    fn patch_updates_every_copy() {
        let mut items = [item("a", None), item("b", None), item("a", None)];
        let data = with_favorite(None, true);
        assert!(patch(items.iter_mut(), "a", &data));
        assert!(items[0].is_favorite() && items[2].is_favorite());
        assert!(!items[1].is_favorite());
        assert!(
            !patch(items.iter_mut(), "a", &data),
            "nothing changes the second time"
        );
        assert!(!patch(items.iter_mut(), "zzz", &data));
    }
}
