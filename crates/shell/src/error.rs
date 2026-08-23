//! Why one shell command could not complete.

use std::{error::Error, fmt, io, path::PathBuf};

use summoners_cards::BuiltInError;
use summoners_match_log::RecordingError;
use summoners_match_log::replay::ReplayError;

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
}

impl fmt::Display for GameFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ActionsExhausted { step } => write!(
                formatter,
                "recorded actions ran out while the game was still playing, at step {step}"
            ),
            Self::OperatorQuit => formatter.write_str("the operator quit"),
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
        }
    }
}

impl Error for ShellError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Usage(_) | Self::Game { .. } => None,
            Self::Transcript { source, .. } => Some(source.as_ref()),
            Self::Io { source, .. } => Some(source),
            Self::Catalog(source) => Some(source),
            Self::Recording { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::PathBuf;

    use summoners_cards::LibraryError;
    use summoners_match_log::replay::{
        ReplayDivergence, ReplayDivergenceKind, ReplayLocation, ReplayPhase,
    };

    use super::*;

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
    fn usage_display_is_the_bare_message() {
        let error = ShellError::Usage("unrecognized subcommand".to_string());

        assert_eq!(error.to_string(), "unrecognized subcommand");
    }
}
