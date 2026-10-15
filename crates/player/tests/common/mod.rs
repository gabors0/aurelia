#![allow(dead_code)] // Each test binary uses part of it.

//! Checks shared by both backends' live tests.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use player::{Control, EndReason, PlaybackHandle, PlayerEvent, TrackKind};

/// A 30 s test pattern as a file, so it has a known duration (lavfi sources
/// report none). Encoded once per test binary with the `mpv` on `PATH`.
pub fn test_media() -> String {
    static MEDIA: OnceLock<PathBuf> = OnceLock::new();
    MEDIA
        .get_or_init(|| {
            let path = std::env::temp_dir()
                .join(format!("aurelia-player-test-{}.mkv", std::process::id()));
            let status = Command::new("mpv")
                .args([
                    "--no-config",
                    "--really-quiet",
                    "av://lavfi:testsrc=duration=30:size=160x120:rate=10",
                ])
                .arg(format!("--o={}", path.display()))
                .status()
                .expect("run mpv to encode the test file");
            assert!(status.success(), "mpv could not encode the test file");
            path
        })
        .display()
        .to_string()
}

/// Waits for the first event matching `want`, failing after `timeout`.
pub fn wait_for(
    handle: &PlaybackHandle,
    timeout: Duration,
    mut want: impl FnMut(&PlayerEvent) -> bool,
) -> PlayerEvent {
    let deadline = Instant::now() + timeout;
    loop {
        match handle.events().try_recv() {
            Ok(event) if want(&event) => return event,
            Ok(PlayerEvent::Ended { reason, .. }) => {
                panic!("session ended early: {reason:?}")
            }
            Ok(_) => {}
            Err(_) => {
                assert!(Instant::now() < deadline, "timed out waiting for an event");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

/// Plays [`test_media`]: drives every control and checks the player
/// reports each change, then stops.
pub fn exercise_controls(handle: &PlaybackHandle) {
    let wait = Duration::from_secs(15);
    let mut saw_duration = false;
    let mut saw_tracks = false;
    wait_for(handle, wait, |event| {
        match event {
            PlayerEvent::Duration(d) => saw_duration = *d >= Duration::from_secs(28),
            PlayerEvent::Tracks(tracks) => {
                saw_tracks = tracks.iter().any(|t| t.kind == Some(TrackKind::Video))
            }
            _ => {}
        }
        *event == PlayerEvent::Started
    });
    let control = handle.control();

    control.send(Control::Pause(true));
    wait_for(handle, wait, |e| *e == PlayerEvent::Paused(true));
    control.send(Control::Pause(false));
    wait_for(handle, wait, |e| *e == PlayerEvent::Paused(false));

    control.send(Control::Volume(40.0));
    wait_for(
        handle,
        wait,
        |e| matches!(e, PlayerEvent::Volume(v) if v.level == 40.0 && !v.muted),
    );
    control.send(Control::Mute(true));
    wait_for(
        handle,
        wait,
        |e| matches!(e, PlayerEvent::Volume(v) if v.muted),
    );

    control.send(Control::Seek(Duration::from_secs(20)));
    let PlayerEvent::Seeked(at) = wait_for(handle, wait, |e| matches!(e, PlayerEvent::Seeked(_)))
    else {
        unreachable!()
    };
    assert!(at >= Duration::from_secs(19), "{at:?}");
    control.send(Control::SeekBy(-10.0));
    let PlayerEvent::Seeked(at) = wait_for(handle, wait, |e| matches!(e, PlayerEvent::Seeked(_)))
    else {
        unreachable!()
    };
    assert!(at < Duration::from_secs(15), "{at:?}");

    // The test pattern has no audio track, so this switches nothing; it just
    // must not upset the session.
    control.send(Control::Audio(player::TrackChoice::Off));

    handle.stop();
    let ended = wait_for(handle, wait, |e| matches!(e, PlayerEvent::Ended { .. }));
    assert!(
        matches!(
            ended,
            PlayerEvent::Ended {
                reason: EndReason::Quit,
                ..
            }
        ),
        "{ended:?}"
    );
    assert!(saw_duration, "duration reported before start");
    assert!(saw_tracks, "tracks reported before start");
    std::thread::sleep(Duration::from_millis(200));
    assert!(handle.events().try_recv().is_err(), "nothing follows Ended");
}
