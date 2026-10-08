//! Persisted sign-in: server, user and access token (never the password).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub server: String,
    pub server_name: String,
    pub user_id: String,
    pub user_name: String,
    pub token: String,
}

/// Files under the config directory (`~/.config/aurelia`).
pub struct SessionStore {
    dir: PathBuf,
}

impl SessionStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn default_location() -> Self {
        let dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("aurelia");
        Self::new(dir)
    }

    pub fn load(&self) -> Option<Session> {
        let text = std::fs::read_to_string(self.dir.join("session.json")).ok()?;
        match serde_json::from_str(&text) {
            Ok(session) => Some(session),
            Err(err) => {
                tracing::warn!("ignoring unreadable session file: {err}");
                None
            }
        }
    }

    pub fn save(&self, session: &Session) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(session).map_err(std::io::Error::other)?;
        write_private(&self.dir, "session.json", &json)?;
        write_private(&self.dir, "last_server", session.server.as_bytes())
    }

    pub fn clear(&self) {
        let _ = std::fs::remove_file(self.dir.join("session.json"));
    }

    /// Stable per-installation id, created on first use.
    pub fn device_id(&self) -> String {
        if let Ok(id) = std::fs::read_to_string(self.dir.join("device_id")) {
            let id = id.trim();
            if !id.is_empty() {
                return id.to_string();
            }
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        if let Err(err) = write_private(&self.dir, "device_id", id.as_bytes()) {
            tracing::warn!("could not persist device id: {err}");
        }
        id
    }

    /// The server address used last, kept after sign-out to pre-fill login.
    pub fn last_server(&self) -> Option<String> {
        let server = std::fs::read_to_string(self.dir.join("last_server")).ok()?;
        let server = server.trim();
        (!server.is_empty()).then(|| server.to_string())
    }
}

/// Atomically writes a file only the current user can read.
fn write_private(dir: &std::path::Path, name: &str, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{name}.tmp"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&tmp)?;
    file.write_all(contents)?;
    file.sync_all()?;
    std::fs::rename(tmp, dir.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn sample() -> Session {
        Session {
            server: "https://jf.example.com/".into(),
            server_name: "Box".into(),
            user_id: "u1".into(),
            user_name: "gabor".into(),
            token: "secret-token".into(),
        }
    }

    #[test]
    fn session_roundtrip_sets_0600() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path().join("nested"));
        assert_eq!(store.load(), None);
        store.save(&sample()).unwrap();
        assert_eq!(store.load(), Some(sample()));
        let mode = std::fs::metadata(dir.path().join("nested/session.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn clear_keeps_last_server() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path().to_path_buf());
        store.save(&sample()).unwrap();
        store.clear();
        assert_eq!(store.load(), None);
        assert_eq!(
            store.last_server().as_deref(),
            Some("https://jf.example.com/")
        );
    }

    #[test]
    fn corrupt_session_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("session.json"), "{not json").unwrap();
        assert_eq!(SessionStore::new(dir.path().to_path_buf()).load(), None);
    }

    #[test]
    fn device_id_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path().to_path_buf());
        let first = store.device_id();
        assert_eq!(first.len(), 32);
        assert_eq!(store.device_id(), first);
    }
}
