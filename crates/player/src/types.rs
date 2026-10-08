use std::sync::Arc;
use std::time::Duration;

/// Which track to select at start.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TrackChoice {
    /// Let the player decide (its own language/default rules).
    #[default]
    Auto,
    Off,
    /// Player-side track id (mpv: 1-based per type).
    Id(u32),
}

/// A subtitle file to load alongside the media.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalSub {
    pub url: String,
    pub title: Option<String>,
    pub lang: Option<String>,
    /// Select this track once added.
    pub select: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayRequest {
    pub url: String,
    pub title: String,
    pub start: Duration,
    /// Full header lines, e.g. `Authorization: MediaBrowser Token="…"`.
    /// Sent over IPC, never on the command line.
    pub http_headers: Vec<String>,
    pub audio: TrackChoice,
    pub subtitle: TrackChoice,
    pub external_subtitles: Vec<ExternalSub>,
}

impl PlayRequest {
    pub fn new(url: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            title: title.into(),
            start: Duration::ZERO,
            http_headers: Vec::new(),
            audio: TrackChoice::Auto,
            subtitle: TrackChoice::Auto,
            external_subtitles: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndReason {
    /// Reached the end of the file.
    Eof,
    /// The user closed the player (or we asked it to stop).
    Quit,
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    /// The media is loaded and playback has begun.
    Started,
    /// Current position; emitted at most about once per second.
    Position(Duration),
    Paused(bool),
    /// A seek finished at this position.
    Seeked(Duration),
    /// Always the final event of a session, exactly once.
    Ended {
        position: Duration,
        reason: EndReason,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("could not start the player ({binary}): {message}")]
    Spawn { binary: String, message: String },
}

/// Controls a running playback session.
pub trait PlaybackControl: Send + Sync {
    fn stop(&self);
}

pub struct PlaybackHandle {
    events: async_channel::Receiver<PlayerEvent>,
    control: Arc<dyn PlaybackControl>,
}

impl PlaybackHandle {
    pub fn new(
        events: async_channel::Receiver<PlayerEvent>,
        control: Arc<dyn PlaybackControl>,
    ) -> Self {
        Self { events, control }
    }

    pub fn events(&self) -> &async_channel::Receiver<PlayerEvent> {
        &self.events
    }

    pub fn control(&self) -> Arc<dyn PlaybackControl> {
        self.control.clone()
    }

    pub fn stop(&self) {
        self.control.stop();
    }
}

pub trait Player: Send + Sync {
    fn play(&self, request: PlayRequest) -> Result<PlaybackHandle, PlayerError>;
}
