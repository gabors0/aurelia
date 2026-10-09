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

    let genres = client
        .genres(None, &[ItemKind::Movie, ItemKind::Series])
        .await
        .unwrap();
    assert!(!genres.is_empty());
    let in_genre = client
        .items(&ItemsQuery {
            include_item_types: vec![ItemKind::Movie, ItemKind::Series],
            genres: vec![genres[0].name.clone()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(in_genre.total_record_count > 0);
    let found = client
        .items(&ItemsQuery {
            search_term: Some(series.name.clone()),
            include_item_types: vec![ItemKind::Series],
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(found.items.iter().any(|i| i.id == series.id));
    if let Some(person) = detail.people.first() {
        client.persons(&person.name, 5).await.unwrap();
        let credits = client
            .items(&ItemsQuery {
                person_ids: vec![person.id.clone()],
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(credits.total_record_count > 0);
    }

    // Images resolve without auth.
    let backdrop = detail.backdrop_image().expect("series has a backdrop");
    let response = reqwest::get(client.image_url(&backdrop, 640))
        .await
        .unwrap();
    assert!(response.status().is_success());
}
