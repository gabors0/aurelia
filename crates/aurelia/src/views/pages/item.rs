//! A movie or an episode: artwork, facts, tracks, cast, and related titles.

use std::collections::HashMap;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, FontWeight, Hsla, ScrollHandle, SharedString, Task, WeakEntity, Window,
    div, hsla, px,
};
use jellyfin::{BaseItem, ItemKind, MediaStream, Person, StreamKind};

use crate::components::art::Art;
use crate::components::button::{glass_button, play_button, round_button};
use crate::components::hero;
use crate::components::meta;
use crate::components::poster_card::PosterCard;
use crate::components::row::{ROW_PADDING, row};
use crate::format;
use crate::images::{ImageRequest, ImageState, ImageStore};
use crate::loadable::Loadable;
use crate::nav::Route;
use crate::playback::{self, TrackSelection, track_label};
use crate::runtime;
use crate::state::AppState;
use crate::theme::{FONT_DISPLAY, Palette};
use crate::views::shell::{self, NAV_HEIGHT};

pub struct ItemPage {
    id: String,
    item: Loadable<BaseItem>,
    related: Loadable<Vec<BaseItem>>,
    audio: Option<i32>,
    subtitle: Option<i32>,
    scroll: ScrollHandle,
    rows: HashMap<&'static str, ScrollHandle>,
    _load: Option<Task<()>>,
    _toggle: Option<Task<()>>,
    play_when_ready: bool,
}

/// Development: `AURELIA_AUTOPLAY=1` plays the first item page opened.
fn dev_autoplay() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static DONE: AtomicBool = AtomicBool::new(false);
    std::env::var_os("AURELIA_AUTOPLAY").is_some() && !DONE.swap(true, Ordering::Relaxed)
}

impl ItemPage {
    pub fn new(id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            id,
            item: Loadable::Loading,
            related: Loadable::Loading,
            audio: None,
            subtitle: None,
            scroll: ScrollHandle::new(),
            rows: HashMap::new(),
            _load: None,
            _toggle: None,
            play_when_ready: false,
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
            let item = client.item(&id).await?;
            let related = match (&item.series_id, &item.season_id, item.kind) {
                (Some(series), Some(season), ItemKind::Episode) => {
                    client.episodes(series, season).await
                }
                _ => client.similar(&id, 16).await,
            };
            Ok((item, related.unwrap_or_default()))
        });
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = fetch.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok((item, related)) => {
                        this.item = Loadable::Ready(item);
                        this.related = Loadable::Ready(related);
                        crate::dev::apply_initial_scroll(&this.scroll);
                        if dev_autoplay() {
                            this.play_when_ready = true;
                        }
                    }
                    Err(err) => {
                        let message = shell::load_failed(&err, cx);
                        if this.item.ready().is_none() {
                            this.item = Loadable::Failed(message);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn play(&mut self, from_start: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.item.ready().is_none() {
            return;
        }
        let selection = TrackSelection {
            audio: self.audio,
            subtitle: self.subtitle,
        };
        playback::play(self.id.clone(), selection, from_start, window, cx);
    }

    fn toggle_played(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.item.ready_mut() else {
            return;
        };
        let played = !item.is_played();
        let data = item.user_data.get_or_insert_with(Default::default);
        let previous = data.clone();
        data.played = played;
        if played {
            data.playback_position_ticks = jellyfin::Ticks::ZERO;
            data.played_percentage = None;
        }
        cx.notify();
        let client = AppState::client(cx);
        let id = self.id.clone();
        let request = runtime::api(cx, async move { client.set_played(&id, played).await });
        self._toggle = Some(self.finish_toggle(request, previous, window, cx));
    }

    fn toggle_favorite(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.item.ready_mut() else {
            return;
        };
        let favorite = !item.is_favorite();
        let data = item.user_data.get_or_insert_with(Default::default);
        let previous = data.clone();
        data.is_favorite = favorite;
        cx.notify();
        let client = AppState::client(cx);
        let id = self.id.clone();
        let request = runtime::api(cx, async move { client.set_favorite(&id, favorite).await });
        self._toggle = Some(self.finish_toggle(request, previous, window, cx));
    }

    /// Applies the server's answer, or rolls back the optimistic change.
    fn finish_toggle(
        &self,
        request: impl std::future::Future<Output = jellyfin::Result<jellyfin::UserData>> + 'static,
        previous: jellyfin::UserData,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                if let Some(item) = this.item.ready_mut() {
                    match result {
                        Ok(data) => item.user_data = Some(data),
                        Err(err) => {
                            item.user_data = Some(previous);
                            shell::report_error(&err, window, cx);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
    }

    fn row_handle(&mut self, id: &'static str) -> ScrollHandle {
        self.rows.entry(id).or_default().clone()
    }

    fn render_hero(
        &self,
        item: &BaseItem,
        accent: Hsla,
        height: gpui_kit::Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let client = AppState::client(cx);
        let resume = item.resume_position();
        let runtime = item.runtime();
        let play_label: SharedString = match resume {
            Some(position) => format!("Resume {}", format::clock(position)).into(),
            None => "Play".into(),
        };
        let remaining = match (runtime, resume) {
            (Some(runtime), Some(position)) if runtime > position => Some(runtime - position),
            (Some(runtime), None) => Some(runtime),
            _ => None,
        };
        let timing: Option<SharedString> = remaining.map(|left| {
            let ends = format::ends_at(left, chrono::Local::now());
            if resume.is_some() {
                format!("{} left · ends at {ends}", format::runtime(left)).into()
            } else {
                format!("Ends at {ends}").into()
            }
        });
        let is_episode = item.kind == ItemKind::Episode;
        let series_route = item.series_id.clone().map(|id| Route::Series {
            id,
            season_id: item.season_id.clone(),
        });

        let title: AnyElement = if is_episode {
            div()
                .font_family(FONT_DISPLAY)
                .font_weight(FontWeight::EXTRA_BOLD)
                .text_size(px(48.))
                .line_height(px(52.))
                .line_clamp(2)
                .text_ellipsis()
                .child(item.name.clone())
                .into_any_element()
        } else {
            hero::title(&client, item, "item-logo", px(150.))
        };

        div()
            .relative()
            .w_full()
            .h(height)
            .flex_shrink_0()
            .child(hero::backdrop(
                "item-backdrop",
                hero::backdrop_request(&client, item),
            ))
            .child(
                v_flex()
                    .absolute()
                    .left(ROW_PADDING)
                    .bottom(px(40.))
                    .w(px(700.))
                    .gap_4()
                    .when_some(
                        item.series_name.clone().filter(|_| is_episode),
                        |this, series| {
                            let route = series_route.clone();
                            this.child(
                                div()
                                    .id("series-link")
                                    .text_size(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(accent)
                                    .cursor_pointer()
                                    .hover(|this| this.underline())
                                    .when_some(route, |this, route| {
                                        this.on_click(move |_, window, cx| {
                                            shell::navigate(route.clone(), window, cx)
                                        })
                                    })
                                    .child(series),
                            )
                        },
                    )
                    .child(title)
                    .child(
                        h_flex()
                            .gap_3()
                            .flex_wrap()
                            .child(meta::line(meta::facts(item)))
                            .child(meta::badge_row(meta::badges(item))),
                    )
                    .when(!item.genres.is_empty(), |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(Palette::text_tertiary())
                                .child(item.genres.join(" · ")),
                        )
                    })
                    .when_some(item.overview.clone(), |this, overview| {
                        this.child(
                            div()
                                .text_color(hsla(0., 0., 0.86, 1.))
                                .line_height(px(24.))
                                .line_clamp(4)
                                .text_ellipsis()
                                .child(overview),
                        )
                    })
                    .child(
                        h_flex()
                            .gap_3()
                            .mt_2()
                            .child(play_button("item-play", play_label, accent).on_click(
                                cx.listener(|this, _, window, cx| this.play(false, window, cx)),
                            ))
                            .when(resume.is_some(), |this| {
                                this.child(
                                    glass_button(
                                        "item-restart",
                                        Some(IconName::RotateCcw),
                                        "From start",
                                    )
                                    .on_click(cx.listener(
                                        |this, _, window, cx| this.play(true, window, cx),
                                    )),
                                )
                            })
                            .child(
                                round_button(
                                    "item-played",
                                    IconName::Check,
                                    item.is_played(),
                                    accent,
                                )
                                .tooltip({
                                    let text = if item.is_played() {
                                        "Mark unwatched"
                                    } else {
                                        "Mark watched"
                                    };
                                    move |window, cx| Tooltip::new(text).build(window, cx)
                                })
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.toggle_played(window, cx)
                                    }),
                                ),
                            )
                            .child(
                                round_button(
                                    "item-favorite",
                                    IconName::Heart,
                                    item.is_favorite(),
                                    accent,
                                )
                                .tooltip(|window, cx| Tooltip::new("Favourite").build(window, cx))
                                .on_click(cx.listener(
                                    |this, _, window, cx| this.toggle_favorite(window, cx),
                                )),
                            ),
                    )
                    .when_some(timing, |this, timing| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(Palette::text_tertiary())
                                .child(timing),
                        )
                    }),
            )
    }

    fn render_tracks(&self, item: &BaseItem, cx: &mut Context<Self>) -> Option<AnyElement> {
        let source = item.media_sources.first()?;
        let audio: Vec<MediaStream> = source.streams(StreamKind::Audio).cloned().collect();
        let subtitles: Vec<MediaStream> = source.streams(StreamKind::Subtitle).cloned().collect();
        let audio_selected = self.audio.or(source.default_audio_stream_index);
        let subtitle_selected = self
            .subtitle
            .or(source.default_subtitle_stream_index)
            .unwrap_or(-1);
        let page = cx.weak_entity();

        let mut column = v_flex().gap_3().w(px(460.));
        if audio.len() > 1 || !audio.is_empty() {
            let current = audio
                .iter()
                .find(|s| Some(s.index) == audio_selected)
                .or(audio.first())
                .map(track_label)
                .unwrap_or_default();
            column = column.child(picker(
                "audio",
                IconName::AudioLines,
                "Audio",
                current,
                audio
                    .iter()
                    .map(|s| (s.index, track_label(s), Some(s.index) == audio_selected))
                    .collect(),
                page.clone(),
                |page, index| page.audio = Some(index),
            ));
        }
        let mut options = vec![(-1, "Off".to_string(), subtitle_selected == -1)];
        options.extend(
            subtitles
                .iter()
                .map(|s| (s.index, track_label(s), s.index == subtitle_selected)),
        );
        let current = options
            .iter()
            .find(|(_, _, selected)| *selected)
            .map(|(_, label, _)| label.clone())
            .unwrap_or_else(|| "Off".into());
        if !subtitles.is_empty() {
            column = column.child(picker(
                "subtitles",
                IconName::Captions,
                "Subtitles",
                current,
                options,
                page,
                |page, index| page.subtitle = Some(index),
            ));
        }
        Some(column.into_any_element())
    }

    fn render_details(&self, item: &BaseItem) -> AnyElement {
        let names = |kind: &str| -> Option<String> {
            let names: Vec<&str> = item
                .people
                .iter()
                .filter(|p| p.person_type.as_deref() == Some(kind))
                .map(|p| p.name.as_str())
                .take(3)
                .collect();
            (!names.is_empty()).then(|| names.join(", "))
        };
        let studios: Vec<&str> = item
            .studios
            .iter()
            .map(|s| s.name.as_str())
            .take(2)
            .collect();
        let rows: Vec<(&str, String)> = [
            ("Directed by", names("Director")),
            ("Written by", names("Writer")),
            ("Studio", (!studios.is_empty()).then(|| studios.join(", "))),
            (
                "Original title",
                item.original_title.clone().filter(|t| *t != item.name),
            ),
        ]
        .into_iter()
        .filter_map(|(label, value)| value.map(|v| (label, v)))
        .collect();

        v_flex()
            .gap_2()
            .flex_1()
            .children(rows.into_iter().map(|(label, value)| {
                h_flex()
                    .gap_4()
                    .child(
                        div()
                            .w(px(120.))
                            .flex_shrink_0()
                            .text_sm()
                            .text_color(Palette::text_tertiary())
                            .child(label),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(Palette::text_secondary())
                            .child(value),
                    )
            }))
            .into_any_element()
    }

    fn render_cast(&mut self, item: &BaseItem, cx: &mut Context<Self>) -> Option<AnyElement> {
        let handle = self.row_handle("cast");
        crate::components::cast::cast_row(item, &handle, cx)
    }

    fn render_related(&mut self, item: &BaseItem, accent: Hsla) -> Option<AnyElement> {
        let related: Vec<BaseItem> = self
            .related
            .ready()?
            .iter()
            .filter(|r| r.id != item.id)
            .cloned()
            .collect();
        if related.is_empty() {
            return None;
        }
        let episodes = item.kind == ItemKind::Episode;
        let title: SharedString = if episodes {
            match item.parent_index_number {
                Some(season) => format!("More from Season {season}").into(),
                None => "More episodes".into(),
            }
        } else {
            "More like this".into()
        };
        let cards = related
            .iter()
            .map(|r| {
                let card = if episodes {
                    PosterCard::landscape(r)
                } else {
                    PosterCard::portrait(r)
                };
                card.accent(accent).into_any_element()
            })
            .collect();
        let handle = self.row_handle("related");
        Some(row("related", title, &handle, cards).into_any_element())
    }
}

/// A labelled dropdown of tracks.
fn picker(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    current: String,
    options: Vec<(i32, String, bool)>,
    page: WeakEntity<ItemPage>,
    apply: fn(&mut ItemPage, i32),
) -> impl IntoElement {
    h_flex()
        .gap_4()
        .child(
            h_flex()
                .w(px(110.))
                .gap_2()
                .text_sm()
                .text_color(Palette::text_tertiary())
                .child(gpui_kit::component::Icon::new(icon).size_4())
                .child(label),
        )
        .child(
            Button::new(id)
                .outline()
                .small()
                .label(current)
                .dropdown_caret(true)
                .dropdown_menu(move |mut menu, _, _| {
                    for (index, text, selected) in options.clone() {
                        let page = page.clone();
                        menu = menu.item(PopupMenuItem::new(text).checked(selected).on_click(
                            move |_, _, cx| {
                                page.update(cx, |page, cx| {
                                    apply(page, index);
                                    cx.notify();
                                })
                                .ok();
                            },
                        ));
                    }
                    menu
                }),
        )
}

impl Render for ItemPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let item = match &self.item {
            Loadable::Ready(item) => item.clone(),
            Loadable::Loading => {
                return div()
                    .size_full()
                    .bg(Palette::bg())
                    .pt(NAV_HEIGHT)
                    .into_any_element();
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
        if std::mem::take(&mut self.play_when_ready) {
            self.play(false, window, cx);
        }
        let client = AppState::client(cx);
        let ambient = item
            .backdrop_image()
            .map(|image| ImageRequest::for_image(&client, &image, 780).ambient());
        let accent = ambient
            .as_ref()
            .and_then(|request| match ImageStore::get(request, window, cx) {
                ImageState::Ready { accent, .. } => accent,
                _ => None,
            })
            .unwrap_or_else(|| Palette::accent().into());
        let height = (window.viewport_size().height * 0.68).clamp(px(520.), px(780.));

        let hero = self
            .render_hero(&item, accent, height, cx)
            .into_any_element();
        let tracks = self.render_tracks(&item, cx);
        let details = self.render_details(&item);
        let cast = self.render_cast(&item, cx);
        let related = self.render_related(&item, accent);

        let below = vec![
            h_flex()
                .items_start()
                .gap_12()
                .px(ROW_PADDING)
                .pt_4()
                .pb_10()
                .children(tracks)
                .child(details)
                .into_any_element(),
            v_flex()
                .gap_10()
                .pb_16()
                .children(cast)
                .children(related)
                .into_any_element(),
        ];

        div()
            .relative()
            .size_full()
            .bg(Palette::bg())
            .child(
                div()
                    .id("item-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        v_flex()
                            .child(hero)
                            .child(crate::components::ambient::section(
                                "item-ambient",
                                ambient,
                                below,
                            )),
                    ),
            )
            .vertical_scrollbar(&self.scroll)
            .into_any_element()
    }
}
