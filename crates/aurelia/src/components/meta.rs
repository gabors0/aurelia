//! The "2024 · 2h 46m · ★ 8.5 · PG-13" line and quality badges.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, FontWeight, SharedString, div, px};
use jellyfin::{BaseItem, ItemKind, StreamKind};

use crate::format;
use crate::nav::Route;
use crate::theme::Palette;
use crate::views::shell;

/// Plain-text facts about an item, in display order.
pub fn facts(item: &BaseItem) -> Vec<SharedString> {
    let mut facts: Vec<SharedString> = Vec::new();
    match item.kind {
        ItemKind::Series => {
            let years = format::years(
                item.production_year,
                item.end_date.as_deref(),
                item.status.as_deref() == Some("Continuing"),
            );
            if !years.is_empty() {
                facts.push(years.into());
            }
            if let Some(count) = item.child_count.filter(|c| *c > 0) {
                facts.push(if count == 1 {
                    "1 season".into()
                } else {
                    format!("{count} seasons").into()
                });
            }
        }
        ItemKind::Episode => {
            let label = format::episode_label(item.parent_index_number, item.index_number);
            if !label.is_empty() {
                facts.push(label.into());
            }
            if let Some(runtime) = item.runtime() {
                facts.push(format::runtime(runtime).into());
            }
        }
        _ => {
            if let Some(year) = item.production_year {
                facts.push(year.to_string().into());
            }
            if let Some(runtime) = item.runtime() {
                facts.push(format::runtime(runtime).into());
            }
        }
    }
    if let Some(rating) = item.community_rating.filter(|r| *r > 0.0) {
        facts.push(format!("★ {rating:.1}").into());
    }
    if let Some(official) = item.official_rating.as_ref().filter(|r| !r.is_empty()) {
        facts.push(official.clone().into());
    }
    facts
}

/// Technical badges from the first media source: 4K, HDR10/DV, Atmos, 5.1.
pub fn badges(item: &BaseItem) -> Vec<SharedString> {
    let mut badges = Vec::new();
    let Some(source) = item.media_sources.first() else {
        return badges;
    };
    if let Some(video) = source.video_stream() {
        let width = video.width.unwrap_or(0);
        if width >= 3200 {
            badges.push("4K".into());
        } else if width >= 1800 {
            badges.push("HD".into());
        }
        match video
            .video_range_type
            .as_deref()
            .or(video.video_range.as_deref())
        {
            Some(range) if range.starts_with("DOVI") => badges.push("Dolby Vision".into()),
            Some("HDR10Plus") => badges.push("HDR10+".into()),
            Some("HDR10" | "HDR" | "HLG") => badges.push("HDR".into()),
            _ => {}
        }
    }
    if let Some(audio) = source
        .streams(StreamKind::Audio)
        .find(|s| s.is_default)
        .or_else(|| source.streams(StreamKind::Audio).next())
    {
        let atmos = audio.audio_spatial_format.as_deref() == Some("DolbyAtmos")
            || audio
                .profile
                .as_deref()
                .is_some_and(|p| p.contains("Atmos"))
            || audio
                .display_title
                .as_deref()
                .is_some_and(|t| t.contains("Atmos"));
        if atmos {
            badges.push("Atmos".into());
        }
        match audio.channels {
            Some(8) => badges.push("7.1".into()),
            Some(6) => badges.push("5.1".into()),
            _ => {}
        }
    }
    badges
}

pub fn line(facts: Vec<SharedString>) -> impl IntoElement {
    let mut row = div()
        .flex()
        .items_center()
        .gap_2()
        .text_color(Palette::text_secondary());
    for (i, fact) in facts.into_iter().enumerate() {
        if i > 0 {
            row = row.child(div().text_color(Palette::text_tertiary()).child("·"));
        }
        row = row.child(fact);
    }
    row
}

/// "Horror · Thriller", each genre a link to its page.
pub fn genre_links(genres: &[String]) -> AnyElement {
    let mut row = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x_2()
        .text_sm()
        .text_color(Palette::text_tertiary());
    for (i, genre) in genres.iter().enumerate() {
        if i > 0 {
            row = row.child("·");
        }
        let route = Route::Genre {
            name: genre.clone(),
        };
        row = row.child(
            div()
                .id(SharedString::from(format!("genre-{genre}")))
                .cursor_pointer()
                .text_color(Palette::text_secondary())
                .hover(|this| this.text_color(Palette::text()).underline())
                .on_click(move |_, window, cx| shell::navigate(route.clone(), window, cx))
                .child(genre.clone()),
        );
    }
    row.into_any_element()
}

pub fn badge_row(badges: Vec<SharedString>) -> AnyElement {
    div()
        .flex()
        .gap_1p5()
        .children(badges.into_iter().map(|badge| {
            div()
                .px_1p5()
                .py(px(1.))
                .rounded(px(4.))
                .border_1()
                .border_color(Palette::border())
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(Palette::text_secondary())
                .child(badge)
        }))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(json: serde_json::Value) -> BaseItem {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn movie_facts() {
        let movie = item(serde_json::json!({
            "Id": "m", "Name": "Dune", "Type": "Movie", "ProductionYear": 2024,
            "RunTimeTicks": 99_600_000_000i64, "CommunityRating": 8.46, "OfficialRating": "PG-13"
        }));
        assert_eq!(facts(&movie), vec!["2024", "2h 46m", "★ 8.5", "PG-13"]);
    }

    #[test]
    fn series_facts() {
        let show = item(serde_json::json!({
            "Id": "s", "Name": "Severance", "Type": "Series", "ProductionYear": 2022,
            "Status": "Continuing", "ChildCount": 2
        }));
        assert_eq!(facts(&show), vec!["2022–present", "2 seasons"]);
    }

    #[test]
    fn badges_from_streams() {
        let movie = item(serde_json::json!({
            "Id": "m", "Name": "X", "Type": "Movie",
            "MediaSources": [{"Id": "a", "MediaStreams": [
                {"Type": "Video", "Index": 0, "Width": 3840, "VideoRangeType": "DOVIWithHDR10"},
                {"Type": "Audio", "Index": 1, "Channels": 8, "IsDefault": true, "AudioSpatialFormat": "DolbyAtmos"}
            ]}]
        }));
        assert_eq!(badges(&movie), vec!["4K", "Dolby Vision", "Atmos", "7.1"]);
    }

    #[test]
    fn minimal_item_has_no_facts_or_badges() {
        let bare = item(serde_json::json!({"Id": "x", "Name": "y", "Type": "Movie"}));
        assert!(facts(&bare).is_empty());
        assert!(badges(&bare).is_empty());
    }
}
