//! A whole library as a poster grid, with sorting and filters.

use std::ops::Range;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, FontWeight, Pixels, SharedString, Task, UniformListScrollHandle, Window,
    div, px, uniform_list,
};
use jellyfin::{BaseItem, ItemFilter, ItemKind, ItemsQuery, SortBy, SortOrder};

use super::grid;
use crate::components::pill::pill;
use crate::components::poster_card::{PORTRAIT_WIDTH, PosterCard};
use crate::components::row::ROW_PADDING;
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

pub struct LibraryPage {
    view_id: String,
    name: SharedString,
    kinds: Vec<ItemKind>,
    sort: SortBy,
    filter: ItemFilter,
    items: Vec<BaseItem>,
    total: Option<u32>,
    loading: bool,
    error: Option<SharedString>,
    columns: usize,
    scroll: UniformListScrollHandle,
    _load: Option<Task<()>>,
}

impl LibraryPage {
    pub fn new(view_id: String, name: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
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
            _ => vec![ItemKind::Movie, ItemKind::Series, ItemKind::Video],
        };
        let mut this = Self {
            view_id,
            name: name.into(),
            kinds,
            sort: SortBy::Name,
            filter: ItemFilter::All,
            items: Vec::new(),
            total: None,
            loading: false,
            error: None,
            columns: 7,
            scroll: UniformListScrollHandle::new(),
            _load: None,
        };
        this.refresh(window, cx);
        this
    }

    /// Reloads from the first page, keeping the current sort and filter.
    pub fn refresh(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.items.clear();
        self.total = None;
        self.error = None;
        self.loading = false;
        self.load_more(cx);
    }

    fn query(&self) -> ItemsQuery {
        let sort_order = match self.sort {
            SortBy::Name => SortOrder::Ascending,
            _ => SortOrder::Descending,
        };
        ItemsQuery {
            parent_id: Some(self.view_id.clone()),
            include_item_types: self.kinds.clone(),
            sort_by: self.sort,
            sort_order,
            filter: self.filter,
            start_index: self.items.len() as u32,
            limit: PAGE_SIZE,
        }
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.loading || self.total.is_some_and(|t| self.items.len() as u32 >= t) {
            return;
        }
        self.loading = true;
        let client = AppState::client(cx);
        let query = self.query();
        let fetch = runtime::api(cx, async move { client.items(&query).await });
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = fetch.await;
            this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(page) => {
                        this.total = Some(page.total_record_count);
                        this.items.extend(page.items);
                    }
                    Err(err) => this.error = Some(shell::load_failed(&err, cx)),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn set_sort(&mut self, sort: SortBy, window: &mut Window, cx: &mut Context<Self>) {
        if self.sort != sort {
            self.sort = sort;
            self.restart(window, cx);
        }
    }

    fn set_filter(&mut self, filter: ItemFilter, window: &mut Window, cx: &mut Context<Self>) {
        if self.filter != filter {
            self.filter = filter;
            self.restart(window, cx);
        }
    }

    fn restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self._load = None;
        self.scroll.scroll_to_item(0, gpui_kit::ScrollStrategy::Top);
        self.refresh(window, cx);
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

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (one, many) = match self.kinds.as_slice() {
            [ItemKind::Movie] => ("movie", "movies"),
            [ItemKind::Series] => ("show", "shows"),
            _ => ("title", "titles"),
        };
        let count = self
            .total
            .map(|t| format!("{} {}", group_thousands(t), if t == 1 { one } else { many }))
            .unwrap_or_default();
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
                                .on_click(cx.listener(
                                    move |this, _, window, cx| this.set_sort(sort, window, cx),
                                ))
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .children(FILTERS.iter().map(|(filter, label)| {
                                let filter = *filter;
                                pill(
                                    SharedString::from(format!("filter-{label}")),
                                    *label,
                                    self.filter == filter,
                                )
                                .on_click(cx.listener(
                                    move |this, _, window, cx| this.set_filter(filter, window, cx),
                                ))
                            })),
                    ),
            )
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
                .child(match self.filter {
                    ItemFilter::Unplayed => "You've watched everything here.",
                    ItemFilter::Favorites => {
                        "No favourites yet. Tap the heart on anything you love."
                    }
                    ItemFilter::All => "This library is empty.",
                })
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
