//! A show: hero with the next episode to watch, season tabs, episode list.

use std::collections::HashMap;

use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, FontWeight, Hsla, ScrollHandle, SharedString, Task, Window, div, hsla, px,
    rgba,
};
use jellyfin::BaseItem;

use crate::components::ambient;
use crate::components::art::Art;
use crate::components::button::{glass_button, on_color, play_button, round_button};
use crate::components::cast::cast_row;
use crate::components::hero;
use crate::components::item_menu;
use crate::components::meta;
use crate::components::pill::pill;
use crate::components::progress;
use crate::components::row::ROW_PADDING;
use crate::format;
use crate::images::{ImageRequest, ImageState, ImageStore};
use crate::loadable::Loadable;
use crate::nav::Route;
use crate::playback::{self, TrackSelection};
use crate::runtime;
use crate::state::AppState;
use crate::theme::Palette;
use crate::user_data;
use crate::views::shell::{self, NAV_HEIGHT};

/// The season to open: the one asked for, else the one with the next
/// episode, else the first regular season (Specials last).
pub fn pick_season(
    seasons: &[BaseItem],
    requested: Option<&str>,
    next_up_season: Option<&str>,
) -> Option<String> {
    let exists = |id: &str| seasons.iter().any(|s| s.id == id);
    if let Some(id) = requested.filter(|id| exists(id)) {
        return Some(id.to_string());
    }
    if let Some(id) = next_up_season.filter(|id| exists(id)) {
        return Some(id.to_string());
    }
    seasons
        .iter()
        .find(|s| s.index_number.is_some_and(|n| n > 0))
        .or(seasons.first())
        .map(|s| s.id.clone())
}

pub struct SeriesPage {
    id: String,
    series: Loadable<BaseItem>,
    seasons: Loadable<Vec<BaseItem>>,
    next_up: Option<BaseItem>,
    selected: Option<String>,
    episodes: HashMap<String, Loadable<Vec<BaseItem>>>,
    scroll: ScrollHandle,
    cast: ScrollHandle,
    _load: Option<Task<()>>,
    /// One load per season, so switching seasons never cancels a load and
    /// leaves that season stuck on "Loading".
    episode_loads: HashMap<String, Task<()>>,
}

impl SeriesPage {
    pub fn new(
        id: String,
        season_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            id,
            series: Loadable::Loading,
            seasons: Loadable::Loading,
            next_up: None,
            selected: season_id,
            episodes: HashMap::new(),
            scroll: ScrollHandle::new(),
            cast: ScrollHandle::new(),
            _load: None,
            episode_loads: HashMap::new(),
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
            let series = client.item(&id).await?;
            let seasons = client.seasons(&id).await?;
            let next_up = client.next_up(Some(&id), 1, true).await.unwrap_or_default();
            Ok((series, seasons, next_up.into_iter().next()))
        });
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = fetch.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok((series, seasons, next_up)) => {
                        let selected = pick_season(
                            &seasons,
                            this.selected.as_deref(),
                            next_up.as_ref().and_then(|e| e.season_id.as_deref()),
                        );
                        this.series = Loadable::Ready(series);
                        this.seasons = Loadable::Ready(seasons);
                        this.next_up = next_up;
                        // Episodes may have changed (after playback): reload
                        // the open season, keeping it on screen meanwhile, and
                        // the others when they're next opened.
                        let shown = selected
                            .as_ref()
                            .and_then(|s| this.episodes.remove_entry(s));
                        this.episodes.clear();
                        this.episode_loads.clear();
                        this.episodes.extend(shown);
                        if let Some(season) = selected {
                            this.selected = Some(season.clone());
                            this.load_episodes(season, cx);
                        }
                    }
                    Err(err) => {
                        let message = shell::load_failed(&err, cx);
                        if this.series.ready().is_none() {
                            this.series = Loadable::Failed(message);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn select_season(&mut self, season: String, cx: &mut Context<Self>) {
        self.selected = Some(season.clone());
        if !self.episodes.contains_key(&season) {
            self.load_episodes(season, cx);
        }
        cx.notify();
    }

    /// Fetches a season's episodes; what's shown stays until they arrive.
    fn load_episodes(&mut self, season: String, cx: &mut Context<Self>) {
        self.episodes
            .entry(season.clone())
            .or_insert(Loadable::Loading);
        let client = AppState::client(cx);
        let series = self.id.clone();
        let season_id = season.clone();
        let fetch = runtime::api(
            cx,
            async move { client.episodes(&series, &season_id).await },
        );
        let key = season.clone();
        let load = cx.spawn(async move |this, cx| {
            let result = fetch.await;
            this.update(cx, |this, cx| {
                this.episode_loads.remove(&season);
                match result {
                    Ok(episodes) => {
                        this.episodes.insert(season, Loadable::Ready(episodes));
                    }
                    Err(err) => {
                        let message = shell::load_failed(&err, cx);
                        let shown = this.episodes.get(&season).and_then(Loadable::ready);
                        if shown.is_none() {
                            this.episodes.insert(season, Loadable::Failed(message));
                        }
                    }
                }
                crate::dev::apply_initial_scroll(&this.scroll);
                cx.notify();
            })
            .ok();
        });
        self.episode_loads.insert(key, load);
        cx.notify();
    }

    pub fn play(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match (&self.next_up, self.series.ready()) {
            (Some(next), _) => playback::play(
                next.id.clone(),
                TrackSelection::default(),
                false,
                window,
                cx,
            ),
            (None, Some(series)) => playback::play_item(&series.clone(), false, window, cx),
            _ => {}
        }
    }

    fn toggle(&mut self, favorite: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(series) = self.series.ready().cloned() else {
            return;
        };
        if favorite {
            user_data::set_favorite(&series, !series.is_favorite(), window, cx);
        } else {
            // Every episode changes too; the reload afterwards picks them up.
            user_data::set_played(&series, !series.is_played(), window, cx);
        }
    }

    pub fn patch_user_data(&mut self, id: &str, data: &jellyfin::UserData) -> bool {
        let items = self
            .series
            .ready_mut()
            .into_iter()
            .chain(self.seasons.ready_mut().into_iter().flatten())
            .chain(
                self.episodes
                    .values_mut()
                    .filter_map(Loadable::ready_mut)
                    .flatten(),
            )
            .chain(self.next_up.iter_mut());
        user_data::patch(items, id, data)
    }

    fn render_hero(&self, series: &BaseItem, accent: Hsla, cx: &mut Context<Self>) -> AnyElement {
        let client = AppState::client(cx);
        let label: SharedString = match &self.next_up {
            Some(next) => {
                let episode = format::episode_label(next.parent_index_number, next.index_number);
                if next.resume_position().is_some() {
                    format!("Resume {episode}").into()
                } else {
                    format!("Play {episode}").into()
                }
            }
            None => "Play".into(),
        };
        let next_title = self.next_up.as_ref().map(|next| next.name.clone());
        let facts = meta::facts(series);

        div()
            .relative()
            .w_full()
            .h(px(640.))
            .flex_shrink_0()
            .child(hero::backdrop(
                "series-backdrop",
                hero::backdrop_request(&client, series),
            ))
            .child(
                v_flex()
                    .absolute()
                    .left(ROW_PADDING)
                    .bottom(px(40.))
                    .w(px(700.))
                    .gap_4()
                    .child(hero::title(&client, series, "series-logo", px(150.)))
                    .child(meta::line(facts))
                    .when(!series.genres.is_empty(), |this| {
                        this.child(meta::genre_links(&series.genres))
                    })
                    .when_some(series.overview.clone(), |this, overview| {
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
                            .child(
                                play_button("series-play", label, accent).on_click(
                                    cx.listener(|this, _, window, cx| this.play(window, cx)),
                                ),
                            )
                            .when_some(self.next_up.clone(), |this, next| {
                                let route = Route::Item {
                                    id: next.id.clone(),
                                };
                                this.child(
                                    glass_button(
                                        "series-next-info",
                                        Some(IconName::Info),
                                        "Episode info",
                                    )
                                    .on_click(
                                        move |_, window, cx| {
                                            shell::navigate(route.clone(), window, cx)
                                        },
                                    ),
                                )
                            })
                            .child(
                                round_button(
                                    "series-played",
                                    IconName::Check,
                                    series.is_played(),
                                    accent,
                                )
                                .tooltip(|window, cx| {
                                    Tooltip::new("Mark the whole show watched").build(window, cx)
                                })
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.toggle(false, window, cx)
                                    }),
                                ),
                            )
                            .child(
                                round_button(
                                    "series-favorite",
                                    IconName::Heart,
                                    series.is_favorite(),
                                    accent,
                                )
                                .tooltip(|window, cx| Tooltip::new("Favourite").build(window, cx))
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.toggle(true, window, cx)
                                    }),
                                ),
                            ),
                    )
                    .when_some(next_title, |this, title| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(Palette::text_tertiary())
                                .child(format!("Up next: {title}")),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_seasons(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let seasons = self.seasons.ready()?;
        if seasons.is_empty() {
            return None;
        }
        Some(
            h_flex()
                .px(ROW_PADDING)
                .gap_2()
                .flex_wrap()
                .children(seasons.iter().map(|season| {
                    let id = season.id.clone();
                    let active = self.selected.as_deref() == Some(season.id.as_str());
                    pill(
                        SharedString::from(format!("season-{}", season.id)),
                        season.name.clone(),
                        active,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.select_season(id.clone(), cx)))
                }))
                .into_any_element(),
        )
    }

    fn render_episodes(&self, accent: Hsla, client: &jellyfin::Client) -> AnyElement {
        let Some(season) = &self.selected else {
            return div().into_any_element();
        };
        match self.episodes.get(season) {
            None | Some(Loadable::Loading) => v_flex()
                .px(ROW_PADDING)
                .gap_4()
                .children((0..4).map(|_| {
                    h_flex()
                        .gap_5()
                        .child(
                            div()
                                .w(px(256.))
                                .h(px(144.))
                                .rounded(px(12.))
                                .bg(Palette::glass()),
                        )
                        .child(
                            v_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .w(px(280.))
                                        .h(px(18.))
                                        .rounded(px(6.))
                                        .bg(Palette::glass()),
                                )
                                .child(
                                    div()
                                        .w(px(480.))
                                        .h(px(14.))
                                        .rounded(px(6.))
                                        .bg(Palette::glass()),
                                ),
                        )
                }))
                .into_any_element(),
            Some(Loadable::Failed(message)) => div()
                .px(ROW_PADDING)
                .text_color(Palette::text_secondary())
                .child(message.clone())
                .into_any_element(),
            Some(Loadable::Ready(episodes)) if episodes.is_empty() => div()
                .px(ROW_PADDING)
                .text_color(Palette::text_secondary())
                .child("No episodes in this season yet.")
                .into_any_element(),
            Some(Loadable::Ready(episodes)) => v_flex()
                .px(ROW_PADDING - px(12.))
                .gap_1()
                .children(
                    episodes
                        .iter()
                        .map(|episode| episode_row(episode, client, accent, self.next_up.as_ref())),
                )
                .into_any_element(),
        }
    }
}

fn episode_row(
    episode: &BaseItem,
    client: &jellyfin::Client,
    accent: Hsla,
    next_up: Option<&BaseItem>,
) -> AnyElement {
    let group: SharedString = format!("episode-{}", episode.id).into();
    let request = episode
        .landscape_image()
        .map(|image| ImageRequest::for_image(client, &image, 512));
    let route = Route::Item {
        id: episode.id.clone(),
    };
    let play_id = episode.id.clone();
    let number = episode
        .index_number
        .map(|n| format!("{n}. "))
        .unwrap_or_default();
    let mut facts: Vec<String> = Vec::new();
    if let Some(runtime) = episode.runtime() {
        facts.push(format::runtime(runtime));
    }
    if let Some(date) = episode.premiere_date.as_deref().and_then(format::date) {
        facts.push(date);
    }
    let is_next = next_up.is_some_and(|n| n.id == episode.id);
    let progress = episode.progress();
    let played = episode.is_played() && progress.is_none();

    let row = h_flex()
        .id(group.clone())
        .group(group.clone())
        .items_start()
        .gap_5()
        .p_3()
        .rounded(px(16.))
        .cursor_pointer()
        .when(is_next, |this| this.bg(accent.opacity(0.08)))
        .hover(|this| this.bg(Palette::glass()))
        .on_click(move |_, window, cx| shell::navigate(route.clone(), window, cx))
        .child(
            div()
                .relative()
                .w(px(256.))
                .h(px(144.))
                .flex_shrink_0()
                .child(
                    Art::new(SharedString::from(format!("{group}-art")), request)
                        .title(episode.name.clone())
                        .radius(px(12.))
                        .size_full(),
                )
                .when(played, |this| {
                    this.child(
                        div()
                            .absolute()
                            .top_2()
                            .right_2()
                            .size_6()
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(accent)
                            .child(
                                Icon::new(IconName::Check)
                                    .size_3p5()
                                    .text_color(on_color(accent)),
                            ),
                    )
                })
                .when_some(progress, |this, fraction| {
                    this.child(
                        div()
                            .absolute()
                            .bottom_2()
                            .left_2()
                            .right_2()
                            .child(progress::bar(fraction, accent)),
                    )
                })
                .child(
                    div()
                        .id(SharedString::from(format!("{group}-play")))
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .opacity(0.)
                        .group_hover(group.clone(), |style| style.opacity(1.))
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            playback::play(
                                play_id.clone(),
                                TrackSelection::default(),
                                false,
                                window,
                                cx,
                            );
                        })
                        .child(
                            div()
                                .size(px(52.))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(rgba(0x0A0B10B3))
                                .border_1()
                                .border_color(hsla(0., 0., 1., 0.25))
                                .hover(move |this| this.bg(accent).border_color(accent))
                                .child(Icon::new(IconName::Play).size_5()),
                        ),
                ),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1p5()
                .pt_1()
                .child(
                    h_flex()
                        .gap_3()
                        .child(
                            div()
                                .text_size(px(17.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(format!("{number}{}", episode.name)),
                        )
                        .when(is_next, |this| {
                            this.child(
                                div()
                                    .px_2()
                                    .py(px(2.))
                                    .rounded_full()
                                    .bg(accent)
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(on_color(accent))
                                    .child("UP NEXT"),
                            )
                        }),
                )
                .when(!facts.is_empty(), |this| {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(Palette::text_tertiary())
                            .child(facts.join(" · ")),
                    )
                })
                .when_some(episode.overview.clone(), |this, overview| {
                    this.child(
                        div()
                            .max_w(px(820.))
                            .text_sm()
                            .line_height(px(21.))
                            .text_color(Palette::text_secondary())
                            .line_clamp(2)
                            .text_ellipsis()
                            .child(overview),
                    )
                }),
        );
    item_menu::attach(row, episode.clone()).into_any_element()
}

impl Render for SeriesPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let series = match &self.series {
            Loadable::Ready(series) => series.clone(),
            Loadable::Loading => {
                return div().size_full().bg(Palette::bg()).into_any_element();
            }
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
        let ambient_request = series
            .backdrop_image()
            .map(|image| ImageRequest::for_image(&client, &image, 780).ambient());
        let accent = ambient_request
            .as_ref()
            .and_then(|request| match ImageStore::get(request, window, cx) {
                ImageState::Ready { accent, .. } => accent,
                _ => None,
            })
            .unwrap_or_else(|| Palette::accent().into());

        let hero = self.render_hero(&series, accent, cx);
        let seasons = self.render_seasons(cx);
        let episodes = self.render_episodes(accent, &client);
        let cast = cast_row(&series, &self.cast, accent, cx);

        let below = vec![
            v_flex()
                .gap_6()
                .pt_4()
                .pb_16()
                .children(seasons)
                .child(episodes)
                .child(div().h_6())
                .children(cast)
                .into_any_element(),
        ];

        div()
            .relative()
            .size_full()
            .bg(Palette::bg())
            .child(
                crate::components::scroller::page("series-scroll", &self.scroll).child(
                    v_flex().child(hero).child(ambient::section(
                        "series-ambient",
                        ambient_request,
                        below,
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

    fn season(id: &str, index: i32) -> BaseItem {
        serde_json::from_value(serde_json::json!({
            "Id": id, "Name": format!("Season {index}"), "Type": "Season", "IndexNumber": index
        }))
        .unwrap()
    }

    #[test]
    fn requested_season_wins() {
        let seasons = [season("s0", 0), season("s1", 1), season("s2", 2)];
        assert_eq!(
            pick_season(&seasons, Some("s2"), Some("s1")).as_deref(),
            Some("s2")
        );
    }

    #[test]
    fn next_up_season_next() {
        let seasons = [season("s0", 0), season("s1", 1), season("s2", 2)];
        assert_eq!(
            pick_season(&seasons, None, Some("s2")).as_deref(),
            Some("s2")
        );
    }

    #[test]
    fn first_regular_season_before_specials() {
        let seasons = [season("s0", 0), season("s1", 1)];
        assert_eq!(pick_season(&seasons, None, None).as_deref(), Some("s1"));
        assert_eq!(
            pick_season(&[season("s0", 0)], None, None).as_deref(),
            Some("s0")
        );
        assert_eq!(pick_season(&[], None, None), None);
    }

    #[test]
    fn unknown_request_falls_back() {
        let seasons = [season("s1", 1)];
        assert_eq!(
            pick_season(&seasons, Some("gone"), None).as_deref(),
            Some("s1")
        );
    }
}
