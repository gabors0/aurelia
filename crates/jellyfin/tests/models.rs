use std::time::Duration;

use jellyfin::{
    AuthResult, BaseItem, ItemKind, ItemsPage, PlaybackInfo, PublicSystemInfo, StreamKind, Ticks,
    User, UserView, normalize_server_url,
};
use serde::de::DeserializeOwned;

fn fixture<T: DeserializeOwned>(name: &str) -> T {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn deserializes_fixtures() {
    let info: PublicSystemInfo = fixture("public_info.json");
    assert_eq!(info.server_name, "Stable Demo");
    assert!(info.version.starts_with("12."));

    let auth: AuthResult = fixture("auth.json");
    assert_eq!(auth.user.name, "demo");
    assert_eq!(auth.access_token.len(), 32);

    let _me: User = fixture("me.json");

    let views: ItemsPage<UserView> = fixture("views.json");
    let kinds: Vec<_> = views
        .items
        .iter()
        .filter_map(|v| v.collection_type.as_deref())
        .collect();
    assert!(kinds.contains(&"movies") && kinds.contains(&"tvshows"));

    for name in [
        "resume.json",
        "nextup.json",
        "items_movies.json",
        "seasons.json",
        "episodes.json",
    ] {
        let page: ItemsPage<BaseItem> = fixture(name);
        assert!(!page.items.is_empty(), "{name} should have items");
    }
    let latest: Vec<BaseItem> = fixture("latest_shows.json");
    assert!(!latest.is_empty());

    let movie: BaseItem = fixture("item_movie.json");
    assert_eq!(movie.kind, ItemKind::Movie);
    let source = &movie.media_sources[0];
    assert!(!source.media_streams.is_empty());
    assert!(
        source
            .media_streams
            .iter()
            .any(|s| s.kind == StreamKind::Audio)
    );
    assert_eq!(source.default_audio_stream_index, Some(1));
    assert!(!movie.people.is_empty());

    let series: BaseItem = fixture("item_series.json");
    assert_eq!(series.kind, ItemKind::Series);

    let episode: BaseItem = fixture("item_episode.json");
    assert_eq!(episode.kind, ItemKind::Episode);
    assert_eq!(episode.index_number, Some(1));
    assert_eq!(episode.parent_index_number, Some(1));
    assert!(episode.series_id.is_some());

    let playback: PlaybackInfo = fixture("playback_info.json");
    assert!(!playback.play_session_id.is_empty());
    assert!(!playback.media_sources.is_empty());
}

#[test]
fn minimal_item_deserializes() {
    let item: BaseItem = fixture("item_minimal.json");
    assert_eq!(item.id, "x");
    assert_eq!(item.name, "Untitled");
    assert_eq!(item.kind, ItemKind::Movie);
    assert!(item.user_data.is_none());
    assert!(item.run_time_ticks.is_none());
    assert!(item.backdrop_image().is_none());
    assert!(item.media_sources.is_empty());
}

#[test]
fn unknown_item_type_is_other() {
    let item: BaseItem =
        serde_json::from_str(r#"{"Id":"a","Name":"b","Type":"MusicAlbum"}"#).unwrap();
    assert_eq!(item.kind, ItemKind::Other);
}

#[test]
fn ticks_roundtrip() {
    assert_eq!(Ticks(15_918_112_740).to_duration().as_secs(), 1591);
    assert_eq!(
        Ticks::from_duration(Duration::from_secs(90)),
        Ticks(900_000_000)
    );
    assert_eq!(Ticks::ZERO.to_duration(), Duration::ZERO);
    assert_eq!(Ticks(-5).to_duration(), Duration::ZERO);
}

#[test]
fn normalizes_server_urls() {
    for input in [
        "jellyfin.gs0.me",
        "https://jellyfin.gs0.me/",
        "https://jellyfin.gs0.me/web/index.html",
        "https://jellyfin.gs0.me/web/#/home",
        "  https://jellyfin.gs0.me  ",
    ] {
        assert_eq!(
            normalize_server_url(input).unwrap().as_str(),
            "https://jellyfin.gs0.me/",
            "input: {input:?}"
        );
    }
    assert_eq!(
        normalize_server_url("http://10.0.0.2:8096/jf/")
            .unwrap()
            .as_str(),
        "http://10.0.0.2:8096/jf/"
    );
    assert_eq!(
        normalize_server_url("192.168.1.5:8096").unwrap().as_str(),
        "http://192.168.1.5:8096/"
    );
    assert!(normalize_server_url("").is_err());
    assert!(normalize_server_url("ftp://host").is_err());
}
