use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Jellyfin's time unit: 100-nanosecond ticks.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Ticks(pub i64);

impl Ticks {
    pub const ZERO: Ticks = Ticks(0);
    const PER_SECOND: i64 = 10_000_000;

    pub fn from_duration(duration: Duration) -> Self {
        Ticks((duration.as_nanos() / 100).min(i64::MAX as u128) as i64)
    }

    /// Negative tick counts (seen on broken metadata) clamp to zero.
    pub fn to_duration(self) -> Duration {
        Duration::from_nanos(self.0.max(0) as u64 * 100)
    }

    pub fn as_secs_f64(self) -> f64 {
        self.0 as f64 / Self::PER_SECOND as f64
    }
}
