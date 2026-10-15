//! Plays media with mpv and reports what happens.
//!
//! [`MpvPlayer`] drives a standalone mpv over JSON IPC; `embedded` renders
//! with libmpv into a Wayland subsurface of the app's window. Both take the
//! same [`Control`]s and report the same [`PlayerEvent`]s.

mod control;
mod mpv;
mod tracks;
mod types;

pub use mpv::MpvPlayer;
pub use tracks::{StreamInfo, TrackKind, mpv_track_id};
pub use types::*;
