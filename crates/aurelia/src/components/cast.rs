//! Round headshots of people: an item's cast, or people found by search.
//! Each opens the person's page.

use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, FontWeight, Hsla, ScrollHandle, SharedString, div, hsla, px};
use jellyfin::{BaseItem, Person};

use crate::components::art::Art;
use crate::components::row::row;
use crate::images::ImageRequest;
use crate::nav::Route;
use crate::state::AppState;
use crate::theme::Palette;
use crate::views::shell;

const SIZE: f32 = 104.;

fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .collect()
}

/// One clickable headshot with a name and, for cast, the role played.
pub fn headshot(
    person_id: &str,
    name: &str,
    role: Option<String>,
    request: Option<ImageRequest>,
    accent: Hsla,
) -> AnyElement {
    let group: SharedString = format!("person-{person_id}").into();
    let route = Route::Person {
        id: person_id.to_string(),
    };
    v_flex()
        .id(group.clone())
        .group(group.clone())
        .w(px(112.))
        .flex_shrink_0()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .on_click(move |_, window, cx| shell::navigate(route.clone(), window, cx))
        .child(
            div()
                .relative()
                .size(px(SIZE))
                .child(
                    Art::new(SharedString::from(format!("{group}-art")), request)
                        .title(initials(name))
                        .radius(px(SIZE / 2.))
                        .size_full(),
                )
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded_full()
                        .border_2()
                        .border_color(gpui_kit::transparent_black())
                        .group_hover(group.clone(), move |style| {
                            style.border_color(accent).bg(hsla(0., 0., 1., 0.06))
                        }),
                ),
        )
        .child(
            div()
                .w_full()
                .text_center()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .truncate()
                .text_color(hsla(0., 0., 0.9, 1.))
                .group_hover(group, |style| style.text_color(Palette::text()))
                .child(name.to_string()),
        )
        .when_some(role.filter(|r| !r.is_empty()), |this, role| {
            this.child(
                div()
                    .w_full()
                    .text_center()
                    .text_xs()
                    .truncate()
                    .text_color(Palette::text_tertiary())
                    .child(role),
            )
        })
        .into_any_element()
}

pub fn cast_row(
    item: &BaseItem,
    handle: &ScrollHandle,
    accent: Hsla,
    cx: &App,
) -> Option<AnyElement> {
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
            let tag = person.primary_image_tag.as_deref();
            let mut request =
                ImageRequest::new(client.person_image_url(&person.id, tag, 240).to_string());
            if let Some(hash) = tag.and_then(|tag| blurhashes.and_then(|b| b.get(tag))) {
                request = request.with_blurhash(hash.clone());
            }
            let request = Some(request);
            headshot(
                &person.id,
                &person.name,
                person.role.clone(),
                request,
                accent,
            )
        })
        .collect();
    Some(row("cast", "Cast", handle, cards).into_any_element())
}

/// A shelf of people found by search.
pub fn people_row(
    id: &str,
    title: &str,
    people: &[BaseItem],
    handle: &ScrollHandle,
    accent: Hsla,
    cx: &App,
) -> AnyElement {
    let client = AppState::client(cx);
    let cards = people
        .iter()
        .map(|person| {
            let request = person
                .primary_image()
                .map(|image| ImageRequest::for_image(&client, &image, 240));
            headshot(&person.id, &person.name, None, request, accent)
        })
        .collect();
    row(id.to_string(), title.to_string(), handle, cards).into_any_element()
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
