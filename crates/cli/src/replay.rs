//! Replaying a recorded transcript against the current engine.

use std::{
    error::Error,
    fmt,
    fs::File,
    io::{self, BufRead, BufReader, Seek},
    path::{Path, PathBuf},
};

use summoners_cards::{BuiltInError, built_in_catalog};
use summoners_match_log::{
    MatchCreatedV1, SetRequirementV1,
    replay::{ReplayError, verify_transcript},
};

/// The facts of a verified transcript that the success line names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationSummary {
    path: PathBuf,
    required_sets: Vec<SetRequirementV1>,
}

impl VerificationSummary {
    fn new(path: &Path, required_sets: Vec<SetRequirementV1>) -> Self {
        Self {
            path: path.to_path_buf(),
            required_sets,
        }
    }

    /// The one confirmation line printed when a transcript verifies.
    #[must_use]
    pub fn confirmation_line(&self) -> String {
        let mut revisions = Vec::new();
        for requirement in &self.required_sets {
            revisions.push(format!(
                "{} revision {}",
                requirement.set, requirement.revision
            ));
        }
        let details = if revisions.is_empty() {
            "no required sets".to_string()
        } else {
            revisions.join(", ")
        };
        format!(
            "OK {}: transcript verified ({details})",
            self.path.display()
        )
    }
}

/// Why a transcript could not be replayed.
#[derive(Debug)]
pub enum ReplayCliError {
    /// The transcript file could not be opened for reading.
    Open { path: PathBuf, source: io::Error },
    /// The embedded card catalog could not be loaded.
    Catalog(BuiltInError),
    /// The transcript did not verify against the current engine.
    Verify(ReplayError),
    /// A verified transcript could not be re-read to collect its set revisions.
    ///
    /// Unreachable for any transcript [`run_replay`] just verified: parsing
    /// already succeeded over the same bytes, so only filesystem I/O remains.
    Summarize(io::Error),
}

impl fmt::Display for ReplayCliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open { path, source } => {
                write!(
                    formatter,
                    "cannot open transcript {}: {source}",
                    path.display()
                )
            }
            Self::Catalog(source) => write!(formatter, "card catalog failed to load: {source}"),
            Self::Verify(source) => write!(formatter, "transcript is invalid: {source}"),
            Self::Summarize(source) => {
                write!(
                    formatter,
                    "cannot summarize the verified transcript: {source}"
                )
            }
        }
    }
}

impl Error for ReplayCliError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Open { source, .. } => Some(source),
            Self::Catalog(source) => Some(source),
            Self::Summarize(source) => Some(source),
            Self::Verify(source) => Some(source),
        }
    }
}

/// Replays one recorded transcript and reports its verified requirements.
///
/// # Errors
///
/// Returns [`ReplayCliError`] when the transcript cannot be opened, the card
/// catalog cannot be loaded, or the transcript does not verify.
pub fn run_replay(path: &Path) -> Result<VerificationSummary, ReplayCliError> {
    let mut file = File::open(path).map_err(|source| ReplayCliError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let catalog = built_in_catalog().map_err(ReplayCliError::Catalog)?;
    verify_transcript(BufReader::new(&file), catalog.library()).map_err(ReplayCliError::Verify)?;
    file.rewind().map_err(ReplayCliError::Summarize)?;
    let created = recorded_match_created(&file)?;
    Ok(VerificationSummary::new(path, created.required_sets))
}

fn recorded_match_created(file: &File) -> Result<MatchCreatedV1, ReplayCliError> {
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        let read = reader
            .read_line(&mut line)
            .map_err(ReplayCliError::Summarize)?;
        if read == 0 {
            return Err(ReplayCliError::Summarize(io::Error::other(
                "the verified transcript holds no match_created record",
            )));
        }
        if let Ok(created) = serde_json::from_str::<MatchCreatedV1>(&line) {
            return Ok(created);
        }
        line.clear();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::path::Path;

    use tempfile::TempDir;

    use super::{ReplayCliError, run_replay};
    use summoners_match_log::SetRequirementV1;

    fn summary_with(revisions: &[(&str, u32)]) -> super::VerificationSummary {
        let required_sets = revisions
            .iter()
            .map(|(set, revision)| SetRequirementV1 {
                set: (*set).to_string(),
                revision: *revision,
            })
            .collect();
        super::VerificationSummary::new(Path::new("goldens/deck.ndjson"), required_sets)
    }

    #[test]
    fn confirmation_line_names_path_and_one_revision() {
        let summary = summary_with(&[("foundations", 1)]);
        assert_eq!(
            summary.confirmation_line(),
            "OK goldens/deck.ndjson: transcript verified (foundations revision 1)"
        );
    }

    #[test]
    fn confirmation_line_lists_every_required_revision_in_order() {
        let summary = summary_with(&[("foundations", 2), ("expansion", 7)]);
        assert_eq!(
            summary.confirmation_line(),
            "OK goldens/deck.ndjson: transcript verified (foundations revision 2, expansion revision 7)"
        );
    }

    #[test]
    fn missing_transcript_is_an_open_error() {
        let directory = TempDir::new().expect("temp directory exists");
        let absent = directory.path().join("absent.ndjson");
        let error = run_replay(&absent).expect_err("an absent transcript cannot open");
        assert!(matches!(error, ReplayCliError::Open { .. }));
        assert!(
            error
                .to_string()
                .starts_with(&format!("cannot open transcript {}", absent.display())),
            "the open failure names the path: {error}"
        );
    }

    #[test]
    fn corrupt_transcript_is_a_verify_error() {
        let directory = TempDir::new().expect("temp directory exists");
        let broken = directory.path().join("broken.ndjson");
        std::fs::write(&broken, "{\"sequence\":0,\"record\":\n").expect("fixture writes");
        let error = run_replay(&broken).expect_err("truncated records cannot verify");
        assert!(matches!(error, ReplayCliError::Verify(_)));
        assert!(
            error.to_string().starts_with("transcript is invalid: "),
            "unexpected message: {error}"
        );
    }
}
