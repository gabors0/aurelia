//! Endpoint methods on [`crate::Client`], grouped by area.

mod auth;
mod browse;
mod images;
mod playback;

pub use browse::{ItemFilter, ItemsQuery, SortBy, SortOrder};
pub use playback::{PlaybackReport, ProgressEvent};

/// Extra fields requested for list endpoints so cards can render without a
/// second round-trip.
pub(crate) const LIST_FIELDS: &str =
    "Overview,Genres,ProductionYear,PrimaryImageAspectRatio,ChildCount,MediaSourceCount";
