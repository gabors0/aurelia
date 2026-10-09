# Search, context menus, people, collections, genres, favourites

Second batch of the v1 checklist. Autoplay-next is deferred to v2 (it fits the
embedded player better).

## What ships

- **Search** (`Route::Search`): a search button in the nav bar and `Ctrl+F` / `/`
  open a page with one large input. Typing searches as you go (250 ms debounce);
  results are shelves: Movies, Shows, Episodes, People, Collections. With an
  empty query the page shows the server's genres as tiles to browse.
- **Context menus**: right-click any poster card, an episode row on a show page,
  or a cast headshot. Items offer Play/Resume, Play from start, Mark
  watched/unwatched, Add to/Remove from favourites, Go to show/season, and Open.
- **People** (`Route::Person`): headshot, birth/death and place, biography, and
  the person's Movies, Shows and Episodes as shelves. Cast headshots link here.
- **Collections** (`Route::Collection`): BoxSets get their own page (hero art,
  overview, play, watched/favourite toggles, the titles as a poster grid). A
  Collections library (`boxsets`) now shows as a nav tab.
- **Genres** (`Route::Genre`): the genres on movie and show pages are links to a
  grid of every movie and show in that genre, with the library page's sorts and
  filters. Library pages also get a genre filter.
- **Favourites shelf** on Home, after Next Up (movies and shows, A–Z).

## How

- `jellyfin`: `ItemsQuery` gains `search_term`, `genres`, `person_ids` and
  `recursive`; new `persons(search)` and `genres(parent)` calls; `BaseItem`
  gains `genre_items` and `production_locations`.
- **One source of truth for watched/favourite**: `views::actions` toggles
  optimistically by patching the item in every page in the history
  (`Page::patch_user_data`), calls the server, applies its answer (or rolls
  back), then reloads every page so derived shelves (Continue Watching, Next
  Up, Favourites) follow. Item and show pages use the same path for their
  buttons.
- Library refresh becomes non-destructive: it refetches what's loaded and swaps
  it in, so the grid neither flashes nor loses its scroll position.
- The library page generalises to a "browse" grid with a scope: a library, a
  genre (all libraries), or both.
- Menus use gpui-component's `ContextMenuExt`; the menu contents are built by
  one function from the item, so every surface offers the same actions.

## Testing

- Mock-server tests for the new query parameters and endpoints.
- Unit tests for menu contents per item kind, user-data patching, and search
  debounce/grouping logic.
- GPUI test: right-clicking a card opens its menu.
- Headless-sway screenshots of Search, a person page, a genre page and a card
  menu against the demo server.
