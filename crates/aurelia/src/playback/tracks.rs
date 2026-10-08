//! Maps the user's audio/subtitle choice (Jellyfin stream indices) onto what
//! the player understands.

use jellyfin::{MediaSource, MediaStream, StreamKind};
use player::{ExternalSub, StreamInfo, TrackChoice, TrackKind, mpv_track_id};

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
    let infos: Vec<StreamInfo> = source
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
        .collect();

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

    #[test]
    fn labels() {
        let s = source();
        assert_eq!(label(&s.media_streams[2]), "English - AC3 - 5.1");
        assert_eq!(label(&s.media_streams[3]), "ENG · SUBRIP");
        assert_eq!(label(&s.media_streams[4]), "Magyar (external)");
    }
}
