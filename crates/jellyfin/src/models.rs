//! Response models. Every field is optional or defaulted: Jellyfin omits or
//! nulls fields freely depending on item type, server version and metadata.

use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};

use crate::Ticks;

/// Treats `null` like a missing field for collections.
fn nullable<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct PublicSystemInfo {
    pub id: String,
    pub server_name: String,
    pub version: String,
    pub product_name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct User {
    pub id: String,
    pub name: String,
    pub primary_image_tag: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AuthResult {
    pub user: User,
    pub access_token: String,
    #[serde(default)]
    pub server_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct UserView {
    pub id: String,
    pub name: String,
    pub collection_type: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub image_tags: HashMap<String, String>,
}

impl UserView {
    /// Libraries holding movies, shows or other video. Mixed-content
    /// folders have no collection type (or "mixed").
    pub fn is_video_library(&self) -> bool {
        matches!(
            self.collection_type.as_deref(),
            None | Some("movies" | "tvshows" | "homevideos" | "mixed")
        )
    }

    /// The Collections library (BoxSets).
    pub fn is_collections(&self) -> bool {
        self.collection_type.as_deref() == Some("boxsets")
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase", bound(deserialize = "T: Deserialize<'de>"))]
pub struct ItemsPage<T> {
    #[serde(default = "Vec::new", deserialize_with = "nullable")]
    pub items: Vec<T>,
    #[serde(default)]
    pub total_record_count: u32,
    #[serde(default)]
    pub start_index: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemKind {
    Movie,
    Series,
    Season,
    Episode,
    BoxSet,
    Folder,
    CollectionFolder,
    Video,
    Trailer,
    Person,
    Genre,
    #[default]
    #[serde(other)]
    Other,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Movie => "Movie",
            ItemKind::Series => "Series",
            ItemKind::Season => "Season",
            ItemKind::Episode => "Episode",
            ItemKind::BoxSet => "BoxSet",
            ItemKind::Folder => "Folder",
            ItemKind::CollectionFolder => "CollectionFolder",
            ItemKind::Video => "Video",
            ItemKind::Trailer => "Trailer",
            ItemKind::Person => "Person",
            ItemKind::Genre => "Genre",
            ItemKind::Other => "",
        }
    }

    pub fn is_playable(self) -> bool {
        matches!(self, ItemKind::Movie | ItemKind::Episode | ItemKind::Video)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct BaseItem {
    pub id: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub kind: ItemKind,
    pub original_title: Option<String>,
    pub overview: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub taglines: Vec<String>,
    pub production_year: Option<i32>,
    pub premiere_date: Option<String>,
    pub end_date: Option<String>,
    /// Series status: "Continuing" or "Ended".
    pub status: Option<String>,
    pub run_time_ticks: Option<Ticks>,
    pub community_rating: Option<f32>,
    pub critic_rating: Option<f32>,
    pub official_rating: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub genres: Vec<String>,
    /// The same genres with their ids.
    #[serde(deserialize_with = "nullable")]
    pub genre_items: Vec<NameId>,
    #[serde(deserialize_with = "nullable")]
    pub studios: Vec<NameId>,
    /// For people: where they were born.
    #[serde(deserialize_with = "nullable")]
    pub production_locations: Vec<String>,

    #[serde(deserialize_with = "nullable")]
    pub image_tags: HashMap<String, String>,
    #[serde(deserialize_with = "nullable")]
    pub backdrop_image_tags: Vec<String>,
    pub parent_backdrop_item_id: Option<String>,
    #[serde(deserialize_with = "nullable")]
    pub parent_backdrop_image_tags: Vec<String>,
    pub parent_logo_item_id: Option<String>,
    pub parent_logo_image_tag: Option<String>,
    pub parent_thumb_item_id: Option<String>,
    pub parent_thumb_image_tag: Option<String>,
    pub parent_primary_image_item_id: Option<String>,
    pub parent_primary_image_tag: Option<String>,
    pub series_primary_image_tag: Option<String>,
    /// Image type → (tag → blurhash).
    #[serde(deserialize_with = "nullable")]
    pub image_blur_hashes: HashMap<String, HashMap<String, String>>,
    pub primary_image_aspect_ratio: Option<f64>,

    pub series_id: Option<String>,
    pub series_name: Option<String>,
    pub season_id: Option<String>,
    pub season_name: Option<String>,
    pub index_number: Option<i32>,
    pub index_number_end: Option<i32>,
    pub parent_index_number: Option<i32>,
    pub child_count: Option<i32>,

    pub user_data: Option<UserData>,
    #[serde(deserialize_with = "nullable")]
    pub media_sources: Vec<MediaSource>,
    #[serde(deserialize_with = "nullable")]
    pub people: Vec<Person>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct NameId {
    pub name: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct UserData {
    pub played_percentage: Option<f64>,
    pub playback_position_ticks: Ticks,
    pub play_count: i32,
    pub played: bool,
    pub is_favorite: bool,
    pub unplayed_item_count: Option<i32>,
    pub last_played_date: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct MediaSource {
    pub id: String,
    pub name: Option<String>,
    pub container: Option<String>,
    pub run_time_ticks: Option<Ticks>,
    pub bitrate: Option<i64>,
    pub supports_direct_play: bool,
    pub default_audio_stream_index: Option<i32>,
    pub default_subtitle_stream_index: Option<i32>,
    #[serde(deserialize_with = "nullable")]
    pub media_streams: Vec<MediaStream>,
}

impl MediaSource {
    pub fn streams(&self, kind: StreamKind) -> impl Iterator<Item = &MediaStream> {
        self.media_streams.iter().filter(move |s| s.kind == kind)
    }

    pub fn video_stream(&self) -> Option<&MediaStream> {
        self.streams(StreamKind::Video).next()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Deserialize)]
pub enum StreamKind {
    Video,
    Audio,
    Subtitle,
    EmbeddedImage,
    Data,
    Lyric,
    #[default]
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct MediaStream {
    #[serde(rename = "Type")]
    pub kind: StreamKind,
    pub index: i32,
    pub codec: Option<String>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub display_title: Option<String>,
    pub is_default: bool,
    pub is_forced: bool,
    pub is_external: bool,
    pub is_text_subtitle_stream: bool,
    pub delivery_url: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub video_range: Option<String>,
    pub video_range_type: Option<String>,
    pub channels: Option<i32>,
    pub channel_layout: Option<String>,
    pub profile: Option<String>,
    pub audio_spatial_format: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub role: Option<String>,
    #[serde(rename = "Type")]
    pub person_type: Option<String>,
    pub primary_image_tag: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct PlaybackInfo {
    #[serde(deserialize_with = "nullable")]
    pub media_sources: Vec<MediaSource>,
    #[serde(deserialize_with = "nullable")]
    pub play_session_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct QuickConnectState {
    pub secret: String,
    pub code: String,
    pub authenticated: bool,
}

/// Which image of an item to fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageKind {
    Primary,
    Backdrop(u32),
    Logo,
    Thumb,
}

impl ImageKind {
    pub fn type_name(self) -> &'static str {
        match self {
            ImageKind::Primary => "Primary",
            ImageKind::Backdrop(_) => "Backdrop",
            ImageKind::Logo => "Logo",
            ImageKind::Thumb => "Thumb",
        }
    }
}

/// A resolved reference to one image on the server: whose image it is, which
/// one, and its tag (cache-busting version).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageRef {
    pub item_id: String,
    pub kind: ImageKind,
    pub tag: String,
    pub blurhash: Option<String>,
}

impl BaseItem {
    fn image_ref(&self, item_id: &str, kind: ImageKind, tag: &str) -> ImageRef {
        let blurhash = self
            .image_blur_hashes
            .get(kind.type_name())
            .and_then(|by_tag| by_tag.get(tag))
            .cloned();
        ImageRef {
            item_id: item_id.to_string(),
            kind,
            tag: tag.to_string(),
            blurhash,
        }
    }

    /// The item's own backdrop, else its parent's (episodes, seasons).
    pub fn backdrop_image(&self) -> Option<ImageRef> {
        if let Some(tag) = self.backdrop_image_tags.first() {
            return Some(self.image_ref(&self.id, ImageKind::Backdrop(0), tag));
        }
        let parent = self.parent_backdrop_item_id.as_deref()?;
        let tag = self.parent_backdrop_image_tags.first()?;
        Some(self.image_ref(parent, ImageKind::Backdrop(0), tag))
    }

    pub fn logo_image(&self) -> Option<ImageRef> {
        if let Some(tag) = self.image_tags.get("Logo") {
            return Some(self.image_ref(&self.id, ImageKind::Logo, tag));
        }
        let parent = self.parent_logo_item_id.as_deref()?;
        let tag = self.parent_logo_image_tag.as_deref()?;
        Some(self.image_ref(parent, ImageKind::Logo, tag))
    }

    pub fn primary_image(&self) -> Option<ImageRef> {
        let tag = self.image_tags.get("Primary")?;
        Some(self.image_ref(&self.id, ImageKind::Primary, tag))
    }

    /// Poster for cards: the item's own, else the series poster for episodes.
    pub fn poster_image(&self) -> Option<ImageRef> {
        if self.kind != ItemKind::Episode
            && let Some(image) = self.primary_image()
        {
            return Some(image);
        }
        if let (Some(series), Some(tag)) = (&self.series_id, &self.series_primary_image_tag) {
            return Some(self.image_ref(series, ImageKind::Primary, tag));
        }
        if let (Some(parent), Some(tag)) = (
            &self.parent_primary_image_item_id,
            &self.parent_primary_image_tag,
        ) {
            return Some(self.image_ref(parent, ImageKind::Primary, tag));
        }
        self.primary_image()
    }

    /// 16:9 artwork for landscape cards: episode still, Thumb, parent Thumb,
    /// then backdrop.
    pub fn landscape_image(&self) -> Option<ImageRef> {
        if self.kind == ItemKind::Episode
            && let Some(image) = self.primary_image()
        {
            return Some(image);
        }
        if let Some(tag) = self.image_tags.get("Thumb") {
            return Some(self.image_ref(&self.id, ImageKind::Thumb, tag));
        }
        if let (Some(parent), Some(tag)) =
            (&self.parent_thumb_item_id, &self.parent_thumb_image_tag)
        {
            return Some(self.image_ref(parent, ImageKind::Thumb, tag));
        }
        self.backdrop_image()
    }

    pub fn runtime(&self) -> Option<std::time::Duration> {
        self.run_time_ticks
            .filter(|t| t.0 > 0)
            .map(Ticks::to_duration)
    }

    pub fn resume_position(&self) -> Option<std::time::Duration> {
        let data = self.user_data.as_ref()?;
        (data.playback_position_ticks.0 > 0).then(|| data.playback_position_ticks.to_duration())
    }

    /// 0.0–1.0 progress for partially watched items.
    pub fn progress(&self) -> Option<f32> {
        let data = self.user_data.as_ref()?;
        let pct = data.played_percentage?;
        (pct > 0.0 && pct < 100.0).then_some((pct / 100.0) as f32)
    }

    pub fn is_played(&self) -> bool {
        self.user_data.as_ref().is_some_and(|d| d.played)
    }

    pub fn is_favorite(&self) -> bool {
        self.user_data.as_ref().is_some_and(|d| d.is_favorite)
    }
}
