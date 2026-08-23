#![allow(clippy::expect_used)]

use std::{
    error::Error,
    ffi::OsStr,
    fmt, fs,
    io::BufReader,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use summoners_cards::{BuiltInError, built_in_catalog};
use summoners_match_log::replay::{ReplayDivergenceKind, ReplayError, verify_transcript};

#[derive(Debug)]
enum GoldenCorpusError {
    Catalog(BuiltInError),
    ReadDirectory {
        directory: PathBuf,
        source: std::io::Error,
    },
    Empty {
        directory: PathBuf,
    },
    ReadFixture {
        fixture: PathBuf,
        source: std::io::Error,
    },
    Replay {
        fixture: PathBuf,
        source: ReplayError,
    },
}

impl fmt::Display for GoldenCorpusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Catalog(source) => source.fmt(formatter),
            Self::ReadDirectory { directory, source } => {
                write!(
                    formatter,
                    "{}: corpus read failed: {source}",
                    directory.display()
                )
            }
            Self::Empty { directory } => {
                write!(formatter, "{}: golden corpus is empty", directory.display())
            }
            Self::ReadFixture { fixture, source } => {
                write!(
                    formatter,
                    "{}: fixture read failed: {source}",
                    fixture.display()
                )
            }
            Self::Replay { fixture, source } => {
                let label = fixture
                    .file_name()
                    .unwrap_or_else(|| fixture.as_os_str())
                    .to_string_lossy();
                write!(formatter, "{label}: {source}")
            }
        }
    }
}

impl Error for GoldenCorpusError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Catalog(source) => Some(source),
            Self::ReadDirectory { source, .. } | Self::ReadFixture { source, .. } => Some(source),
            Self::Replay { source, .. } => Some(source),
            Self::Empty { .. } => None,
        }
    }
}

fn discover_golden_matches(directory: &Path) -> Result<Vec<PathBuf>, GoldenCorpusError> {
    let entries = fs::read_dir(directory).map_err(|source| GoldenCorpusError::ReadDirectory {
        directory: directory.to_path_buf(),
        source,
    })?;
    let mut fixtures = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| GoldenCorpusError::ReadDirectory {
            directory: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension() == Some(OsStr::new("ndjson")) {
            fixtures.push(path);
        }
    }
    fixtures.sort();
    Ok(fixtures)
}

fn verify_golden_corpus(directory: &Path) -> Result<Vec<PathBuf>, GoldenCorpusError> {
    let catalog = built_in_catalog().map_err(GoldenCorpusError::Catalog)?;
    let fixtures = discover_golden_matches(directory)?;
    if fixtures.is_empty() {
        return Err(GoldenCorpusError::Empty {
            directory: directory.to_path_buf(),
        });
    }

    for fixture in &fixtures {
        let file = fs::File::open(fixture).map_err(|source| GoldenCorpusError::ReadFixture {
            fixture: fixture.clone(),
            source,
        })?;
        verify_transcript(BufReader::new(file), catalog.library()).map_err(|source| {
            GoldenCorpusError::Replay {
                fixture: fixture.clone(),
                source,
            }
        })?;
    }

    Ok(fixtures)
}

fn checked_in_corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/goldens")
}

struct TemporaryCorpus {
    path: PathBuf,
}

impl TemporaryCorpus {
    fn new() -> Self {
        static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir();
        for attempt in 0..100 {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!(
                "summoners-match-log-goldens-{}-{sequence}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self { path },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("temporary corpus creation failed: {error}"),
            }
        }
        panic!("temporary corpus path allocation failed");
    }

    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) {
        fs::write(self.path.join(name), bytes).expect("the temporary fixture is written");
    }
}

impl Drop for TemporaryCorpus {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn discovery_includes_ndjson_files_and_ignores_other_files() {
    let corpus = TemporaryCorpus::new();
    corpus.write("second.ndjson", b"");
    corpus.write("notes.txt", b"not a match");
    corpus.write("first.ndjson", b"");

    let fixtures = discover_golden_matches(&corpus.path).expect("the corpus is readable");
    let names: Vec<_> = fixtures
        .iter()
        .map(|path| path.file_name().and_then(OsStr::to_str))
        .collect();

    assert_eq!(names, [Some("first.ndjson"), Some("second.ndjson")]);
}

#[test]
fn an_empty_corpus_fails() {
    let corpus = TemporaryCorpus::new();

    let error = verify_golden_corpus(&corpus.path).expect_err("one golden match is required");

    assert!(matches!(error, GoldenCorpusError::Empty { .. }));
}

#[test]
fn every_checked_in_golden_match_replays() {
    let directory = checked_in_corpus();
    let discovered = discover_golden_matches(&directory).expect("the corpus is readable");

    let verified = verify_golden_corpus(&directory).expect("every golden match must replay");

    assert_eq!(verified, discovered);
    for fixture in verified {
        eprintln!("replayed {}", fixture.display());
    }
}

#[test]
fn changed_line_names_the_fixture_before_the_typed_difference() {
    let original = include_str!("goldens/terminal_empty_deck.ndjson");
    let expected = r#"{"sequence":7,"record":"event","step":3,"index":0,"event":{"kind":"priority_passed","player":"two"}}"#;
    let replacement = r#"{"sequence":7,"record":"event","step":3,"index":0,"event":{"kind":"priority_passed","player":"one"}}"#;
    let changed = original.replacen(expected, replacement, 1);
    assert_ne!(changed, original, "one event line must change");
    let corpus = TemporaryCorpus::new();
    corpus.write("changed-event.ndjson", changed);

    let error = verify_golden_corpus(&corpus.path).expect_err("the changed event must differ");

    let GoldenCorpusError::Replay { fixture, source } = &error else {
        panic!("expected a typed replay error, found {error}");
    };
    assert_eq!(
        fixture.file_name().and_then(OsStr::to_str),
        Some("changed-event.ndjson")
    );
    let ReplayError::Divergence(divergence) = source else {
        panic!("expected a replay divergence, found {source}");
    };
    assert_eq!(divergence.location.step, Some(3));
    assert_eq!(divergence.location.event_index, Some(0));
    assert!(matches!(
        divergence.kind,
        ReplayDivergenceKind::Event { .. }
    ));
    assert!(
        error
            .to_string()
            .starts_with("changed-event.ndjson: step 3, event 0, path events.event:")
    );
}
