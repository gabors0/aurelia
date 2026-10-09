# Keyboard navigation, accounts, animations, light theme, settings

The last batch of the v1 checklist. Decided with the user up front:
saved accounts and the server's public users both appear in "Who's watching?",
which opens at launch once two or more accounts are saved; the animations are
page transitions, card hover lift and micro-interactions; and the light theme
is fully light, heroes included.

## Settings (`settings.rs`, `views/pages/settings.rs`)

- `Settings` (a GPUI global) is saved as `~/.config/aurelia/settings.json`:
  `theme` (Dark, Light or System; default Dark), `animations` (on),
  `profile_picker` ("ask who's watching at launch", on) and `hero_autoplay` (on).
  An unreadable file falls back to defaults.
- `Settings::update` saves the file and applies the change at once: theme,
  `cx.set_reduce_motion(!animations)`, and a window refresh.
- `Route::Settings` is a page in the shell, reached from the account menu or
  `Ctrl+,`. It has sections for Appearance (theme), Motion, Home (slideshow),
  Accounts (the launch picker, saved accounts with Remove, Switch user, Add
  account) and Keyboard (a shortcut list).

## Light theme (`theme.rs`)

- `Palette` keeps its API. Every token picks its dark or light value from a
  process-wide appearance flag, so the call sites don't change. The hard-coded
  colours in components become tokens: scrims (`bg_alpha`), the scrolled nav
  bar, panels, body text, controls over the page, inverted pills, shadow
  strength and the art fallback gradient.
- `theme::apply(appearance, cx)` sets the flag, switches gpui-component to the
  matching mode, maps its colours from the palette and refreshes the windows.
  System follows `window.appearance()` (the XDG portal's colour scheme) through
  `observe_window_appearance`.
- Artwork accents are tuned per theme (`Palette::tune_accent`): lightness 0.70
  on dark, about 0.45 on light, so buttons and accent text keep their contrast.
- Ambient images are no longer darkened when decoded; the layers that use them
  tint them instead (a dark veil on dark, a light veil on light; genre tiles
  keep a dark veil because they have white text). One cached image serves both
  themes.
- Logos: `ImageStyle::LogoOnLight` darkens light lettering that nothing dark
  outlines, mirroring how `Logo` lifts dark lettering for dark pages.

## Animations (`components/motion.rs`)

- `Pressable` wraps a `Stateful<Div>`. It delegates `InteractiveElement`,
  `Styled` and `ParentElement` to the div, so `on_click`, `tooltip` and styling
  work as before. It tracks hover (and keyboard focus) in keyed element state
  and renders its look through a GPUI spring (`with_spring`), so the look can
  move between states.
- Card hover lift: posters, episode thumbnails, headshots and genre tiles rise
  6 px, deepen their shadow, and fade in their accent ring and play glyph. This
  also happens when a card has keyboard focus.
- Micro-interactions: pills, nav tabs, the search and refresh buttons and the
  hero buttons ease their colours; the active hero dot stretches; the nav bar
  fades to solid when you scroll; settings switches slide their knob.
- Page transitions: the shell keys the page container by a serial number.
  Opening a page fades it in and lifts it 12 px (220 ms, ease-out); going back
  or forward only fades.
- Turning Animations off sets GPUI's reduce-motion flag: springs and
  transitions jump to their end state, and the Home crossfade, art fade-ins and
  login aurora stop too.

## Keyboard navigation (`components/key_nav.rs`)

- Focusable `Pressable`s are nav targets. Each one tracks a focus handle (kept
  in keyed element state), has the `NavTarget` key context, and records its
  bounds in a per-frame registry. A canvas at the window root clears the
  registry at the start of every frame. Rows record their scroll handle and
  visible bounds. The shell records the page's vertical scroll handle; for the
  library grid that is the uniform list's base handle.
- Arrow keys (`Shell` and `Profiles` contexts) move focus by position on
  screen. Left/right stay in the same row band. Up/down go to the nearest row
  in that direction, then to the card nearest the current x. The nav bar is its
  own zone: you reach it from the top of the page and leave it with Down. With
  nothing focused, an arrow focuses the first visible target, except ←/→ on
  Home, which still change hero slides.
- Enter/Space: a `NoAction` binding in `NavTarget` stops the shell's Enter
  (Play) from taking the key, so GPUI's built-in keyboard click fires the
  target's existing `on_click`. No click handler is duplicated.
- Tab/Shift+Tab come from gpui-base's root (`focus_next`/`focus_prev`). They
  also reach gpui-component buttons (the genre and track pickers, the account
  menu).
- If the focused target stops being drawn (its slide changed, its shelf
  reloaded), `frame_end` hands focus back to the hosting view, so the keys keep
  working. While a hero button has keyboard focus the slideshow pauses, and
  the outgoing slide's buttons can't take focus.
- Revealing: when a target gains focus it scrolls into view, horizontally
  within its row (inside `ROW_PADDING`) and vertically in the page (below the
  nav bar, with a bottom margin big enough for the next grid row to be
  rendered).
- Back restores the target that was focused on the page you return to, found
  again by its element id. A restore retries for a few frames, because the
  first frame after a window opens is drawn again with new focus handles.

## Accounts and "Who's watching?" (`session.rs`, `views/profiles.rs`)

- `SessionStore` keeps `accounts.json` (0600): every saved account plus the
  active one. An existing `session.json` is migrated into it once.
- Each account gets its own Jellyfin device id (`Session::device_id`). The id
  is chosen when sign-in starts, because Quick Connect binds the token to the
  device that started it. A legacy session without one keeps the installation
  id. The server then never confuses two users of the same installation.
- `Session` also keeps the user's avatar tag for the picker.
- `ProfilesView` shows the saved accounts, then any public users
  (`GET /Users/Public`) on the saved accounts' servers who aren't saved yet,
  then "Add account". A saved account opens at once. A server user without a
  password signs in at once; one with a password opens sign-in with server and
  username filled in. Right-clicking a saved account offers "Remove from this
  device", which also revokes its token. Arrow keys, Enter and Esc work there
  too.
- Flow: at launch, with two or more accounts and the setting on, the picker
  shows; otherwise the active account resumes. The account menu gains "Switch
  user…" (picker; Esc returns to the current shell untouched) and "Settings".
  Sign out removes only the current account, then shows the picker if any are
  left. An expired session drops its account and opens sign-in for that user.
  Sign-in offers "Back" when there are accounts to go back to.

## Testing

- Unit tests: spatial navigation (rows, bands, zones, no wrap), account store
  (upsert, active, remove, legacy migration, 0600), settings serde and
  defaults, accent tuning, `LogoOnLight`.
- jellyfin: mock-server test for `public_users`.
- GPUI tests: arrows move focus along a row (no wrapping) and Enter clicks
  the focused item, and a hovered `Pressable` springs to its hovered look.
- `AURELIA_KEYS` types keys inside the app for headless screenshots, since
  `wtype` drops keys.
- Headless sway screenshots: Home and an item page in both themes, the
  settings page, the profile picker, a focused card.
