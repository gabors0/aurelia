//! The pages the shell navigates between.

pub mod grid;
pub mod home;
pub mod item;
pub mod library;
pub mod series;

use gpui_kit::{AnyView, App, AppContext as _, Entity, Window};

use self::home::HomePage;
use self::item::ItemPage;
use self::library::LibraryPage;
use self::series::SeriesPage;
use crate::nav::Route;

#[derive(Clone)]
pub enum Page {
    Home(Entity<HomePage>),
    Library(Entity<LibraryPage>),
    Item(Entity<ItemPage>),
    Series(Entity<SeriesPage>),
}

impl Page {
    pub fn for_route(route: &Route, window: &mut Window, cx: &mut App) -> Self {
        match route {
            Route::Home => Page::Home(cx.new(|cx| HomePage::new(window, cx))),
            Route::Library { id, name } => {
                Page::Library(cx.new(|cx| LibraryPage::new(id.clone(), name.clone(), window, cx)))
            }
            Route::Item { id } => Page::Item(cx.new(|cx| ItemPage::new(id.clone(), window, cx))),
            Route::Series { .. } => Page::Series(cx.new(|cx| SeriesPage::new(window, cx))),
        }
    }

    pub fn view(&self) -> AnyView {
        match self {
            Page::Home(page) => page.clone().into(),
            Page::Library(page) => page.clone().into(),
            Page::Item(page) => page.clone().into(),
            Page::Series(page) => page.clone().into(),
        }
    }

    /// How far the page is scrolled (0 at the top, negative further down).
    pub fn scroll_offset(&self, cx: &App) -> gpui_kit::Pixels {
        match self {
            Page::Home(page) => page.read(cx).scroll_handle().offset().y,
            Page::Item(page) => page.read(cx).scroll_handle().offset().y,
            _ => gpui_kit::px(0.),
        }
    }

    /// Enter on a page: play what it shows.
    pub fn primary_action(&self, window: &mut Window, cx: &mut App) {
        if let Page::Item(page) = self {
            page.update(cx, |page, cx| page.play(false, window, cx));
        }
    }

    /// Reloads server data (after playback, or on Ctrl+R).
    pub fn refresh(&self, window: &mut Window, cx: &mut App) {
        match self {
            Page::Home(page) => page.update(cx, |page, cx| page.refresh(window, cx)),
            Page::Library(page) => page.update(cx, |page, cx| page.refresh(window, cx)),
            Page::Item(page) => page.update(cx, |page, cx| page.refresh(window, cx)),
            Page::Series(page) => page.update(cx, |page, cx| page.refresh(window, cx)),
        }
    }
}
