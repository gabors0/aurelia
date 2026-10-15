//! Development-only hooks driven by environment variables, used for
//! screenshots and end-to-end checks without driving the mouse.

use gpui_kit::{Pixels, ScrollHandle, point, px};

/// `AURELIA_SCROLL_Y=<px>`: scroll a page down once its data has loaded.
pub fn apply_initial_scroll(handle: &ScrollHandle) {
    if let Some(y) = std::env::var("AURELIA_SCROLL_Y")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
    {
        let y: Pixels = px(y);
        handle.set_offset(point(px(0.), -y));
    }
}

/// `AURELIA_KEYS="down down right enter"`: types those keys into the window,
/// one every 700 ms, starting `AURELIA_KEYS_AFTER` seconds (default 8) after
/// launch. For screenshots of keyboard navigation.
pub fn type_keys(window: &mut gpui_kit::Window, cx: &mut gpui_kit::App) {
    use gpui_kit::{KeyDownEvent, KeyUpEvent, Keystroke, PlatformInput};
    use std::time::Duration;

    let Ok(keys) = std::env::var("AURELIA_KEYS") else {
        return;
    };
    let after = std::env::var("AURELIA_KEYS_AFTER")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(8.);
    let keys: Vec<Keystroke> = keys
        .split_whitespace()
        .filter_map(|k| Keystroke::parse(k).ok())
        .collect();
    window
        .spawn(cx, async move |cx| {
            cx.background_executor()
                .timer(Duration::from_secs_f32(after))
                .await;
            for keystroke in keys {
                cx.update(|window, cx| {
                    window.dispatch_event(
                        PlatformInput::KeyDown(KeyDownEvent {
                            keystroke: keystroke.clone(),
                            is_held: false,
                            prefer_character_input: false,
                        }),
                        cx,
                    );
                    window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
                })
                .ok();
                cx.background_executor()
                    .timer(Duration::from_millis(700))
                    .await;
            }
        })
        .detach();
}

/// `AURELIA_PLAY_FILE=<path or URL>`: opens straight into the embedded
/// player with that media, no server needed, and quits when it closes. For
/// checking the player on screen.
pub fn play_file(
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) -> Option<gpui_kit::Entity<crate::views::player::PlayerView>> {
    use gpui_kit::AppContext as _;

    use crate::views::player::{PlayerInfo, PlayerView};

    let url = std::env::var("AURELIA_PLAY_FILE").ok()?;
    let request = player::PlayRequest::new(url.clone(), "Aurelia player check");
    let (handle, surface) = match crate::playback::start_embedded(request, window, cx) {
        Ok(started) => started,
        Err(reason) => {
            tracing::error!("AURELIA_PLAY_FILE: can't play inside the window: {reason}");
            return None;
        }
    };
    let info = PlayerInfo {
        title: "Player check".into(),
        detail: Some(url.into()),
        ..PlayerInfo::default()
    };
    let control = handle.control();
    let view = cx.new(|cx| PlayerView::new(info, control, Some(surface), cx));
    let events = handle.events().clone();
    let forward = view.downgrade();
    window
        .spawn(cx, async move |cx| {
            while let Ok(event) = events.recv().await {
                tracing::info!("player event: {event:?}");
                let ended = matches!(event, player::PlayerEvent::Ended { .. });
                cx.update(|_, cx| forward.update(cx, |view, cx| view.on_event(&event, cx)))
                    .ok();
                if ended {
                    break;
                }
            }
            cx.update(|_, cx| cx.quit()).ok();
            drop(handle);
        })
        .detach();
    Some(view)
}
