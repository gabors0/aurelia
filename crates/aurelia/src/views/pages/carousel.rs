//! Slide state for Home's hero: which slide shows, when to advance, and the
//! slide being faded out. Clock-driven so the timing rules are testable.

use std::time::{Duration, Instant};

pub const INTERVAL: Duration = Duration::from_secs(8);
/// After the pointer leaves the controls, wait at least this long.
const RESUME_GRACE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub struct Carousel {
    len: usize,
    index: usize,
    previous: Option<usize>,
    shown_at: Instant,
    paused: bool,
}

impl Carousel {
    pub fn new(now: Instant) -> Self {
        Self {
            len: 0,
            index: 0,
            previous: None,
            shown_at: now,
            paused: false,
        }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn previous(&self) -> Option<usize> {
        self.previous
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// New slides arrived; keep showing `keep` if it's still among them.
    pub fn reset(&mut self, len: usize, keep: Option<usize>, now: Instant) {
        self.len = len;
        self.index = keep.filter(|i| *i < len).unwrap_or(0);
        self.previous = None;
        self.shown_at = now;
    }

    /// Arrow buttons and keys: wraps around, restarts the timer.
    pub fn step(&mut self, delta: isize, now: Instant) -> bool {
        if self.len < 2 {
            return false;
        }
        let len = self.len as isize;
        let next = (self.index as isize + delta).rem_euclid(len) as usize;
        self.go_to(next, now)
    }

    pub fn go_to(&mut self, index: usize, now: Instant) -> bool {
        if index >= self.len || index == self.index {
            return false;
        }
        self.previous = Some(self.index);
        self.index = index;
        self.shown_at = now;
        true
    }

    /// Advances when the slide has been up for `INTERVAL` and the user isn't
    /// pointing at the slide's controls.
    pub fn tick(&mut self, now: Instant) -> bool {
        if self.paused || now.duration_since(self.shown_at) < INTERVAL {
            return false;
        }
        self.step(1, now)
    }

    pub fn set_paused(&mut self, paused: bool, now: Instant) {
        if self.paused && !paused {
            // Don't jump the moment the pointer leaves the buttons.
            let earliest = now - (INTERVAL - RESUME_GRACE);
            if self.shown_at < earliest {
                self.shown_at = earliest;
            }
        }
        self.paused = paused;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carousel(len: usize) -> (Carousel, Instant) {
        let t0 = Instant::now();
        let mut c = Carousel::new(t0);
        c.reset(len, None, t0);
        (c, t0)
    }

    #[test]
    fn steps_wrap_both_ways() {
        let (mut c, t0) = carousel(3);
        assert!(c.step(-1, t0));
        assert_eq!((c.index(), c.previous()), (2, Some(0)));
        assert!(c.step(1, t0));
        assert_eq!(c.index(), 0);
        assert!(c.step(1, t0));
        assert_eq!(c.index(), 1);
    }

    #[test]
    fn advances_on_its_own_after_the_interval() {
        let (mut c, t0) = carousel(3);
        assert!(!c.tick(t0 + INTERVAL - Duration::from_millis(1)));
        assert!(c.tick(t0 + INTERVAL));
        assert_eq!(c.index(), 1);
        assert!(
            !c.tick(t0 + INTERVAL + Duration::from_secs(1)),
            "timer restarted"
        );
    }

    #[test]
    fn does_not_advance_while_paused() {
        let (mut c, t0) = carousel(3);
        c.set_paused(true, t0);
        assert!(!c.tick(t0 + INTERVAL * 3));
        assert_eq!(c.index(), 0);
    }

    #[test]
    fn resuming_after_a_long_pause_leaves_a_grace_period() {
        let (mut c, t0) = carousel(3);
        c.set_paused(true, t0);
        let resumed = t0 + INTERVAL * 2;
        c.set_paused(false, resumed);
        assert!(!c.tick(resumed + Duration::from_secs(2)));
        assert!(c.tick(resumed + RESUME_GRACE));
    }

    #[test]
    fn manual_navigation_restarts_the_timer() {
        let (mut c, t0) = carousel(3);
        let later = t0 + INTERVAL - Duration::from_secs(1);
        c.go_to(2, later);
        assert!(!c.tick(t0 + INTERVAL), "just navigated");
        assert!(c.tick(later + INTERVAL));
        assert_eq!(c.index(), 0);
    }

    #[test]
    fn single_or_no_slide_never_moves() {
        let (mut c, t0) = carousel(1);
        assert!(!c.tick(t0 + INTERVAL * 2));
        assert!(!c.step(1, t0));
        let (mut empty, t0) = carousel(0);
        assert!(!empty.tick(t0 + INTERVAL));
        assert!(!empty.go_to(0, t0));
    }

    #[test]
    fn reset_keeps_the_current_slide_when_possible() {
        let (mut c, t0) = carousel(4);
        c.go_to(2, t0);
        c.reset(5, Some(3), t0);
        assert_eq!((c.index(), c.previous()), (3, None));
        c.reset(2, None, t0);
        assert_eq!(c.index(), 0);
    }
}
