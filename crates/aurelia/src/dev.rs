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
