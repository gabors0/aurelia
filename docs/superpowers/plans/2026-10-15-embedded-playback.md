# Aurelia v2: embedded playback — implementation plan

> Steps use checkbox (`- [x]`) syntax for tracking.

**Status (2026-10-15):** Tasks 1–7 implemented on `v2`. Verified here:
unit, UI and live tests (real mpv, libmpv, and `tests/surface.rs` in a
software-rendered headless sway), plus screenshots of the player via
`AURELIA_PLAY_FILE`. Left for the user: a real GPU, a Jellyfin stream (the
demo server wasn't reachable from the build machine) and KWin.

**Goal:** Video plays inside Aurelia's window under Aurelia's own controls
(libmpv render API → EGL → a `wl_subsurface` below GPUI's transparent
window). External mpv stays as a setting.

**Spec:** `docs/superpowers/specs/2026-10-15-embedded-playback-design.md`
(reviewed 2026-10-15; read its "Review" section first).

**Architecture:** the `player` crate gains a shared control/event surface
(`Control`, new `PlayerEvent`s, `Track`) used by both backends, and an
`embedded` module (libmpv FFI loaded with `dlopen`, `VideoSurface`,
`EmbeddedPlayer`). The app gains `views/player.rs` (`PlayerView`), shown by
the `Shell` instead of the nav and pages, a `player` setting with a
remembered fallback, and remembered volume.

## Global constraints

- Never log or put stream auth headers on a command line. Headers reach mpv
  with `change-list http-header-fields append`; mpv's log is forwarded at
  `warn` and above only.
- The `player` crate stays GPUI-free (it may use `raw-window-handle`).
- libmpv is optional at runtime: missing `libmpv.so.2` → external mpv.
- `Ended` is the last event of a session, exactly once, on both backends.
- Teardown order (embedded): free the render context on the render thread
  with its GL context current → `mpv_terminate_destroy` (last `Arc` of the
  core) → EGL surface/context → `wl_egl_window` → viewport, subsurface,
  surface.
- Colours come from palette tokens; white-on-video is the allowed exception,
  as with artwork.
- This environment has no GPU or compositor: build and unit-test here; the
  user runs on-screen checks (headless sway + grim) and KWin.

## Review focus

1. mpv.conf precedence: our required options win after `mpv_initialize`;
   hwdec default is overridable (`embedded::options` tests).
2. Session lifecycle: `Ended` exactly once for EOF, stop, load error, and
   stop before the file loaded (translator tests + headless libmpv test).
3. Track menus label tracks with Jellyfin titles, and reports follow
   switches (`playback::tracks` tests).
4. The desktop never shows through: opaque until `VideoSurface::is_ready`.
5. Fallback: any embed failure → external mpv, one notice per run.

---

### Task 1: Shared control and event surface (`player`)

**Files:** `crates/player/src/types.rs`, `crates/player/src/control.rs`
(new), `crates/player/src/mpv/ipc.rs`, `crates/player/src/mpv/mod.rs`,
`crates/player/src/lib.rs`, `crates/player/examples/play.rs`.

**Interfaces:**
- `Control::{Pause(bool), Seek(Duration), SeekBy(f64), Volume(f64),
  Mute(bool), Audio(TrackChoice), Subtitle(TrackChoice)}`.
- `trait PlaybackControl { fn stop(&self); fn send(&self, control: Control); }`.
- `control::command(&Control) -> Vec<String>` (mpv command words).
- `Volume { level: f64, muted: bool }`; `PlayRequest.volume: Option<Volume>`.
- `Track { id, kind, title, lang, codec, default, forced, external,
  external_filename, ff_index, selected }`.
- `PlayerEvent::{Duration(Duration), Buffering(bool), Volume(Volume),
  Tracks(Vec<Track>)}` added.
- Observed properties (shared ids): time-pos, pause, duration,
  paused-for-cache, volume, mute, track-list (`ipc::OBSERVED`).

- [x] `control::command` maps each control (`set pause yes`, `seek <s>
  absolute`, `seek <s> relative+exact`, `set volume <v>`, `set mute`,
  `set aid|sid auto|no|<id>`); tests per variant.
- [x] Translator: duration (on change), buffering (`paused-for-cache`),
  volume/mute (combined `Volume` on change), track-list parsing (`Tracks`
  on change); existing tests unchanged; new tests for each.
- [x] `MpvPlayer`: observe all properties, `send` writes the command over
  IPC, `--volume`/`--mute` from the request.
- [x] `play` example reads its header from `$AURELIA_PLAY_HEADER`.
- [x] `cargo test -p player`, clippy; commit.

### Task 2: libmpv FFI and the event bridge (`player::embedded`)

**Files:** `crates/player/Cargo.toml` (+ `libloading`),
`crates/player/src/embedded/{mod.rs,libmpv.rs,events.rs,options.rs}`.

**Interfaces:**
- `libmpv::Lib::load() -> Result<Arc<Lib>, String>` (`$AURELIA_LIBMPV`, else
  `libmpv.so.2`); `Core` (`Arc`-shared handle, `Drop` →
  `mpv_terminate_destroy`) with `set_option`, `set_property`, `command`
  (async), `observe`, `wait_event`, `request_log`.
- `events::to_message(&RawEvent) -> Option<serde_json::Value>`: libmpv events
  → IPC-shaped JSON (`property-change` with `MPV_FORMAT_NODE` → JSON,
  `end-file` reason names and `file_error`, `file-loaded`,
  `playback-restart`, `seek`, `start-file`, `idle`, `shutdown`).
- `options::{BEFORE_INIT, AFTER_INIT}` lists (spec's precedence rules).

- [x] Node → JSON conversion is unit-tested on hand-built `mpv_node`s.
- [x] End-file reason mapping tested.
- [x] Options lists tested (required options appear after init; `hwdec`
  only before; `load-auto-profiles` absent).
- [x] Commit.

### Task 3: `EmbeddedPlayer` session

**Files:** `crates/player/src/embedded/mod.rs`, `crates/player/tests/embedded.rs`.

**Interfaces:**
- `EmbeddedPlayer::load() -> Result<Self, EmbedError>`;
  `play(&self, PlayRequest, Arc<VideoSurface>) -> Result<PlaybackHandle, PlayerError>`;
  `#[doc(hidden)] play_without_video(PlayRequest)` (tests: `vid=no`,
  `ao=null`).
- An event thread per session: `wait_event` → `to_message` → shared
  `EventTranslator`; adds external subtitles on `file-loaded`; ends on
  `end-file` (not redirect), `shutdown`, or `idle` after `stop`; then
  detaches the surface, drops its core, emits `on_exit`.
- `EmbeddedControl` (`send` → async command; `stop` → `stop`); holds the
  core only while the session lives.

- [x] `#[ignore]` (needs libmpv) headless test: `av://lavfi:testsrc` plays
  through `Started`, `Duration`, `Tracks`, `Ended { Eof }`; a second test
  stops early → `Ended { Quit }` exactly once; a bad URL → `Ended { Error }`.
  Run them here inside the dev shell.
- [x] Commit.

### Task 4: `VideoSurface` (`player::embedded::surface`)

**Files:** `crates/player/Cargo.toml` (+ `raw-window-handle`,
`wayland-client`, `wayland-backend` with `client_system`,
`wayland-protocols`, `wayland-egl`, `khronos-egl` dynamic),
`crates/player/src/embedded/surface.rs`.

**Interfaces:**
- `VideoSurface::attach(&(impl HasWindowHandle + HasDisplayHandle)) ->
  Result<Arc<VideoSurface>, EmbedError>` (Wayland only).
- `place(&self, x, y, width, height /* logical */, scale)`; dispatches our
  queue, sets position, viewport destination, opaque region; wakes the
  render thread on size change.
- `is_ready(&self) -> bool`: a buffer has been presented.
- Render thread: EGL 1.5 (no alpha), GL 3.3 core, `swap_interval(0)`; clears
  black until a core is attached; `attach_core(Arc<Core>)` creates the render
  context on the thread; `detach_core()` frees it there (blocking).
- `Drop`: stop the thread (frees the context, EGL), then destroy viewport,
  subsurface, surface; flush.

- [x] Builds; a `#[ignore]` test (needs `WAYLAND_DISPLAY`) makes an xdg
  toplevel, attaches, plays testsrc, waits for `Started` and `is_ready`.
- [x] Commit.

### Task 5: Settings, state and backend choice (app)

**Files:** `crates/aurelia/src/settings.rs`, `state.rs`, `playback/mod.rs`,
`playback/tracks.rs`, `views/pages/settings.rs`.

- [x] `Settings { player: PlayerChoice (Embedded default), volume: f64,
  muted: bool }`; round-trip test incl. old files.
- [x] `AppState`: external player as today; `embedded()` loads libmpv once;
  `embed_failure` remembered for the run.
- [x] `playback::play`: Embedded → attach surface (after item and
  PlaybackInfo arrive) → `EmbeddedPlayer::play` → `shell::open_player`;
  any failure → notice once + external.
- [x] Event loop: forward events to the `PlayerView`; `Tracks` updates the
  reported stream indices (`tracks::reported_indices`, tested); `Volume`
  saved at the end; `Ended` closes the player.
- [x] Settings page: "Playback" section with "Play inside Aurelia" and the
  remembered failure reason; Keyboard section lists player keys.
- [x] Commit.

### Task 6: `PlayerView` and shell integration (app)

**Files:** `crates/aurelia/src/views/player.rs` (new), `views/mod.rs`,
`views/shell.rs`, `views/app.rs`, `theme.rs`, `main.rs`.

- [x] `PlayerView::new(info, control, surface: Option<Arc<VideoSurface>>)`;
  `on_event`; emits `PlayerViewEvent::Close`.
- [x] Layout: video canvas → `surface.place`; opaque black until ready, then
  `theme::set_see_through(true)` (theme + `AppRoot` backgrounds
  transparent); top bar (Back, title, episode line), bottom bar (seek bar
  with played part, times, play/pause, ±10 s, mute + volume bar,
  audio/subtitle menus, fullscreen); spinner while loading/buffering.
- [x] Auto-hide after 3 s idle unless paused/loading; opacity spring.
- [x] `Player` key context: space/k, ←/→, ↑/↓, m, f, Esc (leave fullscreen,
  else close); double-click toggles fullscreen; Shell's Back closes.
- [x] Shell: `open_player`/`close_player`; renders only the player while
  open; `PlayCurrent` ignored; leaving fullscreen and see-through on close.
- [x] UI tests with a fake control: keys → controls, Esc and Back close,
  seek-bar click → `Seek`, controls hide after idle and stay while paused.
- [x] Commit.

### Task 7: Packaging, cleanup, docs

- [x] `flake.nix`: `mpv-unwrapped` in `runtimeLibs` (dev shell and wrapper).
- [x] Delete `crates/probe-embed`.
- [x] CLAUDE.md: player crate's embedded module, PlayerView, settings,
  libmpv runtime, on-screen check notes (green `swaybg`, floating window).
- [x] fmt, clippy `-D warnings`, `cargo test --workspace`, ignored libmpv
  tests; commit; push `v2`.

## Left for the user

- On-screen checks in headless sway + grim, KWin, X11 fallback notice.
- Then: autoplay next, skip intro/credits, trickplay, chapters, idle pointer
  hiding, the upstream gpui-kit shadow opt-out.
