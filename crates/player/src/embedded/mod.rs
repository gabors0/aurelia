//! Playback inside the app's window: a libmpv core renders with OpenGL into
//! a [`VideoSurface`] (a Wayland subsurface below the app's own surface), and
//! the app draws its controls on top.
//!
//! libmpv is loaded at runtime ([`EmbeddedPlayer::load`]); without it, or
//! without Wayland, the app falls back to [`crate::MpvPlayer`].

mod events;
mod libmpv;
mod options;
mod surface;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use self::libmpv::{Core, Lib};
pub use self::surface::VideoSurface;
use crate::control::{self, sub_add};
use crate::mpv::ipc::{EventTranslator, OBSERVED};
use crate::{Control, PlayRequest, PlaybackControl, PlaybackHandle, PlayerError, TrackChoice};

/// Why playback can't happen inside the window.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EmbedError {
    #[error("libmpv couldn't be loaded ({0})")]
    Library(String),
    #[error("the window isn't on Wayland")]
    NotWayland,
    #[error("the video surface couldn't be set up ({0})")]
    Surface(String),
}

/// Plays media into a [`VideoSurface`].
#[derive(Clone)]
pub struct EmbeddedPlayer {
    lib: Arc<Lib>,
}

impl std::fmt::Debug for EmbeddedPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EmbeddedPlayer")
    }
}

impl EmbeddedPlayer {
    /// Loads libmpv (`$AURELIA_LIBMPV`, else `libmpv.so.2`).
    pub fn load() -> Result<Self, EmbedError> {
        Lib::load()
            .map(|lib| Self { lib })
            .map_err(EmbedError::Library)
    }

    /// Plays `request` into `surface`. The surface's render context is set
    /// up before this returns, so a GL failure is reported here.
    pub fn play(
        &self,
        request: PlayRequest,
        surface: Arc<VideoSurface>,
    ) -> Result<PlaybackHandle, PlayerError> {
        self.start(request, Some(surface), &[])
    }

    /// Plays with mpv's null outputs: the whole session, events and
    /// controls, without a window. For tests.
    #[doc(hidden)]
    pub fn play_without_video(&self, request: PlayRequest) -> Result<PlaybackHandle, PlayerError> {
        self.start(request, None, &[("vo", "null"), ("ao", "null")])
    }

    fn start(
        &self,
        request: PlayRequest,
        surface: Option<Arc<VideoSurface>>,
        overrides: &[(&str, &str)],
    ) -> Result<PlaybackHandle, PlayerError> {
        let fail = |message: String| PlayerError::Embedded(message);
        let core = Arc::new(Core::create(self.lib.clone()).map_err(fail)?);

        for (name, value) in options::DEFAULTS
            .iter()
            .chain(options::REQUIRED)
            .chain(options::BEFORE_INIT)
            .chain(overrides)
        {
            if let Err(err) = core.set_option(name, value) {
                // Older mpv lacks some (e.g. `load-context-menu`).
                tracing::debug!(target: "mpv", "option not set: {err}");
            }
        }
        core.initialize().map_err(fail)?;
        // mpv.conf was read during init; put back what Aurelia relies on.
        for (name, value) in options::REQUIRED.iter().chain(overrides) {
            if let Err(err) = core.set_property(name, value) {
                tracing::debug!(target: "mpv", "option not reapplied: {err}");
            }
        }
        for name in options::overridden(|name| core.get_property(name)) {
            tracing::warn!(target: "mpv", "mpv.conf sets `{name}`; Aurelia can't turn it off");
        }
        // Warnings and errors only: verbose levels echo option values.
        let _ = core.request_log("warn");
        for (id, name) in OBSERVED {
            core.observe(id, name).map_err(fail)?;
        }

        if let Some(surface) = &surface {
            surface.attach_core(core.clone()).map_err(fail)?;
        }

        let detach = |surface: &Option<Arc<VideoSurface>>| {
            if let Some(surface) = surface {
                surface.detach_core();
            }
        };
        if let Err(err) = load(&core, &request) {
            detach(&surface);
            return Err(fail(err));
        }

        let (events_tx, events_rx) = async_channel::unbounded();
        let control = Arc::new(EmbeddedControl {
            core: Mutex::new(Some(core.clone())),
            stopping: AtomicBool::new(false),
        });
        let session = Session {
            core,
            control: control.clone(),
            surface,
            request,
            events: events_tx,
        };
        std::thread::Builder::new()
            .name("mpv-embedded".into())
            .spawn(move || session.run())
            .map_err(|err| fail(err.to_string()))?;
        Ok(PlaybackHandle::new(events_rx, control))
    }
}

/// Per-file settings, then `loadfile`.
fn load(core: &Core, request: &PlayRequest) -> Result<(), String> {
    let set = |name: &str, value: &str| core.set_property(name, value);
    set("force-media-title", &request.title)?;
    set("start", &format!("{:.3}", request.start.as_secs_f64()))?;
    set("aid", &control::track(request.audio))?;
    let selects_external = request.external_subtitles.iter().any(|s| s.select);
    let subtitle = if selects_external {
        TrackChoice::Off
    } else {
        request.subtitle
    };
    set("sid", &control::track(subtitle))?;
    if let Some(volume) = request.volume {
        set("volume", &format!("{:.0}", volume.level.clamp(0.0, 100.0)))?;
        set("mute", if volume.muted { "yes" } else { "no" })?;
    }
    for header in &request.http_headers {
        // `change-list append` adds one entry verbatim; `set` would split
        // the value on commas.
        core.command(&[
            "change-list".into(),
            "http-header-fields".into(),
            "append".into(),
            header.clone(),
        ])?;
    }
    core.command(&["loadfile".into(), request.url.clone()])
}

struct EmbeddedControl {
    /// Present while the session runs.
    core: Mutex<Option<Arc<Core>>>,
    stopping: AtomicBool,
}

impl PlaybackControl for EmbeddedControl {
    fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        if let Some(core) = self.core.lock().unwrap().as_ref() {
            let _ = core.command(&["stop".into()]);
            // In case it's waiting with nothing loaded.
            core.wakeup();
        }
    }

    fn send(&self, control: Control) {
        if let Some(core) = self.core.lock().unwrap().as_ref()
            && let Err(err) = core.command(&control::command(&control))
        {
            tracing::warn!(target: "mpv", "{err}");
        }
    }
}

/// One playback, from `loadfile` to the end of the file; runs on its own
/// thread, the only one waiting for the core's events.
struct Session {
    core: Arc<Core>,
    control: Arc<EmbeddedControl>,
    surface: Option<Arc<VideoSurface>>,
    request: PlayRequest,
    events: async_channel::Sender<crate::PlayerEvent>,
}

impl Session {
    fn run(self) {
        let mut translator = EventTranslator::default();
        let mut file_active = false;
        loop {
            let event = self.core.wait_event(0.5);
            match event.event_id {
                libmpv::EVENT_NONE => {
                    // Asked to stop before any file started: nothing will end.
                    if self.control.stopping.load(Ordering::SeqCst) && !file_active {
                        translator.on_stop();
                        break;
                    }
                    continue;
                }
                libmpv::EVENT_LOG_MESSAGE => {
                    log(event);
                    continue;
                }
                libmpv::EVENT_COMMAND_REPLY if event.error < 0 => {
                    // Not the command itself: it may carry a header.
                    let error = self.core.lib().error_string(event.error);
                    tracing::warn!(target: "mpv", "command failed: {error}");
                    continue;
                }
                _ => {}
            }
            // SAFETY: the event is the latest from wait_event.
            let Some(message) = (unsafe { events::to_message(self.core.lib(), event) }) else {
                continue;
            };
            let reaction = translator.on_message(&message);
            if reaction.file_loaded {
                for sub in &self.request.external_subtitles {
                    let _ = self.core.command(&sub_add(sub));
                }
            }
            for event in reaction.events {
                let _ = self.events.send_blocking(event);
            }
            match message["event"].as_str() {
                Some("start-file") => file_active = true,
                Some("end-file") if message["reason"] != "redirect" => break,
                Some("shutdown") => break,
                Some("idle") if self.control.stopping.load(Ordering::SeqCst) && !file_active => {
                    translator.on_stop();
                    break;
                }
                _ => {}
            }
        }

        // No more commands; free the render context (on the render thread)
        // before the core goes, then let the core go.
        self.control.core.lock().unwrap().take();
        if let Some(surface) = &self.surface {
            surface.detach_core();
        }
        let Session {
            core,
            surface,
            events,
            ..
        } = self;
        drop(surface);
        drop(core);
        let _ = events.send_blocking(translator.on_exit(None));
    }
}

fn log(event: &libmpv::RawEvent) {
    // SAFETY: log events carry an mpv_event_log_message with C strings.
    let message = unsafe { &*(event.data as *const libmpv::LogMessage) };
    let text = |p: *const std::ffi::c_char| {
        if p.is_null() {
            String::new()
        } else {
            // SAFETY: valid for the event's lifetime.
            unsafe { std::ffi::CStr::from_ptr(p) }
                .to_string_lossy()
                .trim_end()
                .to_string()
        }
    };
    let (prefix, level, body) = (
        text(message.prefix),
        text(message.level),
        text(message.text),
    );
    match level.as_str() {
        "fatal" | "error" => tracing::error!(target: "mpv", "{prefix}: {body}"),
        _ => tracing::warn!(target: "mpv", "{prefix}: {body}"),
    }
}
