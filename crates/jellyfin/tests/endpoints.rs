use jellyfin::{
    BaseItem, Client, DeviceInfo, ImageKind, ImageRef, ItemFilter, ItemKind, ItemsQuery,
    PlaybackReport, ProgressEvent, SortBy, SortOrder, Ticks,
};
use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn client(uri: &str) -> Client {
    Client::new(uri, DeviceInfo::new("Aurelia", "box", "dev", "0.1.0"))
        .unwrap()
        .with_session("tok".into(), "user1".into())
}

#[tokio::test]
async fn authenticate_by_name_posts_credentials() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/Users/AuthenticateByName"))
        .and(body_partial_json(
            json!({"Username": "demo", "Pw": "secret"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("auth.json")))
        .expect(1)
        .mount(&server)
        .await;
    let anonymous = Client::new(&server.uri(), DeviceInfo::new("A", "b", "c", "d")).unwrap();
    let auth = anonymous
        .authenticate_by_name("demo", "secret")
        .await
        .unwrap();
    assert_eq!(auth.user.name, "demo");
}

#[tokio::test]
async fn items_query_encodes_params() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Items"))
        .and(query_param("userId", "user1"))
        .and(query_param("parentId", "lib1"))
        .and(query_param("recursive", "true"))
        .and(query_param("includeItemTypes", "Movie,Series"))
        .and(query_param("sortBy", "DateCreated,SortName"))
        .and(query_param("sortOrder", "Descending"))
        .and(query_param("filters", "IsUnplayed"))
        .and(query_param("startIndex", "100"))
        .and(query_param("limit", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("items_movies.json")))
        .expect(1)
        .mount(&server)
        .await;

    let query = ItemsQuery {
        parent_id: Some("lib1".into()),
        include_item_types: vec![ItemKind::Movie, ItemKind::Series],
        sort_by: SortBy::DateAdded,
        sort_order: SortOrder::Descending,
        filter: ItemFilter::Unplayed,
        start_index: 100,
        limit: 50,
        ..Default::default()
    };
    let page = client(&server.uri()).items(&query).await.unwrap();
    assert_eq!(page.total_record_count, 11);
}

#[tokio::test]
async fn items_query_encodes_search_genres_and_people() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Items"))
        .and(query_param("searchTerm", "night of"))
        .and(query_param("genres", "Horror|Sci-Fi & Fantasy"))
        .and(query_param("personIds", "p1,p2"))
        .and(query_param("recursive", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("items_movies.json")))
        .expect(1)
        .mount(&server)
        .await;
    let query = ItemsQuery {
        search_term: Some("  night of ".into()),
        genres: vec!["Horror".into(), "Sci-Fi & Fantasy".into()],
        person_ids: vec!["p1".into(), "p2".into()],
        ..Default::default()
    };
    client(&server.uri()).items(&query).await.unwrap();
}

#[tokio::test]
async fn collection_titles_are_not_recursive() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Items"))
        .and(query_param("parentId", "boxset1"))
        .and(query_param("recursive", "false"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("items_movies.json")))
        .expect(1)
        .mount(&server)
        .await;
    let query = ItemsQuery {
        parent_id: Some("boxset1".into()),
        recursive: false,
        ..Default::default()
    };
    client(&server.uri()).items(&query).await.unwrap();
}

#[tokio::test]
async fn blank_search_term_is_left_out() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Items"))
        .and(query_param_is_missing("searchTerm"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("items_movies.json")))
        .expect(1)
        .mount(&server)
        .await;
    let query = ItemsQuery {
        search_term: Some("   ".into()),
        ..Default::default()
    };
    client(&server.uri()).items(&query).await.unwrap();
}

#[tokio::test]
async fn persons_search() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Persons"))
        .and(query_param("userId", "user1"))
        .and(query_param("searchTerm", "chris"))
        .and(query_param("limit", "12"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("persons.json")))
        .expect(1)
        .mount(&server)
        .await;
    let people = client(&server.uri()).persons("chris ", 12).await.unwrap();
    assert_eq!(people[0].kind, ItemKind::Person);
    assert!(people[0].primary_image().is_some());
}

#[tokio::test]
async fn genres_for_a_library() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Genres"))
        .and(query_param("parentId", "lib1"))
        .and(query_param("includeItemTypes", "Movie,Series"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("genres.json")))
        .expect(1)
        .mount(&server)
        .await;
    let genres = client(&server.uri())
        .genres(Some("lib1"), &[ItemKind::Movie, ItemKind::Series])
        .await
        .unwrap();
    assert_eq!(genres[0].name, "Action");
    assert_eq!(genres[0].kind, ItemKind::Genre);
    assert_eq!(genres[0].movie_count, Some(1));
    assert_eq!(genres[0].series_count, Some(0));
}

#[tokio::test]
async fn latest_parses_bare_array() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Items/Latest"))
        .and(query_param("parentId", "shows"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("latest_shows.json")))
        .mount(&server)
        .await;
    let items = client(&server.uri()).latest("shows", 16).await.unwrap();
    assert!(!items.is_empty());
}

#[tokio::test]
async fn set_played_uses_post_and_delete() {
    let server = MockServer::start().await;
    let user_data = r#"{"Played":true,"IsFavorite":false,"PlaybackPositionTicks":0}"#;
    Mock::given(method("POST"))
        .and(path("/UserPlayedItems/item9"))
        .and(query_param("userId", "user1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(user_data))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/UserPlayedItems/item9"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"Played":false,"PlaybackPositionTicks":0}"#),
        )
        .expect(1)
        .mount(&server)
        .await;
    let c = client(&server.uri());
    assert!(c.set_played("item9", true).await.unwrap().played);
    assert!(!c.set_played("item9", false).await.unwrap().played);
}

#[tokio::test]
async fn set_favorite_uses_post() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/UserFavoriteItems/item9"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"IsFavorite":true,"PlaybackPositionTicks":0}"#),
        )
        .expect(1)
        .mount(&server)
        .await;
    assert!(
        client(&server.uri())
            .set_favorite("item9", true)
            .await
            .unwrap()
            .is_favorite
    );
}

#[tokio::test]
async fn report_progress_posts_json() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/Sessions/Playing/Progress"))
        .and(body_partial_json(json!({
            "ItemId": "item9",
            "MediaSourceId": "src",
            "PlaySessionId": "sess",
            "PositionTicks": 900_000_000i64,
            "IsPaused": true,
            "PlayMethod": "DirectPlay",
            "CanSeek": true,
            "AudioStreamIndex": 1,
            "SubtitleStreamIndex": -1,
            "EventName": "pause",
        })))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let mut report = PlaybackReport::new("item9", "src", "sess");
    report.position = Ticks(900_000_000);
    report.is_paused = true;
    report.audio_stream_index = Some(1);
    report.subtitle_stream_index = Some(-1);
    report.event = Some(ProgressEvent::Pause);
    client(&server.uri())
        .report_progress(&report)
        .await
        .unwrap();
}

#[tokio::test]
async fn playback_info_posts_with_user() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/Items/item9/PlaybackInfo"))
        .and(query_param("userId", "user1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("playback_info.json")))
        .expect(1)
        .mount(&server)
        .await;
    let info = client(&server.uri()).playback_info("item9").await.unwrap();
    assert!(!info.play_session_id.is_empty());
}

#[test]
fn image_url_shape() {
    let c = client("https://jf.example.com/base");
    let backdrop = ImageRef {
        item_id: "abc".into(),
        kind: ImageKind::Backdrop(0),
        tag: "t1".into(),
        blurhash: None,
    };
    assert_eq!(
        c.image_url(&backdrop, 1920).as_str(),
        "https://jf.example.com/base/Items/abc/Images/Backdrop/0?tag=t1&maxWidth=1920&quality=90"
    );
    let logo = ImageRef {
        kind: ImageKind::Logo,
        ..backdrop
    };
    assert_eq!(
        c.image_url(&logo, 600).as_str(),
        "https://jf.example.com/base/Items/abc/Images/Logo?tag=t1&maxWidth=600&quality=90"
    );
}

#[test]
fn stream_url_has_static_true() {
    let c = client("https://jf.example.com");
    let url = c.stream_url("item9", "src", "sess");
    assert_eq!(url.path(), "/Videos/item9/stream");
    let query: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    assert!(query.contains(&("static".into(), "true".into())));
    assert!(query.contains(&("mediaSourceId".into(), "src".into())));
    assert!(query.contains(&("playSessionId".into(), "sess".into())));
    assert!(
        !url.as_str().contains("tok"),
        "token must never appear in URLs"
    );
}

#[test]
fn subtitle_url_maps_codec_to_format() {
    let c = client("https://jf.example.com");
    assert_eq!(
        c.subtitle_url("item9", "src", 3, Some("subrip")).path(),
        "/Videos/item9/src/Subtitles/3/Stream.srt"
    );
    assert_eq!(
        c.subtitle_url("item9", "src", 4, Some("ass")).path(),
        "/Videos/item9/src/Subtitles/4/Stream.ass"
    );
}

#[test]
fn backdrop_falls_back_to_parent() {
    let episode: BaseItem = serde_json::from_str(&fixture("item_episode.json")).unwrap();
    let backdrop = episode.backdrop_image().expect("episode inherits backdrop");
    assert_eq!(
        Some(backdrop.item_id.as_str()),
        episode.series_id.as_deref()
    );
    assert!(backdrop.blurhash.is_some() || episode.image_blur_hashes.is_empty());
}
