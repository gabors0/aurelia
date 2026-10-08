use jellyfin::{Client, DeviceInfo, Error};
use wiremock::matchers::{header_regex, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn device() -> DeviceInfo {
    DeviceInfo::new("Aurelia", "testbox", "dev-123", "0.1.0")
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[tokio::test]
async fn sends_authorization_header() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Users/Me"))
        .and(header_regex(
            "Authorization",
            r#"^MediaBrowser Client="Aurelia", Device="testbox", DeviceId="dev-123", Version="0.1.0", Token="t"$"#,
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("me.json")))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::new(&server.uri(), device())
        .unwrap()
        .with_session("t".into(), "u".into());
    let me = client.me().await.unwrap();
    assert_eq!(me.name, "demo");
}

#[tokio::test]
async fn anonymous_requests_have_no_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/System/Info/Public"))
        .and(header_regex(
            "Authorization",
            r#"^MediaBrowser Client="Aurelia".*Version="0.1.0"$"#,
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("public_info.json")))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::new(&server.uri(), device()).unwrap();
    assert_eq!(
        client.public_info().await.unwrap().server_name,
        "Stable Demo"
    );
}

#[tokio::test]
async fn respects_server_base_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/jf/System/Info/Public"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("public_info.json")))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::new(&format!("{}/jf", server.uri()), device()).unwrap();
    client.public_info().await.unwrap();
}

#[tokio::test]
async fn maps_401_to_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Users/Me"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri(), device())
        .unwrap()
        .with_session("revoked".into(), "u".into());
    assert!(matches!(client.me().await, Err(Error::Unauthorized)));
}

#[tokio::test]
async fn maps_500_to_http() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Users/Me"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri(), device())
        .unwrap()
        .with_session("t".into(), "u".into());
    assert!(matches!(client.me().await, Err(Error::Http(500))));
}

#[tokio::test]
async fn maps_garbage_to_decode() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/Users/Me"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>proxy login</html>"))
        .mount(&server)
        .await;

    let client = Client::new(&server.uri(), device())
        .unwrap()
        .with_session("t".into(), "u".into());
    assert!(matches!(client.me().await, Err(Error::Decode(_))));
}

#[tokio::test]
async fn unreachable_server_is_network_error() {
    let client = Client::new("http://127.0.0.1:9", device()).unwrap();
    assert!(matches!(client.public_info().await, Err(Error::Network(_))));
}
