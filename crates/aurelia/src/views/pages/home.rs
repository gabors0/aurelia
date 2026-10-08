//! Home: a hero carousel of what to watch, then shelves of Continue Watching,
//! Next Up and the latest additions to every library.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::h_flex;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, Context, ElementId, FontWeight, Hsla, ScrollHandle,
    SharedString, Task, Window, div, hsla, px, rgba,
};
use jellyfin::{BaseItem, ItemKind, UserView};

use super::carousel::Carousel;
use crate::components::button::{glass_button, play_button};
use crate::components::hero;
use crate::components::meta;
use crate::components::poster_card::{LANDSCAPE_WIDTH, PORTRAIT_WIDTH, PosterCard};
use crate::components::row::{ROW_PADDING, row, skeleton_row};
use crate::images::{ImageRequest, ImageState, ImageStore};
use crate::loadable::Loadable;
use crate::nav::Route;
use crate::runtime;
use crate::state::AppState;
use crate::theme::Palette;
use crate::views::shell::{self, NAV_HEIGHT};

const HERO_SLIDES: usize = 5;
const HERO_HEIGHT: f32 = 0.72;

pub struct HomePage {
    resume: Loadable<Vec<BaseItem>>,
    next_up: Loadable<Vec<BaseItem>>,
    latest: Vec<(UserView, Loadable<Vec<BaseItem>>)>,
    hero: Vec<BaseItem>,
    carousel: Carousel,
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
                    if this.carousel.tick(Instant::now()) {
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
            latest: Vec::new(),
            hero: Vec::new(),
            carousel: Carousel::new(Instant::now()),
            scroll: ScrollHandle::new(),
            rows: HashMap::new(),
            _load: None,
            _timer: timer,
        };
        this.refresh(window, cx);
        this
    }

    pub fn refresh(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
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
                this.latest = views
                    .into_iter()
                    .zip(latest_results)
                    .map(|(view, result)| (view, loadable(result)))
                    .collect();
                this.rebuild_hero();
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

    /// Hero picks: what you're in the middle of, then the newest arrivals —
    /// only items with backdrop art, one per show.
    fn rebuild_hero(&mut self) {
        let current = self.hero.get(self.carousel.index()).map(|i| i.id.clone());
        let mut picks: Vec<BaseItem> = Vec::new();
        let mut seen_series = Vec::new();
        let resume = self.resume.ready().into_iter().flatten().take(2);
        let latest = self
            .latest
            .iter()
            .filter_map(|(_, items)| items.ready())
            .flat_map(|items| items.iter().take(4));
        for item in resume.chain(latest) {
            if picks.len() == HERO_SLIDES || item.backdrop_image().is_none() {
                continue;
            }
            let key = item.series_id.clone().unwrap_or_else(|| item.id.clone());
            if seen_series.contains(&key) {
                continue;
            }
            seen_series.push(key);
            picks.push(item.clone());
        }
        let keep = current.and_then(|id| picks.iter().position(|i| i.id == id));
        self.carousel.reset(picks.len(), keep, Instant::now());
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

    fn pause_hero(&mut self, paused: bool) {
        self.carousel.set_paused(paused, Instant::now());
    }

    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll
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
            .unwrap_or_else(|| Palette::accent().into())
    }

    fn render_slide(
        &self,
        index: usize,
        fade_in: bool,
        accent: Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let client = AppState::client(cx);
        let item = &self.hero[index];
        let request = hero::backdrop_request(&client, item);
        let resume = item.resume_position().is_some();
        let label: SharedString = match (item.kind, resume) {
            (ItemKind::Episode, true) => format!(
                "Resume {}",
                crate::format::episode_label(item.parent_index_number, item.index_number)
            )
            .into(),
            (_, true) => "Resume".into(),
            (ItemKind::Series, false) => "Watch now".into(),
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

        let slide = div()
            .absolute()
            .inset_0()
            .child(hero::backdrop(("hero-art", index), request))
            .child(
                div()
                    .id(("hero-content", index))
                    .on_hover(cx.listener(|this, hovered: &bool, _, _| this.pause_hero(*hovered)))
                    .absolute()
                    .left(ROW_PADDING)
                    .bottom(px(150.))
                    .w(px(640.))
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(hero::title(&client, item, ("hero-logo", index), px(140.)))
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
                                .text_color(hsla(0., 0., 0.85, 1.))
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
                            .child(play_button(("hero-play", index), label, accent).on_click(
                                move |_, window, cx| {
                                    crate::playback::play_item(&play_item, false, window, cx)
                                },
                            ))
                            .child(
                                glass_button(
                                    ("hero-info", index),
                                    Some(IconName::Info),
                                    "More info",
                                )
                                .on_click(move |_, window, cx| {
                                    shell::navigate(info_route.clone(), window, cx)
                                }),
                            ),
                    ),
            );

        if fade_in {
            slide
                .with_animation(
                    ElementId::NamedInteger("hero-fade".into(), index as u64),
                    Animation::new(Duration::from_millis(700)).with_easing(gpui_kit::ease_in_out),
                    |slide, t| slide.opacity(t),
                )
                .into_any_element()
        } else {
            slide.into_any_element()
        }
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
                this.child(self.render_slide(previous, false, accent, cx))
            })
            .when(count > 0, |this| {
                this.child(self.render_slide(current, true, accent, cx))
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
                            div()
                                .id(("hero-dot", i))
                                .h(px(6.))
                                .w(if active { px(28.) } else { px(6.) })
                                .rounded_full()
                                .cursor_pointer()
                                .bg(if active {
                                    accent
                                } else {
                                    hsla(0., 0., 1., 0.35)
                                })
                                .hover(|this| this.bg(hsla(0., 0., 1., 0.7)))
                                .on_click(cx.listener(move |this, _, _, cx| this.show_slide(i, cx)))
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

fn hero_arrow(id: &'static str, icon: IconName) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .size(px(40.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .bg(rgba(0x0A0B1099))
        .border_1()
        .border_color(hsla(0., 0., 1., 0.18))
        .hover(|this| {
            this.bg(rgba(0x22232ECC))
                .border_color(hsla(0., 0., 1., 0.35))
        })
        .child(
            gpui_kit::component::Icon::new(icon)
                .size_5()
                .text_color(Palette::text()),
        )
}

fn loadable(result: jellyfin::Result<Vec<BaseItem>>) -> Loadable<Vec<BaseItem>> {
    Loadable::from_result(result)
}

fn ambient(request: &ImageRequest) -> ImageRequest {
    ImageRequest::new(request.url().replace("maxWidth=1920", "maxWidth=780")).ambient()
}

impl Render for HomePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = self.accent(window, cx);
        let viewport = window.viewport_size();
        let hero_height = (viewport.height * HERO_HEIGHT).clamp(px(520.), px(820.));
        let client = AppState::client(cx);
        let ambient_request = self
            .hero
            .get(self.carousel.index())
            .and_then(|item| hero::backdrop_request(&client, item))
            .map(|request| ambient(&request));
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
                                crate::components::ambient::section(
                                    ("ambient", self.carousel.index()),
                                    ambient_request,
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
