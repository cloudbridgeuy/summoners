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
            crate::terminal::path_text(&self.path)
        )
    }
}

#[derive(Debug)]
pub enum ReplayCliError {
    Open { path: PathBuf, source: io::Error },
    Catalog(BuiltInError),
    Verify(ReplayError),
    Summarize(io::Error),
}

impl fmt::Display for ReplayCliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open { path, source } => {
                write!(
                    formatter,
                    "cannot open transcript {}: {source}",
                    crate::terminal::path_text(path)
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
    fn confirmation_line_escapes_control_characters_in_the_path() {
        let name = "bad\n\r\t\u{1b}\u{0007}\u{007f}name";
        let summary = super::VerificationSummary::new(Path::new(name), vec![]);
        let line = summary.confirmation_line();
        assert_escaped(&line);
    }

    #[test]
    fn open_error_escapes_control_characters_in_the_path() {
        let directory = TempDir::new().expect("temp directory exists");
        let name = "bad\n\r\t\u{1b}\u{0007}\u{007f}name";
        let absent = directory.path().join(name);
        let error = run_replay(&absent).expect_err("an absent transcript cannot open");
        assert!(matches!(error, ReplayCliError::Open { .. }));
        assert_escaped(&error.to_string());
    }

    fn assert_escaped(text: &str) {
        for form in ["\\n", "\\r", "\\t", "\\x1b", "\\u{0007}", "\\u{007f}"] {
            assert!(text.contains(form), "missing {form:?} in {text:?}");
        }
        assert!(
            !text.chars().any(char::is_control),
            "raw control character in {text:?}"
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
