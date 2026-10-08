use player::{StreamInfo, TrackKind, mpv_track_id};

fn s(kind: TrackKind, index: i32, external: bool) -> StreamInfo {
    StreamInfo {
        kind,
        index,
        external,
    }
}

#[test]
fn maps_jellyfin_indices_to_mpv_ids() {
    use TrackKind::*;
    let streams = [
        s(Video, 0, false),
        s(Audio, 1, false),
        s(Audio, 2, false),
        s(Subtitle, 3, false),
        s(Subtitle, 4, true),
        s(Subtitle, 5, false),
    ];
    assert_eq!(mpv_track_id(&streams, Audio, 1), Some(1));
    assert_eq!(mpv_track_id(&streams, Audio, 2), Some(2));
    assert_eq!(mpv_track_id(&streams, Subtitle, 3), Some(1));
    assert_eq!(
        mpv_track_id(&streams, Subtitle, 5),
        Some(2),
        "externals don't count"
    );
    assert_eq!(
        mpv_track_id(&streams, Subtitle, 4),
        None,
        "external has no embedded id"
    );
    assert_eq!(mpv_track_id(&streams, Audio, 3), None, "wrong kind");
    assert_eq!(mpv_track_id(&streams, Audio, 99), None);
}

#[test]
fn mapping_uses_container_order_not_list_order() {
    use TrackKind::*;
    // Some servers list external subtitles first.
    let streams = [
        s(Subtitle, 4, true),
        s(Subtitle, 3, false),
        s(Video, 0, false),
        s(Subtitle, 2, false),
        s(Audio, 1, false),
    ];
    assert_eq!(mpv_track_id(&streams, Subtitle, 2), Some(1));
    assert_eq!(mpv_track_id(&streams, Subtitle, 3), Some(2));
}
