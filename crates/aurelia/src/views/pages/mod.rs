//! The pages the shell navigates between.

pub mod carousel;
pub mod collection;
pub mod grid;
pub mod home;
pub mod item;
pub mod library;
pub mod person;
pub mod search;
pub mod series;

use gpui_kit::{AnyView, App, AppContext as _, Entity, FocusHandle, Window};

use self::collection::CollectionPage;
use self::home::HomePage;
use self::item::ItemPage;
use self::library::LibraryPage;
use self::person::PersonPage;
use self::search::SearchPage;
use self::series::SeriesPage;
use crate::nav::Route;

#[derive(Clone)]
pub enum Page {
    Home(Entity<HomePage>),
    Library(Entity<LibraryPage>),
    Item(Entity<ItemPage>),
    Series(Entity<SeriesPage>),
    Collection(Entity<CollectionPage>),
    Person(Entity<PersonPage>),
    Search(Entity<SearchPage>),
}

impl Page {
    pub fn for_route(route: &Route, window: &mut Window, cx: &mut App) -> Self {
        match route {
            Route::Home => Page::Home(cx.new(|cx| HomePage::new(window, cx))),
            Route::Library { id, name } => Page::Library(
                cx.new(|cx| LibraryPage::library(id.clone(), name.clone(), window, cx)),
            ),
            Route::Genre { name } => {
                Page::Library(cx.new(|cx| LibraryPage::genre(name.clone(), window, cx)))
            }
            Route::Item { id } => Page::Item(cx.new(|cx| ItemPage::new(id.clone(), window, cx))),
            Route::Series { id, season_id } => Page::Series(
                cx.new(|cx| SeriesPage::new(id.clone(), season_id.clone(), window, cx)),
            ),
            Route::Collection { id } => {
                Page::Collection(cx.new(|cx| CollectionPage::new(id.clone(), window, cx)))
            }
            Route::Person { id } => {
                Page::Person(cx.new(|cx| PersonPage::new(id.clone(), window, cx)))
            }
            Route::Search => Page::Search(cx.new(|cx| SearchPage::new(window, cx))),
        }
    }

    pub fn view(&self) -> AnyView {
        match self {
            Page::Home(page) => page.clone().into(),
            Page::Library(page) => page.clone().into(),
            Page::Item(page) => page.clone().into(),
            Page::Series(page) => page.clone().into(),
            Page::Collection(page) => page.clone().into(),
            Page::Person(page) => page.clone().into(),
            Page::Search(page) => page.clone().into(),
        }
    }

    /// Where keyboard focus goes when the page is shown; `None` for the
    /// shell itself.
    pub fn focus_handle(&self, cx: &App) -> Option<FocusHandle> {
        match self {
            Page::Search(page) => Some(page.read(cx).focus_handle(cx)),
            _ => None,
        }
    }

    /// How far the page is scrolled (0 at the top, negative further down).
    pub fn scroll_offset(&self, cx: &App) -> gpui_kit::Pixels {
        match self {
            Page::Home(page) => page.read(cx).scroll_handle().offset().y,
            Page::Item(page) => page.read(cx).scroll_handle().offset().y,
            Page::Series(page) => page.read(cx).scroll_handle().offset().y,
            Page::Collection(page) => page.read(cx).scroll_handle().offset().y,
            Page::Person(page) => page.read(cx).scroll_handle().offset().y,
            Page::Search(page) => page.read(cx).scroll_handle().offset().y,
            Page::Library(_) => gpui_kit::px(0.),
        }
    }

    /// Enter on a page: play what it shows.
    pub fn primary_action(&self, window: &mut Window, cx: &mut App) {
        match self {
            Page::Item(page) => page.update(cx, |page, cx| page.play(false, window, cx)),
            Page::Series(page) => page.update(cx, |page, cx| page.play(window, cx)),
            Page::Collection(page) => page.update(cx, |page, cx| page.play(window, cx)),
            _ => {}
        }
    }

    /// ←/→ on Home browse the hero slides.
    pub fn step_hero(&self, delta: isize, cx: &mut App) {
        if let Page::Home(page) = self {
            page.update(cx, |page, cx| page.step_hero(delta, cx));
        }
    }

    /// The refresh button and Ctrl+R: reloads, and Home picks new random
    /// hero slides.
    pub fn reshuffle(&self, window: &mut Window, cx: &mut App) {
        match self {
            Page::Home(page) => page.update(cx, |page, cx| page.reshuffle(window, cx)),
            _ => self.refresh(window, cx),
        }
    }

    /// Replaces an item's watched/favourite state wherever the page shows it.
    pub fn patch_user_data(&self, id: &str, data: &jellyfin::UserData, cx: &mut App) {
        fn apply<P: 'static>(page: &Entity<P>, cx: &mut App, patch: impl FnOnce(&mut P) -> bool) {
            page.update(cx, |page, cx| {
                if patch(page) {
                    cx.notify();
                }
            });
        }
        match self {
            Page::Home(page) => apply(page, cx, |p| p.patch_user_data(id, data)),
            Page::Library(page) => apply(page, cx, |p| p.patch_user_data(id, data)),
            Page::Item(page) => apply(page, cx, |p| p.patch_user_data(id, data)),
            Page::Series(page) => apply(page, cx, |p| p.patch_user_data(id, data)),
            Page::Collection(page) => apply(page, cx, |p| p.patch_user_data(id, data)),
            Page::Person(page) => apply(page, cx, |p| p.patch_user_data(id, data)),
            Page::Search(page) => apply(page, cx, |p| p.patch_user_data(id, data)),
        }
    }

    /// Reloads server data (after playback, coming back to the window).
    pub fn refresh(&self, window: &mut Window, cx: &mut App) {
        match self {
            Page::Home(page) => page.update(cx, |page, cx| page.refresh(window, cx)),
            Page::Library(page) => page.update(cx, |page, cx| page.refresh(window, cx)),
            Page::Item(page) => page.update(cx, |page, cx| page.refresh(window, cx)),
            Page::Series(page) => page.update(cx, |page, cx| page.refresh(window, cx)),
            Page::Collection(page) => page.update(cx, |page, cx| page.refresh(window, cx)),
            Page::Person(page) => page.update(cx, |page, cx| page.refresh(window, cx)),
            Page::Search(page) => page.update(cx, |page, cx| page.refresh(window, cx)),
        }
    }
}
