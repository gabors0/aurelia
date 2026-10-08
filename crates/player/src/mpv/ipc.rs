//! mpv JSON IPC: command encoding and translation of mpv's event stream into
//! [`PlayerEvent`]s. Pure — no sockets — so it can be tested directly.

use std::time::Duration;

use serde_json::Value;

use crate::{EndReason, PlayerEvent};

/// One line of the IPC protocol.
pub(crate) fn encode_command(request_id: u64, command: &[Value]) -> String {
    let mut line = serde_json::json!({ "command": command, "request_id": request_id }).to_string();
    line.push('\n');
    line
}

/// Observed property ids.
pub(crate) const OBSERVE_TIME_POS: u64 = 1;
pub(crate) const OBSERVE_PAUSE: u64 = 2;

/// What the reader should do in response to a message, besides emitting events.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Reaction {
    pub events: Vec<PlayerEvent>,
    /// The file finished loading: time to add external subtitles.
    pub file_loaded: bool,
}

#[derive(Debug, Default)]
pub(crate) struct EventTranslator {
    started: bool,
    file_loaded: bool,
    paused: bool,
    position: Duration,
    last_emitted: Option<Duration>,
    end_reason: Option<EndReason>,
}

impl EventTranslator {
    pub fn on_message(&mut self, message: &Value) -> Reaction {
        let mut reaction = Reaction::default();
        let Some(event) = message.get("event").and_then(Value::as_str) else {
            return reaction;
        };
        match event {
            "property-change" => self.on_property_change(message, &mut reaction.events),
            "file-loaded" => {
                self.file_loaded = true;
                reaction.file_loaded = true;
            }
            "playback-restart" if !self.started && self.file_loaded => {
                self.started = true;
                reaction.events.push(PlayerEvent::Started);
            }
            "playback-restart" if self.started => {
                self.last_emitted = Some(self.position);
                reaction.events.push(PlayerEvent::Seeked(self.position));
            }
            "end-file" => {
                let reason = message.get("reason").and_then(Value::as_str);
                self.end_reason = match reason {
                    Some("eof") => Some(EndReason::Eof),
                    Some("quit" | "stop") => Some(EndReason::Quit),
                    Some("error") => {
                        let detail = message
                            .get("file_error")
                            .and_then(Value::as_str)
                            .unwrap_or("playback error");
                        Some(EndReason::Error(detail.to_string()))
                    }
                    // "redirect" (playlist expansion) is not an ending.
                    _ => self.end_reason.take(),
                };
            }
            _ => {}
        }
        reaction
    }

    fn on_property_change(&mut self, message: &Value, events: &mut Vec<PlayerEvent>) {
        let data = message.get("data");
        match message.get("name").and_then(Value::as_str) {
            Some("time-pos") => {
                let Some(seconds) = data.and_then(Value::as_f64).filter(|s| *s >= 0.0) else {
                    return;
                };
                self.position = Duration::from_secs_f64(seconds);
                let due = self.last_emitted.is_none_or(|last| {
                    let delta = self.position.abs_diff(last);
                    delta >= Duration::from_secs(1)
                });
                if self.started && due {
                    self.last_emitted = Some(self.position);
                    events.push(PlayerEvent::Position(self.position));
                }
            }
            Some("pause") => {
                let Some(paused) = data.and_then(Value::as_bool) else {
                    return;
                };
                if self.started && paused != self.paused {
                    events.push(PlayerEvent::Paused(paused));
                }
                self.paused = paused;
            }
            _ => {}
        }
    }

    /// The player process is gone; produce the final event.
    pub fn on_exit(&mut self, exit_detail: Option<String>) -> PlayerEvent {
        let reason = match self.end_reason.take() {
            Some(reason) => reason,
            None if self.file_loaded => EndReason::Quit,
            None => EndReason::Error(
                exit_detail.unwrap_or_else(|| "the player exited before playback started".into()),
            ),
        };
        PlayerEvent::Ended {
            position: self.position,
            reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn feed(translator: &mut EventTranslator, messages: &[Value]) -> Vec<PlayerEvent> {
        messages
            .iter()
            .flat_map(|m| translator.on_message(m).events)
            .collect()
    }

    #[test]
    fn encodes_commands() {
        let line = encode_command(
            1,
            &[
                json!("change-list"),
                json!("http-header-fields"),
                json!("append"),
                json!(r#"Authorization: MediaBrowser Token="t""#),
            ],
        );
        assert_eq!(
            line,
            "{\"command\":[\"change-list\",\"http-header-fields\",\"append\",\"Authorization: MediaBrowser Token=\\\"t\\\"\"],\"request_id\":1}\n"
        );
    }

    #[test]
    fn started_after_first_playback_restart() {
        let mut t = EventTranslator::default();
        let events = feed(
            &mut t,
            &[
                json!({"event": "start-file"}),
                json!({"event": "property-change", "id": 2, "name": "pause", "data": false}),
                json!({"event": "file-loaded"}),
                json!({"event": "playback-restart"}),
            ],
        );
        assert_eq!(events, vec![PlayerEvent::Started]);
    }

    #[test]
    fn file_loaded_is_signalled() {
        let mut t = EventTranslator::default();
        assert!(t.on_message(&json!({"event": "file-loaded"})).file_loaded);
        assert!(
            !t.on_message(&json!({"event": "playback-restart"}))
                .file_loaded
        );
    }

    #[test]
    fn positions_are_throttled_to_one_per_second() {
        let mut t = EventTranslator::default();
        feed(
            &mut t,
            &[
                json!({"event": "file-loaded"}),
                json!({"event": "playback-restart"}),
            ],
        );
        let tp =
            |s: f64| json!({"event": "property-change", "id": 1, "name": "time-pos", "data": s});
        let events = feed(&mut t, &[tp(10.0), tp(10.2), tp(10.9), tp(11.05), tp(11.5)]);
        assert_eq!(
            events,
            vec![
                PlayerEvent::Position(Duration::from_secs_f64(10.0)),
                PlayerEvent::Position(Duration::from_secs_f64(11.05)),
            ]
        );
    }

    #[test]
    fn pause_changes_emit_only_after_start() {
        let mut t = EventTranslator::default();
        let pause =
            |p: bool| json!({"event": "property-change", "id": 2, "name": "pause", "data": p});
        assert!(feed(&mut t, &[pause(false)]).is_empty());
        feed(
            &mut t,
            &[
                json!({"event": "file-loaded"}),
                json!({"event": "playback-restart"}),
            ],
        );
        assert_eq!(
            feed(&mut t, &[pause(true)]),
            vec![PlayerEvent::Paused(true)]
        );
        assert!(feed(&mut t, &[pause(true)]).is_empty(), "no duplicate");
        assert_eq!(
            feed(&mut t, &[pause(false)]),
            vec![PlayerEvent::Paused(false)]
        );
    }

    #[test]
    fn later_playback_restart_is_a_seek() {
        let mut t = EventTranslator::default();
        feed(
            &mut t,
            &[
                json!({"event": "file-loaded"}),
                json!({"event": "playback-restart"}),
            ],
        );
        let events = feed(
            &mut t,
            &[
                json!({"event": "seek"}),
                json!({"event": "property-change", "id": 1, "name": "time-pos", "data": 600.0}),
                json!({"event": "playback-restart"}),
            ],
        );
        assert_eq!(
            events.last(),
            Some(&PlayerEvent::Seeked(Duration::from_secs(600)))
        );
    }

    #[test]
    fn end_file_reason_carries_into_exit() {
        let mut t = EventTranslator::default();
        feed(
            &mut t,
            &[
                json!({"event": "file-loaded"}),
                json!({"event": "playback-restart"}),
                json!({"event": "property-change", "id": 1, "name": "time-pos", "data": 42.0}),
                json!({"event": "end-file", "reason": "eof"}),
            ],
        );
        assert_eq!(
            t.on_exit(None),
            PlayerEvent::Ended {
                position: Duration::from_secs(42),
                reason: EndReason::Eof
            }
        );
    }

    #[test]
    fn quit_reason() {
        let mut t = EventTranslator::default();
        feed(
            &mut t,
            &[
                json!({"event": "file-loaded"}),
                json!({"event": "end-file", "reason": "quit"}),
            ],
        );
        assert!(matches!(
            t.on_exit(None),
            PlayerEvent::Ended {
                reason: EndReason::Quit,
                ..
            }
        ));
    }

    #[test]
    fn end_file_error_keeps_message() {
        let mut t = EventTranslator::default();
        feed(
            &mut t,
            &[json!({"event": "end-file", "reason": "error", "file_error": "loading failed"})],
        );
        assert_eq!(
            t.on_exit(None),
            PlayerEvent::Ended {
                position: Duration::ZERO,
                reason: EndReason::Error("loading failed".into())
            }
        );
    }

    #[test]
    fn exit_without_anything_is_error() {
        let mut t = EventTranslator::default();
        assert!(matches!(
            t.on_exit(Some("exit status: 1".into())),
            PlayerEvent::Ended {
                reason: EndReason::Error(_),
                ..
            }
        ));
    }

    #[test]
    fn exit_after_load_without_end_file_is_quit() {
        let mut t = EventTranslator::default();
        feed(
            &mut t,
            &[
                json!({"event": "file-loaded"}),
                json!({"event": "playback-restart"}),
            ],
        );
        assert!(matches!(
            t.on_exit(None),
            PlayerEvent::Ended {
                reason: EndReason::Quit,
                ..
            }
        ));
    }

    #[test]
    fn null_time_pos_is_ignored() {
        let mut t = EventTranslator::default();
        feed(
            &mut t,
            &[
                json!({"event": "file-loaded"}),
                json!({"event": "playback-restart"}),
            ],
        );
        let events = feed(
            &mut t,
            &[json!({"event": "property-change", "id": 1, "name": "time-pos", "data": null})],
        );
        assert!(events.is_empty());
    }
}
