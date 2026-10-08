use std::sync::Arc;
use std::time::Duration;

use reqwest::{Method, RequestBuilder};
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::{Error, Result, normalize_server_url};

/// Identifies this client installation to the server (shown in the
/// dashboard's device list and used to scope sessions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    client: String,
    device: String,
    device_id: String,
    version: String,
}

impl DeviceInfo {
    pub fn new(
        client: impl Into<String>,
        device: impl Into<String>,
        device_id: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            client: client.into(),
            device: device.into(),
            device_id: device_id.into(),
            version: version.into(),
        }
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }
}

#[derive(Debug)]
struct Session {
    token: String,
    user_id: String,
}

/// Cheap to clone; clones share the connection pool.
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    base: Url,
    device: Arc<DeviceInfo>,
    session: Option<Arc<Session>>,
}

impl Client {
    pub fn new(server: &str, device: DeviceInfo) -> Result<Self> {
        let base = normalize_server_url(server)?;
        let http = reqwest::Client::builder()
            .user_agent(format!("{}/{}", device.client, device.version))
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(Error::from_reqwest)?;
        Ok(Self {
            http,
            base,
            device: Arc::new(device),
            session: None,
        })
    }

    pub fn with_session(mut self, token: String, user_id: String) -> Self {
        self.session = Some(Arc::new(Session { token, user_id }));
        self
    }

    pub fn base_url(&self) -> &Url {
        &self.base
    }

    pub fn device(&self) -> &DeviceInfo {
        &self.device
    }

    pub fn token(&self) -> Option<&str> {
        self.session.as_ref().map(|s| s.token.as_str())
    }

    pub fn user_id(&self) -> Option<&str> {
        self.session.as_ref().map(|s| s.user_id.as_str())
    }

    pub(crate) fn require_user_id(&self) -> Result<&str> {
        self.user_id().ok_or(Error::NoSession)
    }

    /// The `Authorization` header value Jellyfin expects from clients.
    pub fn auth_header(&self) -> String {
        let d = &self.device;
        let mut header = format!(
            r#"MediaBrowser Client="{}", Device="{}", DeviceId="{}", Version="{}""#,
            header_safe(&d.client),
            header_safe(&d.device),
            header_safe(&d.device_id),
            header_safe(&d.version),
        );
        if let Some(token) = self.token() {
            header.push_str(&format!(r#", Token="{token}""#));
        }
        header
    }

    /// The minimal header media players need to fetch streams.
    pub fn token_header(&self) -> Option<String> {
        self.token()
            .map(|token| format!(r#"Authorization: MediaBrowser Token="{token}""#))
    }

    /// Absolute URL for an API path relative to the server base.
    pub fn url(&self, path: &str) -> Url {
        self.base
            .join(path.trim_start_matches('/'))
            .expect("API paths are valid relative URLs")
    }

    pub(crate) fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.http
            .request(method, self.url(path))
            .header("Authorization", self.auth_header())
            .header("Accept", "application/json")
    }

    pub(crate) async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        self.send_json(self.request(Method::GET, path).query(query))
            .await
    }

    pub(crate) async fn post_json<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
        body: &B,
    ) -> Result<T> {
        self.send_json(self.request(Method::POST, path).query(query).json(body))
            .await
    }

    /// For endpoints that answer 204 No Content.
    pub(crate) async fn send_empty(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&serde_json::Value>,
    ) -> Result<()> {
        let mut request = self.request(method, path).query(query);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(Error::from_reqwest)?;
        check_status(response.status())?;
        Ok(())
    }

    pub(crate) async fn send_json<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
    ) -> Result<T> {
        let response = request.send().await.map_err(Error::from_reqwest)?;
        check_status(response.status())?;
        let body = response.text().await.map_err(Error::from_reqwest)?;
        serde_json::from_str(&body).map_err(|e| {
            let preview: String = body.chars().take(120).collect();
            Error::Decode(format!("{e} (body starts with {preview:?})"))
        })
    }
}

fn check_status(status: reqwest::StatusCode) -> Result<()> {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        Err(Error::Unauthorized)
    } else if !status.is_success() {
        Err(Error::Http(status.as_u16()))
    } else {
        Ok(())
    }
}

/// Header field values are quoted; drop characters that would break parsing.
fn header_safe(value: &str) -> String {
    value
        .chars()
        .filter(|c| *c != '"' && *c != ',' && !c.is_control())
        .collect()
}
