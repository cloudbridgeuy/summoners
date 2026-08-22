//! Best-effort metadata cache shell and deterministic cache policy.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::core::SourceKind;

const METADATA_TTL: Duration = Duration::from_secs(24 * 60 * 60);
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A plain per-entry cache rooted outside the repository.
#[derive(Debug, Clone)]
pub struct Cache {
    root: Option<PathBuf>,
}

impl Cache {
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root: Some(root) }
    }

    #[must_use]
    pub fn from_user_cache_dir() -> Self {
        Self {
            root: dirs::cache_dir().map(|path| path.join("cceroby")),
        }
    }

    /// Read fresh metadata. Any cache error degrades to a miss.
    #[must_use]
    pub fn read_metadata(
        &self,
        source: SourceKind,
        canonical_request: &str,
        now: SystemTime,
    ) -> Option<Vec<u8>> {
        let path = self.metadata_path(source, canonical_request)?;
        let entry = fs::read(&path)
            .ok()
            .and_then(|bytes| decode_entry(&bytes))
            .filter(|entry| is_fresh(entry.fetched_at, now));

        match entry {
            Some(entry) => Some(entry.bytes),
            None => {
                let _ = fs::remove_file(path);
                None
            }
        }
    }

    /// Atomically replace one metadata entry. Failure never leaves a request failed.
    #[must_use]
    pub fn write_metadata(
        &self,
        source: SourceKind,
        canonical_request: &str,
        bytes: &[u8],
        fetched_at: SystemTime,
    ) -> bool {
        let Some(path) = self.metadata_path(source, canonical_request) else {
            return false;
        };
        let Some(parent) = path.parent() else {
            return false;
        };
        if fs::create_dir_all(parent).is_err() {
            return false;
        }

        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp_name = format!(".entry-{}-{sequence}.tmp", std::process::id());
        let temp_path = parent.join(temp_name);
        let Some(encoded) = encode_entry(bytes, fetched_at) else {
            return false;
        };
        let write_result = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .and_then(|mut file| {
                file.write_all(&encoded)?;
                file.sync_all()
            });
        if write_result.is_err() {
            let _ = fs::remove_file(temp_path);
            return false;
        }
        if fs::rename(&temp_path, &path).is_err() {
            let _ = fs::remove_file(temp_path);
            return false;
        }
        true
    }

    /// Remove expired or corrupt metadata entries. Individual I/O errors are ignored.
    #[must_use]
    pub fn prune_expired(&self, now: SystemTime) -> usize {
        let Some(root) = self.root.as_ref().map(|root| root.join("meta")) else {
            return 0;
        };
        let Ok(source_directories) = fs::read_dir(root) else {
            return 0;
        };

        source_directories
            .filter_map(Result::ok)
            .filter_map(|entry| fs::read_dir(entry.path()).ok())
            .flat_map(|entries| entries.filter_map(Result::ok))
            .filter(|entry| {
                fs::read(entry.path())
                    .ok()
                    .and_then(|bytes| decode_entry(&bytes))
                    .is_none_or(|cached| !is_fresh(cached.fetched_at, now))
            })
            .filter(|entry| fs::remove_file(entry.path()).is_ok())
            .count()
    }

    fn metadata_path(&self, source: SourceKind, canonical_request: &str) -> Option<PathBuf> {
        self.root.as_ref().map(|root| {
            root.join("meta")
                .join(source.key())
                .join(format!("{}.json", metadata_key(source, canonical_request)))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CachedMetadata {
    fetched_at: SystemTime,
    bytes: Vec<u8>,
}

#[derive(Debug, Serialize)]
struct DiskMetadata<'a> {
    fetched_at: u64,
    body: &'a str,
}

#[derive(Debug, Deserialize)]
struct OwnedDiskMetadata {
    fetched_at: u64,
    body: String,
}

#[must_use]
fn metadata_key(source: SourceKind, canonical_request: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(source.key().as_bytes());
    hash.update([0]);
    hash.update(canonical_request.as_bytes());
    format!("{:x}", hash.finalize())
}

#[must_use]
fn encode_entry(bytes: &[u8], fetched_at: SystemTime) -> Option<Vec<u8>> {
    let timestamp = fetched_at
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let body = std::str::from_utf8(bytes).ok()?;
    serde_json::to_vec(&DiskMetadata {
        fetched_at: timestamp,
        body,
    })
    .ok()
}

fn decode_entry(bytes: &[u8]) -> Option<CachedMetadata> {
    let entry: OwnedDiskMetadata = serde_json::from_slice(bytes).ok()?;
    let fetched_at = UNIX_EPOCH.checked_add(Duration::from_secs(entry.fetched_at))?;
    Some(CachedMetadata {
        fetched_at,
        bytes: entry.body.into_bytes(),
    })
}

#[must_use]
fn is_fresh(fetched_at: SystemTime, now: SystemTime) -> bool {
    now.duration_since(fetched_at)
        .is_ok_and(|age| age < METADATA_TTL)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use tempfile::tempdir;

    use super::*;

    const REQUEST: &str = "https://example.test/search?q=mask";

    #[test]
    fn metadata_key_is_deterministic_and_source_specific() {
        let first = metadata_key(SourceKind::ArtInstituteChicago, REQUEST);
        let second = metadata_key(SourceKind::ArtInstituteChicago, REQUEST);
        let other_source = metadata_key(SourceKind::ClevelandMuseum, REQUEST);
        assert_eq!(first, second);
        assert_ne!(first, other_source);
        assert_eq!(first.len(), 64);
    }

    #[test]
    fn metadata_path_stays_below_the_cache_root() {
        let root = PathBuf::from("/tmp/cceroby-cache-test");
        let cache = Cache::new(root.clone());
        let path = cache
            .metadata_path(SourceKind::ArtInstituteChicago, "../../escape")
            .expect("cache is enabled");
        assert!(path.starts_with(&root));
        assert_eq!(
            path.parent().and_then(std::path::Path::file_name),
            Some("aic".as_ref())
        );
    }

    #[test]
    fn encoded_entry_round_trips_bytes_and_time() {
        let fetched_at = UNIX_EPOCH + Duration::from_secs(123);
        let encoded = encode_entry(b"metadata", fetched_at).expect("entry encodes");
        let decoded = decode_entry(&encoded).expect("entry is well formed");
        assert_eq!(decoded.fetched_at, fetched_at);
        assert_eq!(decoded.bytes, b"metadata");
        assert_eq!(encode_entry(&[0xff], fetched_at), None);
    }

    #[test]
    fn ttl_is_fresh_before_but_not_at_the_boundary() {
        let fetched_at = UNIX_EPOCH + Duration::from_secs(10);
        assert!(is_fresh(
            fetched_at,
            fetched_at + METADATA_TTL - Duration::from_secs(1)
        ));
        assert!(!is_fresh(fetched_at, fetched_at + METADATA_TTL));
        assert!(!is_fresh(
            fetched_at,
            fetched_at + METADATA_TTL + Duration::from_secs(1)
        ));
    }

    #[test]
    fn corrupt_entry_is_a_miss_and_is_removed() {
        let directory = tempdir().expect("temporary directory exists");
        let cache = Cache::new(directory.path().to_path_buf());
        let path = cache
            .metadata_path(SourceKind::ArtInstituteChicago, REQUEST)
            .expect("cache is enabled");
        fs::create_dir_all(path.parent().expect("path has a parent"))
            .expect("cache directory exists");
        fs::write(&path, b"bad").expect("fixture writes");

        assert_eq!(
            cache.read_metadata(SourceKind::ArtInstituteChicago, REQUEST, SystemTime::now()),
            None
        );
        assert!(!path.exists());
    }

    #[test]
    fn overflowing_timestamp_is_corrupt_for_reads_and_pruning() {
        let directory = tempdir().expect("temporary directory exists");
        let cache = Cache::new(directory.path().to_path_buf());
        let path = cache
            .metadata_path(SourceKind::ArtInstituteChicago, REQUEST)
            .expect("cache is enabled");
        fs::create_dir_all(path.parent().expect("path has a parent"))
            .expect("cache directory exists");
        let overflow = format!(r#"{{"fetched_at":{},"body":"metadata"}}"#, u64::MAX);

        assert_eq!(decode_entry(overflow.as_bytes()), None);
        fs::write(&path, &overflow).expect("fixture writes");
        assert_eq!(
            cache.read_metadata(SourceKind::ArtInstituteChicago, REQUEST, UNIX_EPOCH),
            None
        );
        assert!(!path.exists());

        fs::write(&path, overflow).expect("fixture writes again");
        assert_eq!(cache.prune_expired(UNIX_EPOCH), 1);
        assert!(!path.exists());
    }

    #[test]
    fn atomic_write_replaces_an_existing_entry() {
        let directory = tempdir().expect("temporary directory exists");
        let cache = Cache::new(directory.path().to_path_buf());
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert!(cache.write_metadata(SourceKind::ArtInstituteChicago, REQUEST, b"first", now));
        assert!(cache.write_metadata(SourceKind::ArtInstituteChicago, REQUEST, b"second", now));
        assert_eq!(
            cache.read_metadata(SourceKind::ArtInstituteChicago, REQUEST, now),
            Some(b"second".to_vec())
        );
    }

    #[test]
    fn prune_removes_expired_and_keeps_fresh_metadata() {
        let directory = tempdir().expect("temporary directory exists");
        let cache = Cache::new(directory.path().to_path_buf());
        let now = UNIX_EPOCH + Duration::from_secs(2 * METADATA_TTL.as_secs());
        assert!(cache.write_metadata(SourceKind::ArtInstituteChicago, "fresh", b"fresh", now));
        assert!(cache.write_metadata(
            SourceKind::ArtInstituteChicago,
            "expired",
            b"expired",
            UNIX_EPOCH
        ));

        assert_eq!(cache.prune_expired(now), 1);
        assert_eq!(
            cache.read_metadata(SourceKind::ArtInstituteChicago, "fresh", now),
            Some(b"fresh".to_vec())
        );
    }

    #[test]
    fn cache_write_failure_is_best_effort() {
        let directory = tempdir().expect("temporary directory exists");
        let root_file = directory.path().join("not-a-directory");
        fs::write(&root_file, b"file").expect("fixture writes");
        let cache = Cache::new(root_file);

        assert!(!cache.write_metadata(
            SourceKind::ArtInstituteChicago,
            REQUEST,
            b"metadata",
            SystemTime::now()
        ));
        assert_eq!(
            cache.read_metadata(SourceKind::ArtInstituteChicago, REQUEST, SystemTime::now()),
            None
        );
    }

    #[test]
    fn cache_without_a_user_directory_is_disabled() {
        let cache = Cache { root: None };
        assert_eq!(
            cache.read_metadata(SourceKind::ArtInstituteChicago, REQUEST, SystemTime::now()),
            None
        );
        assert_eq!(cache.prune_expired(SystemTime::now()), 0);
    }
}
