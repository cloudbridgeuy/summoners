//! Why one shell command could not complete.

use std::{error::Error, fmt, io, path::PathBuf};

use summoners_cards::BuiltInError;
use summoners_match_log::RecordingError;
use summoners_match_log::compare::{TranscriptComparisonError, TranscriptDifference};
use summoners_match_log::replay::ReplayError;

use crate::report;

/// Every failure surface a shell command can report.
///
/// Each variant that acts on caller input names the command that was
/// running and the path it was acting on, so a message never loses that
/// context on its way to stderr.
#[derive(Debug)]
pub enum ShellError {
    /// The command line itself could not be understood.
    Usage(String),
    /// A transcript failed to parse or replay to an identical result.
    Transcript {
        command: &'static str,
        path: PathBuf,
        source: Box<ReplayError>,
    },
    /// A file could not be opened, read, or written.
    Io {
        command: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    /// The embedded card catalog could not be loaded.
    Catalog(BuiltInError),
    /// A durable recording could not reach its checkpoint.
    Recording {
        command: &'static str,
        path: PathBuf,
        source: RecordingError,
    },
    /// An interactive game could not run to a terminal state.
    Game {
        command: &'static str,
        path: PathBuf,
        reason: GameFailure,
    },
    /// Two already-parsed transcripts could not be compared.
    Comparison {
        command: &'static str,
        path: PathBuf,
        source: TranscriptComparisonError,
    },
    /// A replay reproduced a complete transcript, but it is not
    /// semantically equal to the transcript it replayed.
    Difference {
        command: &'static str,
        expected_path: PathBuf,
        observed_path: PathBuf,
        difference: Box<TranscriptDifference>,
    },
}

/// Why an interactive game could not continue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameFailure {
    /// The transcript's recorded actions ran out before the game reached a
    /// terminal state.
    ActionsExhausted { step: u64 },
    /// The operator ended the session before the game reached a terminal
    /// state.
    OperatorQuit,
    /// The game entered a broken state: a rule demanded a fact no entity
    /// printed.
    Broken,
}

impl fmt::Display for GameFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ActionsExhausted { step } => write!(
                formatter,
                "recorded actions ran out while the game was still playing, at step {step}"
            ),
            Self::OperatorQuit => formatter.write_str("the operator quit"),
            Self::Broken => formatter.write_str("the game entered a broken state"),
        }
    }
}

impl fmt::Display for ShellError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => formatter.write_str(message.trim_end()),
            Self::Transcript {
                command,
                path,
                source,
            } => write!(formatter, "{command}: {}: {source}", path.display()),
            Self::Io {
                command,
                path,
                source,
            } => write!(formatter, "{command}: {}: {source}", path.display()),
            Self::Catalog(source) => write!(formatter, "catalog: {source}"),
            Self::Recording {
                command,
                path,
                source,
            } => write!(formatter, "{command}: {}: {source}", path.display()),
            Self::Game {
                command,
                path,
                reason,
            } => write!(formatter, "{command}: {}: {reason}", path.display()),
            Self::Comparison {
                command,
                path,
                source,
            } => write!(formatter, "{command}: {}: {source}", path.display()),
            Self::Difference {
                command,
                expected_path,
                observed_path,
                difference,
            } => formatter.write_str(&report::replay_difference(
                command,
                expected_path,
                observed_path,
                difference,
            )),
        }
    }
}

impl Error for ShellError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Usage(_) | Self::Game { .. } | Self::Difference { .. } => None,
            Self::Transcript { source, .. } => Some(source.as_ref()),
            Self::Io { source, .. } => Some(source),
            Self::Catalog(source) => Some(source),
            Self::Recording { source, .. } => Some(source),
            Self::Comparison { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::{fs, path::PathBuf};

    use summoners_cards::LibraryError;
    use summoners_match_log::compare::{
        ComparisonOptions, TranscriptComparison, compare_transcripts,
    };
    use summoners_match_log::replay::{
        ReplayDivergence, ReplayDivergenceKind, ReplayLocation, ReplayPhase,
    };

    use super::*;

    fn golden(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../match-log/tests/goldens")
            .join(name)
    }

    fn sample_difference() -> TranscriptDifference {
        let expected =
            fs::read_to_string(golden("terminal_empty_deck.ndjson")).expect("golden reads");
        let actual = fs::read_to_string(golden("resignation.ndjson")).expect("golden reads");

        match compare_transcripts(
            expected.as_bytes(),
            actual.as_bytes(),
            ComparisonOptions::default(),
        )
        .expect("two distinct goldens compare without error")
        {
            TranscriptComparison::Different(difference) => difference,
            TranscriptComparison::Equal => panic!("two distinct goldens must not compare equal"),
        }
    }

    fn sample_comparison_error() -> TranscriptComparisonError {
        match compare_transcripts(
            &b"not a transcript"[..],
            &b"{}"[..],
            ComparisonOptions::default(),
        ) {
            Err(error) => error,
            Ok(_) => panic!("garbage input must fail to parse"),
        }
    }

    fn sample_transcript_error() -> ReplayError {
        ReplayError::Divergence(Box::new(ReplayDivergence {
            location: ReplayLocation {
                phase: ReplayPhase::Completion,
                step: None,
                event_index: None,
                path: "match_completed.winner",
            },
            kind: ReplayDivergenceKind::CardSetIdentity,
        }))
    }

    #[test]
    fn transcript_display_names_the_command_and_the_path() {
        let error = ShellError::Transcript {
            command: "verify",
            path: PathBuf::from("match.ndjson"),
            source: Box::new(sample_transcript_error()),
        };

        assert_eq!(
            error.to_string(),
            format!("verify: match.ndjson: {}", sample_transcript_error())
        );
    }

    #[test]
    fn io_display_names_the_command_and_the_path() {
        let error = ShellError::Io {
            command: "verify",
            path: PathBuf::from("missing.ndjson"),
            source: io::Error::new(io::ErrorKind::NotFound, "no such file or directory"),
        };

        assert!(error.to_string().starts_with("verify: missing.ndjson: "));
    }

    #[test]
    fn catalog_display_reports_the_built_in_failure() {
        let error =
            ShellError::Catalog(BuiltInError::Library(LibraryError::DuplicateQualifiedKey {
                key: "foundations/summoner".to_string(),
                first_set: "foundations".to_string(),
                duplicate_set: "foundations".to_string(),
            }));

        assert!(error.to_string().starts_with("catalog: "));
    }

    #[test]
    fn recording_display_names_the_command_and_the_path() {
        let error = ShellError::Recording {
            command: "replay",
            path: PathBuf::from("out.ndjson"),
            source: RecordingError::Write(io::Error::other("disk full")),
        };

        assert!(error.to_string().starts_with("replay: out.ndjson: "));
    }

    #[test]
    fn game_display_names_the_command_the_path_and_actions_exhausted() {
        let error = ShellError::Game {
            command: "play",
            path: PathBuf::from("session.ndjson"),
            reason: GameFailure::ActionsExhausted { step: 12 },
        };

        assert_eq!(
            error.to_string(),
            "play: session.ndjson: recorded actions ran out while the game was still playing, at step 12"
        );
    }

    #[test]
    fn game_display_names_the_command_the_path_and_operator_quit() {
        let error = ShellError::Game {
            command: "play",
            path: PathBuf::from("session.ndjson"),
            reason: GameFailure::OperatorQuit,
        };

        assert_eq!(error.to_string(), "play: session.ndjson: the operator quit");
    }

    #[test]
    fn game_display_names_the_command_the_path_and_broken() {
        let error = ShellError::Game {
            command: "replay",
            path: PathBuf::from("out.ndjson.partial"),
            reason: GameFailure::Broken,
        };

        assert_eq!(
            error.to_string(),
            "replay: out.ndjson.partial: the game entered a broken state"
        );
    }

    #[test]
    fn usage_display_is_the_bare_message() {
        let error = ShellError::Usage("unrecognized subcommand".to_string());

        assert_eq!(error.to_string(), "unrecognized subcommand");
    }

    #[test]
    fn comparison_display_names_the_command_and_the_path() {
        let error = ShellError::Comparison {
            command: "replay",
            path: PathBuf::from("out.ndjson"),
            source: sample_comparison_error(),
        };

        assert!(error.to_string().starts_with("replay: out.ndjson: "));
    }

    #[test]
    fn difference_display_matches_the_pure_report_formatting() {
        let difference = sample_difference();
        let error = ShellError::Difference {
            command: "replay",
            expected_path: PathBuf::from("expected.ndjson"),
            observed_path: PathBuf::from("observed.ndjson"),
            difference: Box::new(difference.clone()),
        };

        assert_eq!(
            error.to_string(),
            report::replay_difference(
                "replay",
                &PathBuf::from("expected.ndjson"),
                &PathBuf::from("observed.ndjson"),
                &difference,
            )
        );
    }
}
