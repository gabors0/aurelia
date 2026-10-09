//! A collection (BoxSet): hero artwork, overview, and its titles in release
//! order.

use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, FontWeight, Hsla, ScrollHandle, SharedString, Task, Window, div, hsla, px,
};
use jellyfin::{BaseItem, ItemsQuery, SortBy, SortOrder, UserData};

use super::grid;
use crate::components::ambient;
use crate::components::button::{play_button, round_button};
use crate::components::hero;
use crate::components::meta;
use crate::components::poster_card::{PORTRAIT_WIDTH, PosterCard};
use crate::components::row::ROW_PADDING;
use crate::images::{ImageRequest, ImageState, ImageStore};
use crate::loadable::Loadable;
use crate::playback;
use crate::runtime;
use crate::state::AppState;
use crate::theme::{FONT_DISPLAY, Palette};
use crate::user_data;
use crate::views::shell::{self, NAV_HEIGHT};

const TITLES: u32 = 300;
const GAP: f32 = 22.;

pub struct CollectionPage {
    id: String,
    collection: Loadable<BaseItem>,
    titles: Loadable<Vec<BaseItem>>,
    scroll: ScrollHandle,
    _load: Option<Task<()>>,
}

/// "1979–2017" across the collection's titles.
fn year_span(titles: &[BaseItem]) -> Option<String> {
    let years = titles.iter().filter_map(|t| t.production_year);
    let (first, last) = years.fold(None, |span: Option<(i32, i32)>, year| match span {
        None => Some((year, year)),
        Some((a, b)) => Some((a.min(year), b.max(year))),
    })?;
    Some(if first == last {
        first.to_string()
    } else {
        format!("{first}–{last}")
    })
}

/// What Play starts: the first title not yet watched, else the first.
fn next_title(titles: &[BaseItem]) -> Option<&BaseItem> {
    titles
        .iter()
        .find(|t| !t.is_played())
        .or_else(|| titles.first())
}

impl CollectionPage {
    pub fn new(id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            id,
            collection: Loadable::Loading,
            titles: Loadable::Loading,
            scroll: ScrollHandle::new(),
            _load: None,
        };
        this.refresh(window, cx);
        this
    }

    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll
    }

    pub fn refresh(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let client = AppState::client(cx);
        let id = self.id.clone();
        let fetch = runtime::api(cx, async move {
            let query = ItemsQuery {
                parent_id: Some(id.clone()),
                recursive: false,
                sort_by: SortBy::ReleaseDate,
                sort_order: SortOrder::Ascending,
                limit: TITLES,
                ..ItemsQuery::default()
            };
            let (collection, titles) = tokio::try_join!(client.item(&id), client.items(&query))?;
            Ok((collection, titles.items))
        });
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = fetch.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok((collection, titles)) => {
                        this.collection = Loadable::Ready(collection);
                        this.titles = Loadable::Ready(titles);
                        crate::dev::apply_initial_scroll(&this.scroll);
                    }
                    Err(err) => {
                        let message = shell::load_failed(&err, cx);
                        if this.collection.ready().is_none() {
                            this.collection = Loadable::Failed(message);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn patch_user_data(&mut self, id: &str, data: &UserData) -> bool {
        let items = self
            .collection
            .ready_mut()
            .into_iter()
            .chain(self.titles.ready_mut().into_iter().flatten());
        user_data::patch(items, id, data)
    }

    pub fn play(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(title) = self.titles.ready().and_then(|t| next_title(t)).cloned() {
            playback::play_item(&title, false, window, cx);
        }
    }

    fn render_hero(
        &self,
        collection: &BaseItem,
        accent: Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let client = AppState::client(cx);
        let titles = self.titles.ready().map(Vec::as_slice).unwrap_or_default();
        let mut facts: Vec<SharedString> = Vec::new();
        if !titles.is_empty() {
            facts.push(if titles.len() == 1 {
                "1 title".into()
            } else {
                format!("{} titles", titles.len()).into()
            });
        }
        if let Some(span) = year_span(titles) {
            facts.push(span.into());
        }
        let label: SharedString = match next_title(titles) {
            Some(title) if title.resume_position().is_some() => {
                format!("Resume {}", title.name).into()
            }
            Some(title) if titles.first().is_some_and(|first| first.id != title.id) => {
                format!("Play {}", title.name).into()
            }
            _ => "Play".into(),
        };
        let item = collection.clone();
        let favorite_item = collection.clone();

        div()
            .relative()
            .w_full()
            .h(px(600.))
            .flex_shrink_0()
            .child(hero::backdrop(
                "collection-backdrop",
                hero::backdrop_request(&client, collection),
            ))
            .child(
                v_flex()
                    .absolute()
                    .left(ROW_PADDING)
                    .bottom(px(40.))
                    .w(px(700.))
                    .gap_4()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(accent)
                            .child("COLLECTION"),
                    )
                    .child(hero::title(
                        &client,
                        collection,
                        "collection-logo",
                        px(140.),
                    ))
                    .when(!facts.is_empty(), |this| this.child(meta::line(facts)))
                    .when_some(collection.overview.clone(), |this, overview| {
                        this.child(
                            div()
                                .text_color(hsla(0., 0., 0.86, 1.))
                                .line_height(px(24.))
                                .line_clamp(3)
                                .text_ellipsis()
                                .child(overview),
                        )
                    })
                    .child(
                        h_flex()
                            .gap_3()
                            .mt_2()
                            .when(!titles.is_empty(), |this| {
                                this.child(play_button("collection-play", label, accent).on_click(
                                    cx.listener(|this, _, window, cx| this.play(window, cx)),
                                ))
                            })
                            .child(
                                round_button(
                                    "collection-played",
                                    IconName::Check,
                                    collection.is_played(),
                                    accent,
                                )
                                .tooltip(|window, cx| {
                                    Tooltip::new("Mark everything watched").build(window, cx)
                                })
                                .on_click(move |_, window, cx| {
                                    user_data::set_played(&item, !item.is_played(), window, cx)
                                }),
                            )
                            .child(
                                round_button(
                                    "collection-favorite",
                                    IconName::Heart,
                                    collection.is_favorite(),
                                    accent,
                                )
                                .tooltip(|window, cx| Tooltip::new("Favourite").build(window, cx))
                                .on_click(move |_, window, cx| {
                                    user_data::set_favorite(
                                        &favorite_item,
                                        !favorite_item.is_favorite(),
                                        window,
                                        cx,
                                    )
                                }),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_titles(&self, accent: Hsla, window: &Window) -> AnyElement {
        let titles = match &self.titles {
            Loadable::Ready(titles) => titles,
            Loadable::Loading => return div().into_any_element(),
            Loadable::Failed(message) => {
                return div()
                    .px(ROW_PADDING)
                    .text_color(Palette::text_secondary())
                    .child(message.clone())
                    .into_any_element();
            }
        };
        if titles.is_empty() {
            return div()
                .px(ROW_PADDING)
                .text_color(Palette::text_secondary())
                .child("This collection is empty.")
                .into_any_element();
        }
        let insets = gpui_kit::component::window_paddings(window);
        let content = window.viewport_size().width - insets.left - insets.right;
        let available = f32::from(content - ROW_PADDING * 2. - px(8.));
        let (_, width) = grid::columns(available, f32::from(PORTRAIT_WIDTH), GAP);
        v_flex()
            .gap_4()
            .child(
                div()
                    .px(ROW_PADDING)
                    .font_family(FONT_DISPLAY)
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(20.))
                    .child("In this collection"),
            )
            .child(
                div()
                    .px(ROW_PADDING)
                    .flex()
                    .flex_wrap()
                    .gap(px(GAP))
                    .children(titles.iter().map(|title| {
                        PosterCard::portrait(title)
                            .width(px(width))
                            .accent(accent)
                            .into_any_element()
                    })),
            )
            .into_any_element()
    }
}

impl Render for CollectionPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let collection = match &self.collection {
            Loadable::Ready(collection) => collection.clone(),
            Loadable::Loading => return div().size_full().bg(Palette::bg()).into_any_element(),
            Loadable::Failed(message) => {
                return div()
                    .size_full()
                    .bg(Palette::bg())
                    .pt(NAV_HEIGHT + px(48.))
                    .px(ROW_PADDING)
                    .text_color(Palette::text_secondary())
                    .child(message.clone())
                    .into_any_element();
            }
        };
        let client = AppState::client(cx);
        let ambient_request = collection
            .backdrop_image()
            .map(|image| ImageRequest::for_image(&client, &image, 780).ambient());
        let accent = ambient_request
            .as_ref()
            .and_then(|request| match ImageStore::get(request, window, cx) {
                ImageState::Ready { accent, .. } => accent,
                _ => None,
            })
            .unwrap_or_else(|| Palette::accent().into());

        let hero = self.render_hero(&collection, accent, cx);
        let titles = self.render_titles(accent, window);

        div()
            .relative()
            .size_full()
            .bg(Palette::bg())
            .child(
                crate::components::scroller::page("collection-scroll", &self.scroll).child(
                    v_flex().child(hero).child(ambient::section(
                        "collection-ambient",
                        ambient_request,
                        vec![v_flex().pt_4().pb_16().child(titles).into_any_element()],
                    )),
                ),
            )
            .vertical_scrollbar(&self.scroll)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title(id: &str, year: Option<i32>, played: bool) -> BaseItem {
        serde_json::from_value(serde_json::json!({
            "Id": id, "Name": id, "Type": "Movie", "ProductionYear": year,
            "UserData": {"Played": played}
        }))
        .unwrap()
    }

    #[test]
    fn spans_the_years() {
        let titles = [
            title("a", Some(1986), false),
            title("b", None, false),
            title("c", Some(1979), false),
        ];
        assert_eq!(year_span(&titles).as_deref(), Some("1979–1986"));
        assert_eq!(
            year_span(&[title("a", Some(2001), false)]).as_deref(),
            Some("2001")
        );
        assert_eq!(year_span(&[title("a", None, false)]), None);
    }

    #[test]
    fn play_starts_at_the_first_unwatched() {
        let titles = [title("a", None, true), title("b", None, false)];
        assert_eq!(next_title(&titles).map(|t| t.id.as_str()), Some("b"));
        let all_seen = [title("a", None, true), title("b", None, true)];
        assert_eq!(next_title(&all_seen).map(|t| t.id.as_str()), Some("a"));
        assert!(next_title(&[]).is_none());
    }
}
