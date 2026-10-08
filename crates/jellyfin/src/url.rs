use std::net::IpAddr;

use url::Url;

use crate::{Error, Result};

/// Turns whatever the user typed or pasted into a server base URL that always
/// ends in `/`, so relative API paths join underneath it.
///
/// Accepts bare hosts (`jellyfin.example.com` → https, `192.168.1.5:8096` →
/// http), and strips web-client paths such as `/web/index.html#/home`.
pub fn normalize_server_url(input: &str) -> Result<Url> {
    let input = input.trim();
    if input.is_empty() {
        return Err(Error::InvalidUrl("empty address".into()));
    }

    let with_scheme = if input.contains("://") {
        input.to_string()
    } else {
        format!("{}://{input}", default_scheme(input))
    };

    let mut url =
        Url::parse(&with_scheme).map_err(|e| Error::InvalidUrl(format!("{input}: {e}")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::InvalidUrl(format!(
            "{input}: only http and https are supported"
        )));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(Error::InvalidUrl(format!("{input}: missing host")));
    }

    url.set_fragment(None);
    url.set_query(None);

    let segments: Vec<String> = url
        .path_segments()
        .map(|s| s.filter(|s| !s.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default();
    let kept: Vec<&str> = segments
        .iter()
        .map(String::as_str)
        .take_while(|s| *s != "web" && !s.ends_with(".html"))
        .collect();
    let mut path = String::from("/");
    for segment in kept {
        path.push_str(segment);
        path.push('/');
    }
    url.set_path(&path);
    Ok(url)
}

/// LAN-looking addresses (IPs, localhost, explicit ports) usually run plain
/// HTTP; public host names are almost always behind TLS.
fn default_scheme(input: &str) -> &'static str {
    let authority = input.split('/').next().unwrap_or(input);
    let (host, has_port) = match authority.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => (host, true),
        _ => (authority, false),
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if has_port || host == "localhost" || host.parse::<IpAddr>().is_ok() {
        "http"
    } else {
        "https"
    }
}
