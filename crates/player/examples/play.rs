//! Plays a URL through `MpvPlayer` and prints the events — a manual check
//! of the IPC flow.
//!
//! cargo run -p player --example play -- <url> [header] [start-secs] [subtitle-url]

use std::time::Duration;

use player::{ExternalSub, MpvPlayer, PlayRequest, Player, PlayerEvent};

fn main() {
    let mut args = std::env::args().skip(1);
    let url = args
        .next()
        .expect("usage: play <url> [header] [start] [subtitle-url]");
    let mut request = PlayRequest::new(url, "player example");
    if let Some(header) = args.next().filter(|h| !h.is_empty()) {
        request.http_headers.push(header);
    }
    if let Some(start) = args.next().and_then(|s| s.parse::<f64>().ok()) {
        request.start = Duration::from_secs_f64(start);
    }
    if let Some(subtitle) = args.next() {
        request.external_subtitles.push(ExternalSub {
            url: subtitle,
            title: Some("External".into()),
            lang: None,
            select: true,
        });
    }
    let extra: Vec<String> = std::env::var("MPV_EXTRA_ARGS")
        .map(|v| v.split_whitespace().map(String::from).collect())
        .unwrap_or_default();
    let player = MpvPlayer::find()
        .expect("mpv on PATH")
        .with_extra_args(extra);
    let handle = player.play(request).expect("spawn mpv");
    while let Ok(event) = handle.events().recv_blocking() {
        println!("{event:?}");
        if let PlayerEvent::Ended { .. } = event {
            break;
        }
    }
}
