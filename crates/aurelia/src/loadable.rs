use gpui_kit::SharedString;

/// Remote data as a page sees it.
#[derive(Debug, Clone, Default)]
pub enum Loadable<T> {
    #[default]
    Loading,
    Ready(T),
    Failed(SharedString),
}

impl<T> Loadable<T> {
    pub fn from_result(result: jellyfin::Result<T>) -> Self {
        match result {
            Ok(value) => Loadable::Ready(value),
            Err(err) => Loadable::Failed(describe(&err).into()),
        }
    }

    pub fn ready(&self) -> Option<&T> {
        match self {
            Loadable::Ready(value) => Some(value),
            _ => None,
        }
    }

    pub fn ready_mut(&mut self) -> Option<&mut T> {
        match self {
            Loadable::Ready(value) => Some(value),
            _ => None,
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Loadable::Loading)
    }
}

/// A sentence a person can act on.
pub fn describe(err: &jellyfin::Error) -> String {
    match err {
        jellyfin::Error::Unauthorized => "Your session has expired. Sign in again.".into(),
        jellyfin::Error::NoSession => "You're not signed in.".into(),
        jellyfin::Error::Network(detail) => format!("Can't reach the server ({detail})."),
        jellyfin::Error::Http(404) => "This item no longer exists on the server.".into(),
        jellyfin::Error::Http(code) => format!("The server returned an error (HTTP {code})."),
        jellyfin::Error::Decode(_) => {
            "The server sent something unexpected. Is this a Jellyfin server?".into()
        }
        jellyfin::Error::InvalidUrl(detail) => {
            format!("That doesn't look like a server address: {detail}")
        }
    }
}
