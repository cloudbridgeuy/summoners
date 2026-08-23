//! Direct cache management tests split out to keep the implementation compact.

use std::fs;
use std::path::Path;

use tempfile::tempdir;

use super::*;

#[test]
fn stats_for_a_missing_root_are_empty_and_include_the_resolved_path() {
    let directory = tempdir().expect("temporary directory exists");
    let root = directory.path().join("missing");
    let stats = Cache::new(root.clone()).stats(UNIX_EPOCH);

    assert_eq!(stats.root, Some(root));
    assert_eq!(stats.metadata, CacheLayerStats::default());
    assert_eq!(stats.thumbnails, CacheLayerStats::default());
}

#[test]
fn stats_classify_nested_fresh_expired_and_corrupt_entries_with_bytes() {
    let directory = tempdir().expect("temporary directory exists");
    let cache = Cache::new(directory.path().join("cceroby"));
    let now = UNIX_EPOCH + Duration::from_secs(2 * THUMBNAIL_TTL.as_secs());
    assert!(cache.write_metadata(SourceKind::ArtInstituteChicago, "fresh", b"fresh", now));
    assert!(cache.write_metadata(
        SourceKind::ArtInstituteChicago,
        "expired",
        b"expired",
        now - METADATA_TTL
    ));
    assert!(cache.write_thumbnail("fresh", DisplayMediaType::Png, b"image", now));
    assert!(cache.write_thumbnail(
        "expired",
        DisplayMediaType::Jpeg,
        b"old image",
        now - THUMBNAIL_TTL
    ));
    let corrupt = directory
        .path()
        .join("cceroby/meta/nested/deeper/corrupt.json");
    fs::create_dir_all(corrupt.parent().expect("corrupt file has a parent"))
        .expect("nested fixture directory exists");
    fs::write(&corrupt, b"bad").expect("corrupt fixture writes");

    let stats = cache.stats(now);
    assert_eq!(stats.metadata.fresh.files, 1);
    assert_eq!(stats.metadata.expired.files, 2);
    assert_eq!(stats.thumbnails.fresh.files, 1);
    assert_eq!(stats.thumbnails.expired.files, 1);
    assert_eq!(
        stats.metadata.fresh.bytes + stats.metadata.expired.bytes,
        count_layer_bytes(&directory.path().join("cceroby/meta"))
    );
    assert_eq!(
        stats.thumbnails.fresh.bytes + stats.thumbnails.expired.bytes,
        count_layer_bytes(&directory.path().join("cceroby/thumbs"))
    );
}

#[test]
fn clear_removes_only_the_selected_root_and_is_idempotent() {
    let directory = tempdir().expect("temporary directory exists");
    let root = directory.path().join("cceroby");
    let cache = Cache::new(root.clone());
    fs::create_dir_all(root.join("meta/nested")).expect("cache fixture directory exists");
    fs::create_dir_all(root.join("thumbs")).expect("cache fixture directory exists");
    fs::write(root.join("meta/one"), b"one").expect("fixture writes");
    fs::write(root.join("meta/nested/two"), b"two").expect("fixture writes");
    fs::write(root.join("thumbs/three"), b"three").expect("fixture writes");
    let outside = directory.path().join("keep.txt");
    fs::write(&outside, b"keep").expect("outside fixture writes");

    assert_eq!(cache.clear().expect("cache clear succeeds"), 3);
    assert!(!root.exists());
    assert_eq!(
        fs::read(&outside).expect("outside fixture remains readable"),
        b"keep"
    );
    assert_eq!(cache.clear().expect("second cache clear succeeds"), 0);
}

#[cfg(unix)]
#[test]
fn stats_prune_and_clear_do_not_follow_symbolic_links() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory exists");
    let root = directory.path().join("cceroby");
    let outside = directory.path().join("outside");
    fs::create_dir_all(&outside).expect("outside directory exists");
    fs::write(outside.join("keep"), b"outside").expect("outside fixture writes");
    fs::create_dir_all(root.join("meta")).expect("cache fixture directory exists");
    symlink(&outside, root.join("meta/link")).expect("symbolic link is created");
    let cache = Cache::new(root.clone());

    let stats = cache.stats(UNIX_EPOCH);
    assert_eq!(stats.metadata.expired.files, 1);
    assert_eq!(cache.prune_expired(UNIX_EPOCH), 1);
    assert_eq!(
        fs::read(outside.join("keep")).expect("outside fixture remains readable"),
        b"outside"
    );

    symlink(&outside, root.join("thumbs-link")).expect("symbolic link is created");
    assert_eq!(cache.clear().expect("cache clear succeeds"), 1);
    assert_eq!(
        fs::read(outside.join("keep")).expect("outside fixture remains readable"),
        b"outside"
    );
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_cache_root_is_never_traversed() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory exists");
    let outside = directory.path().join("outside");
    fs::create_dir_all(outside.join("meta/aic")).expect("outside directory exists");
    let outside_entry = outside.join("meta/aic/keep.json");
    fs::write(&outside_entry, b"outside").expect("outside fixture writes");
    let root = directory.path().join("cceroby");
    symlink(&outside, &root).expect("symbolic link is created");
    let cache = Cache::new(root.clone());

    assert_eq!(cache.stats(UNIX_EPOCH).metadata, CacheLayerStats::default());
    assert_eq!(cache.prune_expired(UNIX_EPOCH), 0);
    assert!(!cache.write_metadata(
        SourceKind::ArtInstituteChicago,
        "request",
        b"metadata",
        UNIX_EPOCH
    ));
    assert_eq!(
        fs::read(&outside_entry).expect("outside file remains"),
        b"outside"
    );
    assert_eq!(cache.clear().expect("cache link clear succeeds"), 1);
    assert!(!root.exists());
    assert_eq!(
        fs::read(&outside_entry).expect("outside file remains"),
        b"outside"
    );
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_source_directory_cannot_redirect_cache_io() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory exists");
    let root = directory.path().join("cceroby");
    let outside = directory.path().join("outside");
    fs::create_dir_all(root.join("meta")).expect("metadata directory exists");
    fs::create_dir_all(&outside).expect("outside directory exists");
    symlink(&outside, root.join("meta/aic")).expect("symbolic link is created");
    let cache = Cache::new(root);

    assert!(!cache.write_metadata(
        SourceKind::ArtInstituteChicago,
        "request",
        b"metadata",
        UNIX_EPOCH
    ));
    assert_eq!(
        cache.read_metadata(SourceKind::ArtInstituteChicago, "request", UNIX_EPOCH),
        None
    );
    assert_eq!(
        fs::read_dir(outside)
            .expect("outside directory remains")
            .count(),
        0
    );
}

fn count_layer_bytes(root: &Path) -> u64 {
    let mut bytes = 0;
    visit_files(root, &mut |_, size| bytes += size);
    bytes
}
