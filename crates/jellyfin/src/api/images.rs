use url::Url;

use crate::{Client, ImageKind, ImageRef};

impl Client {
    /// Image URL resized server-side to at most `max_width` pixels. Image
    /// endpoints need no authentication.
    pub fn image_url(&self, image: &ImageRef, max_width: u32) -> Url {
        let path = match image.kind {
            ImageKind::Backdrop(index) => {
                format!("Items/{}/Images/Backdrop/{index}", image.item_id)
            }
            kind => format!("Items/{}/Images/{}", image.item_id, kind.type_name()),
        };
        let mut url = self.url(&path);
        url.query_pairs_mut()
            .append_pair("tag", &image.tag)
            .append_pair("maxWidth", &max_width.to_string())
            .append_pair("quality", "90");
        url
    }

    /// Headshot of a person (cast/crew). Cast lists often omit the tag even
    /// when the person has a picture; without one the URL still finds it.
    pub fn person_image_url(&self, person_id: &str, tag: Option<&str>, max_width: u32) -> Url {
        match tag {
            Some(tag) => {
                let image = ImageRef {
                    item_id: person_id.to_string(),
                    kind: ImageKind::Primary,
                    tag: tag.to_string(),
                    blurhash: None,
                };
                self.image_url(&image, max_width)
            }
            None => {
                let mut url = self.url(&format!("Items/{person_id}/Images/Primary"));
                url.query_pairs_mut()
                    .append_pair("maxWidth", &max_width.to_string())
                    .append_pair("quality", "90");
                url
            }
        }
    }

    pub fn user_image_url(&self, user_id: &str, tag: &str, max_width: u32) -> Url {
        let mut url = self.url(&format!("Users/{user_id}/Images/Primary"));
        url.query_pairs_mut()
            .append_pair("tag", tag)
            .append_pair("maxWidth", &max_width.to_string());
        url
    }
}
