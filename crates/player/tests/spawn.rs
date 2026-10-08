use std::path::PathBuf;
use std::time::Duration;

use player::{EndReason, MpvPlayer, PlayRequest, Player, PlayerEvent};

fn request() -> PlayRequest {
    PlayRequest::new("av://lavfi:testsrc=duration=2", "Test pattern")
}

fn next_event(events: &async_channel::Receiver<PlayerEvent>, timeout: Duration) -> PlayerEvent {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Ok(event) = events.try_recv() {
            return event;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for event"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn missing_binary_is_spawn_error() {
    let player = MpvPlayer::new(PathBuf::from("/nonexistent/mpv"));
    assert!(player.play(request()).is_err());
}

#[test]
fn exit_without_file_loaded_emits_ended() {
    // `false` exits immediately without ever opening the IPC socket.
    let player = MpvPlayer::new(PathBuf::from("false"));
    let handle = player.play(request()).expect("spawn succeeds");
    match next_event(handle.events(), Duration::from_secs(5)) {
        PlayerEvent::Ended {
            position,
            reason: EndReason::Error(_),
        } => {
            assert_eq!(position, Duration::ZERO)
        }
        other => panic!("expected Ended(Error), got {other:?}"),
    }
}

#[test]
#[ignore = "needs a real mpv binary"]
fn mpv_plays_generated_file() {
    let player = MpvPlayer::find().expect("mpv on PATH").with_extra_args([
        "--vo=null",
        "--ao=null",
        "--force-window=no",
    ]);
    let mut req = request();
    req.start = Duration::from_millis(500);
    let handle = player.play(req).unwrap();

    let mut saw_started = false;
    let mut saw_position = false;
    loop {
        match next_event(handle.events(), Duration::from_secs(15)) {
            PlayerEvent::Started => saw_started = true,
            PlayerEvent::Position(_) => saw_position = true,
            PlayerEvent::Ended { position, reason } => {
                assert_eq!(reason, EndReason::Eof);
                assert!(position >= Duration::from_secs(1), "{position:?}");
                break;
            }
            _ => {}
        }
    }
    assert!(saw_started && saw_position);
}

#[test]
#[ignore = "needs a real mpv binary"]
fn stop_ends_with_quit() {
    let player = MpvPlayer::find().expect("mpv on PATH").with_extra_args([
        "--vo=null",
        "--ao=null",
        "--force-window=no",
    ]);
    let mut req = PlayRequest::new("av://lavfi:testsrc=duration=30", "Long");
    req.start = Duration::from_secs(3);
    let handle = player.play(req).unwrap();
    loop {
        if let PlayerEvent::Started = next_event(handle.events(), Duration::from_secs(10)) {
            break;
        }
    }
    handle.stop();
    loop {
        if let PlayerEvent::Ended { position, reason } =
            next_event(handle.events(), Duration::from_secs(10))
        {
            assert_eq!(reason, EndReason::Quit);
            assert!(position >= Duration::from_secs(3), "{position:?}");
            break;
        }
    }
}
