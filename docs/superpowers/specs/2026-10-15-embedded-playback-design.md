# v2: embedded playback — design

Status: **reviewed with the user 2026-10-15.** Written after a feasibility probe;
the review's changes are folded in below and listed under "Review". Plan:
`docs/superpowers/plans/2026-10-15-embedded-playback.md`.

## Goal

Video plays inside Aurelia's window, under Aurelia's own controls, instead of
in a separate mpv window.

### Decided with the user (2026-10-15)

- **Scope: core player first.** Play/pause, seek (bar, ±10 s), volume/mute,
  fullscreen, audio and subtitle switching, leave with Back/Esc. Autoplay of the
  next episode, skip intro/credits (media segments), trickplay seek previews and
  chapters are **out of scope for now**; ask the user about them once the core
  player works.
- **External mpv stays as a setting.** Embedded is the default; Settings can
  switch back to the standalone mpv window (today's `MpvPlayer`).
- **The user's `mpv.conf` applies** (hwdec, shaders, profiles, language
  preferences), but not keybindings, the OSC or Lua/JS scripts, because Aurelia
  owns the controls.
- **Playback takes over the window.** It's a full-window player view with
  overlay controls; Back/Esc returns to where the user was. There's no
  mini-player or picture-in-picture.

## How video reaches the screen

GPUI (gpui-pre 0.3.8) has no external-texture or video primitive on Linux:
`paint_surface` takes a macOS `CVPixelBuffer`, and the wgpu renderer is
private. These approaches were considered:

1. **Wayland subsurface (chosen).** libmpv's render API (OpenGL, `vo=libmpv`)
   draws with EGL into a `wl_subsurface` of GPUI's window surface, placed
   *below* it. GPUI leaves the video area transparent and draws the controls on
   top. Zero-copy, hardware decoding, and full mpv rendering quality.
2. CPU frame copy (libmpv SW renderer → `RenderImage` each frame). Portable, but
   4K means gigabytes per second of copies plus atlas churn.
3. Forking GPUI's wgpu renderer to import dmabufs. Best integration, but a fork
   of a pinned Zed snapshot to maintain.

### Feasibility probe: it works

`crates/probe-embed` (THROWAWAY; delete when the real implementation lands) is a
GPUI window with the subsurface underneath. In headless sway (gles2,
2400×1500 @ 1.25, floating window over a green background) it showed:

- mpv's `av://lavfi:testsrc2` and a real demo-server stream
  (`/Videos/<id>/stream?static=true`, h264 1080p) render in the subsurface at
  the bounds GPUI laid out, and mpv letterboxes them.
- GPUI content draws over the video, and translucent fills (60 % black bar)
  blend correctly.
- Fractional scale: buffer = logical × scale, and `wp_viewport.set_destination`
  sets the logical size. Looks sharp at 1.25.
- Resizing works (the subsurface follows the GPUI layout's bounds).
- mpv loads its VA-API EGL dmabuf interop (`GL_EXT_EGL_image_storage`) on the
  Intel GPU.

Findings to act on:

- **gpui-component's root plugin paints `theme.colors.background`** over the
  whole window. The probe had to set it to transparent. The player has to keep
  everything above the video area transparent: the theme background,
  `AppRoot`/`Shell` backgrounds and the page fade.
- The probe window looked slightly **dimmed** overall: green showed darker
  inside the window bounds and the video looked washed out. **Found:**
  gpui-component's `window_border` gives the content div two 18 % black
  `BoxShadow`s, and GPUI's shadow shader fills the whole blurred rectangle,
  not just the part outside the element, so transparent content shows ~33 %
  black. It's only drawn while the window isn't tiled (fullscreen and
  maximized count as tiled), and the app can't turn it off without forking
  gpui-component. **Decision: accept it in floating windows for now**;
  fullscreen, maximized and tiled playback are clean. Fix upstream later
  (an opt-out for the content shadow in gpui-kit).
- **hwdec:** with `hwdec=auto-safe` mpv tried `h264-vulkan` first
  (`VK_KHR_video_decode_queue` missing), failed and fell back to *software*
  decoding instead of trying VA-API. Use an explicit order (e.g.
  `hwdec=vaapi,auto-safe`) unless the user's `mpv.conf` sets `hwdec`.
- Untested: **KWin** (the user's desktop), X11 sessions, multi-monitor
  scale changes, HDR. KWin must be checked by the user. Don't run test windows
  on their desktop (see CLAUDE.md).

How the probe attaches (keep this in the real code):

- `Window` implements `raw_window_handle::HasWindowHandle` /
  `HasDisplayHandle` and returns the `wl_surface*` and `wl_display*`.
- `wayland_backend::client::Backend::from_foreign_display` (GPUI already enables
  `client_system`) gives a `Connection` on GPUI's display. Use our **own event
  queue** (`registry_queue_init`) and bind `wl_compositor`,
  `wl_subcompositor` and `wp_viewporter`. The parent surface proxy is
  `ObjectId::from_ptr(WlSurface::interface(), ptr)`.
- Child surface: `place_below(parent)`, `set_desync()`, an empty input region
  (pointer input goes to GPUI), and a viewport. `set_position` and
  `place_below` apply on the parent's next commit, which GPUI makes when it
  draws.
- Render thread: EGL 1.5 (`khronos-egl` dynamic), `get_platform_display(
  EGL_PLATFORM_WAYLAND_KHR, wl_display)`, an OpenGL 3.3 core context,
  `wayland-egl` window, `swap_interval(0)` (a hidden surface gets no frame
  callbacks, so blocking swaps could deadlock). mpv render params: API type
  `"opengl"`, `get_proc_address` → `eglGetProcAddress`, `WL_DISPLAY` (for hwdec
  interop), `FLIP_Y=1`, FBO 0. The update callback signals a condvar, and the
  thread renders and swaps on each signal or resize.
- libmpv is `dlopen`ed (`libloading`, `libmpv.so.2`), so Aurelia still starts
  without it and falls back to external mpv.

## Architecture

### `player` crate (no GPUI)

- `embedded/libmpv.rs`: minimal FFI over `libmpv.so.2` (create, options,
  `command_async`, `set_property`, `observe_property`, `wait_event`, render
  context functions) with a safe `Mpv` wrapper. ~20 symbols, so no bindgen.
- `embedded/surface.rs`: `VideoSurface::attach(raw display, raw parent
  surface)` → the subsurface, viewport and render thread; `place(rect_logical,
  scale)`; `Drop` tears down in order (free the render context on the render
  thread with its context current, destroy EGL, then the subsurface and surface).
- `embedded/mod.rs`: `EmbeddedPlayer`. Owns the mpv core and an event thread
  that translates mpv events/properties into `PlayerEvent`s (pure translator,
  unit-tested like `mpv/ipc.rs`).
  - Options: `config=yes` reads `~/.config/mpv/mpv.conf` **inside
    `mpv_initialize`, after and over anything set before it** (mpv 0.41:
    `mp_parse_cfgfiles` runs with no preserve flag). So:
    - set before init as overridable defaults: `hwdec=vaapi,auto-safe`;
    - set before init and again after it, because Aurelia depends on them:
      `vo=libmpv`, `osd-level=0` (mpv's OSD is replaced by ours),
      `input-default-bindings=no`, `input-vo-keyboard=no`, `ytdl=no`,
      `keep-open=no`, `idle=yes`, `terminal=no`, and the built-in scripts
      `osc`, `load-stats-overlay`, `load-console`, `load-select`,
      `load-positioning`, `load-commands`, `load-context-menu` = `no` (mpv
      loads and unloads built-in scripts as these options change);
    - set before init only: `load-scripts=no` (the user's own scripts load
      during init). If mpv.conf turns it back on, log a warning;
    - **`load-auto-profiles` stays on**, so conditional profiles
      (`profile-cond`) in mpv.conf still apply.
    - `keep-open=no` (not `yes`): with `yes` mpv never sends `end-file` at
      EOF, so `Ended { Eof }` would never arrive.
    Headers, start time and tracks as in `MpvPlayer` (headers via
    `change-list http-header-fields append`, never logged; mpv's log is
    forwarded at `warn` and above only).
  - **One translator for both backends.** libmpv events are converted into
    the JSON shape of mpv's IPC messages (which is how mpv's own IPC builds
    them), so `mpv/ipc.rs`'s `EventTranslator` serves both.
- Shared API changes:
  - `PlaybackControl` grows `send(Control)`, with `Control::{Pause(bool),
    Seek(Duration), SeekBy(f64), Volume(f64), Mute(bool),
    Audio(TrackChoice), Subtitle(TrackChoice)}`. One pure function maps a
    `Control` to an mpv command, used by both backends (IPC for `MpvPlayer`),
    so they share one control surface. One method keeps test fakes trivial.
  - `PlayerEvent` grows `Duration(Duration)`, `Buffering(bool)`,
    `Volume { level, muted }` and `Tracks(Vec<Track>)`, from mpv's
    `track-list` (id, kind, title, language, codec, default, forced,
    external file name, `ff-index`, selected). The app labels tracks with
    Jellyfin's display titles by matching `ff-index` (embedded) or the
    subtitle URL (external), and keeps the playback reports' stream indices
    in step when the user switches. The reporter ignores the new variants.
  - `PlayRequest` grows `volume: Option<Volume>`: the remembered level and
    mute, applied by both backends.
  - `Ended` stays the last event, exactly once.

### `aurelia` app

- `views/player.rs`: `PlayerView`, a full-window view the `Shell` shows
  *instead of* the nav bar and pages while playing. It isn't in `Nav` history,
  so the page underneath stays alive and Back restores it.
  - Video area = the whole content area. A `canvas` reports its bounds each
    frame to `VideoSurface::place`.
  - Controls overlay (Aurelia look, palette tokens; white-on-video is the
    exception, as with artwork): top bar (Back, title, episode label), bottom
    bar (seek bar with buffered/played, times, play/pause, ±10 s, volume,
    audio/subtitle menus, fullscreen). Auto-hides after ~3 s without pointer
    movement and stays visible while paused or loading. GPUI 0.3.8 has no
    public way to hide the pointer on a timer (it hides it on key presses
    itself), so idle pointer hiding is deferred.
  - Until the subsurface has presented its first frame, the player is opaque
    black; only then do the backgrounds turn transparent, so the desktop
    never shows through. The EGL config has no alpha and the subsurface an
    opaque region.
  - The controls are mouse-only (not keyboard-nav targets); keys go through
    the `Player` context, so Space never both presses a button and pauses.
  - Keys (a `Player` key context): Space/K play-pause, ←/→ ±10 s, ↑/↓ volume,
    M mute, F or double-click fullscreen, Esc leaves fullscreen and otherwise
    leaves the player, Back (mouse/Alt+←) leaves the player.
  - While shown, the theme background and shell backgrounds are transparent
    (see the findings above). Everything else stays opaque as before.
- `playback/mod.rs`: `play` picks the backend from settings and availability,
  and opens `PlayerView` for the embedded one. Reporting (`reporter.rs`),
  `NowPlaying` and `refresh_all` on end stay as they are. Leaving the player
  stops playback; playback ending closes the player.
- `settings`: `player: Embedded | External` (default Embedded), shown in
  Settings, plus the remembered `volume` and `muted` (decided: remembered
  across runs, overriding mpv.conf's volume). If Embedded isn't possible
  (not Wayland, `libmpv.so.2` missing, surface or EGL setup failed), fall back
  to external mpv with a notice saying why; the reason is remembered for the
  rest of the run, so the notice shows once and setup isn't retried.
- `flake.nix`: add `mpv-unwrapped`'s lib to `runtimeLibs` (dev shell
  `LD_LIBRARY_PATH` and the package wrapper) so `dlopen` finds `libmpv.so.2`.

## Review (2026-10-15)

Agreed changes, all folded in above: the window-shadow cause of the dimming
(accepted for floating windows), mpv.conf precedence and which options are
re-applied, auto profiles kept, `keep-open=no`, one translator for both
backends, `Control` + `Tracks(Vec<Track>)`, volume remembered across runs,
opaque until the first frame, mouse-only controls, the fallback reason
remembered per run. Also: dispatch our own Wayland event queue (surface
`enter`/`preferred_buffer_scale` events otherwise pile up), and the player
example reads its auth header from the environment, not its command line.

## Error handling

- libmpv load or initialization failure, EGL or subsurface failure → external
  mpv with a notice. If that's also missing, today's "mpv wasn't found" message.
- Stream errors → `Ended { reason: Error }` → the existing "Playback failed: …"
  message, and the player closes.
- 401 while preparing → `load_failed`/session expiry as today.

## Testing

- Unit: the mpv event/property translator; the control → command mapping; the
  settings round trip.
- GPUI UI tests (`#[gpui_kit::test]`): `PlayerView` with a fake
  `PlaybackControl`/event stream (no video): controls show and hide, keys
  call the right controls, Esc and Back leave, the seek bar maps clicks to
  positions.
- `--ignored` live: `EmbeddedPlayer` against `av://lavfi:testsrc2`, which needs
  a Wayland compositor (headless sway). Checks events through `Ended`.
- Manual: the headless sway + `grim` workflow from CLAUDE.md. Note: give sway
  `output * bg #00aa00 solid_color` (needs `swaybg`) and float the window to see
  transparency mistakes. The user checks KWin.
