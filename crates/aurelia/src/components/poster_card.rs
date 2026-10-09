//! Item cards for rows and grids: portrait posters and 16:9 stills.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, FontWeight, Hsla, ObjectFit, Pixels, SharedString, Window, div, hsla, px, rgba,
};
use jellyfin::{BaseItem, ItemKind};

use crate::components::art::Art;
use crate::components::item_menu;
use crate::components::progress;
use crate::format;
use crate::images::ImageRequest;
use crate::nav::Route;
use crate::state::AppState;
use crate::theme::Palette;
use crate::views::shell;

pub const PORTRAIT_WIDTH: Pixels = px(172.);
pub const LANDSCAPE_WIDTH: Pixels = px(316.);
const RADIUS: Pixels = px(12.);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CardShape {
    Portrait,
    Landscape,
}

#[derive(IntoElement)]
pub struct PosterCard {
    item: BaseItem,
    shape: CardShape,
    width: Pixels,
    accent: Hsla,
}

impl PosterCard {
    pub fn portrait(item: &BaseItem) -> Self {
        Self::new(item, CardShape::Portrait)
    }

    pub fn landscape(item: &BaseItem) -> Self {
        Self::new(item, CardShape::Landscape)
    }

    fn new(item: &BaseItem, shape: CardShape) -> Self {
        Self {
            item: item.clone(),
            shape,
            width: match shape {
                CardShape::Portrait => PORTRAIT_WIDTH,
                CardShape::Landscape => LANDSCAPE_WIDTH,
            },
            accent: Palette::accent().into(),
        }
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = width;
        self
    }

    pub fn accent(mut self, accent: Hsla) -> Self {
        self.accent = accent;
        self
    }
}

/// Primary and secondary caption lines.
pub fn captions(item: &BaseItem, shape: CardShape) -> (SharedString, Option<SharedString>) {
    match item.kind {
        ItemKind::Episode => {
            let series = item
                .series_name
                .clone()
                .unwrap_or_else(|| item.name.clone());
            let label = format::episode_label(item.parent_index_number, item.index_number);
            let detail = match (label.is_empty(), shape) {
                (true, _) => item.name.clone(),
                (false, CardShape::Landscape) => format!("{label} · {}", item.name),
                (false, CardShape::Portrait) => label,
            };
            (series.into(), Some(detail.into()))
        }
        ItemKind::Series => {
            let years = format::years(
                item.production_year,
                item.end_date.as_deref(),
                item.status.as_deref() == Some("Continuing"),
            );
            (
                item.name.clone().into(),
                (!years.is_empty()).then(|| years.into()),
            )
        }
        ItemKind::Season => (
            item.series_name.clone().unwrap_or_default().into(),
            Some(item.name.clone().into()),
        ),
        _ => {
            let remaining = item
                .progress()
                .zip(item.runtime())
                .map(|(progress, runtime)| {
                    let left = runtime.mul_f32(1.0 - progress);
                    format!("{} left", format::runtime(left))
                });
            let secondary = remaining.or_else(|| item.production_year.map(|y| y.to_string()));
            (item.name.clone().into(), secondary.map(Into::into))
        }
    }
}

impl RenderOnce for PosterCard {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let client = AppState::client(cx);
        let (image, art_height, max_width) = match self.shape {
            CardShape::Portrait => (self.item.poster_image(), self.width * 1.5, 360),
            CardShape::Landscape => (self.item.landscape_image(), self.width * (9.0 / 16.0), 640),
        };
        let request = image.map(|image| ImageRequest::for_image(&client, &image, max_width));
        let group: SharedString = format!("card-{}-{:?}", self.item.id, self.shape as u8).into();
        let (title, subtitle) = captions(&self.item, self.shape);
        let progress = self.item.progress();
        // A rewatch in progress shows its progress, not a stale checkmark.
        let played = self.item.is_played() && progress.is_none();
        let unplayed = self
            .item
            .user_data
            .as_ref()
            .and_then(|d| d.unplayed_item_count)
            .filter(|n| *n > 0 && self.item.kind == ItemKind::Series);
        let accent = self.accent;
        let route = Route::for_item(&self.item);

        let card = div()
            .id(group.clone())
            .group(group.clone())
            .flex()
            .flex_col()
            .gap_2()
            .w(self.width)
            .flex_shrink_0()
            .cursor_pointer()
            .on_click(move |_, window, cx| shell::navigate(route.clone(), window, cx))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(art_height)
                    .rounded(RADIUS)
                    .shadow(vec![gpui_kit::BoxShadow {
                        color: hsla(0., 0., 0., 0.35),
                        offset: gpui_kit::point(px(0.), px(8.)),
                        blur_radius: px(24.),
                        spread_radius: px(-8.),
                        inset: false,
                    }])
                    .child(
                        Art::new(format!("{group}-art"), request)
                            .title(self.item.name.clone())
                            .radius(RADIUS)
                            .fit(ObjectFit::Cover)
                            .size_full(),
                    )
                    // Hover: brighten, accent ring, play glyph.
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(RADIUS)
                            .border_2()
                            .border_color(gpui_kit::transparent_black())
                            .group_hover(group.clone(), move |style| {
                                style.border_color(accent).bg(hsla(0., 0., 1., 0.06))
                            }),
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .opacity(0.)
                            .group_hover(group.clone(), |style| style.opacity(1.))
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
                                    .child(
                                        Icon::new(IconName::Play)
                                            .size_5()
                                            .text_color(Palette::text()),
                                    ),
                            ),
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
                                        .text_color(crate::components::button::on_color(accent)),
                                ),
                        )
                    })
                    .when_some(unplayed, |this, count| {
                        this.child(
                            div()
                                .absolute()
                                .top_2()
                                .right_2()
                                .min_w_6()
                                .h_6()
                                .px_1p5()
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(accent)
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(crate::components::button::on_color(accent))
                                .child(count.to_string()),
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
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .px_0p5()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .group_hover(group.clone(), |style| style.text_color(Palette::text()))
                            .text_color(hsla(0., 0., 0.9, 1.))
                            .child(title),
                    )
                    .when_some(subtitle, |this, subtitle| {
                        this.child(
                            div()
                                .text_xs()
                                .truncate()
                                .text_color(Palette::text_tertiary())
                                .child(subtitle),
                        )
                    }),
            );
        item_menu::attach(card, self.item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(json: serde_json::Value) -> BaseItem {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn episode_captions() {
        let episode = item(serde_json::json!({
            "Id": "e", "Name": "Half Loop", "Type": "Episode", "SeriesName": "Severance",
            "ParentIndexNumber": 1, "IndexNumber": 3
        }));
        assert_eq!(
            captions(&episode, CardShape::Landscape),
            ("Severance".into(), Some("S1:E3 · Half Loop".into()))
        );
        assert_eq!(
            captions(&episode, CardShape::Portrait),
            ("Severance".into(), Some("S1:E3".into()))
        );
    }

    #[test]
    fn movie_in_progress_shows_time_left() {
        let movie = item(serde_json::json!({
            "Id": "m", "Name": "Dune", "Type": "Movie", "ProductionYear": 2024,
            "RunTimeTicks": 60_000_000_000i64,
            "UserData": {"PlayedPercentage": 25.0, "PlaybackPositionTicks": 15_000_000_000i64}
        }));
        assert_eq!(
            captions(&movie, CardShape::Landscape),
            ("Dune".into(), Some("1h 15m left".into()))
        );
    }

    #[test]
    fn movie_without_progress_shows_year() {
        let movie = item(serde_json::json!({
            "Id": "m", "Name": "Dune", "Type": "Movie", "ProductionYear": 2024
        }));
        assert_eq!(
            captions(&movie, CardShape::Portrait),
            ("Dune".into(), Some("2024".into()))
        );
    }
}
