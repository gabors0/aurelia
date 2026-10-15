//! Starting playback and reporting it to the server.

mod reporter;
mod tracks;

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::{App, AppContext as _, Entity, Global, SharedString, Window};
use jellyfin::{BaseItem, Client, ItemKind, PlaybackReport, Ticks};
use player::{
    EndReason, PlayRequest, PlaybackControl, PlaybackHandle, PlayerEvent, VideoSurface, Volume,
};

use self::reporter::{ReportKind, Reporter};
pub use self::tracks::{KnownTrack, TrackSelection, label as track_label, menu_label};
use crate::format;
use crate::runtime;
use crate::settings::{self, PlayerChoice};
use crate::state::AppState;
use crate::views::player::{PlayerInfo, PlayerView};
use crate::views::shell;

const NO_MPV: &str =
    "Aurelia plays video with mpv, but mpv wasn't found. Install mpv or set AURELIA_MPV.";

/// The session currently playing; starting another stops it.
#[derive(Default)]
struct NowPlaying(Option<Arc<dyn PlaybackControl>>);

impl Global for NowPlaying {}

/// Plays `item`, resuming unless `from_start`. Series and seasons start
/// their next (or first) episode.
pub fn play_item(item: &BaseItem, from_start: bool, window: &mut Window, cx: &mut App) {
    match item.kind {
        ItemKind::Series | ItemKind::Season => {
            let client = AppState::client(cx);
            let (series_id, season_id) = match item.kind {
                ItemKind::Series => (item.id.clone(), None),
                _ => (
                    item.series_id.clone().unwrap_or_default(),
                    Some(item.id.clone()),
                ),
            };
            let find = runtime::api(cx, async move {
                next_episode(&client, &series_id, season_id.as_deref()).await
            });
            window
                .spawn(cx, async move |cx| match find.await {
                    Ok(Some(episode)) => {
                        cx.update(|window, cx| {
                            play(episode, TrackSelection::default(), from_start, window, cx)
                        })
                        .ok();
                    }
                    Ok(None) => {
                        cx.update(|window, cx| {
                            shell::notify("There's nothing to play yet.", window, cx)
                        })
                        .ok();
                    }
                    Err(err) => {
                        cx.update(|window, cx| shell::report_error(&err, window, cx))
                            .ok();
                    }
                })
                .detach();
        }
        _ => play(
            item.id.clone(),
            TrackSelection::default(),
            from_start,
            window,
            cx,
        ),
    }
}

/// The episode to start a show (or season) with: Next Up, else the first
/// unwatched, else the first.
async fn next_episode(
    client: &Client,
    series_id: &str,
    season_id: Option<&str>,
) -> jellyfin::Result<Option<String>> {
    if season_id.is_none()
        && let Some(next) = client.next_up(Some(series_id), 1, true).await?.first()
    {
        return Ok(Some(next.id.clone()));
    }
    let season_id = match season_id {
        Some(id) => id.to_string(),
        None => match client.seasons(series_id).await?.first() {
            Some(season) => season.id.clone(),
            None => return Ok(None),
        },
    };
    let episodes = client.episodes(series_id, &season_id).await?;
    Ok(episodes
        .iter()
        .find(|e| !e.is_played())
        .or(episodes.first())
        .map(|e| e.id.clone()))
}

/// Window title for the player.
fn media_title(item: &BaseItem) -> String {
    match (item.kind, &item.series_name) {
        (ItemKind::Episode, Some(series)) => {
            let label = format::episode_label(item.parent_index_number, item.index_number);
            if label.is_empty() {
                format!("{series} — {}", item.name)
            } else {
                format!("{series} — {label} · {}", item.name)
            }
        }
        _ => match item.production_year {
            Some(year) => format!("{} ({year})", item.name),
            None => item.name.clone(),
        },
    }
}

/// The player's heading: the movie or show, then the episode or year.
fn player_heading(item: &BaseItem) -> (SharedString, Option<SharedString>) {
    match (item.kind, &item.series_name) {
        (ItemKind::Episode, Some(series)) => {
            let label = format::episode_label(item.parent_index_number, item.index_number);
            let detail = if label.is_empty() {
                item.name.clone()
            } else {
                format!("{label} · {}", item.name)
            };
            (series.clone().into(), Some(detail.into()))
        }
        _ => (
            item.name.clone().into(),
            item.production_year.map(|year| year.to_string().into()),
        ),
    }
}

/// Starts `request` inside the window when the settings ask for it and it
/// works, else in mpv's own window. `None` when nothing could start (the
/// user has been told why).
fn start_playback(
    request: PlayRequest,
    info: PlayerInfo,
    window: &mut Window,
    cx: &mut App,
) -> Option<(PlaybackHandle, Option<Entity<PlayerView>>)> {
    if settings::get(cx).player == PlayerChoice::Embedded {
        match start_embedded(request.clone(), window, cx) {
            Ok((handle, surface)) => {
                let control = handle.control();
                let view = cx.new(|cx| PlayerView::new(info, control, Some(surface), cx));
                shell::open_player(view.clone(), window, cx);
                return Some((handle, Some(view)));
            }
            Err(reason) => {
                if AppState::embedding_failed(cx, reason.clone()) {
                    shell::notify(
                        format!("Playing in a separate mpv window: {reason}."),
                        window,
                        cx,
                    );
                }
            }
        }
    }
    let Some(external) = AppState::global(cx).player() else {
        shell::report_message(NO_MPV, window, cx);
        return None;
    };
    match external.play(request) {
        Ok(handle) => Some((handle, None)),
        Err(err) => {
            shell::report_message(err.to_string(), window, cx);
            None
        }
    }
}

/// Starts `request` inside `window`.
pub(crate) fn start_embedded(
    request: PlayRequest,
    window: &mut Window,
    cx: &mut App,
) -> Result<(PlaybackHandle, Arc<VideoSurface>), String> {
    let player = AppState::embedded(cx)?;
    let surface = VideoSurface::attach(&*window).map_err(|err| err.to_string())?;
    let handle = player
        .play(request, surface.clone())
        .map_err(|err| err.to_string())?;
    Ok((handle, surface))
}

/// Plays one movie or episode with the given tracks.
pub fn play(
    item_id: String,
    selection: TrackSelection,
    from_start: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let external = settings::get(cx).player == PlayerChoice::External;
    if external && AppState::global(cx).player().is_none() {
        shell::report_message(NO_MPV, window, cx);
        return;
    }
    let client = AppState::client(cx);
    let fetch_client = client.clone();
    let prepare = runtime::api(cx, async move {
        let item = fetch_client.item(&item_id).await?;
        let info = fetch_client.playback_info(&item_id).await?;
        Ok((item, info))
    });

    window
        .spawn(cx, async move |cx| {
            let (item, info) = match prepare.await {
                Ok(prepared) => prepared,
                Err(err) => {
                    cx.update(|window, cx| shell::report_error(&err, window, cx))
                        .ok();
                    return;
                }
            };
            let Some(source) = info
                .media_sources
                .first()
                .or(item.media_sources.first())
                .cloned()
            else {
                cx.update(|window, cx| {
                    shell::report_message("This item has no playable media.", window, cx)
                })
                .ok();
                return;
            };

            let subtitle_url = |stream: &jellyfin::MediaStream| {
                client
                    .subtitle_url(&item.id, &source.id, stream.index, stream.codec.as_deref())
                    .to_string()
            };
            let tracks = tracks::resolve(&source, selection, subtitle_url);
            let known = tracks::known_tracks(&source, subtitle_url);
            let Ok(saved) = cx.update(|_, cx| {
                let settings = settings::get(cx);
                Volume {
                    level: settings.volume,
                    muted: settings.muted,
                }
            }) else {
                return;
            };
            let start = if from_start {
                Duration::ZERO
            } else {
                item.resume_position().unwrap_or_default()
            };
            let request = PlayRequest {
                url: client
                    .stream_url(&item.id, &source.id, &info.play_session_id)
                    .to_string(),
                title: media_title(&item),
                start,
                http_headers: client.token_header().into_iter().collect(),
                audio: tracks.audio,
                subtitle: tracks.subtitle,
                external_subtitles: tracks.external.clone(),
                volume: Some(saved),
            };
            tracing::info!("playing {:?} from {:?}", request.title, request.start);
            let (title, detail) = player_heading(&item);
            let player_info = PlayerInfo {
                title,
                detail,
                runtime: item.runtime(),
                known_tracks: known.clone(),
                volume: saved,
            };
            let Ok(Some((handle, view))) =
                cx.update(|window, cx| start_playback(request, player_info, window, cx))
            else {
                return;
            };
            let control = handle.control();
            cx.update(|_, cx| {
                let previous = cx.default_global::<NowPlaying>().0.replace(control.clone());
                if let Some(previous) = previous {
                    previous.stop();
                }
            })
            .ok();

            let mut reporter = Reporter::new(start, item.runtime());
            let mut base = PlaybackReport::new(&item.id, &source.id, &info.play_session_id);
            base.audio_stream_index = tracks.audio_index;
            base.subtitle_stream_index = Some(tracks.subtitle_index);

            let mut volume = saved;
            let events = handle.events().clone();
            while let Ok(event) = events.recv().await {
                let ended = matches!(event, PlayerEvent::Ended { .. });
                if let Some(view) = &view {
                    cx.update(|_, cx| view.update(cx, |view, cx| view.on_event(&event, cx)))
                        .ok();
                }
                match &event {
                    PlayerEvent::Tracks(list) => {
                        let (audio, subtitle) = tracks::reported_indices(list, &known);
                        base.audio_stream_index = audio.or(base.audio_stream_index);
                        base.subtitle_stream_index = subtitle.or(base.subtitle_stream_index);
                    }
                    PlayerEvent::Volume(now) => volume = *now,
                    _ => {}
                }
                if let PlayerEvent::Ended {
                    reason: EndReason::Error(message),
                    ..
                } = &event
                {
                    let message = format!("Playback failed: {message}");
                    cx.update(|window, cx| shell::report_message(message, window, cx))
                        .ok();
                }
                for report in reporter.on_event(&event, Instant::now()) {
                    let mut body = base.clone();
                    body.position = Ticks::from_duration(report.position);
                    body.is_paused = report.paused;
                    body.event = match report.kind {
                        ReportKind::Progress(event) => event,
                        _ => None,
                    };
                    let client = client.clone();
                    let Ok(send) = cx.update(|_, cx| {
                        runtime::api(cx, async move {
                            match report.kind {
                                ReportKind::Start => client.report_start(&body).await,
                                ReportKind::Progress(_) => client.report_progress(&body).await,
                                ReportKind::Stopped => client.report_stopped(&body).await,
                            }
                        })
                    }) else {
                        continue;
                    };
                    // Progress reports run in the background; the final one is
                    // awaited so the refresh below sees the new resume point.
                    if ended {
                        match send.await {
                            Ok(()) => tracing::info!("reported stop at {:?}", report.position),
                            Err(err) => tracing::warn!("playback stop report failed: {err}"),
                        }
                    }
                }
                if ended {
                    break;
                }
            }

            cx.update(|window, cx| {
                let now = cx.default_global::<NowPlaying>();
                if now.0.as_ref().is_some_and(|c| Arc::ptr_eq(c, &control)) {
                    now.0 = None;
                }
                if volume != saved {
                    settings::update(cx, |s| {
                        s.volume = volume.level;
                        s.muted = volume.muted;
                    });
                }
                if let Some(view) = &view {
                    view.update(cx, |view, cx| view.close(false, window, cx));
                }
                shell::refresh_all(window, cx);
            })
            .ok();
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_headings() {
        let episode: BaseItem = serde_json::from_value(serde_json::json!({
            "Id": "e", "Name": "Half Loop", "Type": "Episode", "SeriesName": "Severance",
            "ParentIndexNumber": 1, "IndexNumber": 3
        }))
        .unwrap();
        assert_eq!(
            player_heading(&episode),
            ("Severance".into(), Some("S1:E3 · Half Loop".into()))
        );
        let movie: BaseItem = serde_json::from_value(serde_json::json!({
            "Id": "m", "Name": "Dune", "Type": "Movie", "ProductionYear": 2024
        }))
        .unwrap();
        assert_eq!(player_heading(&movie), ("Dune".into(), Some("2024".into())));
    }

    #[test]
    fn titles() {
        let episode: BaseItem = serde_json::from_value(serde_json::json!({
            "Id": "e", "Name": "Half Loop", "Type": "Episode", "SeriesName": "Severance",
            "ParentIndexNumber": 1, "IndexNumber": 3
        }))
        .unwrap();
        assert_eq!(media_title(&episode), "Severance — S1:E3 · Half Loop");
        let movie: BaseItem = serde_json::from_value(serde_json::json!({
            "Id": "m", "Name": "Dune", "Type": "Movie", "ProductionYear": 2024
        }))
        .unwrap();
        assert_eq!(media_title(&movie), "Dune (2024)");
    }
}
