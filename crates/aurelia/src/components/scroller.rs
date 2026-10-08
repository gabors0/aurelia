//! Vertically scrolling page container shared by the cinematic pages.

use gpui_kit::prelude::*;
use gpui_kit::{Div, ElementId, ScrollHandle, Stateful, div};

/// A full-size, vertically scrolling page. It only reacts to vertical wheel
/// motion, so sideways gestures over the shelves inside stay with them.
pub fn page(id: impl Into<ElementId>, handle: &ScrollHandle) -> Stateful<Div> {
    div()
        .id(id)
        .size_full()
        .overflow_y_scroll()
        .restrict_scroll_to_axis()
        .track_scroll(handle)
}
