//! The right-click menu for a movie, show, season, episode or collection:
//! the same actions wherever the item appears.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{ContextMenu, ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::prelude::*;
use gpui_kit::{App, SharedString, Styled, Window};
use jellyfin::{BaseItem, ItemKind};

use crate::format;
use crate::nav::Route;
use crate::playback;
use crate::user_data;
use crate::views::shell;

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Play,
    Resume(Duration),
    PlayFromStart,
    SetPlayed(bool),
    SetFavorite(bool),
    GoToSeries {
        id: String,
        name: String,
    },
    GoToSeason {
        series_id: String,
        season_id: String,
        name: String,
    },
    Separator,
}

/// What the menu offers for `item`, top to bottom.
pub fn entries(item: &BaseItem) -> Vec<Entry> {
    let mut groups: Vec<Vec<Entry>> = Vec::new();

    let playable = matches!(
        item.kind,
        ItemKind::Movie | ItemKind::Episode | ItemKind::Video | ItemKind::Series | ItemKind::Season
    );
    if playable {
        groups.push(
            match item.resume_position().filter(|_| item.kind.is_playable()) {
                Some(position) => vec![Entry::Resume(position), Entry::PlayFromStart],
                None => vec![Entry::Play],
            },
        );
    }

    let mut marks = Vec::new();
    if playable || item.kind == ItemKind::BoxSet {
        marks.push(Entry::SetPlayed(!item.is_played()));
    }
    if playable || matches!(item.kind, ItemKind::BoxSet | ItemKind::Person) {
        marks.push(Entry::SetFavorite(!item.is_favorite()));
    }
    groups.push(marks);

    let mut go = Vec::new();
    if matches!(item.kind, ItemKind::Episode | ItemKind::Season)
        && let Some(series_id) = item.series_id.clone()
    {
        go.push(Entry::GoToSeries {
            id: series_id.clone(),
            name: item.series_name.clone().unwrap_or_else(|| "show".into()),
        });
        if item.kind == ItemKind::Episode
            && let Some(season_id) = item.season_id.clone()
        {
            let name = item
                .season_name
                .clone()
                .or_else(|| item.parent_index_number.map(|n| format!("Season {n}")))
                .unwrap_or_else(|| "season".into());
            go.push(Entry::GoToSeason {
                series_id,
                season_id,
                name,
            });
        }
    }
    groups.push(go);

    let mut entries = Vec::new();
    for group in groups.into_iter().filter(|g| !g.is_empty()) {
        if !entries.is_empty() {
            entries.push(Entry::Separator);
        }
        entries.extend(group);
    }
    entries
}

fn label(entry: &Entry) -> (SharedString, IconName) {
    match entry {
        Entry::Play => ("Play".into(), IconName::Play),
        Entry::Resume(position) => (
            format!("Resume from {}", format::clock(*position)).into(),
            IconName::Play,
        ),
        Entry::PlayFromStart => ("Play from start".into(), IconName::RotateCcw),
        Entry::SetPlayed(true) => ("Mark as watched".into(), IconName::CircleCheck),
        Entry::SetPlayed(false) => ("Mark as unwatched".into(), IconName::EyeOff),
        Entry::SetFavorite(true) => ("Add to favourites".into(), IconName::Heart),
        Entry::SetFavorite(false) => ("Remove from favourites".into(), IconName::HeartOff),
        Entry::GoToSeries { name, .. } => (format!("Go to {name}").into(), IconName::Tv),
        Entry::GoToSeason { name, .. } => (format!("Go to {name}").into(), IconName::Layers),
        Entry::Separator => (SharedString::default(), IconName::Minus),
    }
}

fn run(entry: &Entry, item: &BaseItem, window: &mut Window, cx: &mut App) {
    match entry {
        Entry::Play | Entry::Resume(_) => playback::play_item(item, false, window, cx),
        Entry::PlayFromStart => playback::play_item(item, true, window, cx),
        Entry::SetPlayed(played) => user_data::set_played(item, *played, window, cx),
        Entry::SetFavorite(favorite) => user_data::set_favorite(item, *favorite, window, cx),
        Entry::GoToSeries { id, .. } => shell::navigate(
            Route::Series {
                id: id.clone(),
                season_id: None,
            },
            window,
            cx,
        ),
        Entry::GoToSeason {
            series_id,
            season_id,
            ..
        } => shell::navigate(
            Route::Series {
                id: series_id.clone(),
                season_id: Some(season_id.clone()),
            },
            window,
            cx,
        ),
        Entry::Separator => {}
    }
}

/// Fills `menu` with the actions for `item`.
pub fn build(mut menu: PopupMenu, item: &BaseItem) -> PopupMenu {
    for entry in entries(item) {
        if entry == Entry::Separator {
            menu = menu.separator();
            continue;
        }
        let (text, icon) = label(&entry);
        let item = item.clone();
        menu = menu.item(
            PopupMenuItem::new(text)
                .icon(icon)
                .on_click(move |_, window, cx| run(&entry, &item, window, cx)),
        );
    }
    menu.min_w(gpui_kit::px(220.))
}

/// Gives `element` the item's right-click menu.
pub fn attach<E>(element: E, item: BaseItem) -> ContextMenu<E>
where
    E: InteractiveElement + ParentElement + Styled + 'static,
{
    element.context_menu(move |menu, _, _| build(menu, &item))
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        Bounds, Context, IntoElement, Render, TestAppContext, TestSupportExt as _, Window,
        WindowBounds, WindowOptions, div, px, size,
    };

    use super::*;

    fn item(json: serde_json::Value) -> BaseItem {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn movie_in_progress() {
        let movie = item(serde_json::json!({
            "Id": "m", "Name": "Dune", "Type": "Movie",
            "UserData": {"PlaybackPositionTicks": 6_000_000_000i64, "IsFavorite": true}
        }));
        assert_eq!(
            entries(&movie),
            vec![
                Entry::Resume(Duration::from_secs(600)),
                Entry::PlayFromStart,
                Entry::Separator,
                Entry::SetPlayed(true),
                Entry::SetFavorite(false),
            ]
        );
    }

    #[test]
    fn watched_episode_links_to_its_show_and_season() {
        let episode = item(serde_json::json!({
            "Id": "e", "Name": "Half Loop", "Type": "Episode",
            "SeriesId": "show", "SeriesName": "Severance",
            "SeasonId": "s1", "ParentIndexNumber": 1,
            "UserData": {"Played": true}
        }));
        assert_eq!(
            entries(&episode),
            vec![
                Entry::Play,
                Entry::Separator,
                Entry::SetPlayed(false),
                Entry::SetFavorite(true),
                Entry::Separator,
                Entry::GoToSeries {
                    id: "show".into(),
                    name: "Severance".into()
                },
                Entry::GoToSeason {
                    series_id: "show".into(),
                    season_id: "s1".into(),
                    name: "Season 1".into()
                },
            ]
        );
    }

    #[test]
    fn collections_and_people_are_not_played() {
        let boxset = item(serde_json::json!({"Id": "b", "Name": "Alien", "Type": "BoxSet"}));
        assert_eq!(
            entries(&boxset),
            vec![Entry::SetPlayed(true), Entry::SetFavorite(true)]
        );
        let person = item(serde_json::json!({"Id": "p", "Name": "Sigourney", "Type": "Person"}));
        assert_eq!(entries(&person), vec![Entry::SetFavorite(true)]);
    }

    struct Card;

    impl Render for Card {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let movie: BaseItem = serde_json::from_value(serde_json::json!({
                "Id": "m", "Name": "Dune", "Type": "Movie"
            }))
            .unwrap();
            div().size_full().child(attach(
                div().id("card").test_support().size(px(120.)),
                movie,
            ))
        }
    }

    #[gpui_kit::test]
    fn right_click_opens_the_menu(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (window, _) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(600.), px(400.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| Card),
            )
            .expect("open test window")
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("popup-menu").is_none());
            window.right_click("card", cx);
        })
        .unwrap();
        // The menu is built in a deferred callback after the click.
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("popup-menu").is_some(), "menu is open");
        })
        .unwrap();
    }
}
