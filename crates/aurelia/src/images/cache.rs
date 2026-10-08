//! On-disk cache of downloaded image bytes.

use std::path::{Path, PathBuf};

/// Stable 64-bit FNV-1a of the URL, as hex. Image URLs carry the server's
/// image tag, so a changed image gets a new key automatically.
pub fn cache_key(url: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in url.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub struct DiskCache {
    dir: PathBuf,
}

impl DiskCache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn default_location() -> Self {
        let dir = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("aurelia")
            .join("images");
        Self::new(dir)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, url: &str) -> PathBuf {
        self.dir.join(cache_key(url))
    }

    pub fn read(&self, url: &str) -> Option<Vec<u8>> {
        std::fs::read(self.path(url)).ok()
    }

    pub fn write(&self, url: &str, bytes: &[u8]) {
        let path = self.path(url);
        let tmp = path.with_extension("tmp");
        let result = std::fs::create_dir_all(&self.dir)
            .and_then(|()| std::fs::write(&tmp, bytes))
            .and_then(|()| std::fs::rename(&tmp, &path));
        if let Err(err) = result {
            tracing::debug!("image cache write failed: {err}");
        }
    }

    /// Deletes least-recently-written files until the cache is under `max_bytes`.
    pub fn prune(&self, max_bytes: u64) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let meta = entry.metadata().ok()?;
                if !meta.is_file() {
                    return None;
                }
                Some((meta.modified().ok()?, meta.len(), entry.path()))
            })
            .collect();
        let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
        files.sort_by_key(|(modified, _, _)| *modified);
        for (_, len, path) in files {
            if total <= max_bytes {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= len;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_stable() {
        let key = cache_key("https://jf.example.com/Items/a/Images/Primary?tag=1&maxWidth=400");
        assert_eq!(key.len(), 16);
        assert_eq!(
            key,
            cache_key("https://jf.example.com/Items/a/Images/Primary?tag=1&maxWidth=400")
        );
        assert_ne!(
            key,
            cache_key("https://jf.example.com/Items/a/Images/Primary?tag=2&maxWidth=400")
        );
        assert_eq!(cache_key(""), "cbf29ce484222325");
    }

    #[test]
    fn write_then_read() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path().join("img"));
        assert_eq!(cache.read("u"), None);
        cache.write("u", b"bytes");
        assert_eq!(cache.read("u").as_deref(), Some(&b"bytes"[..]));
    }

    #[test]
    fn prune_removes_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path().to_path_buf());
        cache.write("old", &[0; 600]);
        std::thread::sleep(std::time::Duration::from_millis(20));
        cache.write("new", &[0; 600]);
        cache.prune(1000);
        assert_eq!(cache.read("old"), None);
        assert!(cache.read("new").is_some());
    }
}
