# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Aurelia is a Jellyfin desktop client (Movies & TV) in Rust on GPUI via `gpui-kit` 0.7.1 (which pins `gpui-pre` 0.3.8, a Zed snapshot). Video plays in an external mpv. Linux/NixOS is the target platform.

## Commands

Everything runs inside the Nix dev shell (`rustc` is not on the system PATH; GPUI needs the shell's Vulkan/Wayland libraries at runtime):

```sh
nix develop -c cargo build -p aurelia
nix develop -c cargo run -p aurelia
nix develop -c cargo test --workspace                 # unit, mock-server and GPUI UI tests
nix develop -c cargo test --workspace -- --ignored    # live demo server + real mpv tests
nix develop -c cargo test -p aurelia carousel         # one module/test by name filter
nix develop -c cargo clippy --workspace --all-targets
nix develop -c cargo fmt --all
nix build                                             # release package: result/bin/aurelia (+ .desktop, icon)
```

- `cargo test` does not rebuild the `target/debug/aurelia` binary; run `cargo build` before launching the app after test-only runs.
- The `jellyfin` and `player` crates don't depend on GPUI. Test them in a separate target dir so they don't wait on the app build lock: `CARGO_TARGET_DIR=target/jf nix develop -c cargo test -p jellyfin`.
- The first GPUI build takes ~12 minutes; the `gpui-kit` `test-support` dev-dependency builds a second copy of the GPUI stack for `cargo test -p aurelia`.
- `cargo run -p player --example play -- <url> [header] [start-secs] [subtitle-url]` drives the mpv backend by hand.

## Running and inspecting the UI

- Don't drive the GUI with xdotool/ydotool: on the user's KDE desktop it triggers a "Remote Control" permission prompt.
- Don't screenshot with `spectacle -a` while the user is at the machine; it captures whatever window has focus. Run the app in a headless compositor instead, e.g. `WLR_BACKENDS=headless WLR_RENDERER=gles2 WLR_RENDER_DRM_DEVICE=/dev/dri/renderD128 sway -c <conf>` (with `output * resolution 2400x1500 scale 1.25` and an `exec` that starts the app and calls `grim`). Unset `DISPLAY` so GPUI can't fall back to the user's X server.
- Keep test sessions away from the user's real config with `XDG_CONFIG_HOME`/`XDG_CACHE_HOME`.
- Development hooks (`crates/aurelia/src/dev.rs`, `views/app.rs`, `views/shell.rs`, `views/pages/item.rs`): `AURELIA_SERVER`/`AURELIA_USER`/`AURELIA_PASSWORD` auto sign-in (server alone pre-fills and probes it), `AURELIA_ROUTE=item:<id>|series:<id>|library:<id>|collection:<id>|person:<id>|genre:<name>|search:<query>`, `AURELIA_AUTOPLAY=1`, `AURELIA_SCROLL_Y=<px>`, `AURELIA_MPV=<path>`, `AURELIA_LOG=<tracing filter>`.
- Clicks in the headless compositor need a virtual pointer that lives for the whole session (a small pywayland client on `zwlr_virtual_pointer_v1`). `wlrctl pointer click` and `swaymsg seat … cursor press` create or lack a pointer device per command, so the app's `wl_pointer` misses the button; hover still works.
- Test against the public demo server `https://demo.jellyfin.org/stable` (user `demo`, empty password). It is a shared account (other people change its watch state) and it serves streams without auth, so auth behaviour must be tested elsewhere (e.g. a local server that returns 401 without the header).
- Use `pkill -x aurelia` to stop the app; `pkill -f <pattern>` can match and kill the calling shell itself.

## Architecture

Cargo workspace with three crates:

- `crates/jellyfin`: typed async Jellyfin client (reqwest + serde), no UI. `Client` is cheap to clone and carries device info and the optional session token; every request sends the `MediaBrowser Client=…, Token=…` `Authorization` header. Models default every field because Jellyfin omits or nulls fields freely. Image helpers on `BaseItem` (`backdrop_image`, `poster_image`, `landscape_image`, …) fall back to parent/series art and carry blurhashes. Tests use recorded fixtures in `tests/fixtures` plus `wiremock`.
- `crates/player`: the `Player` trait (the seam for a future embedded backend) and `MpvPlayer`. mpv is spawned with `--idle=once` and controlled over a JSON IPC Unix socket; the stream URL and the auth header are sent over IPC, never on the command line (`change-list http-header-fields append` keeps commas in the header intact). `mpv/ipc.rs` translates mpv's event stream into `PlayerEvent`s and is pure/testable; `Ended` is always the last event, exactly once.
- `crates/aurelia`: the GPUI app.

How the app fits together:

- **Async:** network calls run on a small Tokio runtime (`runtime::api` / `runtime::run`) and are awaited from GPUI tasks (`cx.spawn`, `cx.spawn_in`).
- **Global state:** `state::AppState` is a GPUI `Global` holding the session, the signed-in `Client`, the user's libraries and the player. `session::SessionStore` persists the token (0600) under `~/.config/aurelia`, never the password.
- **Screens:** `views/app.rs` (`AppRoot`) switches between starting, login, main shell and offline, and handles sign-in, sign-out and session expiry.
- **Shell and pages:** `views/shell.rs` (`Shell`) owns a `nav::Nav<Page>` history that keeps page entities alive, so going back restores them instantly. Pages reach the shell through the `ShellHandle` global helpers (`shell::navigate`, `report_error`, `load_failed`, `refresh_all`); `load_failed` turns a 401 into session expiry. Key bindings live in the `Shell` key context.
- **Page types:** `views/pages/` holds Home, Library (a library, optionally one genre of it, or a genre across all libraries), Item (movie/episode), Series, Collection, Person and Search; `Page` is the enum the shell dispatches over (refresh, scroll offset, primary action, focus, user-data patches). Page data is `loadable::Loadable<T>`. Refreshes swap new data in without blanking what's shown.
- **Watched/favourite:** always go through `user_data::set_played`/`set_favorite`. They patch the item in every page in the history (`shell::patch_user_data` → `Page::patch_user_data`), call the server, apply its answer or roll back, then `refresh_all` so derived shelves follow. `components::item_menu` builds the right-click menu for any item and is attached to poster cards and episode rows.
- **Images:** `images::ImageStore` downloads, decodes and (for the `Ambient` style) blurs artwork off the UI thread. It caches in memory (byte-budget LRU, evicted with `cx.drop_image`) and on disk (`~/.cache/aurelia/images`), and re-renders only the views that asked for an image (`window.current_view()` waiters). Render artwork through `components::art::Art`, which shows the blurhash placeholder and fades the image in.
- **Playback:** `playback::play` fetches the item and `PlaybackInfo`, maps the chosen Jellyfin audio/subtitle indices to mpv track ids (`playback/tracks.rs`), launches mpv, and feeds `PlayerEvent`s through `playback/reporter.rs`, which throttles progress reports to the `/Sessions/Playing*` endpoints. When playback ends it refreshes every page. Starting new playback stops the previous mpv (`NowPlaying` global).
- **Look:** `theme.rs` holds the palette tokens and bundled Inter fonts and restyles gpui-component. Each page's accent colour comes from its artwork (`ImageState::Ready { accent }`).

## GPUI gotchas found in this codebase

- A container that scrolls on one axis converts the other axis's wheel motion unless it has `.restrict_scroll_to_axis()`. Use `components::scroller::page()` for page scrollers; shelves (`components::row`) already restrict.
- `overflow_hidden` does not clip to rounded corners; images clip to their own radius (`Art::radius`).
- Semi-transparent overlays above text let the text show through far more than their alpha suggests (the transparent window background); keep bars that cover scrolled text fully opaque.
- Stretched textures can show seams at their edges on some setups; keep generated gradient images larger than the area they light (`components/aurora.rs`).
- gpui-component's root already wraps the window in `window_border` for client-side decorations; don't add another.
- Use `gpui_kit::assets::AllAssets` and `gpui_kit::assets::IconName` for the full Lucide icon set.
- UI integration tests use `#[gpui_kit::test]` with `gpui_kit::test::TestWindowExt`; mark elements to target with `.test_support()` (a no-op outside tests). See `components/row.rs`.

## Jellyfin quirks

- `/Items/Latest` returns a bare array, not `{ Items: [...] }`.
- The server ignores resume positions for items shorter than 5 minutes or under 5% played.
- Mixed-content libraries have no `CollectionType`; `UserView::is_video_library` includes them.
