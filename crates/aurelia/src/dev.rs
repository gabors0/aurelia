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
