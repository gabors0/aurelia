//! A small, typed async client for the parts of the Jellyfin API that a
//! video-first desktop client needs. Compatible with Jellyfin 10.9 and newer.

mod api;
mod client;
mod error;
mod models;
mod ticks;
mod url;

pub use client::{Client, DeviceInfo};
pub use error::{Error, Result};
pub use models::*;
pub use ticks::Ticks;
pub use url::normalize_server_url;
