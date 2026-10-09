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

fn parse_day(timestamp: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(timestamp.get(..10)?, "%Y-%m-%d").ok()
}

/// "Mar 3, 2022" from a Jellyfin timestamp.
pub fn date(timestamp: &str) -> Option<String> {
    Some(parse_day(timestamp)?.format("%b %-d, %Y").to_string())
}

/// Whole years from `from` to `to`.
fn age(from: chrono::NaiveDate, to: chrono::NaiveDate) -> Option<u32> {
    to.years_since(from)
}

/// A person's dates: "Born Apr 20, 1945 (age 81)" or
/// "Apr 20, 1945 – Dec 1, 2020 (aged 75)".
pub fn lifespan(
    born: Option<&str>,
    died: Option<&str>,
    today: chrono::NaiveDate,
) -> Option<String> {
    let born = born.and_then(parse_day);
    let died = died.and_then(parse_day);
    let show = |day: chrono::NaiveDate| day.format("%b %-d, %Y").to_string();
    match (born, died) {
        (Some(born), Some(died)) => Some(match age(born, died) {
            Some(years) => format!("{} – {} (aged {years})", show(born), show(died)),
            None => format!("{} – {}", show(born), show(died)),
        }),
        (Some(born), None) => Some(match age(born, today) {
            Some(years) => format!("Born {} (age {years})", show(born)),
            None => format!("Born {}", show(born)),
        }),
        (None, Some(died)) => Some(format!("Died {}", show(died))),
        (None, None) => None,
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
    fn format_date() {
        assert_eq!(
            date("2022-03-03T00:00:00.0000000Z").as_deref(),
            Some("Mar 3, 2022")
        );
        assert_eq!(
            date("2010-12-25T00:00:00Z").as_deref(),
            Some("Dec 25, 2010")
        );
        assert_eq!(date("garbage"), None);
    }

    #[test]
    fn format_lifespan() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 9).unwrap();
        assert_eq!(
            lifespan(Some("1945-04-20T00:00:00.0000000Z"), None, today).as_deref(),
            Some("Born Apr 20, 1945 (age 81)")
        );
        assert_eq!(
            lifespan(Some("1945-12-20T00:00:00Z"), None, today).as_deref(),
            Some("Born Dec 20, 1945 (age 80)"),
            "no birthday yet this year"
        );
        assert_eq!(
            lifespan(
                Some("1931-02-08T00:00:00Z"),
                Some("1955-09-30T00:00:00Z"),
                today
            )
            .as_deref(),
            Some("Feb 8, 1931 – Sep 30, 1955 (aged 24)")
        );
        assert_eq!(
            lifespan(None, Some("1955-09-30T00:00:00Z"), today).as_deref(),
            Some("Died Sep 30, 1955")
        );
        assert_eq!(lifespan(None, None, today), None);
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
