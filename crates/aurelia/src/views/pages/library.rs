//! A poster grid with sorting and filters: a whole library (optionally one
//! genre of it), or one genre across every library.

use std::ops::Range;

use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, FontWeight, Pixels, SharedString, Task, UniformListScrollHandle, Window,
    div, px, uniform_list,
};
use jellyfin::{BaseItem, ItemFilter, ItemKind, ItemsQuery, SortBy, SortOrder, UserData};

use super::grid;
use crate::components::pill::pill;
use crate::components::poster_card::{PORTRAIT_WIDTH, PosterCard};
use crate::components::row::ROW_PADDING;
use crate::loadable::Loadable;
use crate::runtime;
use crate::state::AppState;
use crate::theme::{FONT_DISPLAY, Palette};
use crate::views::shell::{self, NAV_HEIGHT};

const PAGE_SIZE: u32 = 100;
const GAP: f32 = 22.;
const CAPTION_HEIGHT: f32 = 50.;

const SORTS: [(SortBy, &str); 4] = [
    (SortBy::Name, "A–Z"),
    (SortBy::DateAdded, "Recently added"),
    (SortBy::ReleaseDate, "Release date"),
    (SortBy::Rating, "Rating"),
];

const FILTERS: [(ItemFilter, &str); 3] = [
    (ItemFilter::All, "All"),
    (ItemFilter::Unplayed, "Unwatched"),
    (ItemFilter::Favorites, "Favourites"),
];

/// What the grid lists.
enum Scope {
    /// One library; `genre` narrows it and can be changed on the page.
    Library { view_id: String },
    /// Every movie and show in one genre.
    Genre,
}

pub struct LibraryPage {
    scope: Scope,
    name: SharedString,
    kinds: Vec<ItemKind>,
    sort: SortBy,
    filter: ItemFilter,
    genre: Option<String>,
    /// The library's genres, for its genre picker.
    genres: Loadable<Vec<String>>,
    items: Vec<BaseItem>,
    total: Option<u32>,
    loading: bool,
    error: Option<SharedString>,
    columns: usize,
    scroll: UniformListScrollHandle,
    _load: Option<Task<()>>,
    _genres: Option<Task<()>>,
}

impl LibraryPage {
    pub fn library(
        view_id: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let view = AppState::global(cx)
            .views()
            .iter()
            .find(|v| v.id == view_id)
            .cloned();
        let name = view
            .as_ref()
            .map(|v| v.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or(name);
        let kinds = match view.as_ref().and_then(|v| v.collection_type.as_deref()) {
            Some("movies") => vec![ItemKind::Movie],
            Some("tvshows") => vec![ItemKind::Series],
            Some("boxsets") => vec![ItemKind::BoxSet],
            _ => vec![ItemKind::Movie, ItemKind::Series, ItemKind::Video],
        };
        let mut this = Self::new(Scope::Library { view_id }, name, kinds, None);
        this.load_genres(cx);
        this.refresh(window, cx);
        this
    }

    pub fn genre(name: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let kinds = vec![ItemKind::Movie, ItemKind::Series];
        let mut this = Self::new(Scope::Genre, name.clone(), kinds, Some(name));
        this.refresh(window, cx);
        this
    }

    fn new(scope: Scope, name: String, kinds: Vec<ItemKind>, genre: Option<String>) -> Self {
        Self {
            scope,
            name: name.into(),
            kinds,
            sort: SortBy::Name,
            filter: ItemFilter::All,
            genre,
            genres: Loadable::Loading,
            items: Vec::new(),
            total: None,
            loading: false,
            error: None,
            columns: 7,
            scroll: UniformListScrollHandle::new(),
            _load: None,
            _genres: None,
        }
    }

    fn view_id(&self) -> Option<&str> {
        match &self.scope {
            Scope::Library { view_id } => Some(view_id),
            Scope::Genre => None,
        }
    }

    fn load_genres(&mut self, cx: &mut Context<Self>) {
        // Collections have no genres of their own.
        if self.kinds == [ItemKind::BoxSet] {
            self.genres = Loadable::Ready(Vec::new());
            return;
        }
        let client = AppState::client(cx);
        let parent = self.view_id().map(str::to_string);
        let kinds = self.kinds.clone();
        let fetch = runtime::api(cx, async move {
            let genres = client.genres(parent.as_deref(), &kinds).await?;
            Ok(genres.into_iter().map(|g| g.name).collect())
        });
        self._genres = Some(cx.spawn(async move |this, cx| {
            let result = fetch.await;
            this.update(cx, |this, cx| {
                this.genres = Loadable::from_result(result);
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn patch_user_data(&mut self, id: &str, data: &UserData) -> bool {
        crate::user_data::patch(self.items.iter_mut(), id, data)
    }

    /// Reloads what's loaded and swaps it in, so the grid doesn't flash or
    /// lose its place; keeps the current sort and filters.
    pub fn refresh(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.items.is_empty() {
            self.error = None;
            self.total = None;
            self.loading = false;
            self.load_more(cx);
            return;
        }
        let mut query = self.query();
        query.start_index = 0;
        query.limit = (self.items.len() as u32).max(PAGE_SIZE);
        self.fetch(query, true, cx);
    }

    fn query(&self) -> ItemsQuery {
        let sort_order = match self.sort {
            SortBy::Name => SortOrder::Ascending,
            _ => SortOrder::Descending,
        };
        ItemsQuery {
            parent_id: self.view_id().map(str::to_string),
            include_item_types: self.kinds.clone(),
            genres: self.genre.clone().into_iter().collect(),
            sort_by: self.sort,
            sort_order,
            filter: self.filter,
            start_index: self.items.len() as u32,
            limit: PAGE_SIZE,
            ..ItemsQuery::default()
        }
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.loading || self.total.is_some_and(|t| self.items.len() as u32 >= t) {
            return;
        }
        let query = self.query();
        self.fetch(query, false, cx);
    }

    /// Runs `query`; `replace` swaps the results in instead of appending.
    fn fetch(&mut self, query: ItemsQuery, replace: bool, cx: &mut Context<Self>) {
        self.loading = true;
        let client = AppState::client(cx);
        let fetch = runtime::api(cx, async move { client.items(&query).await });
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = fetch.await;
            this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(page) => {
                        this.total = Some(page.total_record_count);
                        if replace {
                            this.items = page.items;
                        } else {
                            this.items.extend(page.items);
                        }
                        this.error = None;
                    }
                    Err(err) => this.error = Some(shell::load_failed(&err, cx)),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn set_sort(&mut self, sort: SortBy, cx: &mut Context<Self>) {
        if self.sort != sort {
            self.sort = sort;
            self.restart(cx);
        }
    }

    fn set_filter(&mut self, filter: ItemFilter, cx: &mut Context<Self>) {
        if self.filter != filter {
            self.filter = filter;
            self.restart(cx);
        }
    }

    fn set_genre(&mut self, genre: Option<String>, cx: &mut Context<Self>) {
        if self.genre != genre {
            self.genre = genre;
            self.restart(cx);
        }
    }

    /// Starts over from the top (new sort or filter).
    fn restart(&mut self, cx: &mut Context<Self>) {
        self._load = None;
        self.items.clear();
        self.total = None;
        self.error = None;
        self.loading = false;
        self.scroll.scroll_to_item(0, gpui_kit::ScrollStrategy::Top);
        self.load_more(cx);
        cx.notify();
    }

    fn render_rows(
        &mut self,
        rows: Range<usize>,
        card_width: Pixels,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let columns = self.columns;
        if grid::needs_more(
            self.items.len(),
            self.total.unwrap_or(0) as usize,
            rows.end,
            columns,
        ) {
            self.load_more(cx);
        }
        rows.map(|row| {
            let start = row * columns;
            let end = (start + columns).min(self.items.len());
            div()
                .flex()
                .gap(px(GAP))
                .px(ROW_PADDING)
                .pb(px(GAP))
                .children(self.items[start..end].iter().map(|item| {
                    PosterCard::portrait(item)
                        .width(card_width)
                        .into_any_element()
                }))
                .into_any_element()
        })
        .collect()
    }

    /// The library's genre picker; nothing for genre pages or when the
    /// library has no genres.
    fn render_genre_picker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if matches!(self.scope, Scope::Genre) {
            return None;
        }
        let genres = self.genres.ready()?.clone();
        if genres.is_empty() {
            return None;
        }
        let page = cx.weak_entity();
        let current = self.genre.clone();
        let label: SharedString = current
            .clone()
            .unwrap_or_else(|| "All genres".into())
            .into();
        Some(
            Button::new("genre-picker")
                .outline()
                .small()
                .h(px(32.))
                .px_3()
                .rounded_full()
                .label(label)
                .dropdown_caret(true)
                .dropdown_menu(move |menu, _, _| {
                    let choices = std::iter::once(None).chain(genres.iter().cloned().map(Some));
                    let mut menu = menu.scrollable(true).max_h(px(440.)).min_w(px(200.));
                    for (i, choice) in choices.enumerate() {
                        let text = choice.clone().unwrap_or_else(|| "All genres".into());
                        let page = page.clone();
                        let checked = choice == current;
                        menu = menu.item(PopupMenuItem::new(text).checked(checked).on_click(
                            move |_, _, cx| {
                                let choice = choice.clone();
                                page.update(cx, |page, cx| page.set_genre(choice, cx)).ok();
                            },
                        ));
                        if i == 0 {
                            menu = menu.separator();
                        }
                    }
                    menu
                })
                .into_any_element(),
        )
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (one, many) = match self.kinds.as_slice() {
            [ItemKind::Movie] => ("movie", "movies"),
            [ItemKind::Series] => ("show", "shows"),
            [ItemKind::BoxSet] => ("collection", "collections"),
            _ => ("title", "titles"),
        };
        let count = self
            .total
            .map(|t| format!("{} {}", group_thousands(t), if t == 1 { one } else { many }))
            .unwrap_or_default();
        let eyebrow = matches!(self.scope, Scope::Genre).then_some("GENRE");
        div()
            .flex()
            .flex_col()
            .gap_5()
            .px(ROW_PADDING)
            .pt(NAV_HEIGHT + px(28.))
            .pb_6()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when_some(eyebrow, |this, eyebrow| {
                        this.child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(Palette::accent())
                                .child(eyebrow),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap_4()
                            .child(
                                div()
                                    .font_family(FONT_DISPLAY)
                                    .font_weight(FontWeight::EXTRA_BOLD)
                                    .text_size(px(44.))
                                    .line_height(px(48.))
                                    .child(self.name.clone()),
                            )
                            .child(
                                div()
                                    .pb_1()
                                    .text_color(Palette::text_tertiary())
                                    .child(count),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .flex_wrap()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .children(SORTS.iter().map(|(sort, label)| {
                                let sort = *sort;
                                pill(
                                    SharedString::from(format!("sort-{label}")),
                                    *label,
                                    self.sort == sort,
                                )
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.set_sort(sort, cx)),
                                )
                            })),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .children(self.render_genre_picker(cx))
                            .children(FILTERS.iter().map(|(filter, label)| {
                                let filter = *filter;
                                pill(
                                    SharedString::from(format!("filter-{label}")),
                                    *label,
                                    self.filter == filter,
                                )
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.set_filter(filter, cx)),
                                )
                            })),
                    ),
            )
    }

    fn empty_message(&self) -> &'static str {
        match (self.filter, &self.genre) {
            (ItemFilter::Unplayed, _) => "You've watched everything here.",
            (ItemFilter::Favorites, _) => "No favourites yet. Tap the heart on anything you love.",
            (ItemFilter::All, Some(_)) => "Nothing in this genre yet.",
            (ItemFilter::All, None) => "This library is empty.",
        }
    }
}

fn group_thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

impl Render for LibraryPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The window's client-side shadow is part of the viewport; the
        // scrollbar gutter is ~8px.
        let insets = gpui_kit::component::window_paddings(window);
        let content = window.viewport_size().width - insets.left - insets.right;
        let available = f32::from(content - ROW_PADDING * 2. - px(8.));
        let (columns, card_width) = grid::columns(available, f32::from(PORTRAIT_WIDTH), GAP);
        self.columns = columns;
        let card_width = px(card_width);
        let row_height = card_width * 1.5 + px(CAPTION_HEIGHT + GAP);
        let rows = grid::row_count(self.items.len(), columns);

        let body = if let Some(error) = self.error.clone().filter(|_| self.items.is_empty()) {
            div()
                .px(ROW_PADDING)
                .text_color(Palette::text_secondary())
                .child(error)
                .into_any_element()
        } else if self.items.is_empty() && (self.loading || self.total.is_none()) {
            div()
                .flex()
                .flex_col()
                .gap(px(GAP))
                .px(ROW_PADDING)
                .children((0..3).map(|_| {
                    div().flex().gap(px(GAP)).children((0..columns).map(|_| {
                        div()
                            .w(card_width)
                            .h(card_width * 1.5)
                            .rounded(px(12.))
                            .bg(Palette::glass())
                    }))
                }))
                .into_any_element()
        } else if self.items.is_empty() {
            div()
                .px(ROW_PADDING)
                .text_color(Palette::text_secondary())
                .child(self.empty_message())
                .into_any_element()
        } else {
            div()
                .size_full()
                .child(
                    uniform_list(
                        "library-grid",
                        rows,
                        cx.processor(move |this, range: Range<usize>, _window, cx| {
                            this.render_rows(range, card_width, cx)
                        }),
                    )
                    .size_full()
                    .track_scroll(&self.scroll),
                )
                .vertical_scrollbar(&self.scroll)
                .into_any_element()
        };
        let _ = row_height;

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(Palette::bg())
            .child(self.render_header(cx))
            .child(div().flex_1().min_h_0().child(body))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn thousands() {
        assert_eq!(super::group_thousands(7), "7");
        assert_eq!(super::group_thousands(1234), "1,234");
        assert_eq!(super::group_thousands(1234567), "1,234,567");
    }
}
