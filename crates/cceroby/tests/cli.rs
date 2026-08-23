//! User-visible command-line behavior.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

use tempfile::tempdir;

#[test]
fn invalid_output_path_has_one_short_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_cceroby"))
        .args(["search", "mask", "--out", "Cargo.toml"])
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) => panic!("cannot run cceroby: {error}"),
    };
    let stderr = match String::from_utf8(output.stderr) {
        Ok(stderr) => stderr,
        Err(error) => panic!("stderr is not UTF-8: {error}"),
    };

    assert!(!output.status.success());
    assert_eq!(stderr, "output path is not a directory: Cargo.toml\n");
}

#[test]
fn cache_info_reports_the_exact_empty_cache_summary() {
    let home = tempdir().unwrap_or_else(|error| panic!("cannot create temporary home: {error}"));
    let root = cache_root_for_test(home.path());
    let output = isolated_command(home.path())
        .args(["cache", "info"])
        .output()
        .unwrap_or_else(|error| panic!("cannot run cceroby: {error}"));

    assert!(output.status.success());
    assert_eq!(output.stderr, b"");
    assert_eq!(
        String::from_utf8(output.stdout)
            .unwrap_or_else(|error| panic!("stdout is not UTF-8: {error}")),
        format!(
            "Cache: {}\nMetadata: 0 fresh files, 0 bytes; 0 expired files, 0 bytes.\nThumbnails: 0 fresh files, 0 bytes; 0 expired files, 0 bytes.\n",
            root.display()
        )
    );
}

#[test]
fn cache_clear_removes_only_the_isolated_cceroby_root() {
    let home = tempdir().unwrap_or_else(|error| panic!("cannot create temporary home: {error}"));
    let root = cache_root_for_test(home.path());
    std::fs::create_dir_all(root.join("meta/nested"))
        .unwrap_or_else(|error| panic!("cannot create cache fixture: {error}"));
    std::fs::create_dir_all(root.join("thumbs"))
        .unwrap_or_else(|error| panic!("cannot create cache fixture: {error}"));
    std::fs::write(root.join("meta/nested/one"), b"one")
        .unwrap_or_else(|error| panic!("cannot write cache fixture: {error}"));
    std::fs::write(root.join("thumbs/two"), b"two")
        .unwrap_or_else(|error| panic!("cannot write cache fixture: {error}"));
    let keep = root.parent().map(|parent| parent.join("keep"));
    let Some(keep) = keep else {
        panic!("cache root has no parent");
    };
    std::fs::write(&keep, b"keep")
        .unwrap_or_else(|error| panic!("cannot write outside fixture: {error}"));

    let output = isolated_command(home.path())
        .args(["cache", "clear"])
        .output()
        .unwrap_or_else(|error| panic!("cannot run cceroby: {error}"));

    assert!(output.status.success());
    assert_eq!(output.stderr, b"");
    assert_eq!(
        String::from_utf8(output.stdout)
            .unwrap_or_else(|error| panic!("stdout is not UTF-8: {error}")),
        format!("Removed 2 cache files from {}.\n", root.display())
    );
    assert!(!root.exists());
    assert_eq!(
        std::fs::read(keep).unwrap_or_else(|error| panic!("cannot read outside fixture: {error}")),
        b"keep"
    );
}

fn isolated_command(home: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cceroby"));
    command
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"));
    command
}

#[cfg(target_os = "macos")]
fn cache_root_for_test(home: &std::path::Path) -> std::path::PathBuf {
    home.join("Library/Caches/cceroby")
}

#[cfg(not(target_os = "macos"))]
fn cache_root_for_test(home: &std::path::Path) -> std::path::PathBuf {
    home.join("cache/cceroby")
}
