//! Human-readable strings for durations, dates and episode numbers.

use std::time::Duration;

use chrono::{DateTime, Local};

/// "2h 46m", "45m", "2h".
pub fn runtime(duration: Duration) -> String {
    let minutes = ((duration.as_secs() + 30) / 60).max(1);
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// Player-style clock: "1:12:04", "1:05".
pub fn clock(duration: Duration) -> String {
    let total = duration.as_secs();
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Wall-clock time when something `remaining` long finishes: "22:41".
pub fn ends_at(remaining: Duration, now: DateTime<Local>) -> String {
    let end = now + chrono::Duration::seconds(remaining.as_secs() as i64);
    end.format("%H:%M").to_string()
}

/// "S2:E3", "E3", or "" when unknown.
pub fn episode_label(season: Option<i32>, episode: Option<i32>) -> String {
    match (season, episode) {
        (Some(s), Some(e)) => format!("S{s}:E{e}"),
        (None, Some(e)) => format!("E{e}"),
        _ => String::new(),
    }
}

/// "2019", "2019–2023", "2019–present".
pub fn years(start: Option<i32>, end_date: Option<&str>, continuing: bool) -> String {
    let Some(start) = start else {
        return String::new();
    };
    let end = end_date
        .and_then(|date| date.get(..4))
        .and_then(|year| year.parse::<i32>().ok());
    match end {
        _ if continuing => format!("{start}–present"),
        Some(end) if end > start => format!("{start}–{end}"),
        _ => start.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn format_runtime() {
        assert_eq!(runtime(Duration::from_secs(166 * 60)), "2h 46m");
        assert_eq!(runtime(Duration::from_secs(45 * 60)), "45m");
        assert_eq!(runtime(Duration::from_secs(120 * 60)), "2h");
        assert_eq!(runtime(Duration::from_secs(44 * 60 + 40)), "45m");
        assert_eq!(runtime(Duration::from_secs(20)), "1m");
    }

    #[test]
    fn format_clock() {
        assert_eq!(clock(Duration::from_secs(4324)), "1:12:04");
        assert_eq!(clock(Duration::from_secs(65)), "1:05");
        assert_eq!(clock(Duration::ZERO), "0:00");
    }

    #[test]
    fn format_ends_at() {
        let now = Local.with_ymd_and_hms(2026, 10, 8, 21, 0, 0).unwrap();
        assert_eq!(ends_at(Duration::from_secs(101 * 60), now), "22:41");
    }

    #[test]
    fn format_episode_label() {
        assert_eq!(episode_label(Some(2), Some(3)), "S2:E3");
        assert_eq!(episode_label(None, Some(3)), "E3");
        assert_eq!(episode_label(Some(1), None), "");
    }

    #[test]
    fn format_years() {
        assert_eq!(years(Some(2019), None, false), "2019");
        assert_eq!(
            years(Some(2019), Some("2023-05-01T00:00:00.0000000Z"), false),
            "2019–2023"
        );
        assert_eq!(
            years(Some(2019), Some("2019-12-01T00:00:00Z"), false),
            "2019"
        );
        assert_eq!(years(Some(2019), None, true), "2019–present");
        assert_eq!(years(None, None, true), "");
    }
}
