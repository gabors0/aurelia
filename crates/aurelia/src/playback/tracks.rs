//! Maps the user's audio/subtitle choice (Jellyfin stream indices) onto what
//! the player understands.

use jellyfin::{MediaSource, MediaStream, StreamKind};
use player::{ExternalSub, StreamInfo, Track, TrackChoice, TrackKind, mpv_track_id};

/// Jellyfin stream indices. `None` = server default; subtitle `Some(-1)` = off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrackSelection {
    pub audio: Option<i32>,
    pub subtitle: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTracks {
    pub audio: TrackChoice,
    pub subtitle: TrackChoice,
    pub external: Vec<ExternalSub>,
    /// For playback reports.
    pub audio_index: Option<i32>,
    pub subtitle_index: i32,
}

pub fn resolve(
    source: &MediaSource,
    selection: TrackSelection,
    subtitle_url: impl Fn(&MediaStream) -> String,
) -> ResolvedTracks {
    let infos = stream_infos(source);

    let audio_index = selection.audio.or(source.default_audio_stream_index);
    let audio = audio_index
        .and_then(|index| mpv_track_id(&infos, TrackKind::Audio, index))
        .map(TrackChoice::Id)
        .unwrap_or(TrackChoice::Auto);

    let subtitle_index = selection
        .subtitle
        .or(source.default_subtitle_stream_index)
        .unwrap_or(-1);
    let chosen = source
        .streams(StreamKind::Subtitle)
        .find(|s| s.index == subtitle_index);
    let subtitle = match chosen {
        None => TrackChoice::Off,
        Some(stream) if stream.is_external => TrackChoice::Off,
        Some(stream) => mpv_track_id(&infos, TrackKind::Subtitle, stream.index)
            .map(TrackChoice::Id)
            .unwrap_or(TrackChoice::Auto),
    };

    let external = source
        .streams(StreamKind::Subtitle)
        .filter(|s| s.is_external)
        .map(|s| ExternalSub {
            url: subtitle_url(s),
            title: s.title.clone().or_else(|| s.display_title.clone()),
            lang: s.language.clone(),
            select: s.index == subtitle_index,
        })
        .collect();

    ResolvedTracks {
        audio,
        subtitle,
        external,
        audio_index,
        subtitle_index: chosen.map(|s| s.index).unwrap_or(-1),
    }
}

/// An audio or subtitle stream the server knows, for the player's menus
/// and for reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownTrack {
    pub kind: TrackKind,
    /// The server's stream index.
    pub index: i32,
    pub label: String,
    /// mpv's id for a stream inside the container.
    pub mpv_id: Option<u32>,
    /// Where an external subtitle was loaded from.
    pub url: Option<String>,
}

/// The source's audio and subtitle streams, matched to what mpv will call
/// them (the same mapping `resolve` uses to pick them).
pub fn known_tracks(
    source: &MediaSource,
    subtitle_url: impl Fn(&MediaStream) -> String,
) -> Vec<KnownTrack> {
    let infos = stream_infos(source);
    source
        .media_streams
        .iter()
        .filter_map(|stream| {
            let kind = match stream.kind {
                StreamKind::Audio => TrackKind::Audio,
                StreamKind::Subtitle => TrackKind::Subtitle,
                _ => return None,
            };
            Some(KnownTrack {
                kind,
                index: stream.index,
                label: label(stream),
                mpv_id: (!stream.is_external)
                    .then(|| mpv_track_id(&infos, kind, stream.index))
                    .flatten(),
                url: stream.is_external.then(|| subtitle_url(stream)),
            })
        })
        .collect()
}

/// What the server calls one of the player's tracks.
pub fn find<'a>(track: &Track, known: &'a [KnownTrack]) -> Option<&'a KnownTrack> {
    if track.external {
        let url = track.external_filename.as_deref()?;
        known.iter().find(|k| k.url.as_deref() == Some(url))
    } else {
        known
            .iter()
            .find(|k| Some(k.kind) == track.kind && k.mpv_id == Some(track.id))
    }
}

/// A menu entry for one of the player's tracks: the server's title when it
/// knows the track, else what the player says about it.
pub fn menu_label(track: &Track, known: &[KnownTrack]) -> String {
    if let Some(known) = find(track, known) {
        return known.label.clone();
    }
    track
        .title
        .clone()
        .or_else(|| {
            let parts: Vec<String> = [track.lang.as_deref(), track.codec.as_deref()]
                .into_iter()
                .flatten()
                .map(str::to_uppercase)
                .collect();
            (!parts.is_empty()).then(|| parts.join(" · "))
        })
        .unwrap_or_else(|| format!("Track {}", track.id))
}

/// The server's stream indices for what the player has selected, for
/// playback reports: audio, and subtitle (-1 for none). `None` where the
/// selected track isn't one the server knows.
pub fn reported_indices(tracks: &[Track], known: &[KnownTrack]) -> (Option<i32>, Option<i32>) {
    let selected = |kind| tracks.iter().find(|t| t.kind == Some(kind) && t.selected);
    let audio = selected(TrackKind::Audio)
        .and_then(|t| find(t, known))
        .map(|k| k.index);
    let subtitle = match selected(TrackKind::Subtitle) {
        None => Some(-1),
        Some(track) => find(track, known).map(|k| k.index),
    };
    (audio, subtitle)
}

fn stream_infos(source: &MediaSource) -> Vec<StreamInfo> {
    source
        .media_streams
        .iter()
        .filter_map(|s| {
            let kind = match s.kind {
                StreamKind::Video => TrackKind::Video,
                StreamKind::Audio => TrackKind::Audio,
                StreamKind::Subtitle => TrackKind::Subtitle,
                _ => return None,
            };
            Some(StreamInfo {
                kind,
                index: s.index,
                external: s.is_external,
            })
        })
        .collect()
}

/// Human label for a picker entry.
pub fn label(stream: &MediaStream) -> String {
    let base = stream
        .display_title
        .clone()
        .or_else(|| stream.title.clone())
        .unwrap_or_else(|| {
            let parts: Vec<String> = [stream.language.as_deref(), stream.codec.as_deref()]
                .into_iter()
                .flatten()
                .map(str::to_uppercase)
                .collect();
            if parts.is_empty() {
                format!("Track {}", stream.index)
            } else {
                parts.join(" · ")
            }
        });
    if stream.is_external {
        format!("{base} (external)")
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> MediaSource {
        serde_json::from_value(serde_json::json!({
            "Id": "src",
            "DefaultAudioStreamIndex": 2,
            "DefaultSubtitleStreamIndex": -1,
            "MediaStreams": [
                {"Type": "Video", "Index": 0},
                {"Type": "Audio", "Index": 1, "Language": "jpn", "DisplayTitle": "Japanese - AAC - Stereo"},
                {"Type": "Audio", "Index": 2, "Language": "eng", "DisplayTitle": "English - AC3 - 5.1"},
                {"Type": "Subtitle", "Index": 3, "Language": "eng", "Codec": "subrip"},
                {"Type": "Subtitle", "Index": 4, "Language": "hun", "Codec": "ass", "IsExternal": true, "Title": "Magyar"}
            ]
        }))
        .unwrap()
    }

    fn url(stream: &MediaStream) -> String {
        format!("https://jf/sub/{}", stream.index)
    }

    #[test]
    fn defaults_follow_the_server() {
        let resolved = resolve(&source(), TrackSelection::default(), url);
        assert_eq!(resolved.audio, TrackChoice::Id(2));
        assert_eq!(resolved.subtitle, TrackChoice::Off);
        assert_eq!(resolved.audio_index, Some(2));
        assert_eq!(resolved.subtitle_index, -1);
        assert_eq!(resolved.external.len(), 1, "externals are always offered");
        assert!(!resolved.external[0].select);
    }

    #[test]
    fn embedded_subtitle_maps_to_mpv_id() {
        let selection = TrackSelection {
            audio: Some(1),
            subtitle: Some(3),
        };
        let resolved = resolve(&source(), selection, url);
        assert_eq!(resolved.audio, TrackChoice::Id(1));
        assert_eq!(resolved.subtitle, TrackChoice::Id(1));
        assert_eq!(resolved.subtitle_index, 3);
    }

    #[test]
    fn external_subtitle_is_added_and_selected() {
        let selection = TrackSelection {
            audio: None,
            subtitle: Some(4),
        };
        let resolved = resolve(&source(), selection, url);
        assert_eq!(
            resolved.subtitle,
            TrackChoice::Off,
            "the sidecar is selected instead"
        );
        assert_eq!(
            resolved.external,
            vec![ExternalSub {
                url: "https://jf/sub/4".into(),
                title: Some("Magyar".into()),
                lang: Some("hun".into()),
                select: true,
            }]
        );
    }

    #[test]
    fn unknown_audio_index_falls_back_to_auto() {
        let selection = TrackSelection {
            audio: Some(42),
            subtitle: None,
        };
        assert_eq!(resolve(&source(), selection, url).audio, TrackChoice::Auto);
    }

    fn mpv_track(kind: TrackKind, id: u32) -> Track {
        Track {
            id,
            kind: Some(kind),
            ..Track::default()
        }
    }

    #[test]
    fn known_tracks_match_mpv_ids_and_urls() {
        let known = known_tracks(&source(), url);
        assert_eq!(known.len(), 4, "audio and subtitles only");
        let japanese = mpv_track(TrackKind::Audio, 1);
        assert_eq!(find(&japanese, &known).unwrap().index, 1);
        assert_eq!(menu_label(&japanese, &known), "Japanese - AAC - Stereo");
        let embedded_sub = mpv_track(TrackKind::Subtitle, 1);
        assert_eq!(find(&embedded_sub, &known).unwrap().index, 3);
        let sidecar = Track {
            external: true,
            external_filename: Some("https://jf/sub/4".into()),
            ..mpv_track(TrackKind::Subtitle, 2)
        };
        assert_eq!(find(&sidecar, &known).unwrap().index, 4);
        assert_eq!(menu_label(&sidecar, &known), "Magyar (external)");
    }

    #[test]
    fn unknown_tracks_describe_themselves() {
        let stranger = Track {
            lang: Some("fre".into()),
            codec: Some("opus".into()),
            ..mpv_track(TrackKind::Audio, 9)
        };
        assert_eq!(menu_label(&stranger, &[]), "FRE · OPUS");
        assert_eq!(menu_label(&mpv_track(TrackKind::Audio, 9), &[]), "Track 9");
    }

    #[test]
    fn reports_follow_the_selection() {
        let known = known_tracks(&source(), url);
        let mut tracks = vec![
            mpv_track(TrackKind::Audio, 1),
            mpv_track(TrackKind::Audio, 2),
            mpv_track(TrackKind::Subtitle, 1),
        ];
        tracks[1].selected = true;
        assert_eq!(reported_indices(&tracks, &known), (Some(2), Some(-1)));
        tracks[2].selected = true;
        assert_eq!(reported_indices(&tracks, &known), (Some(2), Some(3)));
        let unknown = vec![Track {
            selected: true,
            ..mpv_track(TrackKind::Subtitle, 7)
        }];
        assert_eq!(reported_indices(&unknown, &known), (None, None));
    }

    #[test]
    fn labels() {
        let s = source();
        assert_eq!(label(&s.media_streams[2]), "English - AC3 - 5.1");
        assert_eq!(label(&s.media_streams[3]), "ENG · SUBRIP");
        assert_eq!(label(&s.media_streams[4]), "Magyar (external)");
    }
}
