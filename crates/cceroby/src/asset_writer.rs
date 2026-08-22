//! Same-directory durable temporary writes and atomic JPEG replacement.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tempfile::Builder;
use thiserror::Error;

use crate::download::{SavedAsset, Slug, WriteDisposition};

static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Write one JPEG to its final safe name through a unique sibling file.
pub fn write_atomic_replace(
    output: &Path,
    slug: &Slug,
    jpeg: &[u8],
) -> Result<SavedAsset, AssetWriteError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|_| AssetWriteError::WriteFailed)?;
    let target = target_path(output, slug);
    let disposition = if target.exists() {
        WriteDisposition::Replaced
    } else {
        WriteDisposition::Created
    };
    let prefix = format!(".{}.jpg.", slug.as_str());
    let mut temporary = Builder::new()
        .prefix(&prefix)
        .suffix(".tmp")
        .tempfile_in(output)
        .map_err(|_| AssetWriteError::WriteFailed)?;
    temporary
        .write_all(jpeg)
        .map_err(|_| AssetWriteError::WriteFailed)?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| AssetWriteError::WriteFailed)?;
    let persisted = temporary
        .persist(&target)
        .map_err(|_| AssetWriteError::WriteFailed)?;
    drop(persisted);
    sync_directory(output)?;
    Ok(SavedAsset {
        path: target,
        disposition,
    })
}

#[cfg(unix)]
fn sync_directory(output: &Path) -> Result<(), AssetWriteError> {
    File::open(output)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| AssetWriteError::WriteFailed)
}

#[cfg(not(unix))]
fn sync_directory(_output: &Path) -> Result<(), AssetWriteError> {
    Ok(())
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

    use std::sync::{Arc, Barrier};

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

    #[test]
    fn concurrent_same_slug_writes_report_one_create_and_one_replace() {
        let directory = tempdir().expect("temporary directory exists");
        let start = Arc::new(Barrier::new(3));
        let handles = [b'a', b'b'].map(|byte| {
            let output = directory.path().to_owned();
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                let slug = Slug::parse("mask").expect("slug is valid");
                let payload = vec![byte; 4 * 1024 * 1024];
                start.wait();
                write_atomic_replace(&output, &slug, &payload).expect("write succeeds")
            })
        });
        start.wait();
        let saved = handles.map(|handle| handle.join().expect("writer completes"));
        assert_eq!(
            saved
                .iter()
                .filter(|asset| asset.disposition == WriteDisposition::Created)
                .count(),
            1
        );
        assert_eq!(
            saved
                .iter()
                .filter(|asset| asset.disposition == WriteDisposition::Replaced)
                .count(),
            1
        );
        let target = std::fs::read(directory.path().join("mask.jpg")).expect("target reads");
        assert!(target.iter().all(|byte| *byte == b'a') || target.iter().all(|byte| *byte == b'b'));
        assert_eq!(
            std::fs::read_dir(directory.path())
                .expect("directory reads")
                .count(),
            1
        );
    }
}
