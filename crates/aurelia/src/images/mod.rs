//! Remote artwork: fetched on the network runtime, decoded off the UI thread,
//! cached in memory (LRU) and on disk.

pub mod cache;
pub mod decode;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::{App, AppContext as _, EntityId, Global, Hsla, RenderImage, SharedString, Window};

use self::cache::DiskCache;
use crate::runtime;

/// Decoded images kept in memory before least-recently-used ones are evicted.
const MEMORY_BUDGET: usize = 600 * 1024 * 1024;
/// Images used this recently are never evicted (they're probably on screen).
const IN_USE_WINDOW: Duration = Duration::from_secs(3);
const DISK_BUDGET: u64 = 1024 * 1024 * 1024;
const MAX_CONCURRENT_DOWNLOADS: usize = 8;
const PLACEHOLDER_SIZE: (u32, u32) = (32, 20);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageStyle {
    Plain,
    /// Blurred ambient light; also yields the accent colour.
    Ambient,
    /// A title logo, with dark lettering lifted so it reads on dark art.
    Logo,
    /// A title logo for a light page: light lettering lowered.
    LogoOnLight,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageRequest {
    url: SharedString,
    style: ImageStyle,
    blurhash: Option<SharedString>,
}

impl ImageRequest {
    pub fn new(url: impl Into<SharedString>) -> Self {
        Self {
            url: url.into(),
            style: ImageStyle::Plain,
            blurhash: None,
        }
    }

    /// Request for a Jellyfin image, with its blurhash as placeholder.
    pub fn for_image(
        client: &jellyfin::Client,
        image: &jellyfin::ImageRef,
        max_width: u32,
    ) -> Self {
        Self {
            url: client.image_url(image, max_width).to_string().into(),
            style: ImageStyle::Plain,
            blurhash: image.blurhash.clone().map(Into::into),
        }
    }

    pub fn with_blurhash(mut self, hash: impl Into<SharedString>) -> Self {
        self.blurhash = Some(hash.into());
        self
    }

    pub fn ambient(mut self) -> Self {
        self.style = ImageStyle::Ambient;
        self
    }

    /// A title logo, made legible on the current theme's page.
    pub fn logo(mut self) -> Self {
        self.style = if crate::theme::is_light() {
            ImageStyle::LogoOnLight
        } else {
            ImageStyle::Logo
        };
        self
    }

    pub fn url(&self) -> &SharedString {
        &self.url
    }
}

#[derive(Clone)]
pub enum ImageState {
    Loading {
        placeholder: Option<Arc<RenderImage>>,
    },
    Ready {
        image: Arc<RenderImage>,
        accent: Option<Hsla>,
    },
    Failed {
        placeholder: Option<Arc<RenderImage>>,
    },
}

enum Slot {
    Loading(Vec<EntityId>),
    Ready {
        image: Arc<RenderImage>,
        accent: Option<Hsla>,
        bytes: usize,
    },
    Failed,
}

struct Entry {
    slot: Slot,
    last_used: Instant,
}

type Key = (SharedString, ImageStyle);

pub struct ImageStore {
    disk: Arc<DiskCache>,
    http: reqwest::Client,
    downloads: Arc<tokio::sync::Semaphore>,
    entries: HashMap<Key, Entry>,
    placeholders: HashMap<SharedString, Option<Arc<RenderImage>>>,
    bytes: usize,
}

impl Global for ImageStore {}

impl ImageStore {
    pub fn init(cx: &mut App) {
        let disk = Arc::new(DiskCache::default_location());
        let prune = disk.clone();
        cx.background_spawn(async move { prune.prune(DISK_BUDGET) })
            .detach();
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("http client");
        cx.set_global(ImageStore {
            disk,
            http,
            downloads: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_DOWNLOADS)),
            entries: HashMap::new(),
            placeholders: HashMap::new(),
            bytes: 0,
        });
    }

    /// Current state of `request`, starting the load if needed. The view being
    /// rendered is re-rendered when the image arrives.
    pub fn get(request: &ImageRequest, window: &Window, cx: &mut App) -> ImageState {
        let view = window.current_view();
        let key = (request.url.clone(), request.style);
        let placeholder = request
            .blurhash
            .as_ref()
            .and_then(|hash| Self::placeholder(hash, cx));
        let store = cx.global_mut::<ImageStore>();
        let now = Instant::now();
        match store.entries.get_mut(&key) {
            Some(entry) => {
                entry.last_used = now;
                match &mut entry.slot {
                    Slot::Ready { image, accent, .. } => ImageState::Ready {
                        image: image.clone(),
                        accent: *accent,
                    },
                    Slot::Loading(waiters) => {
                        if !waiters.contains(&view) {
                            waiters.push(view);
                        }
                        ImageState::Loading { placeholder }
                    }
                    Slot::Failed => ImageState::Failed { placeholder },
                }
            }
            None => {
                store.entries.insert(
                    key.clone(),
                    Entry {
                        slot: Slot::Loading(vec![view]),
                        last_used: now,
                    },
                );
                Self::load(key, cx);
                ImageState::Loading { placeholder }
            }
        }
    }

    /// Forgets failed loads so they are retried (e.g. after reconnecting).
    pub fn retry_failed(cx: &mut App) {
        cx.global_mut::<ImageStore>()
            .entries
            .retain(|_, entry| !matches!(entry.slot, Slot::Failed));
    }

    fn placeholder(hash: &SharedString, cx: &mut App) -> Option<Arc<RenderImage>> {
        let store = cx.global_mut::<ImageStore>();
        store
            .placeholders
            .entry(hash.clone())
            .or_insert_with(|| {
                let (w, h) = PLACEHOLDER_SIZE;
                decode::blurhash_image(hash, w, h).map(decode::to_render_image)
            })
            .clone()
    }

    fn load(key: Key, cx: &mut App) {
        let store = cx.global::<ImageStore>();
        let (disk, http, downloads) = (
            store.disk.clone(),
            store.http.clone(),
            store.downloads.clone(),
        );
        let url = key.0.to_string();
        let fetch = runtime::run(cx, async move {
            let cached_url = url.clone();
            let disk_read = disk.clone();
            if let Ok(Some(bytes)) =
                tokio::task::spawn_blocking(move || disk_read.read(&cached_url)).await
            {
                return Some(bytes);
            }
            let _permit = downloads.acquire().await.ok()?;
            let response = http.get(&url).send().await.ok()?.error_for_status().ok()?;
            let bytes = response.bytes().await.ok()?.to_vec();
            let to_write = bytes.clone();
            tokio::task::spawn_blocking(move || disk.write(&url, &to_write));
            Some(bytes)
        });
        let style = key.1;
        cx.spawn(async move |cx| {
            let bytes = fetch.await.flatten();
            let decoded = match bytes {
                Some(bytes) => {
                    cx.background_spawn(async move {
                        let image = decode::decode(&bytes)?;
                        Some(match style {
                            ImageStyle::Plain => (image, None),
                            ImageStyle::Ambient => {
                                let accent = decode::accent(&image);
                                (decode::ambient(&image), accent)
                            }
                            ImageStyle::Logo => (decode::legible_logo(image), None),
                            ImageStyle::LogoOnLight => (decode::legible_logo_on_light(image), None),
                        })
                    })
                    .await
                }
                None => None,
            };
            let decoded = decoded.map(|(image, accent)| {
                let bytes = image.as_raw().len();
                (decode::to_render_image(image), accent, bytes)
            });
            cx.update(|cx| Self::finish(key, decoded, cx));
        })
        .detach();
    }

    fn finish(key: Key, decoded: Option<(Arc<RenderImage>, Option<Hsla>, usize)>, cx: &mut App) {
        let store = cx.global_mut::<ImageStore>();
        let Some(entry) = store.entries.get_mut(&key) else {
            return;
        };
        let new_slot = match decoded {
            Some((image, accent, bytes)) => {
                store.bytes += bytes;
                Slot::Ready {
                    image,
                    accent,
                    bytes,
                }
            }
            None => {
                tracing::debug!("image failed: {}", key.0);
                Slot::Failed
            }
        };
        let waiters = match std::mem::replace(&mut entry.slot, new_slot) {
            Slot::Loading(waiters) => waiters,
            _ => Vec::new(),
        };
        let evicted = store.evict();
        for image in evicted {
            cx.drop_image(image, None);
        }
        for view in waiters {
            cx.notify(view);
        }
    }

    fn evict(&mut self) -> Vec<Arc<RenderImage>> {
        if self.bytes <= MEMORY_BUDGET {
            return Vec::new();
        }
        let now = Instant::now();
        let mut candidates: Vec<(Instant, Key)> = self
            .entries
            .iter()
            .filter(|(_, e)| matches!(e.slot, Slot::Ready { .. }))
            .filter(|(_, e)| now.duration_since(e.last_used) > IN_USE_WINDOW)
            .map(|(k, e)| (e.last_used, k.clone()))
            .collect();
        candidates.sort_by_key(|(used, _)| *used);
        let mut evicted = Vec::new();
        for (_, key) in candidates {
            if self.bytes <= MEMORY_BUDGET * 3 / 4 {
                break;
            }
            if let Some(Entry {
                slot: Slot::Ready { image, bytes, .. },
                ..
            }) = self.entries.remove(&key)
            {
                self.bytes -= bytes;
                evicted.push(image);
            }
        }
        evicted
    }
}
