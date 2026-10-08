pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// The server rejected our token (revoked, expired, or never valid).
    #[error("the server rejected the session")]
    Unauthorized,
    #[error("the server answered with HTTP {0}")]
    Http(u16),
    #[error("could not reach the server: {0}")]
    Network(String),
    #[error("unexpected response from the server: {0}")]
    Decode(String),
    #[error("not a valid server address: {0}")]
    InvalidUrl(String),
    #[error("not signed in")]
    NoSession,
}

impl Error {
    pub(crate) fn from_reqwest(err: reqwest::Error) -> Self {
        match err.status() {
            Some(status) if status == reqwest::StatusCode::UNAUTHORIZED => Error::Unauthorized,
            Some(status) => Error::Http(status.as_u16()),
            None if err.is_decode() => Error::Decode(err.to_string()),
            None => Error::Network(root_cause(&err)),
        }
    }
}

/// reqwest's top-level message is just "error sending request"; the useful part
/// (DNS failure, connection refused, TLS) is in the source chain.
fn root_cause(err: &(dyn std::error::Error + 'static)) -> String {
    let mut current = err;
    while let Some(source) = current.source() {
        current = source;
    }
    current.to_string()
}
