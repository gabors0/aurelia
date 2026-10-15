//! The embedded backend against the real libmpv, without a window (mpv's
//! null outputs). Needs `libmpv.so.2` (the dev shell provides it) and `mpv`
//! to encode the test file.

mod common;

use std::time::Duration;

use player::{EmbeddedPlayer, EndReason, PlayRequest, PlayerEvent};

fn player() -> EmbeddedPlayer {
    EmbeddedPlayer::load().expect("libmpv.so.2")
}

fn ended(handle: &player::PlaybackHandle) -> (Duration, EndReason) {
    match common::wait_for(handle, Duration::from_secs(20), |e| {
        matches!(e, PlayerEvent::Ended { .. })
    }) {
        PlayerEvent::Ended { position, reason } => (position, reason),
        _ => unreachable!(),
    }
}

#[test]
#[ignore = "needs libmpv"]
fn controls_reach_libmpv() {
    let handle = player()
        .play_without_video(PlayRequest::new(common::test_media(), "Controls"))
        .unwrap();
    common::exercise_controls(&handle);
}

#[test]
#[ignore = "needs libmpv"]
fn plays_to_the_end() {
    let mut request = PlayRequest::new(common::test_media(), "To the end");
    request.start = Duration::from_secs(28);
    let handle = player().play_without_video(request).unwrap();
    common::wait_for(&handle, Duration::from_secs(15), |e| {
        *e == PlayerEvent::Started
    });
    let (position, reason) = ended(&handle);
    assert_eq!(reason, EndReason::Eof);
    assert!(position >= Duration::from_secs(28), "{position:?}");
}

#[test]
#[ignore = "needs libmpv"]
fn a_missing_file_is_an_error() {
    let handle = player()
        .play_without_video(PlayRequest::new("/nonexistent/aurelia.mkv", "Missing"))
        .unwrap();
    let (_, reason) = ended(&handle);
    assert!(matches!(reason, EndReason::Error(_)), "{reason:?}");
}

#[test]
#[ignore = "needs libmpv"]
fn stopping_at_once_ends_exactly_once() {
    let handle = player()
        .play_without_video(PlayRequest::new(common::test_media(), "Stop"))
        .unwrap();
    handle.stop();
    let (_, reason) = ended(&handle);
    assert_eq!(reason, EndReason::Quit);
    std::thread::sleep(Duration::from_millis(300));
    assert!(handle.events().try_recv().is_err(), "nothing follows Ended");
}
