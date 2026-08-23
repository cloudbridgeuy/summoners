//! Shared support for the shell's subprocess tests: a golden-file locator
//! and a uniquely named temporary directory that cleans itself up.
//!
//! Every subcommand's subprocess tests need both, so they live here once
//! instead of being copied into each `tests/<command>.rs` file.

#![allow(dead_code)]

use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

/// The path to one golden transcript fixture owned by `summoners_match_log`.
pub fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../match-log/tests/goldens")
        .join(name)
}

/// A uniquely named directory under the system temporary directory,
/// removed on drop. Every temporary file a subprocess test writes must
/// live under one of these, never in the repository.
pub struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    pub fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "summoners-shell-test-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("the temporary directory is created");
        Self { path }
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
