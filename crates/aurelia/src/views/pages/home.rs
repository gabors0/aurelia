//! Home: a hero carousel of what to watch, then shelves of Continue Watching,
//! Next Up, Favourites and the latest additions to every library.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::h_flex;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, App, Context, ElementId, FontWeight, Hsla,
    ScrollHandle, SharedString, SpringAnimation, Task, Window, div, px,
};
use jellyfin::{BaseItem, ItemFilter, ItemKind, ItemsQuery, SortBy, UserData, UserView};

use super::carousel::{self, Carousel};
use crate::components::button::{focus_ring, glass_button, play_button};
use crate::components::hero;
use crate::components::key_nav;
use crate::components::meta;
use crate::components::motion::{Pressable, SPRING, mix, pressable};
use crate::components::poster_card::{LANDSCAPE_WIDTH, PORTRAIT_WIDTH, PosterCard};
use crate::components::row::{ROW_PADDING, row, skeleton_row};
use crate::images::{ImageRequest, ImageState, ImageStore};
use crate::loadable::Loadable;
use crate::nav::Route;
use crate::runtime;
use crate::state::AppState;
use crate::theme::Palette;
use crate::views::shell::{self, NAV_HEIGHT};

const HERO_RESUME: usize = 2;
const HERO_RANDOM: usize = 4;
/// Random candidates fetched so enough of them have backdrop art.
const HERO_RANDOM_POOL: u32 = 30;
const HERO_HEIGHT: f32 = 0.72;
const FAVORITES: u32 = 24;

pub struct HomePage {
    resume: Loadable<Vec<BaseItem>>,
    next_up: Loadable<Vec<BaseItem>>,
    favorites: Loadable<Vec<BaseItem>>,
    latest: Vec<(UserView, Loadable<Vec<BaseItem>>)>,
    /// Random movies and shows for the hero; kept across refreshes so slides
    /// don't change under the user, re-rolled by the refresh button.
    random: Vec<BaseItem>,
    hero: Vec<BaseItem>,
    /// How many of the first hero slides come from Continue Watching.
    hero_resume: usize,
    carousel: Carousel,
    hero_hovered: bool,
    hero_focused: bool,
    scroll: ScrollHandle,
    rows: HashMap<SharedString, ScrollHandle>,
    _load: Option<Task<()>>,
    _timer: Task<()>,
}

impl HomePage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let timer = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let Ok(()) = this.update(cx, |this, cx| {
                    let now = Instant::now();
                    let autoplay = crate::settings::get(cx).hero_autoplay;
                    if (autoplay && this.carousel.tick(now)) | this.carousel.settle(now) {
                        cx.notify();
                    }
                }) else {
                    return;
                };
            }
        });
        let mut this = Self {
            resume: Loadable::Loading,
            next_up: Loadable::Loading,
            favorites: Loadable::Loading,
            latest: Vec::new(),
            random: Vec::new(),
            hero: Vec::new(),
            hero_resume: 0,
            carousel: Carousel::new(Instant::now()),
            hero_hovered: false,
            hero_focused: false,
            scroll: ScrollHandle::new(),
            rows: HashMap::new(),
            _load: None,
            _timer: timer,
        };
        this.refresh(window, cx);
        this
    }

    /// Reloads server data (after playback, coming back to the window).
    pub fn refresh(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.load(false, cx);
    }

    /// The refresh button: reloads and picks new random hero slides.
    pub fn reshuffle(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.load(true, cx);
    }

    fn load(&mut self, reshuffle: bool, cx: &mut Context<Self>) {
        let client = AppState::client(cx);
        let views: Vec<UserView> = AppState::global(cx)
            .views()
            .iter()
            .filter(|v| v.is_video_library())
            .cloned()
            .collect();
        if self.latest.is_empty() {
            self.latest = views
                .iter()
                .map(|v| (v.clone(), Loadable::Loading))
                .collect();
        }

        let c = client.clone();
        let resume = runtime::api(cx, async move { c.resume_items(16).await });
        let c = client.clone();
        let next_up = runtime::api(cx, async move { c.next_up(None, 16, false).await });
        let c = client.clone();
        let favorites = runtime::api(cx, async move {
            c.items(&ItemsQuery {
                include_item_types: vec![ItemKind::Movie, ItemKind::Series],
                filter: ItemFilter::Favorites,
                limit: FAVORITES,
                ..ItemsQuery::default()
            })
            .await
            .map(|page| page.items)
        });
        let c = client.clone();
        let random = (reshuffle || self.random.is_empty()).then(|| {
            runtime::api(cx, async move {
                c.items(&ItemsQuery {
                    include_item_types: vec![ItemKind::Movie, ItemKind::Series],
                    sort_by: SortBy::Random,
                    limit: HERO_RANDOM_POOL,
                    ..ItemsQuery::default()
                })
                .await
            })
        });
        let latest: Vec<_> = views
            .iter()
            .map(|view| {
                let c = client.clone();
                let id = view.id.clone();
                runtime::api(cx, async move { c.latest(&id, 20).await })
            })
            .collect();

        self._load = Some(cx.spawn(async move |this, cx| {
            let resume = resume.await;
            let next_up = next_up.await;
            let favorites = favorites.await;
            // Only decoration: if it fails, keep the slides we have.
            let random = match random {
                Some(fetch) => fetch.await.ok().map(|page| page.items),
                None => None,
            };
            let mut latest_results = Vec::new();
            for fetch in latest {
                latest_results.push(fetch.await);
            }
            this.update(cx, |this, cx| {
                for result in [&resume, &next_up].into_iter().chain(latest_results.iter()) {
                    if let Err(err) = result {
                        shell::load_failed(err, cx);
                        break;
                    }
                }
                this.resume = loadable(resume);
                this.next_up = loadable(next_up);
                this.favorites = loadable(favorites);
                this.latest = views
                    .into_iter()
                    .zip(latest_results)
                    .map(|(view, result)| (view, loadable(result)))
                    .collect();
                let reshuffled = random.is_some();
                if let Some(random) = random {
                    this.random = random;
                }
                this.rebuild_hero(reshuffle && reshuffled);
                if let Some(y) = std::env::var("AURELIA_SCROLL_Y")
                    .ok()
                    .and_then(|v| v.parse::<f32>().ok())
                {
                    this.scroll.set_offset(gpui_kit::point(px(0.), px(-y)));
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Hero picks: what you're in the middle of, then random movies and shows
    /// from the whole library — only items with backdrop art, one per show.
    /// `reshuffled` shows the first new random slide; otherwise the slide on
    /// screen stays put.
    fn rebuild_hero(&mut self, reshuffled: bool) {
        let current = self.hero.get(self.carousel.index()).map(|i| i.id.clone());
        let mut picks: Vec<BaseItem> = Vec::new();
        let mut seen_series = Vec::new();
        let resume = self.resume.ready().map(Vec::as_slice).unwrap_or_default();
        self.hero_resume = pick(&mut picks, &mut seen_series, resume, HERO_RESUME);
        pick(&mut picks, &mut seen_series, &self.random, HERO_RANDOM);
        let now = Instant::now();
        if reshuffled && picks.len() > self.hero_resume {
            self.carousel.restart(picks.len(), self.hero_resume, now);
        } else {
            let keep = current.and_then(|id| picks.iter().position(|i| i.id == id));
            self.carousel.reset(picks.len(), keep, now);
        }
        self.hero = picks;
    }

    fn show_slide(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.carousel.go_to(index, Instant::now()) {
            cx.notify();
        }
    }

    /// Previous/next slide (arrow buttons, ←/→ keys).
    pub fn step_hero(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.carousel.step(delta, Instant::now()) {
            cx.notify();
        }
    }

    /// Pointing at the hero pauses it, and so does keyboard focus on its
    /// buttons (the slide changing would take the focused button away).
    fn pause_hero(&mut self, hovered: bool) {
        self.hero_hovered = hovered;
        self.carousel
            .set_paused(self.hero_hovered || self.hero_focused, Instant::now());
    }

    fn follow_hero_focus(&mut self, window: &Window, cx: &App) {
        let focused = matches!(
            key_nav::focused_key(window, cx),
            Some(ElementId::Name(name)) if name.starts_with("hero-")
        );
        if focused != self.hero_focused {
            self.hero_focused = focused;
            self.carousel
                .set_paused(self.hero_hovered || self.hero_focused, Instant::now());
        }
    }

    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll
    }

    pub fn patch_user_data(&mut self, id: &str, data: &UserData) -> bool {
        let shelves = [&mut self.resume, &mut self.next_up, &mut self.favorites]
            .into_iter()
            .chain(self.latest.iter_mut().map(|(_, items)| items))
            .filter_map(Loadable::ready_mut)
            .flatten();
        let items = shelves
            .chain(self.random.iter_mut())
            .chain(self.hero.iter_mut());
        crate::user_data::patch(items, id, data)
    }

    fn row_handle(&mut self, id: &str) -> ScrollHandle {
        self.rows
            .entry(SharedString::from(id.to_string()))
            .or_default()
            .clone()
    }

    fn accent(&self, window: &mut Window, cx: &mut Context<Self>) -> Hsla {
        let client = AppState::client(cx);
        self.hero
            .get(self.carousel.index())
            .and_then(|item| hero::backdrop_request(&client, item))
            .and_then(
                |request| match ImageStore::get(&ambient(&request), window, cx) {
                    ImageState::Ready { accent, .. } => accent,
                    _ => None,
                },
            )
            .map(Palette::tune_accent)
            .unwrap_or_else(|| Palette::accent().into())
    }

    /// One hero slide. Its fade is keyed by `serial` (one per showing) and its
    /// elements by the item, so a slide keeps its state while it's on screen —
    /// as the current slide and then under the next one — and fades in afresh
    /// each time it comes back.
    /// Starts loading every slide's artwork so slides don't fade in empty.
    fn prefetch_hero(&self, window: &mut Window, cx: &mut Context<Self>) {
        let client = AppState::client(cx);
        for item in &self.hero {
            if let Some(request) = hero::backdrop_request(&client, item) {
                ImageStore::get(&request, window, cx);
                ImageStore::get(&ambient(&request), window, cx);
            }
            if let Some(request) = hero::logo_request(&client, item) {
                ImageStore::get(&request, window, cx);
            }
        }
    }

    /// `outgoing`: the slide fading out under the next one; its buttons
    /// can't take keyboard focus (they're about to go).
    fn render_slide(
        &self,
        index: usize,
        serial: u64,
        outgoing: bool,
        accent: Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let client = AppState::client(cx);
        let item = &self.hero[index];
        let from_resume = index < self.hero_resume;
        let request = hero::backdrop_request(&client, item);
        let resume = item.resume_position().is_some();
        let label: SharedString = match (item.kind, resume) {
            (ItemKind::Episode, true) => format!(
                "Resume {}",
                crate::format::episode_label(item.parent_index_number, item.index_number)
            )
            .into(),
            (_, true) => "Resume".into(),
            _ => "Play".into(),
        };
        let route = Route::for_item(item);
        let play_item = item.clone();
        let info_route = match item.kind {
            ItemKind::Episode => item
                .series_id
                .clone()
                .map(|id| Route::Series {
                    id,
                    season_id: item.season_id.clone(),
                })
                .unwrap_or_else(|| route.clone()),
            _ => route.clone(),
        };
        let mut facts = meta::facts(item);
        facts.extend(
            item.genres
                .iter()
                .take(2)
                .map(|g| SharedString::from(g.clone())),
        );
        let subtitle = match item.kind {
            ItemKind::Episode => Some(item.name.clone()),
            _ => item.taglines.first().cloned(),
        };

        div()
            .id(SharedString::from(item.id.clone()))
            .absolute()
            .inset_0()
            .child(hero::backdrop("hero-art", request))
            .child(
                div()
                    .id("hero-content")
                    .on_hover(cx.listener(|this, hovered: &bool, _, _| this.pause_hero(*hovered)))
                    .absolute()
                    .left(ROW_PADDING)
                    .bottom(px(150.))
                    .w(px(640.))
                    .flex()
                    .flex_col()
                    .gap_4()
                    .when(from_resume, |this| {
                        this.child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(accent)
                                .child("CONTINUE WATCHING"),
                        )
                    })
                    .child(hero::title(&client, item, "hero-logo", px(140.)))
                    .when_some(subtitle, |this, subtitle| {
                        this.child(
                            div()
                                .text_size(px(18.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(subtitle),
                        )
                    })
                    .child(meta::line(facts))
                    .when_some(item.overview.clone(), |this, overview| {
                        this.child(
                            div()
                                .text_color(Palette::text_body())
                                .line_height(px(24.))
                                .line_clamp(3)
                                .text_ellipsis()
                                .child(overview),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .mt_2()
                            .child(
                                play_button("hero-play", label, accent)
                                    .when(outgoing, |this| this.unfocusable())
                                    .on_click(move |_, window, cx| {
                                        crate::playback::play_item(&play_item, false, window, cx)
                                    }),
                            )
                            .child(
                                glass_button("hero-info", Some(IconName::Info), "More info")
                                    .when(outgoing, |this| this.unfocusable())
                                    .on_click(move |_, window, cx| {
                                        shell::navigate(info_route.clone(), window, cx)
                                    }),
                            ),
                    ),
            )
            .with_animation(
                ElementId::NamedInteger("hero-fade".into(), serial),
                Animation::new(carousel::FADE).with_easing(gpui_kit::ease_in_out),
                |slide, t| slide.opacity(t),
            )
            .into_any_element()
    }

    fn render_hero(
        &self,
        height: gpui_kit::Pixels,
        accent: Hsla,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let count = self.hero.len();
        let current = self.carousel.index();
        div()
            .id("hero")
            .relative()
            .w_full()
            .h(height)
            .flex_shrink_0()
            .when_some(self.carousel.previous(), |this, previous| {
                this.child(self.render_slide(
                    previous,
                    self.carousel.previous_serial(),
                    true,
                    accent,
                    cx,
                ))
            })
            .when(count > 0, |this| {
                this.child(self.render_slide(current, self.carousel.serial(), false, accent, cx))
            })
            .when(count > 1, |this| {
                this.child(
                    h_flex()
                        .id("hero-controls")
                        .on_hover(
                            cx.listener(|this, hovered: &bool, _, _| this.pause_hero(*hovered)),
                        )
                        .absolute()
                        .right(ROW_PADDING)
                        .bottom(px(146.))
                        .gap_3()
                        .child(
                            hero_arrow("hero-prev", IconName::ChevronLeft)
                                .on_click(cx.listener(|this, _, _, cx| this.step_hero(-1, cx))),
                        )
                        .child(h_flex().gap_2().children((0..count).map(|i| {
                            let active = i == current;
                            let dot = Hsla::from(Palette::text()).opacity(0.35);
                            div()
                                .id(("hero-dot", i))
                                .h(px(6.))
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|this| this.bg(Hsla::from(Palette::text()).opacity(0.7)))
                                .on_click(cx.listener(move |this, _, _, cx| this.show_slide(i, cx)))
                                .with_spring(
                                    ("hero-dot-width", i),
                                    SpringAnimation::new(SPRING).to(active),
                                    move |this, phase| {
                                        let t = phase.0;
                                        this.w(px(6. + 22. * t.max(0.))).bg(mix(
                                            dot,
                                            accent,
                                            t.clamp(0., 1.),
                                        ))
                                    },
                                )
                        })))
                        .child(
                            hero_arrow("hero-next", IconName::ChevronRight)
                                .on_click(cx.listener(|this, _, _, cx| this.step_hero(1, cx))),
                        ),
                )
            })
    }

    fn render_rows(&mut self, accent: Hsla) -> Vec<AnyElement> {
        let mut rows = Vec::new();
        let landscape_height = LANDSCAPE_WIDTH * (9. / 16.) + px(40.);
        let portrait_height = PORTRAIT_WIDTH * 1.5 + px(40.);

        let shelves: Vec<(String, SharedString, Loadable<Vec<BaseItem>>, bool)> = [
            (
                "resume".to_string(),
                "Continue Watching".into(),
                self.resume.clone(),
                true,
            ),
            (
                "next-up".to_string(),
                "Next Up".into(),
                self.next_up.clone(),
                true,
            ),
            (
                "favorites".to_string(),
                "Favourites".into(),
                // Many people have none: no placeholder shelf while loading.
                match &self.favorites {
                    Loadable::Loading => Loadable::Ready(Vec::new()),
                    favorites => favorites.clone(),
                },
                false,
            ),
        ]
        .into_iter()
        .chain(self.latest.iter().map(|(view, items)| {
            (
                format!("latest-{}", view.id),
                format!("Latest in {}", view.name).into(),
                items.clone(),
                false,
            )
        }))
        .collect();

        for (id, title, items, landscape) in shelves {
            match items {
                Loadable::Loading => rows.push(
                    skeleton_row(
                        if landscape {
                            LANDSCAPE_WIDTH
                        } else {
                            PORTRAIT_WIDTH
                        },
                        if landscape {
                            landscape_height
                        } else {
                            portrait_height
                        } - px(40.),
                    )
                    .into_any_element(),
                ),
                Loadable::Failed(_) => {}
                Loadable::Ready(items) if items.is_empty() => {}
                Loadable::Ready(items) => {
                    let handle = self.row_handle(&id);
                    let cards = items
                        .iter()
                        .map(|item| {
                            let card = if landscape {
                                PosterCard::landscape(item)
                            } else {
                                PosterCard::portrait(item)
                            };
                            card.accent(accent).into_any_element()
                        })
                        .collect();
                    rows.push(row(id, title, &handle, cards).into_any_element());
                }
            }
        }
        rows
    }

    fn render_error(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let message = match (&self.resume, &self.next_up) {
            (Loadable::Failed(message), _) | (_, Loadable::Failed(message)) => message.clone(),
            _ => return None,
        };
        Some(
            div()
                .mx(ROW_PADDING)
                .p_6()
                .rounded(px(16.))
                .bg(Palette::glass())
                .flex()
                .items_center()
                .justify_between()
                .child(div().text_color(Palette::text_secondary()).child(message))
                .child(
                    glass_button("home-retry", Some(IconName::RefreshCw), "Try again").on_click(
                        cx.listener(|this, _, window, cx| {
                            this.resume = Loadable::Loading;
                            this.next_up = Loadable::Loading;
                            this.refresh(window, cx);
                            cx.notify();
                        }),
                    ),
                )
                .into_any_element(),
        )
    }
}

fn hero_arrow(id: &'static str, icon: IconName) -> Pressable {
    pressable(id).look(move |this, m| {
        this.relative()
            .size(px(40.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .bg(m.mix(Palette::control(), Palette::control_hover()))
            .border_1()
            .border_color(Palette::control_border())
            .child(focus_ring(m, px(20.)))
            .child(
                gpui_kit::component::Icon::new(icon)
                    .size_5()
                    .text_color(Palette::text()),
            )
    })
}

/// Adds up to `max` of `items` to the hero picks; returns how many it added.
fn pick(
    picks: &mut Vec<BaseItem>,
    seen_series: &mut Vec<String>,
    items: &[BaseItem],
    max: usize,
) -> usize {
    let start = picks.len();
    for item in items {
        if picks.len() - start == max {
            break;
        }
        if item.backdrop_image().is_none() {
            continue;
        }
        let key = item.series_id.clone().unwrap_or_else(|| item.id.clone());
        if seen_series.contains(&key) {
            continue;
        }
        seen_series.push(key);
        picks.push(item.clone());
    }
    picks.len() - start
}

fn loadable(result: jellyfin::Result<Vec<BaseItem>>) -> Loadable<Vec<BaseItem>> {
    Loadable::from_result(result)
}

fn ambient(request: &ImageRequest) -> ImageRequest {
    ImageRequest::new(request.url().replace("maxWidth=1920", "maxWidth=780")).ambient()
}

impl Render for HomePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.follow_hero_focus(window, cx);
        let accent = self.accent(window, cx);
        let viewport = window.viewport_size();
        let hero_height = (viewport.height * HERO_HEIGHT).clamp(px(520.), px(820.));
        let client = AppState::client(cx);
        // The outgoing slide's light stays under the incoming one's while it
        // fades in.
        let ambient_layers: Vec<_> = self
            .carousel
            .previous()
            .into_iter()
            .chain(Some(self.carousel.index()))
            .filter_map(|index| self.hero.get(index))
            .map(|item| {
                (
                    ElementId::Name(format!("ambient-{}", item.id).into()),
                    hero::backdrop_request(&client, item).map(|request| ambient(&request)),
                )
            })
            .collect();
        self.prefetch_hero(window, cx);
        let rows = self.render_rows(accent);
        let error = self.render_error(cx);
        let empty =
            self.hero.is_empty() && !self.resume.is_loading() && rows.is_empty() && error.is_none();
        let _ = cx.theme();

        div()
            .relative()
            .size_full()
            .bg(Palette::bg())
            .child(
                crate::components::scroller::page("home-scroll", &self.scroll)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .when(self.hero.is_empty(), |this| this.pt(NAV_HEIGHT + px(24.)))
                            .when(!self.hero.is_empty(), |this| {
                                this.child(self.render_hero(hero_height, accent, cx))
                            })
                            .child(
                                crate::components::ambient::layered(
                                    ambient_layers,
                                    vec![
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_10()
                                            .pb_16()
                                            // Shelves rise into the hero; the ambient light doesn't.
                                            .when(!self.hero.is_empty(), |this| this.mt(px(-110.)))
                                            .children(error)
                                            .children(rows)
                                            .when(empty, |this| {
                                                this.child(
                                                    div()
                                                        .px(ROW_PADDING)
                                                        .pt_16()
                                                        .text_color(Palette::text_secondary())
                                                        .child("Nothing here yet. Add some movies or shows to your server."),
                                                )
                                            })
                                            .into_any_element(),
                                    ],
                                ),
                            ),
                    ),
            )
            .vertical_scrollbar(&self.scroll)
    }
}
