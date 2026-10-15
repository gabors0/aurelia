//! [`Control`]s as mpv commands, shared by both mpv backends.

use crate::{Control, ExternalSub, TrackChoice};

/// The mpv command (its words) that carries out `control`.
pub(crate) fn command(control: &Control) -> Vec<String> {
    let words: Vec<String> = match *control {
        Control::Pause(paused) => vec!["set".into(), "pause".into(), yes_no(paused)],
        Control::Seek(position) => vec![
            "seek".into(),
            number(position.as_secs_f64()),
            "absolute".into(),
        ],
        // Exact, so ±10 s lands 10 s away rather than on a keyframe.
        Control::SeekBy(delta) => vec!["seek".into(), number(delta), "relative+exact".into()],
        Control::Volume(level) => vec![
            "set".into(),
            "volume".into(),
            number(level.clamp(0.0, 100.0)),
        ],
        Control::Mute(muted) => vec!["set".into(), "mute".into(), yes_no(muted)],
        Control::Audio(choice) => vec!["set".into(), "aid".into(), track(choice)],
        Control::Subtitle(choice) => vec!["set".into(), "sid".into(), track(choice)],
    };
    words
}

/// Adds an external subtitle file (selecting it if asked).
pub(crate) fn sub_add(sub: &ExternalSub) -> Vec<String> {
    let flag = if sub.select { "select" } else { "auto" };
    vec![
        "sub-add".into(),
        sub.url.clone(),
        flag.into(),
        sub.title.clone().unwrap_or_default(),
        sub.lang.clone().unwrap_or_default(),
    ]
}

fn yes_no(on: bool) -> String {
    if on { "yes" } else { "no" }.into()
}

/// A number as mpv parses it, without needless digits.
fn number(value: f64) -> String {
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".into()
    } else {
        text.into()
    }
}

/// `--aid`/`--sid` value.
pub(crate) fn track(choice: TrackChoice) -> String {
    match choice {
        TrackChoice::Auto => "auto".into(),
        TrackChoice::Off => "no".into(),
        TrackChoice::Id(id) => id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn words(control: Control) -> Vec<String> {
        command(&control)
    }

    #[test]
    fn pause_and_mute() {
        assert_eq!(words(Control::Pause(true)), ["set", "pause", "yes"]);
        assert_eq!(words(Control::Pause(false)), ["set", "pause", "no"]);
        assert_eq!(words(Control::Mute(true)), ["set", "mute", "yes"]);
    }

    #[test]
    fn seeks() {
        assert_eq!(
            words(Control::Seek(Duration::from_millis(83_250))),
            ["seek", "83.25", "absolute"]
        );
        assert_eq!(
            words(Control::SeekBy(-10.0)),
            ["seek", "-10", "relative+exact"]
        );
        assert_eq!(
            words(Control::Seek(Duration::ZERO)),
            ["seek", "0", "absolute"]
        );
    }

    #[test]
    fn volume_is_clamped() {
        assert_eq!(words(Control::Volume(55.5)), ["set", "volume", "55.5"]);
        assert_eq!(words(Control::Volume(140.0)), ["set", "volume", "100"]);
        assert_eq!(words(Control::Volume(-3.0)), ["set", "volume", "0"]);
    }

    #[test]
    fn tracks() {
        assert_eq!(
            words(Control::Audio(TrackChoice::Id(2))),
            ["set", "aid", "2"]
        );
        assert_eq!(
            words(Control::Subtitle(TrackChoice::Off)),
            ["set", "sid", "no"]
        );
        assert_eq!(
            words(Control::Subtitle(TrackChoice::Auto)),
            ["set", "sid", "auto"]
        );
    }
}
