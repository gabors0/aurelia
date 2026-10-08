//! Back/forward navigation. Pages are kept alive in the history so going back
//! restores them instantly, scroll position included.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Home,
    Library { id: String, name: String },
    Item { id: String },
}

const MAX_HISTORY: usize = 24;

pub struct Nav<P> {
    back: Vec<(Route, P)>,
    current: (Route, P),
    forward: Vec<(Route, P)>,
}

impl<P> Nav<P> {
    pub fn new(route: Route, page: P) -> Self {
        Self {
            back: Vec::new(),
            current: (route, page),
            forward: Vec::new(),
        }
    }

    pub fn route(&self) -> &Route {
        &self.current.0
    }

    pub fn page(&self) -> &P {
        &self.current.1
    }

    /// Navigates to `route`. Returns false (and drops `page`) when already there.
    pub fn push(&mut self, route: Route, page: P) -> bool {
        if self.current.0 == route {
            return false;
        }
        let previous = std::mem::replace(&mut self.current, (route, page));
        self.back.push(previous);
        if self.back.len() > MAX_HISTORY {
            self.back.remove(0);
        }
        self.forward.clear();
        true
    }

    pub fn back(&mut self) -> bool {
        let Some(previous) = self.back.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.current, previous);
        self.forward.push(current);
        true
    }

    pub fn forward(&mut self) -> bool {
        let Some(next) = self.forward.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.current, next);
        self.back.push(current);
        true
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn pages(&self) -> impl Iterator<Item = &P> {
        self.back
            .iter()
            .chain(std::iter::once(&self.current))
            .chain(self.forward.iter())
            .map(|(_, page)| page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> Route {
        Route::Item { id: id.into() }
    }

    #[test]
    fn nav_push_back_forward() {
        let mut nav = Nav::new(Route::Home, 0);
        assert!(!nav.can_go_back());
        assert!(nav.push(item("a"), 1));
        assert!(nav.push(item("b"), 2));
        assert_eq!(nav.route(), &item("b"));
        assert!(nav.back());
        assert_eq!((nav.route(), *nav.page()), (&item("a"), 1));
        assert!(nav.back());
        assert_eq!(nav.route(), &Route::Home);
        assert!(!nav.back());
        assert!(nav.forward());
        assert_eq!(*nav.page(), 1);
        assert!(nav.forward());
        assert_eq!(*nav.page(), 2);
        assert!(!nav.forward());
    }

    #[test]
    fn nav_push_clears_forward() {
        let mut nav = Nav::new(Route::Home, 0);
        nav.push(item("a"), 1);
        nav.back();
        nav.push(item("c"), 3);
        assert!(!nav.forward());
        assert_eq!(nav.pages().copied().collect::<Vec<_>>(), vec![0, 3]);
    }

    #[test]
    fn nav_push_same_route_is_noop() {
        let mut nav = Nav::new(Route::Home, 0);
        assert!(!nav.push(Route::Home, 9));
        assert_eq!(*nav.page(), 0);
        assert!(!nav.can_go_back());
    }

    #[test]
    fn nav_history_is_bounded() {
        let mut nav = Nav::new(Route::Home, 0);
        for i in 1..100 {
            nav.push(item(&i.to_string()), i);
        }
        assert!(nav.pages().count() <= MAX_HISTORY + 1);
        assert_eq!(*nav.page(), 99);
    }
}
