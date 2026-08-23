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
fn clear_propagates_an_inaccessible_parent_error() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary directory exists");
    let parent = directory.path().join("locked");
    let root = parent.join("cceroby");
    fs::create_dir_all(&root).expect("cache fixture directory exists");
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o000))
        .expect("parent permissions change");

    let result = Cache::new(root).clear();

    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))
        .expect("parent permissions restore");
    let error = result.expect_err("inaccessible cache parent must fail");
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
}

#[cfg(unix)]
#[test]
fn clear_propagates_a_snapshot_count_error() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary directory exists");
    let root = directory.path().join("cceroby");
    fs::create_dir_all(&root).expect("cache fixture directory exists");
    fs::write(root.join("one"), b"one").expect("cache fixture writes");
    let cache = Cache::new(root);

    let result = cache.clear_with_after_detach(|| {
        let tombstone = clear_tombstone(directory.path());
        fs::set_permissions(tombstone, fs::Permissions::from_mode(0o000))
            .expect("snapshot permissions change");
    });

    let tombstone = clear_tombstone(directory.path());
    fs::set_permissions(&tombstone, fs::Permissions::from_mode(0o700))
        .expect("snapshot permissions restore");
    let error = result.expect_err("snapshot count failure must propagate");
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
}

#[cfg(unix)]
#[test]
fn clear_propagates_a_snapshot_delete_error() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary directory exists");
    let root = directory.path().join("cceroby");
    fs::create_dir_all(&root).expect("cache fixture directory exists");
    fs::write(root.join("one"), b"one").expect("cache fixture writes");
    let snapshot = crate::cache::cache_fs::detach_root(&root)
        .expect("cache root detaches")
        .expect("cache root exists");

    let result = crate::cache::cache_fs::remove_snapshot_with_delete_hook(&snapshot, || {
        let tombstone = clear_tombstone(directory.path());
        fs::set_permissions(tombstone, fs::Permissions::from_mode(0o000))
            .expect("snapshot permissions change");
    });

    let tombstone = clear_tombstone(directory.path());
    fs::set_permissions(&tombstone, fs::Permissions::from_mode(0o700))
        .expect("snapshot permissions restore");
    let error = result.expect_err("snapshot delete failure must propagate");
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
}

#[cfg(unix)]
#[test]
fn a_concurrent_writer_after_detach_creates_a_new_root_that_clear_preserves() {
    let directory = tempdir().expect("temporary directory exists");
    let root = directory.path().join("cceroby");
    let cache = Cache::new(root.clone());
    let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
    assert!(cache.write_metadata(SourceKind::ArtInstituteChicago, "old", b"old", now));
    assert!(cache.write_thumbnail("old", DisplayMediaType::Jpeg, b"old image", now));

    let removed = cache
        .clear_with_after_detach(|| {
            std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        assert!(cache.write_metadata(
                            SourceKind::ArtInstituteChicago,
                            "new",
                            b"new",
                            now
                        ));
                    })
                    .join()
                    .expect("concurrent cache writer completes");
            });
        })
        .expect("cache snapshot clears");

    assert_eq!(removed, 2);
    assert_eq!(
        cache.read_metadata(SourceKind::ArtInstituteChicago, "new", now),
        Some(b"new".to_vec())
    );
    assert_eq!(
        cache.read_metadata(SourceKind::ArtInstituteChicago, "old", now),
        None
    );
    assert!(
        !directory
            .path()
            .read_dir()
            .expect("cache parent remains readable")
            .flatten()
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".cceroby-clear-"))
    );
}

#[cfg(unix)]
#[test]
fn an_open_root_handle_does_not_follow_a_swapped_root_link() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory exists");
    let root = directory.path().join("cceroby");
    let moved = directory.path().join("moved-cache");
    let outside = directory.path().join("outside");
    fs::create_dir_all(root.join("meta/aic")).expect("cache fixture directory exists");
    fs::create_dir_all(outside.join("meta/aic")).expect("outside fixture directory exists");
    fs::write(root.join("meta/aic/entry"), b"inside").expect("cache fixture writes");
    fs::write(outside.join("meta/aic/entry"), b"outside").expect("outside fixture writes");

    let bytes =
        crate::cache::cache_fs::read_with_after_root_open(&root, &["meta", "aic"], "entry", || {
            fs::rename(&root, &moved).expect("cache root moves");
            symlink(&outside, &root).expect("replacement root link is created");
        })
        .expect("pinned root remains readable");

    assert_eq!(bytes, b"inside");
    assert_eq!(
        fs::read(outside.join("meta/aic/entry")).expect("outside remains"),
        b"outside"
    );
}

#[cfg(unix)]
#[test]
fn a_swapped_entry_link_is_rejected_before_open() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory exists");
    let root = directory.path().join("cceroby");
    let entry = root.join("meta/aic/entry");
    let moved = root.join("meta/aic/moved-entry");
    let outside = directory.path().join("outside");
    fs::create_dir_all(entry.parent().expect("entry has a parent"))
        .expect("cache fixture directory exists");
    fs::write(&entry, b"inside").expect("cache fixture writes");
    fs::write(&outside, b"outside").expect("outside fixture writes");

    let result = crate::cache::cache_fs::read_with_before_file_open(
        &root,
        &["meta", "aic"],
        "entry",
        || {
            fs::rename(&entry, &moved).expect("cache entry moves");
            symlink(&outside, &entry).expect("replacement entry link is created");
        },
    );

    assert!(result.is_err());
    assert_eq!(fs::read(&outside).expect("outside remains"), b"outside");
}

#[cfg(unix)]
#[test]
fn clear_removes_a_swapped_root_link_without_following_its_target() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory exists");
    let root = directory.path().join("cceroby");
    let moved = directory.path().join("moved-cache");
    let outside = directory.path().join("outside");
    fs::create_dir_all(&root).expect("cache fixture directory exists");
    fs::create_dir_all(&outside).expect("outside fixture directory exists");
    fs::write(root.join("old"), b"old").expect("cache fixture writes");
    fs::write(outside.join("keep"), b"outside").expect("outside fixture writes");

    let snapshot = crate::cache::cache_fs::detach_root_with_rename_hook(&root, || {
        fs::rename(&root, &moved).expect("cache root moves");
        symlink(&outside, &root).expect("replacement root link is created");
    })
    .expect("replacement root detaches")
    .expect("replacement root exists");
    let removed =
        crate::cache::cache_fs::remove_snapshot(&snapshot).expect("detached root link is removed");

    assert_eq!(removed, 1);
    assert!(!root.exists());
    assert_eq!(
        fs::read(moved.join("old")).expect("moved cache remains"),
        b"old"
    );
    assert_eq!(
        fs::read(outside.join("keep")).expect("outside remains"),
        b"outside"
    );
}

#[cfg(unix)]
#[test]
fn prune_removes_a_swapped_entry_link_without_reading_its_target() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory exists");
    let root = directory.path().join("cceroby");
    let entry = root.join("meta/entry");
    let moved = root.join("meta/moved-entry");
    let outside = directory.path().join("outside");
    fs::create_dir_all(entry.parent().expect("entry has a parent"))
        .expect("cache fixture directory exists");
    fs::write(&entry, b"corrupt").expect("cache fixture writes");
    fs::write(&outside, b"outside").expect("outside fixture writes");
    let mut swapped = false;

    let removed = crate::cache::cache_fs::prune_with_before_leaf(
        &root,
        "meta",
        UNIX_EPOCH,
        CacheLayer::Metadata,
        |_| {
            assert!(!swapped, "fixture has one cache entry");
            swapped = true;
            fs::rename(&entry, &moved).expect("cache entry moves");
            symlink(&outside, &entry).expect("replacement entry link is created");
        },
    );

    assert!(swapped);
    assert_eq!(removed, 1);
    assert!(!entry.exists());
    assert_eq!(
        fs::read(&moved).expect("moved cache entry remains"),
        b"corrupt"
    );
    assert_eq!(fs::read(&outside).expect("outside remains"), b"outside");
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
    let Ok(entries) = fs::read_dir(root) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                return 0;
            };
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                count_layer_bytes(&entry.path())
            } else {
                metadata.len()
            }
        })
        .sum()
}

#[cfg(unix)]
fn clear_tombstone(parent: &Path) -> std::path::PathBuf {
    parent
        .read_dir()
        .expect("cache parent remains readable")
        .flatten()
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".cceroby-clear-")
        })
        .map(|entry| entry.path())
        .expect("clear tombstone exists")
}
