//! Same-directory durable temporary writes and atomic JPEG replacement.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;

use crate::download::{SavedAsset, Slug, WriteDisposition};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Write one JPEG to its final safe name through a unique sibling file.
pub fn write_atomic_replace(
    output: &Path,
    slug: &Slug,
    jpeg: &[u8],
) -> Result<SavedAsset, AssetWriteError> {
    let target = target_path(output, slug);
    let disposition = if target.exists() {
        WriteDisposition::Replaced
    } else {
        WriteDisposition::Created
    };
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = output.join(format!(
        ".{}.jpg.{}.{}.tmp",
        slug.as_str(),
        std::process::id(),
        sequence
    ));
    let result = (|| {
        let mut file = create_temporary(&temporary)?;
        file.write_all(jpeg)
            .map_err(|_| AssetWriteError::WriteFailed)?;
        file.sync_all().map_err(|_| AssetWriteError::WriteFailed)?;
        drop(file);
        std::fs::rename(&temporary, &target).map_err(|_| AssetWriteError::WriteFailed)?;
        File::open(output)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| AssetWriteError::WriteFailed)?;
        Ok(SavedAsset {
            path: target,
            disposition,
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

fn create_temporary(path: &Path) -> Result<File, AssetWriteError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| AssetWriteError::WriteFailed)
}

#[must_use]
fn target_path(output: &Path, slug: &Slug) -> PathBuf {
    output.join(format!("{}.jpg", slug.as_str()))
}

/// A local asset could not be durably replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AssetWriteError {
    #[error("the JPEG could not be written")]
    WriteFailed,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn target_path_cannot_escape_the_output_directory() {
        let output = Path::new("assets");
        let slug = Slug::parse("ritual-mask").expect("slug is valid");
        assert_eq!(target_path(output, &slug), output.join("ritual-mask.jpg"));
    }

    #[test]
    fn atomic_write_creates_then_replaces_the_same_target_without_sidecars() {
        let directory = tempdir().expect("temporary directory exists");
        let slug = Slug::parse("mask").expect("slug is valid");
        let created =
            write_atomic_replace(directory.path(), &slug, b"first").expect("first write succeeds");
        assert_eq!(created.disposition, WriteDisposition::Created);
        assert_eq!(
            std::fs::read(&created.path).expect("target reads"),
            b"first"
        );

        let replaced =
            write_atomic_replace(directory.path(), &slug, b"second").expect("replacement succeeds");
        assert_eq!(replaced.disposition, WriteDisposition::Replaced);
        assert_eq!(
            std::fs::read(&replaced.path).expect("target reads"),
            b"second"
        );
        let entries = std::fs::read_dir(directory.path())
            .expect("directory reads")
            .collect::<Result<Vec<_>, _>>()
            .expect("entries read");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_name(), "mask.jpg");
    }

    #[test]
    fn write_failure_removes_any_temporary_file() {
        let directory = tempdir().expect("temporary directory exists");
        let missing = directory.path().join("missing");
        let slug = Slug::parse("mask").expect("slug is valid");
        assert_eq!(
            write_atomic_replace(&missing, &slug, b"jpeg"),
            Err(AssetWriteError::WriteFailed)
        );
        assert!(!missing.exists());
    }
}
