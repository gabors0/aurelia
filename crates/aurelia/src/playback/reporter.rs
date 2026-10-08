//! Turns player events into Jellyfin playback reports. Pure and clock-driven
//! so the throttling rules are testable.

use std::time::{Duration, Instant};

use jellyfin::ProgressEvent;
use player::{EndReason, PlayerEvent};

const PROGRESS_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportKind {
    Start,
    Progress(Option<ProgressEvent>),
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    pub kind: ReportKind,
    pub position: Duration,
    pub paused: bool,
}

pub struct Reporter {
    start_position: Duration,
    runtime: Option<Duration>,
    position: Duration,
    paused: bool,
    started: bool,
    stopped: bool,
    last_progress: Option<Instant>,
}

impl Reporter {
    pub fn new(start_position: Duration, runtime: Option<Duration>) -> Self {
        Self {
            start_position,
            runtime,
            position: start_position,
            paused: false,
            started: false,
            stopped: false,
            last_progress: None,
        }
    }

    fn report(&self, kind: ReportKind) -> Report {
        Report {
            kind,
            position: self.position,
            paused: self.paused,
        }
    }

    pub fn on_event(&mut self, event: &PlayerEvent, now: Instant) -> Vec<Report> {
        if self.stopped {
            return Vec::new();
        }
        match event {
            PlayerEvent::Started => {
                self.started = true;
                self.last_progress = Some(now);
                vec![self.report(ReportKind::Start)]
            }
            PlayerEvent::Position(position) => {
                self.position = *position;
                let due = self
                    .last_progress
                    .is_none_or(|last| now.duration_since(last) >= PROGRESS_INTERVAL);
                if self.started && due {
                    self.last_progress = Some(now);
                    vec![self.report(ReportKind::Progress(Some(ProgressEvent::TimeUpdate)))]
                } else {
                    Vec::new()
                }
            }
            PlayerEvent::Paused(paused) => {
                self.paused = *paused;
                if !self.started {
                    return Vec::new();
                }
                self.last_progress = Some(now);
                let event = if *paused {
                    ProgressEvent::Pause
                } else {
                    ProgressEvent::Unpause
                };
                vec![self.report(ReportKind::Progress(Some(event)))]
            }
            PlayerEvent::Seeked(position) => {
                self.position = *position;
                if !self.started {
                    return Vec::new();
                }
                self.last_progress = Some(now);
                vec![self.report(ReportKind::Progress(Some(ProgressEvent::TimeUpdate)))]
            }
            PlayerEvent::Ended { position, reason } => {
                self.stopped = true;
                self.paused = false;
                self.position = match reason {
                    _ if !self.started => self.start_position,
                    EndReason::Eof => self.runtime.unwrap_or(*position).max(*position),
                    _ => *position,
                };
                vec![self.report(ReportKind::Stopped)]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn started(start: u64) -> (Reporter, Instant) {
        let mut reporter = Reporter::new(secs(start), Some(secs(3600)));
        let t0 = Instant::now();
        let reports = reporter.on_event(&PlayerEvent::Started, t0);
        assert_eq!(
            reports,
            vec![Report {
                kind: ReportKind::Start,
                position: secs(start),
                paused: false
            }]
        );
        (reporter, t0)
    }

    #[test]
    fn throttles_position_reports_to_10s() {
        let (mut r, t0) = started(0);
        assert!(
            r.on_event(&PlayerEvent::Position(secs(1)), t0 + secs(1))
                .is_empty()
        );
        assert!(
            r.on_event(&PlayerEvent::Position(secs(9)), t0 + secs(9))
                .is_empty()
        );
        assert_eq!(
            r.on_event(&PlayerEvent::Position(secs(10)), t0 + secs(10)),
            vec![Report {
                kind: ReportKind::Progress(Some(ProgressEvent::TimeUpdate)),
                position: secs(10),
                paused: false
            }]
        );
        assert!(
            r.on_event(&PlayerEvent::Position(secs(15)), t0 + secs(15))
                .is_empty()
        );
    }

    #[test]
    fn pause_reports_immediately() {
        let (mut r, t0) = started(0);
        r.on_event(&PlayerEvent::Position(secs(3)), t0 + secs(3));
        assert_eq!(
            r.on_event(&PlayerEvent::Paused(true), t0 + secs(4)),
            vec![Report {
                kind: ReportKind::Progress(Some(ProgressEvent::Pause)),
                position: secs(3),
                paused: true
            }]
        );
        assert_eq!(
            r.on_event(&PlayerEvent::Paused(false), t0 + secs(5))[0].kind,
            ReportKind::Progress(Some(ProgressEvent::Unpause))
        );
    }

    #[test]
    fn seek_reports_immediately() {
        let (mut r, t0) = started(0);
        assert_eq!(
            r.on_event(&PlayerEvent::Seeked(secs(600)), t0 + secs(1)),
            vec![Report {
                kind: ReportKind::Progress(Some(ProgressEvent::TimeUpdate)),
                position: secs(600),
                paused: false
            }]
        );
    }

    #[test]
    fn reports_stopped_on_early_exit() {
        let mut r = Reporter::new(secs(120), Some(secs(3600)));
        let reports = r.on_event(
            &PlayerEvent::Ended {
                position: Duration::ZERO,
                reason: EndReason::Error("boom".into()),
            },
            Instant::now(),
        );
        assert_eq!(
            reports,
            vec![Report {
                kind: ReportKind::Stopped,
                position: secs(120),
                paused: false
            }],
            "keeps the resume point when nothing played"
        );
    }

    #[test]
    fn eof_reports_full_runtime() {
        let (mut r, t0) = started(0);
        let reports = r.on_event(
            &PlayerEvent::Ended {
                position: secs(3590),
                reason: EndReason::Eof,
            },
            t0 + secs(3590),
        );
        assert_eq!(
            reports,
            vec![Report {
                kind: ReportKind::Stopped,
                position: secs(3600),
                paused: false
            }]
        );
    }

    #[test]
    fn quit_reports_last_position_once() {
        let (mut r, t0) = started(30);
        let end = PlayerEvent::Ended {
            position: secs(1234),
            reason: EndReason::Quit,
        };
        assert_eq!(r.on_event(&end, t0)[0].position, secs(1234));
        assert!(r.on_event(&end, t0).is_empty(), "only one Stopped");
    }
}
