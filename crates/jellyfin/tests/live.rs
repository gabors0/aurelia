//! Smoke test against the public demo server. Run with
//! `cargo test -p jellyfin -- --ignored`.

use jellyfin::{Client, DeviceInfo, ItemKind, ItemsQuery};

const DEMO: &str = "https://demo.jellyfin.org/stable";

#[tokio::test]
#[ignore = "talks to demo.jellyfin.org"]
async fn live_demo_smoke() {
    let device = DeviceInfo::new("Aurelia", "ci", "aurelia-live-test", "0.1.0");
    let anonymous = Client::new(DEMO, device.clone()).unwrap();
    let info = anonymous.public_info().await.unwrap();
    assert!(!info.version.is_empty());

    let auth = anonymous.authenticate_by_name("demo", "").await.unwrap();
    let client = anonymous.with_session(auth.access_token, auth.user.id);

    let views = client.user_views().await.unwrap();
    let movies = views
        .iter()
        .find(|v| v.collection_type.as_deref() == Some("movies"))
        .expect("demo has a movies library");
    let shows = views
        .iter()
        .find(|v| v.collection_type.as_deref() == Some("tvshows"))
        .expect("demo has a shows library");

    client.resume_items(12).await.unwrap();
    client.next_up(None, 12, false).await.unwrap();
    assert!(!client.latest(&movies.id, 8).await.unwrap().is_empty());

    let page = client
        .items(&ItemsQuery {
            parent_id: Some(shows.id.clone()),
            include_item_types: vec![ItemKind::Series],
            limit: 5,
            ..Default::default()
        })
        .await
        .unwrap();
    let series = &page.items[0];
    let detail = client.item(&series.id).await.unwrap();
    assert_eq!(detail.kind, ItemKind::Series);

    let seasons = client.seasons(&series.id).await.unwrap();
    let episodes = client.episodes(&series.id, &seasons[0].id).await.unwrap();
    let episode = client.item(&episodes[0].id).await.unwrap();
    assert!(!episode.media_sources.is_empty());

    let playback = client.playback_info(&episode.id).await.unwrap();
    assert!(!playback.play_session_id.is_empty());
    client.similar(&series.id, 6).await.unwrap();

    // Images resolve without auth.
    let backdrop = detail.backdrop_image().expect("series has a backdrop");
    let response = reqwest::get(client.image_url(&backdrop, 640))
        .await
        .unwrap();
    assert!(response.status().is_success());
}
