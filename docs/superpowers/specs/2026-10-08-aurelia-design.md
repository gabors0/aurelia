# Aurelia — a cinematic Jellyfin desktop client in GPUI

Date: 2026-10-08 · Status: approved direction, details delegated to implementer

## Intent

A **daily-driver** Jellyfin client for the user's Linux desktop (NixOS, KDE
Plasma, Wayland), replacing the web UI for Movies & TV. It must be reliable
(resume/progress always in sync with the server) and **beautiful** — a
cinematic, artwork-led look built with GPUI.

Success = the user can log in to their server (`https://jellyfin.gs0.me`,
Jellyfin 10.11), see Home (Continue Watching / Next Up / Latest), browse
Movies & Shows libraries, open a series → seasons → episodes, choose audio and
subtitle tracks, press play and have **mpv** start at the resume point, and
after closing mpv see correct progress/played state in Jellyfin (in Aurelia and
in the web UI).

### Decided by the user
- Purpose: daily driver. Scope v1: Movies & TV only (no music, search,
  multi-server).
- Playback: hand off to mpv now; player behind a trait so an embedded (libmpv)
  backend can be added later.
- Player niceties in v1: resume + progress sync, in-app audio/subtitle pick.
- Input: mouse + keyboard shortcuts (no spatial focus navigation).
- Look: cinematic (full-bleed backdrops, hero, poster rows, glassy top nav).
- Stack: GPUI via gpui-kit (`gpui-component`).
- Git: commit locally; create the remote only when asked.

### Decided by the implementer (assumptions)
- Linux first; code stays portable but only Linux is built/tested. mpv IPC uses
  a Unix socket (Windows named pipe is out of scope).
- One server + one user at a time. Session persisted as an access token in
  `~/.config/aurelia/session.json` (0600). Password never stored.
- Login with username/password or Quick Connect.
- Direct play only: mpv receives the original file (`static=true`). No
  server-side transcoding.
- Dark theme only. Deferred: auto-play next episode, version picker, search,
  music, trailers.
- Name: **Aurelia** (genus of the moon jellyfish); binary `aurelia`.

## Architecture

Cargo workspace, three crates:

```
crates/jellyfin   async API client (reqwest + serde). No GPUI. Unit-tested
                  with recorded JSON fixtures + a mock HTTP server; one
                  ignored live test against demo.jellyfin.org.
crates/player     `Player` trait + `MpvPlayer` (spawns mpv, JSON IPC over a
                  Unix socket, emits PlayerEvents). No GPUI, no Jellyfin.
crates/aurelia    The GPUI application (binary).
```

### Stack
- `gpui-component` 0.7.1 on `gpui-pre` 0.3.8 + `gpui-pre-platform` 0.3.8
  (Zed snapshot zed@279fe07, wgpu renderer).
- `reqwest` (rustls) on a dedicated 2-worker Tokio runtime owned by a GPUI
  global; GPUI tasks `await` Tokio `JoinHandle`s.
- `image` for decode/resize/blur; `blurhash` for instant placeholders.
- `tracing` for logs (`AURELIA_LOG=debug`).
- Bundled Inter font (OFL).

### Jellyfin client (`crates/jellyfin`)
`Client { base_url, device: DeviceInfo, token: Option<String> }`, cheap to
clone. Every request sends
`Authorization: MediaBrowser Client="Aurelia", Device="<hostname>", DeviceId="<uuid>", Version="<ver>"[, Token="<token>"]`.

Endpoints (all exist on Jellyfin ≥ 10.9, verified on 10.11 and 12.2):

| Purpose | Endpoint |
|---|---|
| Server probe | `GET /System/Info/Public` |
| Login | `POST /Users/AuthenticateByName` |
| Quick Connect | `GET /QuickConnect/Enabled`, `POST /QuickConnect/Initiate`, `GET /QuickConnect/Connect?secret=`, `POST /Users/AuthenticateWithQuickConnect` |
| Current user | `GET /Users/Me` |
| Libraries | `GET /UserViews?userId=` (keep `movies`, `tvshows`) |
| Continue watching | `GET /UserItems/Resume?userId=&mediaTypes=Video` |
| Next up | `GET /Shows/NextUp?userId=` |
| Latest | `GET /Items/Latest?userId=&parentId=` |
| Browse | `GET /Items?userId=&parentId=&recursive=true&includeItemTypes=&sortBy=&sortOrder=&filters=&startIndex=&limit=&fields=` |
| Item | `GET /Items/{id}?userId=` |
| Seasons / episodes | `GET /Shows/{id}/Seasons`, `GET /Shows/{id}/Episodes?seasonId=` |
| Similar | `GET /Items/{id}/Similar` |
| Played / favourite | `POST`/`DELETE /UserPlayedItems/{id}`, `/UserFavoriteItems/{id}` |
| Playback | `POST /Items/{id}/PlaybackInfo`, `POST /Sessions/Playing`, `/Sessions/Playing/Progress`, `/Sessions/Playing/Stopped` |
| Logout | `POST /Sessions/Logout` |
| Images | `GET /Items/{id}/Images/{Primary|Backdrop/0|Logo|Thumb}?tag=&maxWidth=&quality=` (no auth needed) |
| Stream | `GET /Videos/{id}/stream?static=true&mediaSourceId=&playSessionId=` (auth header) |

Models are tolerant `serde` structs (`#[serde(default)]`, PascalCase) for
`BaseItem`, `UserData`, `MediaSource`, `MediaStream`, `Person`, `UserView`,
`AuthResult`, `PublicSystemInfo`. Time is `Ticks(i64)` (100 ns) with
`Duration` conversions. Errors: `Error::{Http(status), Unauthorized, Network,
Decode}`.

### Player (`crates/player`)
```rust
trait Player { fn play(&self, req: PlayRequest) -> Result<PlaybackHandle>; }
struct PlayRequest { url, title, start: Duration, http_headers: Vec<String>,
                     audio: Option<u32>/* mpv aid */, subtitle: SubtitleChoice }
enum SubtitleChoice { Off, Embedded(u32 /* mpv sid */), External(String /* url */) , Auto }
enum PlayerEvent { Started, Position(Duration), Paused(bool), Seeked(Duration),
                   Ended { position: Duration, reason: EndReason } }
```
`MpvPlayer` spawns `mpv --idle=once --force-window=immediate
--input-ipc-server=$XDG_RUNTIME_DIR/aurelia-mpv-<pid>-<n>.sock`, connects,
sets `http-header-fields` (JSON array node, so commas are safe), `start`,
`aid`, `sid`, `force-media-title`, then `loadfile`. External subtitles are
added with `sub-add <url> select` after `file-loaded`. Observes `time-pos`,
`pause`, `seeking`; `end-file` + process exit produce `Ended`. Jellyfin stream
index → mpv track id: the n-th embedded stream of that type (container order)
is mpv id n (1-based). The token never appears on the mpv command line.

### Playback reporting (`crates/aurelia/src/playback.rs`)
On play: `PlaybackInfo` → `PlaySessionId`; `POST /Sessions/Playing`; forward
events: `Paused`/`Seeked` → immediate progress report, `Position` → progress
report at most every 10 s; `Ended` → `Stopped` with last position (runtime if
EOF). Then the app refreshes visible data (item user data, home rows). The
server decides played/resume using its own thresholds.

### App (`crates/aurelia`)
- `state`: `AppState` global entity — `Session { server, user, token,
  device_id }`, library views, Tokio runtime handle, Jellyfin client.
- `images`: `ImageStore` — request `(url, kind)` → `Arc<RenderImage>`;
  memory LRU + disk cache in `~/.cache/aurelia/images/<sha>`; kinds: `Plain`,
  `Ambient` (downscaled, heavy gaussian blur, darkened) and accent-colour
  extraction. Blurhash decoded synchronously for placeholders. Images fade in.
- `nav`: back/forward stack of `Route::{Home, Library(view), Item(id)}`.
- `views`: `login`, `shell` (title bar + nav + content + toasts), `home`,
  `library`, `item` (movie/episode), `series` (seasons + episodes).
- `components`: `hero`, `poster_card` (portrait/landscape), `row`
  (horizontal scroller with hover arrows), `progress`, `glass`, `track_picker`,
  `skeleton`, `meta_chips`.
- `theme`: tokens (bg `#0A0B10`, glass `white/6%`, border `white/9%`, text
  `#F2F3F7` / `#A3A7B5` / `#6B7080`, accent fallback lilac `#B69CFF`), type
  scale, radii.
- Data on every page is `Loadable<T> = Loading | Ready(T) | Failed(String)`.

### Screens
- **Login**: glass card over a slowly drifting aurora gradient. Step 1 server
  URL (pre-filled `https://jellyfin.gs0.me`), validated via public info
  ("s920 · Jellyfin 10.11.11"). Step 2 username/password or Quick Connect code.
- **Shell**: custom client-side title bar that doubles as the top nav: logo,
  Home + one tab per movies/tvshows library, avatar menu (sign out), window
  controls. Content scrolls underneath.
- **Home**: hero carousel (up to 5 items from resume/latest, 8 s crossfade,
  dots, pauses on hover; backdrop + logo or title + meta + overview + Play/
  Resume + More info). Rows: Continue Watching (landscape w/ progress), Next Up,
  Latest in each library (posters).
- **Library**: header (name, count), sort (name, date added, release date,
  rating) and filter (all/unwatched/favourites); responsive virtualised poster
  grid with paging (100 per page).
- **Item (movie/episode)**: ambient blurred background, large backdrop with
  bottom fade, logo/title, meta chips (year, runtime, ★, official rating,
  4K/HDR/DV/Atmos/5.1 badges), genres, overview, Resume (with "ends at"),
  Play from start, played + favourite toggles, audio/subtitle pickers
  (pre-selected from server defaults), cast row, More like this.
- **Series**: same hero with "Play S2:E3" (next up), season tabs, episode list
  (thumb, number, title, runtime, overview, played tick, progress, hover play).

### Keyboard
Esc / Alt+← back, Alt+→ forward, Enter play on item pages, Ctrl+R refresh,
Ctrl+Q quit. Mouse back/forward buttons navigate.

## Error handling
- Network/HTTP errors → per-section inline error with Retry; action failures
  (toggle played, start playback) → toast.
- 401 from any call → session cleared, back to login with "Session expired".
- Server unreachable at startup with a saved session → offline panel with
  Retry and Sign out (session kept).
- mpv missing or crash → toast; playback reporter still sends `Stopped`.
- Image errors → blurhash/gradient placeholder stays.

## Testing
- `jellyfin`: fixture deserialisation tests, request-shape tests against a
  local mock server (wiremock), `#[ignore]` live test against the demo server.
- `player`: IPC message encoding/decoding, track-id mapping, event parsing;
  `#[ignore]` test that spawns real mpv headless (`--vo=null --ao=null`) on a
  generated file.
- `aurelia`: unit tests for pure logic (nav stack, Loadable, formatting,
  accent extraction, cache keys, playback reporter throttling). UI verified by
  running against the demo server (dev auto-login env vars) and screenshots.

## Packaging
`flake.nix`: dev shell (rust toolchain, pkg-config, wayland, libxkbcommon,
vulkan-loader, X11 libs, fontconfig, mpv) and `packages.default`
(`rustPlatform.buildRustPackage`, wrapped with runtime libs + mpv on PATH),
plus a `.desktop` file and icon.
