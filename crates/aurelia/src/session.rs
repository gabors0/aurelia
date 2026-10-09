//! Saved sign-ins: every account used on this device (server, user and
//! access token, never the password) and which one is active.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub server: String,
    pub server_name: String,
    pub user_id: String,
    pub user_name: String,
    pub token: String,
    /// The Jellyfin device id this account signed in with. Each account has
    /// its own, so the server never mixes up two users of this installation.
    /// Sessions saved before accounts existed use the installation id.
    #[serde(default)]
    pub device_id: Option<String>,
    /// The user's avatar, for the profile picker.
    #[serde(default)]
    pub image_tag: Option<String>,
}

impl Session {
    pub fn is_same_account(&self, other: &Session) -> bool {
        self.server == other.server && self.user_id == other.user_id
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Accounts {
    /// Most recently used first.
    accounts: Vec<Session>,
    /// Index into `accounts` of the signed-in one.
    #[serde(default)]
    active: Option<usize>,
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

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn read(&self) -> Accounts {
        if let Ok(text) = std::fs::read_to_string(self.dir.join("accounts.json")) {
            return match serde_json::from_str::<Accounts>(&text) {
                Ok(mut accounts) => {
                    accounts.active = accounts.active.filter(|i| *i < accounts.accounts.len());
                    accounts
                }
                Err(err) => {
                    tracing::warn!("ignoring unreadable accounts file: {err}");
                    Accounts::default()
                }
            };
        }
        // Before accounts: one `session.json`. Moved over on first read.
        let legacy = std::fs::read_to_string(self.dir.join("session.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Session>(&text).ok());
        match legacy {
            Some(session) => {
                let accounts = Accounts {
                    accounts: vec![session],
                    active: Some(0),
                };
                if self.write(&accounts).is_ok() {
                    let _ = std::fs::remove_file(self.dir.join("session.json"));
                }
                accounts
            }
            None => Accounts::default(),
        }
    }

    fn write(&self, accounts: &Accounts) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(accounts).map_err(std::io::Error::other)?;
        write_private(&self.dir, "accounts.json", &json)
    }

    /// Every saved account, most recently used first.
    pub fn accounts(&self) -> Vec<Session> {
        self.read().accounts
    }

    /// The signed-in account, if any.
    pub fn load(&self) -> Option<Session> {
        let accounts = self.read();
        accounts.active.map(|i| accounts.accounts[i].clone())
    }

    /// Saves `session` (replacing an older copy of the account) and makes it
    /// the active one.
    pub fn save(&self, session: &Session) -> std::io::Result<()> {
        let mut accounts = self.read();
        accounts.accounts.retain(|s| !s.is_same_account(session));
        accounts.accounts.insert(0, session.clone());
        accounts.active = Some(0);
        self.write(&accounts)?;
        write_private(&self.dir, "last_server", session.server.as_bytes())
    }

    /// Forgets `session`'s account.
    pub fn remove(&self, session: &Session) {
        let mut accounts = self.read();
        let active = accounts.active.map(|i| accounts.accounts[i].clone());
        accounts.accounts.retain(|s| !s.is_same_account(session));
        accounts.active = active
            .filter(|a| !a.is_same_account(session))
            .and_then(|a| accounts.accounts.iter().position(|s| s.is_same_account(&a)));
        if let Err(err) = self.write(&accounts) {
            tracing::warn!("could not update accounts: {err}");
        }
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
fn write_private(dir: &Path, name: &str, contents: &[u8]) -> std::io::Result<()> {
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

    fn account(user: &str) -> Session {
        Session {
            server: "https://jf.example.com/".into(),
            server_name: "Box".into(),
            user_id: format!("id-{user}"),
            user_name: user.into(),
            token: format!("token-{user}"),
            device_id: Some(format!("device-{user}")),
            image_tag: None,
        }
    }

    #[test]
    fn accounts_roundtrip_with_0600() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path().join("nested"));
        assert_eq!(store.load(), None);
        store.save(&account("gabor")).unwrap();
        assert_eq!(store.load(), Some(account("gabor")));
        let mode = std::fs::metadata(dir.path().join("nested/accounts.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn saving_activates_and_replaces_older_copies() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path().to_path_buf());
        store.save(&account("a")).unwrap();
        store.save(&account("b")).unwrap();
        assert_eq!(store.load().unwrap().user_name, "b");
        let mut fresh = account("a");
        fresh.token = "new".into();
        store.save(&fresh).unwrap();
        let names: Vec<String> = store.accounts().into_iter().map(|s| s.user_name).collect();
        assert_eq!(names, ["a", "b"], "most recent first, no duplicates");
        assert_eq!(store.load().unwrap().token, "new");
    }

    #[test]
    fn removing_keeps_the_others_and_last_server() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path().to_path_buf());
        store.save(&account("a")).unwrap();
        store.save(&account("b")).unwrap();
        store.remove(&account("a"));
        assert_eq!(store.load().unwrap().user_name, "b", "b stays active");
        store.remove(&account("b"));
        assert_eq!(store.load(), None);
        assert!(store.accounts().is_empty());
        assert_eq!(
            store.last_server().as_deref(),
            Some("https://jf.example.com/")
        );
    }

    #[test]
    fn removing_the_active_account_leaves_none_active() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path().to_path_buf());
        store.save(&account("a")).unwrap();
        store.save(&account("b")).unwrap();
        store.remove(&account("b"));
        assert_eq!(store.load(), None);
        assert_eq!(store.accounts().len(), 1);
    }

    #[test]
    fn legacy_session_file_becomes_the_active_account() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("session.json"),
            r#"{"server":"https://jf.example.com/","server_name":"Box","user_id":"u1",
                "user_name":"gabor","token":"t"}"#,
        )
        .unwrap();
        let store = SessionStore::new(dir.path().to_path_buf());
        let session = store.load().unwrap();
        assert_eq!(session.user_name, "gabor");
        assert_eq!(session.device_id, None, "keeps the installation id");
        assert!(!dir.path().join("session.json").exists());
        assert_eq!(store.accounts().len(), 1);
    }

    #[test]
    fn corrupt_accounts_file_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("accounts.json"), "{not json").unwrap();
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
