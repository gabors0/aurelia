//! Plays media in an external player and reports what happens.
//!
//! [`Player`] is the seam for alternative backends (an embedded libmpv
//! renderer later); [`MpvPlayer`] drives a standalone mpv over JSON IPC.

mod mpv;
mod tracks;
mod types;

pub use mpv::MpvPlayer;
pub use tracks::{StreamInfo, TrackKind, mpv_track_id};
pub use types::*;
