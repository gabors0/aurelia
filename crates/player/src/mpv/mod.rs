mod ipc;

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use self::ipc::{EventTranslator, OBSERVE_PAUSE, OBSERVE_TIME_POS, encode_command};
use crate::{
    ExternalSub, PlayRequest, PlaybackControl, PlaybackHandle, Player, PlayerError, PlayerEvent,
    TrackChoice,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const STDERR_TAIL: usize = 12;

/// Plays media in a standalone mpv window. The user's mpv configuration
/// (keybindings, scripts, shaders) applies as usual.
#[derive(Debug, Clone)]
pub struct MpvPlayer {
    binary: PathBuf,
    extra_args: Vec<String>,
}

impl MpvPlayer {
    pub fn new(binary: PathBuf) -> Self {
        Self {
            binary,
            extra_args: Vec::new(),
        }
    }

    /// `$AURELIA_MPV`, else `mpv` from `PATH`.
    pub fn find() -> Option<Self> {
        if let Some(path) = std::env::var_os("AURELIA_MPV").map(PathBuf::from) {
            return path.is_file().then(|| Self::new(path));
        }
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join("mpv"))
            .find(|candidate| candidate.is_file())
            .map(Self::new)
    }

    pub fn binary(&self) -> &Path {
        &self.binary
    }

    pub fn with_extra_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.extra_args.extend(args.into_iter().map(Into::into));
        self
    }

    fn command(&self, request: &PlayRequest, socket: &Path) -> Command {
        let mut command = Command::new(&self.binary);
        command
            .arg("--idle=once")
            .arg("--force-window=immediate")
            .arg(format!("--input-ipc-server={}", socket.display()))
            .arg("--no-resume-playback")
            .arg("--save-position-on-quit=no")
            .arg("--keep-open=no")
            .arg(format!("--force-media-title={}", request.title));
        if !request.start.is_zero() {
            command.arg(format!("--start={:.3}", request.start.as_secs_f64()));
        }
        if let Some(aid) = track_arg(request.audio) {
            command.arg(format!("--aid={aid}"));
        }
        let selects_external = request.external_subtitles.iter().any(|s| s.select);
        let subtitle = if selects_external {
            TrackChoice::Off
        } else {
            request.subtitle
        };
        if let Some(sid) = track_arg(subtitle) {
            command.arg(format!("--sid={sid}"));
        }
        command
            .args(&self.extra_args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        command
    }
}

fn track_arg(choice: TrackChoice) -> Option<String> {
    match choice {
        TrackChoice::Auto => None,
        TrackChoice::Off => Some("no".into()),
        TrackChoice::Id(id) => Some(id.to_string()),
    }
}

fn socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    dir.join(format!("aurelia-mpv-{}-{n}.sock", std::process::id()))
}

impl Player for MpvPlayer {
    fn play(&self, request: PlayRequest) -> Result<PlaybackHandle, PlayerError> {
        let socket = socket_path();
        let _ = std::fs::remove_file(&socket);
        let mut child =
            self.command(&request, &socket)
                .spawn()
                .map_err(|e| PlayerError::Spawn {
                    binary: self.binary.display().to_string(),
                    message: e.to_string(),
                })?;

        let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
        if let Some(stderr) = child.stderr.take() {
            let tail = stderr_tail.clone();
            std::thread::Builder::new()
                .name("mpv-stderr".into())
                .spawn(move || {
                    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                        tracing::debug!(target: "mpv", "{line}");
                        let mut tail = tail.lock().unwrap();
                        if tail.len() == STDERR_TAIL {
                            tail.pop_front();
                        }
                        tail.push_back(line);
                    }
                })
                .expect("spawn thread");
        }

        let (events_tx, events_rx) = async_channel::unbounded();
        let control = Arc::new(MpvControl {
            child: Mutex::new(child),
            writer: Mutex::new(None),
            next_request: AtomicU64::new(1),
        });
        let session = Session {
            control: control.clone(),
            socket,
            request,
            events: events_tx,
            stderr_tail,
        };
        std::thread::Builder::new()
            .name("mpv-session".into())
            .spawn(move || session.run())
            .expect("spawn thread");

        Ok(PlaybackHandle::new(events_rx, control))
    }
}

struct MpvControl {
    child: Mutex<Child>,
    writer: Mutex<Option<UnixStream>>,
    next_request: AtomicU64,
}

impl MpvControl {
    fn send(&self, command: &[Value]) -> bool {
        let id = self.next_request.fetch_add(1, Ordering::Relaxed);
        let line = encode_command(id, command);
        let mut writer = self.writer.lock().unwrap();
        match writer.as_mut() {
            Some(stream) => stream.write_all(line.as_bytes()).is_ok(),
            None => false,
        }
    }

    fn exit_status(&self) -> Option<String> {
        let mut child = self.child.lock().unwrap();
        child
            .try_wait()
            .ok()
            .flatten()
            .map(|status| status.to_string())
    }
}

impl PlaybackControl for MpvControl {
    fn stop(&self) {
        if !self.send(&[json!("quit")]) {
            let _ = self.child.lock().unwrap().kill();
        }
    }
}

/// Owns one mpv process from spawn to exit; runs on its own thread.
struct Session {
    control: Arc<MpvControl>,
    socket: PathBuf,
    request: PlayRequest,
    events: async_channel::Sender<PlayerEvent>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
}

impl Session {
    fn run(self) {
        let mut translator = EventTranslator::default();
        let detail = match self.connect() {
            Ok(stream) => {
                self.drive(stream, &mut translator);
                None
            }
            Err(detail) => Some(detail),
        };

        let status = self.wait_for_exit();
        let _ = std::fs::remove_file(&self.socket);
        let detail = detail.or_else(|| {
            let tail = self.stderr_tail.lock().unwrap();
            let last = tail.back().cloned();
            match (status, last) {
                (Some(status), Some(line)) => Some(format!("{line} ({status})")),
                (Some(status), None) => Some(format!("mpv exited ({status})")),
                (None, line) => line,
            }
        });
        let _ = self.events.send_blocking(translator.on_exit(detail));
    }

    fn connect(&self) -> Result<UnixStream, String> {
        let deadline = Instant::now() + CONNECT_TIMEOUT;
        loop {
            if let Ok(stream) = UnixStream::connect(&self.socket) {
                return Ok(stream);
            }
            if let Some(status) = self.control.exit_status() {
                let tail = self.stderr_tail.lock().unwrap();
                return Err(match tail.back() {
                    Some(line) => format!("{line} ({status})"),
                    None => format!("mpv exited before it was ready ({status})"),
                });
            }
            if Instant::now() > deadline {
                let _ = self.control.child.lock().unwrap().kill();
                return Err("mpv did not open its control socket".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn drive(&self, stream: UnixStream, translator: &mut EventTranslator) {
        match stream.try_clone() {
            Ok(writer) => *self.control.writer.lock().unwrap() = Some(writer),
            Err(err) => {
                tracing::warn!("mpv socket clone failed: {err}");
                let _ = self.control.child.lock().unwrap().kill();
                return;
            }
        }

        let control = &self.control;
        control.send(&[
            json!("observe_property"),
            json!(OBSERVE_TIME_POS),
            json!("time-pos"),
        ]);
        control.send(&[
            json!("observe_property"),
            json!(OBSERVE_PAUSE),
            json!("pause"),
        ]);
        for header in &self.request.http_headers {
            // `change-list append` adds one entry verbatim; `set` would split
            // the value on commas.
            control.send(&[
                json!("change-list"),
                json!("http-header-fields"),
                json!("append"),
                json!(header),
            ]);
        }
        control.send(&[json!("loadfile"), json!(self.request.url)]);

        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if message
                .get("error")
                .and_then(Value::as_str)
                .is_some_and(|e| e != "success")
            {
                tracing::warn!(target: "mpv", "command failed: {message}");
            }
            let reaction = translator.on_message(&message);
            if reaction.file_loaded {
                for sub in &self.request.external_subtitles {
                    control.send(&sub_add(sub));
                }
            }
            for event in reaction.events {
                let _ = self.events.send_blocking(event);
            }
        }
        *self.control.writer.lock().unwrap() = None;
    }

    fn wait_for_exit(&self) -> Option<String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.control.exit_status() {
                return Some(status);
            }
            if Instant::now() > deadline {
                let _ = self.control.child.lock().unwrap().kill();
                return None;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

fn sub_add(sub: &ExternalSub) -> Vec<Value> {
    let flag = if sub.select { "select" } else { "auto" };
    vec![
        json!("sub-add"),
        json!(sub.url),
        json!(flag),
        json!(sub.title.clone().unwrap_or_default()),
        json!(sub.lang.clone().unwrap_or_default()),
    ]
}
