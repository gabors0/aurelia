//! A person: headshot, dates, biography, and the movies, shows and episodes
//! they appear in.

use std::collections::HashMap;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, FontWeight, Hsla, ScrollHandle, SharedString, Task, Window, div, hsla, px,
    rgba,
};
use jellyfin::{BaseItem, ItemKind, ItemsQuery, SortBy, SortOrder, UserData};

use crate::components::ambient;
use crate::components::art::Art;
use crate::components::hero;
use crate::components::poster_card::PosterCard;
use crate::components::row::{ROW_PADDING, row};
use crate::format;
use crate::images::{ImageRequest, ImageState, ImageStore};
use crate::loadable::Loadable;
use crate::runtime;
use crate::state::AppState;
use crate::theme::{FONT_DISPLAY, Palette};
use crate::user_data;
use crate::views::shell::{self, NAV_HEIGHT};

/// Credits fetched; enough for prolific actors without paging.
const CREDITS: u32 = 300;
const HEADSHOT_WIDTH: f32 = 220.;
/// Biography lines shown before "Read more".
const BIO_LINES: usize = 6;
/// About how many characters fit on a line of the biography column.
const BIO_LINE_CHARS: usize = 120;

/// Roughly how many lines `text` wraps to: each paragraph rounds up, and the
/// blank line between paragraphs counts.
fn estimated_lines(text: &str, line_chars: usize) -> usize {
    text.trim()
        .lines()
        .map(|line| line.chars().count().div_ceil(line_chars).max(1))
        .sum()
}

pub struct PersonPage {
    id: String,
    person: Loadable<BaseItem>,
    credits: Vec<BaseItem>,
    bio_open: bool,
    scroll: ScrollHandle,
    rows: HashMap<&'static str, ScrollHandle>,
    _load: Option<Task<()>>,
}

/// Credits split into shelves, newest first within each.
fn shelves(credits: &[BaseItem]) -> Vec<(&'static str, &'static str, Vec<&BaseItem>)> {
    [
        ("movies", "Movies", ItemKind::Movie),
        ("shows", "Shows", ItemKind::Series),
        ("episodes", "Episodes", ItemKind::Episode),
    ]
    .into_iter()
    .map(|(id, title, kind)| {
        let items: Vec<&BaseItem> = credits.iter().filter(|i| i.kind == kind).collect();
        (id, title, items)
    })
    .filter(|(_, _, items)| !items.is_empty())
    .collect()
}

/// The credit whose artwork lights the page: the best-rated movie or show
/// with a backdrop.
fn showcase(credits: &[BaseItem]) -> Option<&BaseItem> {
    credits
        .iter()
        .filter(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Series))
        .filter(|i| i.backdrop_image().is_some())
        .max_by(|a, b| {
            let a = a.community_rating.unwrap_or(0.);
            let b = b.community_rating.unwrap_or(0.);
            a.total_cmp(&b)
        })
}

impl PersonPage {
    pub fn new(id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            id,
            person: Loadable::Loading,
            credits: Vec::new(),
            bio_open: false,
            scroll: ScrollHandle::new(),
            rows: HashMap::new(),
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
                person_ids: vec![id.clone()],
                include_item_types: vec![ItemKind::Movie, ItemKind::Series, ItemKind::Episode],
                sort_by: SortBy::ReleaseDate,
                sort_order: SortOrder::Descending,
                limit: CREDITS,
                ..ItemsQuery::default()
            };
            let (person, credits) = tokio::try_join!(client.item(&id), client.items(&query))?;
            Ok((person, credits.items))
        });
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = fetch.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok((person, credits)) => {
                        this.person = Loadable::Ready(person);
                        this.credits = credits;
                        crate::dev::apply_initial_scroll(&this.scroll);
                    }
                    Err(err) => {
                        let message = shell::load_failed(&err, cx);
                        if this.person.ready().is_none() {
                            this.person = Loadable::Failed(message);
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
            .person
            .ready_mut()
            .into_iter()
            .chain(self.credits.iter_mut());
        user_data::patch(items, id, data)
    }

    fn row_handle(&mut self, id: &'static str) -> ScrollHandle {
        self.rows.entry(id).or_default().clone()
    }

    fn render_header(
        &self,
        person: &BaseItem,
        backdrop: Option<ImageRequest>,
        accent: Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let client = AppState::client(cx);
        let headshot = person
            .primary_image()
            .map(|image| ImageRequest::for_image(&client, &image, 480));
        let today = chrono::Local::now().date_naive();
        let mut facts: Vec<SharedString> = Vec::new();
        if let Some(life) = format::lifespan(
            person.premiere_date.as_deref(),
            person.end_date.as_deref(),
            today,
        ) {
            facts.push(life.into());
        }
        if let Some(place) = person.production_locations.first() {
            facts.push(place.clone().into());
        }
        let titles = self.credits.len();
        let bio = person.overview.clone().filter(|b| !b.trim().is_empty());
        let long_bio = bio
            .as_ref()
            .is_some_and(|b| estimated_lines(b, BIO_LINE_CHARS) > BIO_LINES);
        let open = self.bio_open;

        div()
            .relative()
            .w_full()
            .flex_shrink_0()
            .pt(NAV_HEIGHT + px(56.))
            .pb_10()
            // Someone else's artwork: keep it a dim backdrop behind the person.
            .child(hero::backdrop("person-backdrop", backdrop))
            .child(div().absolute().inset_0().bg(rgba(0x0A0B1099)))
            .child(
                h_flex()
                    .relative()
                    .px(ROW_PADDING)
                    .items_end()
                    .gap_10()
                    .child(
                        div()
                            .w(px(HEADSHOT_WIDTH))
                            .h(px(HEADSHOT_WIDTH * 1.5))
                            .flex_shrink_0()
                            .rounded(px(16.))
                            .shadow(vec![gpui_kit::BoxShadow {
                                color: hsla(0., 0., 0., 0.5),
                                offset: gpui_kit::point(px(0.), px(16.)),
                                blur_radius: px(40.),
                                spread_radius: px(-8.),
                                inset: false,
                            }])
                            .child(
                                Art::new("person-headshot", headshot)
                                    .title(person.name.clone())
                                    .radius(px(16.))
                                    .size_full(),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .max_w(px(860.))
                            .gap_4()
                            .pb_1()
                            .child(
                                div()
                                    .font_family(FONT_DISPLAY)
                                    .font_weight(FontWeight::EXTRA_BOLD)
                                    .text_size(px(52.))
                                    .line_height(px(56.))
                                    .line_clamp(2)
                                    .text_ellipsis()
                                    .child(person.name.clone()),
                            )
                            .when(!facts.is_empty(), |this| {
                                this.child(crate::components::meta::line(facts))
                            })
                            .when(titles > 0, |this| {
                                this.child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(accent)
                                        .child(if titles == 1 {
                                            "1 title in your libraries".to_string()
                                        } else {
                                            format!("{titles} titles in your libraries")
                                        }),
                                )
                            })
                            .when_some(bio, |this, bio| {
                                this.child(
                                    div()
                                        .text_color(hsla(0., 0., 0.86, 1.))
                                        .line_height(px(24.))
                                        .when(!open, |this| {
                                            this.line_clamp(BIO_LINES).text_ellipsis()
                                        })
                                        .child(bio),
                                )
                                .when(long_bio, |this| {
                                    this.child(
                                        div()
                                            .id("person-bio-toggle")
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(Palette::text_secondary())
                                            .cursor_pointer()
                                            .hover(|this| this.text_color(Palette::text()))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.bio_open = !this.bio_open;
                                                cx.notify();
                                            }))
                                            .child(if open { "Show less" } else { "Read more" }),
                                    )
                                })
                            }),
                    ),
            )
            .into_any_element()
    }
}

impl Render for PersonPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let person = match &self.person {
            Loadable::Ready(person) => person.clone(),
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
        let showcase = showcase(&self.credits);
        let backdrop = showcase.and_then(|item| hero::backdrop_request(&client, item));
        let ambient = showcase
            .and_then(BaseItem::backdrop_image)
            .map(|image| ImageRequest::for_image(&client, &image, 780).ambient());
        let accent = ambient
            .as_ref()
            .and_then(|request| match ImageStore::get(request, window, cx) {
                ImageState::Ready { accent, .. } => accent,
                _ => None,
            })
            .unwrap_or_else(|| Palette::accent().into());

        let header = self.render_header(&person, backdrop, accent, cx);
        let shelves: Vec<(&'static str, &'static str, Vec<BaseItem>)> = shelves(&self.credits)
            .into_iter()
            .map(|(id, title, items)| (id, title, items.into_iter().cloned().collect()))
            .collect();
        let mut rows: Vec<AnyElement> = Vec::new();
        for (id, title, items) in shelves {
            let episodes = id == "episodes";
            let cards = items
                .iter()
                .map(|item| {
                    let card = if episodes {
                        PosterCard::landscape(item)
                    } else {
                        PosterCard::portrait(item)
                    };
                    card.accent(accent).into_any_element()
                })
                .collect();
            let handle = self.row_handle(id);
            rows.push(row(id, title, &handle, cards).into_any_element());
        }
        if rows.is_empty() {
            rows.push(
                div()
                    .px(ROW_PADDING)
                    .text_color(Palette::text_secondary())
                    .child(format!(
                        "Nothing with {} in your libraries yet.",
                        person.name
                    ))
                    .into_any_element(),
            );
        }

        div()
            .relative()
            .size_full()
            .bg(Palette::bg())
            .child(
                crate::components::scroller::page("person-scroll", &self.scroll).child(
                    v_flex().child(header).child(ambient::section(
                        "person-ambient",
                        ambient,
                        vec![
                            v_flex()
                                .gap_10()
                                .pt_4()
                                .pb_16()
                                .children(rows)
                                .into_any_element(),
                        ],
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

    fn credit(id: &str, kind: &str, rating: f32, backdrop: bool) -> BaseItem {
        let mut json = serde_json::json!({
            "Id": id, "Name": id, "Type": kind, "CommunityRating": rating
        });
        if backdrop {
            json["BackdropImageTags"] = serde_json::json!(["t"]);
        }
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn biography_length_counts_paragraphs() {
        let short = format!("{}\n\n{}", "a".repeat(330), "b".repeat(140));
        assert_eq!(estimated_lines(&short, 120), 3 + 1 + 2);
        assert_eq!(estimated_lines("  one line  ", 120), 1);
        assert_eq!(estimated_lines(&"c".repeat(1000), 120), 9);
    }

    #[test]
    fn credits_group_by_kind_in_order() {
        let credits = [
            credit("e1", "Episode", 7., false),
            credit("m1", "Movie", 6., true),
            credit("s1", "Series", 8., false),
            credit("m2", "Movie", 9., false),
        ];
        let groups: Vec<(&str, Vec<&str>)> = shelves(&credits)
            .into_iter()
            .map(|(id, _, items)| (id, items.iter().map(|i| i.id.as_str()).collect()))
            .collect();
        assert_eq!(
            groups,
            vec![
                ("movies", vec!["m1", "m2"]),
                ("shows", vec!["s1"]),
                ("episodes", vec!["e1"]),
            ]
        );
    }

    #[test]
    fn showcase_is_the_best_rated_title_with_art() {
        let credits = [
            credit("m1", "Movie", 6., true),
            credit("m2", "Movie", 9., false),
            credit("s1", "Series", 7.5, true),
            credit("e1", "Episode", 9.9, true),
        ];
        assert_eq!(showcase(&credits).map(|i| i.id.as_str()), Some("s1"));
        assert!(showcase(&[credit("m", "Movie", 5., false)]).is_none());
    }
}
