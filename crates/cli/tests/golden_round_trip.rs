#![allow(clippy::expect_used)]

use std::{error::Error, ffi::OsStr, fmt, fs, path::PathBuf};

use summoners_cli::replay::{ReplayCliError, run_replay};

fn checked_in_corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/goldens")
}

#[derive(Debug)]
enum CorpusDiscoveryError {
    ReadDirectory {
        directory: PathBuf,
        source: std::io::Error,
    },
    Empty {
        directory: PathBuf,
    },
}

impl fmt::Display for CorpusDiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadDirectory { directory, source } => write!(
                formatter,
                "{}: goldens read failed: {source}",
                directory.display()
            ),
            Self::Empty { directory } => {
                write!(formatter, "{}: golden corpus is empty", directory.display())
            }
        }
    }
}

impl Error for CorpusDiscoveryError {}

fn discover_golden_transcripts(
    directory: &std::path::Path,
) -> Result<Vec<PathBuf>, CorpusDiscoveryError> {
    let entries =
        fs::read_dir(directory).map_err(|source| CorpusDiscoveryError::ReadDirectory {
            directory: directory.to_path_buf(),
            source,
        })?;
    let mut fixtures = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| CorpusDiscoveryError::ReadDirectory {
            directory: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension() == Some(OsStr::new("ndjson")) {
            fixtures.push(path);
        }
    }
    fixtures.sort();
    if fixtures.is_empty() {
        return Err(CorpusDiscoveryError::Empty {
            directory: directory.to_path_buf(),
        });
    }
    Ok(fixtures)
}

#[test]
fn the_checked_in_goldens_discover_in_sorted_order() {
    let discovered =
        discover_golden_transcripts(&checked_in_corpus()).expect("the corpus is readable");
    assert!(
        discovered.len() >= 2,
        "at least two goldens must be checked in"
    );
    eprintln!("discovered {} goldens", discovered.len());
}

#[test]
fn every_checked_in_golden_replays_through_the_cli_runner() {
    let directory = checked_in_corpus();
    let discovered = discover_golden_transcripts(&directory).expect("the corpus is readable");

    for fixture in &discovered {
        let summary = match run_replay(fixture) {
            Ok(summary) => summary,
            Err(error) => panic!(
                "{} must verify through the crate's runner: {error}",
                fixture.display()
            ),
        };
        let confirmation = summary.confirmation_line();
        assert!(
            confirmation.starts_with("OK "),
            "the confirmation line must read as success: {confirmation}"
        );
        eprintln!("{confirmation}");
    }
}

#[test]
fn a_mutated_event_line_fails_verification_naming_the_diverging_step() {
    const ORIGINAL: &str = include_str!("goldens/four_card_deck_loss.ndjson");
    const EXPECTED_LINE: &str = r#"{"sequence":5,"record":"event","step":2,"index":0,"event":{"kind":"priority_passed","player":"two"}}"#;
    const REPLACEMENT_LINE: &str = r#"{"sequence":5,"record":"event","step":2,"index":0,"event":{"kind":"priority_passed","player":"one"}}"#;
    let changed = ORIGINAL.replacen(EXPECTED_LINE, REPLACEMENT_LINE, 1);
    assert_ne!(changed, ORIGINAL, "one event line must change");

    let directory = tempfile::tempdir().expect("a temporary directory exists");
    let mutated = directory.path().join("mutated.ndjson");
    fs::write(&mutated, changed).expect("the mutated copy is written");

    let error = run_replay(&mutated).expect_err("a corrupted event cannot verify");

    assert!(matches!(error, ReplayCliError::Verify(_)));
    assert!(
        error.to_string().contains("step 2, event 0"),
        "the failure must name the diverging step: {error}"
    );
}
