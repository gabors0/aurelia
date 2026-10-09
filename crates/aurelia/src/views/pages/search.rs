//! Search as you type across every library: movies, shows, collections,
//! people and episodes. Before anything is typed, genres to browse.

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, Entity, FocusHandle, Focusable as _, FontWeight, ScrollHandle,
    SharedString, Subscription, Task, Window, div, hsla, linear_color_stop, linear_gradient, px,
};
use jellyfin::{BaseItem, ItemKind, ItemsQuery, UserData};

use super::grid;
use crate::components::art::Art;
use crate::components::cast::people_row;
use crate::components::poster_card::PosterCard;
use crate::components::row::{ROW_PADDING, row};
use crate::images::ImageRequest;
use crate::loadable::Loadable;
use crate::nav::Route;
use crate::runtime;
use crate::state::AppState;
use crate::theme::{FONT_DISPLAY, Palette};
use crate::user_data;
use crate::views::shell::{self, NAV_HEIGHT};

const DEBOUNCE: Duration = Duration::from_millis(250);
const PER_SHELF: u32 = 24;
const TILE_MIN_WIDTH: f32 = 220.;
const TILE_GAP: f32 = 16.;

/// Shelves in the order they're shown.
const SHELVES: [(Shelf, &str); 5] = [
    (Shelf::Movies, "Movies"),
    (Shelf::Shows, "Shows"),
    (Shelf::Collections, "Collections"),
    (Shelf::People, "People"),
    (Shelf::Episodes, "Episodes"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Shelf {
    Movies,
    Shows,
    Collections,
    People,
    Episodes,
}

impl Shelf {
    fn id(self) -> &'static str {
        match self {
            Shelf::Movies => "search-movies",
            Shelf::Shows => "search-shows",
            Shelf::Collections => "search-collections",
            Shelf::People => "search-people",
            Shelf::Episodes => "search-episodes",
        }
    }

    fn kind(self) -> ItemKind {
        match self {
            Shelf::Movies => ItemKind::Movie,
            Shelf::Shows => ItemKind::Series,
            Shelf::Collections => ItemKind::BoxSet,
            Shelf::People => ItemKind::Person,
            Shelf::Episodes => ItemKind::Episode,
        }
    }
}

/// What a search found, per shelf.
#[derive(Debug, Clone, Default)]
struct Results(HashMap<Shelf, Vec<BaseItem>>);

impl Results {
    fn get(&self, shelf: Shelf) -> &[BaseItem] {
        self.0.get(&shelf).map(Vec::as_slice).unwrap_or_default()
    }

    fn is_empty(&self) -> bool {
        self.0.values().all(Vec::is_empty)
    }

    fn items_mut(&mut self) -> impl Iterator<Item = &mut BaseItem> {
        self.0.values_mut().flatten()
    }
}

async fn search(client: jellyfin::Client, term: String) -> jellyfin::Result<Results> {
    let items = |shelf: Shelf| {
        let client = client.clone();
        let term = term.clone();
        async move {
            let query = ItemsQuery {
                search_term: Some(term),
                include_item_types: vec![shelf.kind()],
                limit: PER_SHELF,
                ..ItemsQuery::default()
            };
            client.items(&query).await.map(|page| page.items)
        }
    };
    let (movies, shows, collections, episodes, people) = tokio::try_join!(
        items(Shelf::Movies),
        items(Shelf::Shows),
        items(Shelf::Collections),
        items(Shelf::Episodes),
        client.persons(&term, PER_SHELF),
    )?;
    Ok(Results(HashMap::from([
        (Shelf::Movies, movies),
        (Shelf::Shows, shows),
        (Shelf::Collections, collections),
        (Shelf::People, people),
        (Shelf::Episodes, episodes),
    ])))
}

pub struct SearchPage {
    input: Entity<InputState>,
    /// The text the results are for (trimmed).
    query: String,
    results: Loadable<Results>,
    searching: bool,
    genres: Loadable<Vec<BaseItem>>,
    scroll: ScrollHandle,
    rows: HashMap<Shelf, ScrollHandle>,
    _search: Option<Task<()>>,
    _genres: Option<Task<()>>,
    _input: Subscription,
}

impl SearchPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Movies, shows, people, collections…")
        });
        let subscription = cx.subscribe_in(&input, window, Self::on_input);
        let mut this = Self {
            input,
            query: String::new(),
            results: Loadable::Ready(Results::default()),
            searching: false,
            genres: Loadable::Loading,
            scroll: ScrollHandle::new(),
            rows: SHELVES
                .iter()
                .map(|(shelf, _)| (*shelf, ScrollHandle::new()))
                .collect(),
            _search: None,
            _genres: None,
            _input: subscription,
        };
        this.load_genres(cx);
        this
    }

    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll
    }

    pub fn focus_handle(&self, cx: &gpui_kit::App) -> FocusHandle {
        self.input.focus_handle(cx)
    }

    /// Fills in the box and searches right away (development hook).
    pub fn set_query(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.set_value(text.to_string(), window, cx);
            input.set_clean_on_escape(!text.is_empty());
        });
        self.search_now(text.trim().to_string(), cx);
    }

    fn load_genres(&mut self, cx: &mut Context<Self>) {
        let client = AppState::client(cx);
        let fetch = runtime::api(cx, async move {
            client
                .genres(None, &[ItemKind::Movie, ItemKind::Series])
                .await
        });
        self._genres = Some(cx.spawn(async move |this, cx| {
            let result = fetch.await;
            this.update(cx, |this, cx| {
                if let Err(err) = &result {
                    shell::load_failed(err, cx);
                }
                this.genres = Loadable::from_result(result);
                cx.notify();
            })
            .ok();
        }));
    }

    fn on_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        // Esc clears what's typed; on an empty box it goes back.
        let typed = !input.read(cx).value().is_empty();
        input.update(cx, |input, _| input.set_clean_on_escape(typed));
        let text = input.read(cx).value().trim().to_string();
        if text == self.query {
            return;
        }
        if text.is_empty() {
            self._search = None;
            self.query.clear();
            self.searching = false;
            self.results = Loadable::Ready(Results::default());
            cx.notify();
            return;
        }
        // Wait for a pause in typing; a newer keystroke drops this task.
        self.searching = true;
        self._search = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            this.update(cx, |this, cx| this.search_now(text, cx)).ok();
        }));
        cx.notify();
    }

    fn search_now(&mut self, text: String, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }
        self.query = text.clone();
        self.searching = true;
        let fetch = runtime::api(cx, search(AppState::client(cx), text));
        self._search = Some(cx.spawn(async move |this, cx| {
            let result = fetch.await;
            this.update(cx, |this, cx| {
                this.searching = false;
                match result {
                    Ok(results) => {
                        this.results = Loadable::Ready(results);
                        this.scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
                        for handle in this.rows.values() {
                            handle.set_offset(gpui_kit::point(px(0.), px(0.)));
                        }
                    }
                    Err(err) => this.results = Loadable::Failed(shell::load_failed(&err, cx)),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Searches again (new items, changed watch state).
    pub fn refresh(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.query.is_empty() {
            self.search_now(self.query.clone(), cx);
        }
    }

    pub fn patch_user_data(&mut self, id: &str, data: &UserData) -> bool {
        match self.results.ready_mut() {
            Some(results) => user_data::patch(results.items_mut(), id, data),
            None => false,
        }
    }

    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.set_clean_on_escape(false);
            input.focus(window, cx);
        });
        self._search = None;
        self.query.clear();
        self.searching = false;
        self.results = Loadable::Ready(Results::default());
        cx.notify();
    }

    fn render_box(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.input.focus_handle(cx).is_focused(window);
        let has_text = !self.input.read(cx).value().is_empty();
        h_flex()
            .id("search-box")
            .h(px(60.))
            .px_5()
            .gap_3()
            .rounded_full()
            .bg(Palette::glass_strong())
            .border_1()
            .border_color(if focused {
                Palette::accent().into()
            } else {
                hsla(0., 0., 1., 0.12)
            })
            .child(
                Icon::new(IconName::Search)
                    .size_5()
                    .text_color(Palette::text_secondary()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(18.))
                    .child(Input::new(&self.input).appearance(false).text_size(px(18.))),
            )
            .when(has_text, |this| {
                this.child(
                    div()
                        .id("search-clear")
                        .size_8()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .cursor_pointer()
                        .hover(|this| this.bg(Palette::glass_strong()))
                        .on_click(cx.listener(|this, _, window, cx| this.clear(window, cx)))
                        .child(
                            Icon::new(IconName::X)
                                .size_4()
                                .text_color(Palette::text_secondary()),
                        ),
                )
            })
    }

    fn render_results(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let results = match &self.results {
            Loadable::Ready(results) => results,
            Loadable::Loading => return Vec::new(),
            Loadable::Failed(message) => {
                return vec![
                    div()
                        .px(ROW_PADDING)
                        .text_color(Palette::text_secondary())
                        .child(message.clone())
                        .into_any_element(),
                ];
            }
        };
        if results.is_empty() {
            let message = if self.searching {
                String::new()
            } else {
                format!("Nothing matches “{}”.", self.query)
            };
            return vec![
                div()
                    .px(ROW_PADDING)
                    .text_color(Palette::text_secondary())
                    .child(message)
                    .into_any_element(),
            ];
        }
        let accent = Palette::accent().into();
        let mut rows = Vec::new();
        for (shelf, title) in SHELVES {
            let items = results.get(shelf);
            if items.is_empty() {
                continue;
            }
            let Some(handle) = self.rows.get(&shelf) else {
                continue;
            };
            let element = match shelf {
                Shelf::People => people_row(shelf.id(), title, items, handle, accent, cx),
                _ => {
                    let cards = items
                        .iter()
                        .map(|item| {
                            let card = if shelf == Shelf::Episodes {
                                PosterCard::landscape(item)
                            } else {
                                PosterCard::portrait(item)
                            };
                            card.accent(accent).into_any_element()
                        })
                        .collect();
                    row(shelf.id(), title, handle, cards).into_any_element()
                }
            };
            rows.push(element);
        }
        rows
    }

    fn render_genres(&self, window: &Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(genres) = self.genres.ready().filter(|g| !g.is_empty()) else {
            return Vec::new();
        };
        let client = AppState::client(cx);
        let insets = gpui_kit::component::window_paddings(window);
        let content = window.viewport_size().width - insets.left - insets.right;
        let available = f32::from(content - ROW_PADDING * 2. - px(8.));
        let (_, width) = grid::columns(available, TILE_MIN_WIDTH, TILE_GAP);
        let tiles = genres.iter().map(|genre| {
            let request = genre
                .primary_image()
                .map(|image| ImageRequest::for_image(&client, &image, 480).ambient());
            genre_tile(genre, request, px(width))
        });
        vec![
            v_flex()
                .gap_4()
                .px(ROW_PADDING)
                .child(
                    div()
                        .font_family(FONT_DISPLAY)
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(20.))
                        .child("Browse by genre"),
                )
                .child(div().flex().flex_wrap().gap(px(TILE_GAP)).children(tiles))
                .into_any_element(),
        ]
    }
}

/// Movies and shows in a genre (its child count includes episodes).
fn title_count(genre: &BaseItem) -> Option<i32> {
    let titles = match (genre.movie_count, genre.series_count) {
        (None, None) => genre.child_count,
        (movies, shows) => Some(movies.unwrap_or(0) + shows.unwrap_or(0)),
    };
    titles.filter(|n| *n > 0)
}

/// A genre as a tile of colour taken from its artwork.
fn genre_tile(
    genre: &BaseItem,
    request: Option<ImageRequest>,
    width: gpui_kit::Pixels,
) -> AnyElement {
    let group: SharedString = format!("genre-tile-{}", genre.id).into();
    let route = Route::Genre {
        name: genre.name.clone(),
    };
    let count = title_count(genre).map(|n| {
        if n == 1 {
            "1 title".to_string()
        } else {
            format!("{n} titles")
        }
    });
    div()
        .id(group.clone())
        .group(group.clone())
        .relative()
        .w(width)
        .h(px(112.))
        .rounded(px(16.))
        .cursor_pointer()
        .bg(Palette::surface())
        .on_click(move |_, window, cx| shell::navigate(route.clone(), window, cx))
        .child(
            Art::new(SharedString::from(format!("{group}-art")), request)
                .bare()
                .radius(px(16.))
                .size_full(),
        )
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(16.))
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(hsla(0., 0., 0., 0.55), 0.),
                    linear_color_stop(hsla(0., 0., 0., 0.05), 1.),
                )),
        )
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(16.))
                .border_1()
                .border_color(hsla(0., 0., 1., 0.1))
                .group_hover(group, |style| {
                    style
                        .border_color(hsla(0., 0., 1., 0.45))
                        .bg(hsla(0., 0., 1., 0.05))
                }),
        )
        .child(
            v_flex()
                .absolute()
                .left_5()
                .bottom_4()
                .right_5()
                .child(
                    div()
                        .font_family(FONT_DISPLAY)
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(20.))
                        .truncate()
                        .child(genre.name.clone()),
                )
                .when_some(count, |this, count| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(hsla(0., 0., 1., 0.7))
                            .child(count),
                    )
                }),
        )
        .into_any_element()
}

impl Render for SearchPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if self.query.is_empty() && !self.searching {
            self.render_genres(window, cx)
        } else {
            self.render_results(cx)
        };
        let search_box = self.render_box(window, cx);

        div()
            .relative()
            .size_full()
            .bg(Palette::bg())
            .child(
                crate::components::scroller::page("search-scroll", &self.scroll).child(
                    v_flex()
                        .pt(NAV_HEIGHT + px(28.))
                        .pb_16()
                        .gap_10()
                        .child(
                            div()
                                .px(ROW_PADDING)
                                .child(div().max_w(px(820.)).child(search_box)),
                        )
                        .children(body),
                ),
            )
            .vertical_scrollbar(&self.scroll)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> BaseItem {
        BaseItem {
            id: id.into(),
            ..Default::default()
        }
    }

    #[test]
    fn results_report_emptiness_and_patch_every_shelf() {
        let mut results = Results::default();
        assert!(results.is_empty());
        results.0.insert(Shelf::Movies, vec![item("a")]);
        results
            .0
            .insert(Shelf::Episodes, vec![item("a"), item("b")]);
        assert!(!results.is_empty());
        let data = user_data::with_favorite(None, true);
        assert!(user_data::patch(results.items_mut(), "a", &data));
        assert!(results.get(Shelf::Movies)[0].is_favorite());
        assert!(results.get(Shelf::Episodes)[0].is_favorite());
        assert!(results.get(Shelf::People).is_empty());
    }

    #[test]
    fn genre_counts_leave_out_episodes() {
        let genre: BaseItem = serde_json::from_value(serde_json::json!({
            "Id": "g", "Name": "Drama", "Type": "Genre",
            "ChildCount": 9, "MovieCount": 2, "SeriesCount": 1
        }))
        .unwrap();
        assert_eq!(title_count(&genre), Some(3));
        let old_server = BaseItem {
            child_count: Some(4),
            ..Default::default()
        };
        assert_eq!(title_count(&old_server), Some(4));
        assert_eq!(title_count(&BaseItem::default()), None);
    }

    #[test]
    fn every_shelf_is_shown_once() {
        let mut shelves: Vec<Shelf> = SHELVES.iter().map(|(s, _)| *s).collect();
        shelves.dedup();
        assert_eq!(shelves.len(), 5);
        assert_eq!(Shelf::People.kind(), ItemKind::Person);
    }
}
