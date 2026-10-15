use std::sync::Arc;
use std::time::Duration;

use crate::TrackKind;

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

#[derive(Debug, Clone, PartialEq)]
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
    /// Start at this volume instead of the player's own default.
    pub volume: Option<Volume>,
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
            volume: None,
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

/// Volume as the player reports it: 0–100 (mpv allows up to `volume-max`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Volume {
    pub level: f64,
    pub muted: bool,
}

impl Default for Volume {
    fn default() -> Self {
        Self {
            level: 100.0,
            muted: false,
        }
    }
}

/// One audio, video or subtitle track as the player sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Track {
    /// Player-side id (mpv: 1-based per kind); what `TrackChoice::Id` takes.
    pub id: u32,
    pub kind: Option<TrackKind>,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub codec: Option<String>,
    pub default: bool,
    pub forced: bool,
    pub external: bool,
    /// For external tracks: the file or URL it was loaded from.
    pub external_filename: Option<String>,
    /// For tracks inside the container: the container's stream index.
    pub ff_index: Option<i64>,
    pub selected: bool,
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
    /// The media's length, once known (and whenever it changes).
    Duration(Duration),
    /// Playback is waiting for the network.
    Buffering(bool),
    Volume(Volume),
    /// Every track of the media, with the current selection.
    Tracks(Vec<Track>),
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
    #[error("could not start playback: {0}")]
    Embedded(String),
}

/// A change to a running playback session.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    Pause(bool),
    /// Jump to this position.
    Seek(Duration),
    /// Jump this many seconds forward (negative: back).
    SeekBy(f64),
    /// 0–100.
    Volume(f64),
    Mute(bool),
    Audio(TrackChoice),
    Subtitle(TrackChoice),
}

/// Controls a running playback session.
pub trait PlaybackControl: Send + Sync {
    /// Ends the session; `Ended` follows.
    fn stop(&self);
    /// Ignored once the session has ended.
    fn send(&self, control: Control);
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
