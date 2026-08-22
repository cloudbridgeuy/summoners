//! Best-effort metadata cache shell and deterministic cache policy.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::core::SourceKind;
use crate::providers::DisplayMediaType;

const METADATA_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const THUMBNAIL_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const THUMBNAIL_MAGIC: &[u8; 8] = b"CCERTHM1";
const THUMBNAIL_TIMESTAMP_BYTES: usize = 16;
const THUMBNAIL_CHECKSUM_BYTES: usize = 32;
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
        let Some(encoded) = encode_entry(bytes, fetched_at) else {
            return false;
        };
        write_atomic(&path, &encoded)
    }

    /// Read fresh thumbnail bytes. Any cache error degrades to a miss.
    #[must_use]
    pub fn read_thumbnail(&self, canonical_url: &str, now: SystemTime) -> Option<CachedThumbnail> {
        let path = self.thumbnail_path(canonical_url)?;
        let entry = fs::read(&path)
            .ok()
            .and_then(|bytes| decode_thumbnail_entry(&bytes))
            .filter(|entry| is_thumbnail_fresh(entry.fetched_at, now));

        match entry {
            Some(entry) => Some(CachedThumbnail {
                bytes: entry.bytes,
                media_type: entry.media_type,
            }),
            None => {
                let _ = fs::remove_file(path);
                None
            }
        }
    }

    /// Atomically replace one thumbnail entry. Failure never fails a request.
    #[must_use]
    pub fn write_thumbnail(
        &self,
        canonical_url: &str,
        media_type: DisplayMediaType,
        bytes: &[u8],
        fetched_at: SystemTime,
    ) -> bool {
        let Some(path) = self.thumbnail_path(canonical_url) else {
            return false;
        };
        let Some(encoded) = encode_thumbnail_entry(media_type, bytes, fetched_at) else {
            return false;
        };
        write_atomic(&path, &encoded)
    }

    /// Remove expired or corrupt metadata and thumbnail entries.
    #[must_use]
    pub fn prune_expired(&self, now: SystemTime) -> usize {
        let Some(root) = self.root.as_ref() else {
            return 0;
        };
        prune_metadata_directory(root.join("meta"), now)
            + prune_thumbnail_directory(root.join("thumbs"), now)
    }

    fn metadata_path(&self, source: SourceKind, canonical_request: &str) -> Option<PathBuf> {
        self.root.as_ref().map(|root| {
            root.join("meta")
                .join(source.key())
                .join(format!("{}.json", metadata_key(source, canonical_request)))
        })
    }

    fn thumbnail_path(&self, canonical_url: &str) -> Option<PathBuf> {
        self.root.as_ref().map(|root| {
            root.join("thumbs")
                .join(format!("{}.bin", thumbnail_key(canonical_url)))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CachedMetadata {
    fetched_at: SystemTime,
    bytes: Vec<u8>,
}

/// Fresh provider-derived image bytes and their validated response type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedThumbnail {
    pub bytes: Vec<u8>,
    pub media_type: DisplayMediaType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThumbnailEntry {
    fetched_at: SystemTime,
    bytes: Vec<u8>,
    media_type: DisplayMediaType,
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
fn thumbnail_key(canonical_url: &str) -> String {
    format!("{:x}", Sha256::digest(canonical_url.as_bytes()))
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

#[must_use]
fn encode_thumbnail_entry(
    media_type: DisplayMediaType,
    bytes: &[u8],
    fetched_at: SystemTime,
) -> Option<Vec<u8>> {
    if bytes.is_empty() {
        return None;
    }
    let timestamp = fetched_at.duration_since(UNIX_EPOCH).ok()?.as_nanos();
    let checksum = Sha256::digest(bytes);
    let mut encoded = Vec::with_capacity(
        THUMBNAIL_MAGIC.len()
            + THUMBNAIL_TIMESTAMP_BYTES
            + 1
            + THUMBNAIL_CHECKSUM_BYTES
            + bytes.len(),
    );
    encoded.extend_from_slice(THUMBNAIL_MAGIC);
    encoded.extend_from_slice(&timestamp.to_be_bytes());
    encoded.push(media_type_code(media_type));
    encoded.extend_from_slice(&checksum);
    encoded.extend_from_slice(bytes);
    Some(encoded)
}

fn decode_thumbnail_entry(bytes: &[u8]) -> Option<ThumbnailEntry> {
    let header_length =
        THUMBNAIL_MAGIC.len() + THUMBNAIL_TIMESTAMP_BYTES + 1 + THUMBNAIL_CHECKSUM_BYTES;
    if bytes.len() <= header_length || bytes.get(..THUMBNAIL_MAGIC.len()) != Some(THUMBNAIL_MAGIC) {
        return None;
    }
    let timestamp_start = THUMBNAIL_MAGIC.len();
    let timestamp_end = timestamp_start + THUMBNAIL_TIMESTAMP_BYTES;
    let timestamp: [u8; THUMBNAIL_TIMESTAMP_BYTES] =
        bytes.get(timestamp_start..timestamp_end)?.try_into().ok()?;
    let fetched_at = system_time_from_epoch_nanos(u128::from_be_bytes(timestamp))?;
    let media_type = decode_media_type(*bytes.get(timestamp_end)?)?;
    let checksum_start = timestamp_end + 1;
    let checksum_end = checksum_start + THUMBNAIL_CHECKSUM_BYTES;
    let checksum = bytes.get(checksum_start..checksum_end)?;
    let payload = bytes.get(checksum_end..)?;
    if &Sha256::digest(payload)[..] != checksum {
        return None;
    }
    Some(ThumbnailEntry {
        fetched_at,
        bytes: payload.to_vec(),
        media_type,
    })
}

#[must_use]
const fn media_type_code(media_type: DisplayMediaType) -> u8 {
    match media_type {
        DisplayMediaType::Jpeg => 1,
        DisplayMediaType::Png => 2,
        DisplayMediaType::Webp => 3,
    }
}

#[must_use]
const fn decode_media_type(code: u8) -> Option<DisplayMediaType> {
    match code {
        1 => Some(DisplayMediaType::Jpeg),
        2 => Some(DisplayMediaType::Png),
        3 => Some(DisplayMediaType::Webp),
        _ => None,
    }
}

fn system_time_from_epoch_nanos(timestamp: u128) -> Option<SystemTime> {
    let seconds = u64::try_from(timestamp / 1_000_000_000).ok()?;
    let nanoseconds = u32::try_from(timestamp % 1_000_000_000).ok()?;
    UNIX_EPOCH.checked_add(Duration::new(seconds, nanoseconds))
}

#[must_use]
fn is_thumbnail_fresh(fetched_at: SystemTime, now: SystemTime) -> bool {
    now.duration_since(fetched_at)
        .is_ok_and(|age| age < THUMBNAIL_TTL)
}

fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    if fs::create_dir_all(parent).is_err() {
        return false;
    }
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp_path = parent.join(format!(".entry-{}-{sequence}.tmp", std::process::id()));
    let write_result = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        });
    if write_result.is_err() || fs::rename(&temp_path, path).is_err() {
        let _ = fs::remove_file(temp_path);
        return false;
    }
    true
}

fn prune_metadata_directory(root: PathBuf, now: SystemTime) -> usize {
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

fn prune_thumbnail_directory(root: PathBuf, now: SystemTime) -> usize {
    let Ok(entries) = fs::read_dir(root) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            fs::read(entry.path())
                .ok()
                .and_then(|bytes| decode_thumbnail_entry(&bytes))
                .is_none_or(|cached| !is_thumbnail_fresh(cached.fetched_at, now))
        })
        .filter(|entry| fs::remove_file(entry.path()).is_ok())
        .count()
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
    fn thumbnail_key_is_deterministic_and_url_specific() {
        let first = thumbnail_key("https://example.test/image-one.jpg");
        let second = thumbnail_key("https://example.test/image-one.jpg");
        let other = thumbnail_key("https://example.test/image-two.jpg");
        assert_eq!(first, second);
        assert_ne!(first, other);
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
    fn thumbnail_path_stays_below_the_cache_root() {
        let root = PathBuf::from("/tmp/cceroby-cache-test");
        let cache = Cache::new(root.clone());
        let path = cache
            .thumbnail_path("../../escape?url=https://other.test")
            .expect("cache is enabled");
        assert!(path.starts_with(&root));
        assert_eq!(path.parent(), Some(root.join("thumbs").as_path()));
        assert_eq!(
            path.extension().and_then(std::ffi::OsStr::to_str),
            Some("bin")
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
    fn encoded_thumbnail_round_trips_binary_bytes_and_time() {
        let fetched_at = UNIX_EPOCH + Duration::new(123, 456_789_123);
        let bytes = [0xff, 0xd8, 0x00, 0xff, 0xd9];
        let encoded = encode_thumbnail_entry(DisplayMediaType::Jpeg, &bytes, fetched_at)
            .expect("entry encodes");
        let decoded = decode_thumbnail_entry(&encoded).expect("entry is well formed");
        assert_eq!(decoded.fetched_at, fetched_at);
        assert_eq!(decoded.bytes, bytes);
        assert_eq!(decoded.media_type, DisplayMediaType::Jpeg);
        assert_eq!(
            encode_thumbnail_entry(DisplayMediaType::Jpeg, &[], fetched_at),
            None
        );
        assert_eq!(decode_thumbnail_entry(b"bad"), None);
        assert_eq!(decode_thumbnail_entry(THUMBNAIL_MAGIC), None);
    }

    #[test]
    fn thumbnail_media_type_codes_are_complete_and_reject_unknown_values() {
        let cases = [
            (DisplayMediaType::Jpeg, 1),
            (DisplayMediaType::Png, 2),
            (DisplayMediaType::Webp, 3),
        ];
        for (media_type, code) in cases {
            assert_eq!(media_type_code(media_type), code);
            assert_eq!(decode_media_type(code), Some(media_type));
        }
        assert_eq!(decode_media_type(0), None);
        assert_eq!(decode_media_type(u8::MAX), None);
    }

    #[test]
    fn epoch_nanoseconds_round_trip_without_losing_subsecond_precision() {
        let timestamp = 123_456_789_012_345_678_u128;
        assert_eq!(
            system_time_from_epoch_nanos(timestamp),
            Some(UNIX_EPOCH + Duration::new(123_456_789, 12_345_678))
        );
        assert_eq!(
            system_time_from_epoch_nanos((u128::from(u64::MAX) + 1) * 1_000_000_000),
            None
        );
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
    fn thumbnail_ttl_is_fresh_before_but_not_at_the_thirty_day_boundary() {
        let fetched_at = UNIX_EPOCH + Duration::new(10, 900_000_001);
        assert!(is_thumbnail_fresh(
            fetched_at,
            fetched_at + THUMBNAIL_TTL - Duration::from_nanos(1)
        ));
        assert!(!is_thumbnail_fresh(fetched_at, fetched_at + THUMBNAIL_TTL));
        assert!(!is_thumbnail_fresh(
            fetched_at,
            fetched_at + THUMBNAIL_TTL + Duration::from_secs(1)
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
    fn thumbnail_atomic_write_replaces_an_existing_entry() {
        let directory = tempdir().expect("temporary directory exists");
        let cache = Cache::new(directory.path().to_path_buf());
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert!(cache.write_thumbnail(REQUEST, DisplayMediaType::Jpeg, b"first", now));
        assert!(cache.write_thumbnail(REQUEST, DisplayMediaType::Png, b"second", now));
        assert_eq!(
            cache.read_thumbnail(REQUEST, now),
            Some(CachedThumbnail {
                bytes: b"second".to_vec(),
                media_type: DisplayMediaType::Png,
            })
        );
    }

    #[test]
    fn corrupt_and_expired_thumbnail_entries_are_misses_and_are_removed() {
        let directory = tempdir().expect("temporary directory exists");
        let cache = Cache::new(directory.path().to_path_buf());
        let path = cache.thumbnail_path(REQUEST).expect("cache is enabled");
        fs::create_dir_all(path.parent().expect("path has a parent"))
            .expect("cache directory exists");
        fs::write(&path, b"bad").expect("fixture writes");
        assert_eq!(cache.read_thumbnail(REQUEST, UNIX_EPOCH), None);
        assert!(!path.exists());

        assert!(cache.write_thumbnail(REQUEST, DisplayMediaType::Jpeg, b"image", UNIX_EPOCH));
        assert_eq!(
            cache.read_thumbnail(REQUEST, UNIX_EPOCH + THUMBNAIL_TTL),
            None
        );
        assert!(!path.exists());
    }

    #[test]
    fn tampered_thumbnail_payload_is_a_miss_and_is_removed() {
        let directory = tempdir().expect("temporary directory exists");
        let cache = Cache::new(directory.path().to_path_buf());
        let path = cache.thumbnail_path(REQUEST).expect("cache is enabled");
        fs::create_dir_all(path.parent().expect("path has a parent"))
            .expect("cache directory exists");
        let mut encoded = encode_thumbnail_entry(
            DisplayMediaType::Webp,
            b"valid payload",
            UNIX_EPOCH + Duration::new(1, 7),
        )
        .expect("entry encodes");
        let last = encoded.last_mut().expect("payload is present");
        *last ^= 0xff;
        fs::write(&path, encoded).expect("fixture writes");

        assert_eq!(
            cache.read_thumbnail(REQUEST, UNIX_EPOCH + Duration::from_secs(2)),
            None
        );
        assert!(!path.exists());
    }

    #[test]
    fn thumbnail_cache_preserves_the_exact_ttl_boundary_after_round_trip() {
        let directory = tempdir().expect("temporary directory exists");
        let cache = Cache::new(directory.path().to_path_buf());
        let fetched_at = UNIX_EPOCH + Duration::new(10, 900_000_001);
        assert!(cache.write_thumbnail(REQUEST, DisplayMediaType::Webp, b"image", fetched_at));
        assert!(
            cache
                .read_thumbnail(
                    REQUEST,
                    fetched_at + THUMBNAIL_TTL - Duration::from_nanos(1)
                )
                .is_some()
        );
        assert_eq!(
            cache.read_thumbnail(REQUEST, fetched_at + THUMBNAIL_TTL),
            None
        );
    }

    #[test]
    fn startup_pruning_covers_metadata_and_thumbnail_entries() {
        let directory = tempdir().expect("temporary directory exists");
        let cache = Cache::new(directory.path().to_path_buf());
        let now = UNIX_EPOCH + Duration::from_secs(2 * THUMBNAIL_TTL.as_secs());
        assert!(cache.write_metadata(
            SourceKind::ArtInstituteChicago,
            "expired-meta",
            b"metadata",
            UNIX_EPOCH
        ));
        assert!(cache.write_thumbnail(
            "expired-thumb",
            DisplayMediaType::Jpeg,
            b"image",
            UNIX_EPOCH
        ));
        assert!(cache.write_thumbnail("fresh-thumb", DisplayMediaType::Jpeg, b"image", now));

        assert_eq!(cache.prune_expired(now), 2);
        assert_eq!(
            cache.read_thumbnail("fresh-thumb", now),
            Some(CachedThumbnail {
                bytes: b"image".to_vec(),
                media_type: DisplayMediaType::Jpeg,
            })
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
        assert!(!cache.write_thumbnail(
            REQUEST,
            DisplayMediaType::Jpeg,
            b"image",
            SystemTime::now()
        ));
        assert_eq!(cache.read_thumbnail(REQUEST, SystemTime::now()), None);
    }

    #[test]
    fn cache_without_a_user_directory_is_disabled() {
        let cache = Cache { root: None };
        assert_eq!(
            cache.read_metadata(SourceKind::ArtInstituteChicago, REQUEST, SystemTime::now()),
            None
        );
        assert_eq!(cache.prune_expired(SystemTime::now()), 0);
        assert_eq!(cache.read_thumbnail(REQUEST, SystemTime::now()), None);
    }
}
