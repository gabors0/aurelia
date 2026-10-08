//! A shelf of round headshots.

use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, FontWeight, ScrollHandle, SharedString, div, px};
use jellyfin::{BaseItem, Person};

use crate::components::art::Art;
use crate::components::row::row;
use crate::images::ImageRequest;
use crate::state::AppState;
use crate::theme::Palette;

fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .collect()
}

pub fn cast_row(item: &BaseItem, handle: &ScrollHandle, cx: &App) -> Option<AnyElement> {
    let client = AppState::client(cx);
    let cast: Vec<&Person> = item
        .people
        .iter()
        .filter(|p| matches!(p.person_type.as_deref(), Some("Actor" | "GuestStar")))
        .take(24)
        .collect();
    if cast.is_empty() {
        return None;
    }
    let blurhashes = item.image_blur_hashes.get("Primary");
    let cards = cast
        .into_iter()
        .map(|person| {
            let request = person.primary_image_tag.as_ref().map(|tag| {
                let mut request =
                    ImageRequest::new(client.person_image_url(&person.id, tag, 240).to_string());
                if let Some(hash) = blurhashes.and_then(|b| b.get(tag)) {
                    request = request.with_blurhash(hash.clone());
                }
                request
            });
            v_flex()
                .w(px(112.))
                .flex_shrink_0()
                .items_center()
                .gap_2()
                .child(
                    Art::new(SharedString::from(format!("person-{}", person.id)), request)
                        .title(initials(&person.name))
                        .radius(px(56.))
                        .size(px(104.)),
                )
                .child(
                    div()
                        .w_full()
                        .text_center()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .truncate()
                        .child(person.name.clone()),
                )
                .when_some(
                    person.role.clone().filter(|r| !r.is_empty()),
                    |this, role| {
                        this.child(
                            div()
                                .w_full()
                                .text_center()
                                .text_xs()
                                .truncate()
                                .text_color(Palette::text_tertiary())
                                .child(role),
                        )
                    },
                )
                .into_any_element()
        })
        .collect();
    Some(row("cast", "Cast", handle, cards).into_any_element())
}

#[cfg(test)]
mod tests {
    #[test]
    fn initials_take_two_words() {
        assert_eq!(super::initials("George A. Romero"), "GA");
        assert_eq!(super::initials("Cher"), "C");
        assert_eq!(super::initials(""), "");
    }
}
