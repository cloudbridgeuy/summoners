//! Pure mapping from a shell error to a process exit code.

use crate::error::ShellError;

/// The process exit code for one shell error.
///
/// The contract is stable across every subcommand this shell will ever
/// gain: 2 is a command-line argument problem, 3 is an invalid or diverging
/// transcript, 4 is a file, catalog, or recording failure, and 5 is a
/// broken or incomplete game.
#[must_use]
pub fn exit_code(error: &ShellError) -> u8 {
    match error {
        ShellError::Usage(_) => 2,
        ShellError::Transcript { .. } => 3,
        ShellError::Io { .. } | ShellError::Catalog(_) | ShellError::Recording { .. } => 4,
        ShellError::Game { .. } => 5,
    }
}

#[cfg(test)]
mod tests {
    use std::{io, path::PathBuf};

    use summoners_cards::{BuiltInError, LibraryError};
    use summoners_match_log::RecordingError;
    use summoners_match_log::replay::{
        ReplayDivergence, ReplayDivergenceKind, ReplayError, ReplayLocation, ReplayPhase,
    };

    use crate::error::GameFailure;

    use super::*;

    #[test]
    fn usage_exits_two() {
        assert_eq!(
            exit_code(&ShellError::Usage("bad arguments".to_string())),
            2
        );
    }

    #[test]
    fn transcript_exits_three() {
        let error = ShellError::Transcript {
            command: "verify",
            path: PathBuf::from("match.ndjson"),
            source: Box::new(ReplayError::Divergence(Box::new(ReplayDivergence {
                location: ReplayLocation {
                    phase: ReplayPhase::Completion,
                    step: None,
                    event_index: None,
                    path: "match_completed.winner",
                },
                kind: ReplayDivergenceKind::CardSetIdentity,
            }))),
        };

        assert_eq!(exit_code(&error), 3);
    }

    #[test]
    fn io_catalog_and_recording_exit_four() {
        let io_error = ShellError::Io {
            command: "verify",
            path: PathBuf::from("missing.ndjson"),
            source: io::Error::new(io::ErrorKind::NotFound, "no such file"),
        };
        let catalog_error =
            ShellError::Catalog(BuiltInError::Library(LibraryError::DuplicateQualifiedKey {
                key: "foundations/summoner".to_string(),
                first_set: "foundations".to_string(),
                duplicate_set: "foundations".to_string(),
            }));
        let recording_error = ShellError::Recording {
            command: "replay",
            path: PathBuf::from("out.ndjson"),
            source: RecordingError::Write(io::Error::other("disk full")),
        };

        assert_eq!(exit_code(&io_error), 4);
        assert_eq!(exit_code(&catalog_error), 4);
        assert_eq!(exit_code(&recording_error), 4);
    }

    #[test]
    fn game_exits_five() {
        let error = ShellError::Game {
            command: "play",
            path: PathBuf::from("session.ndjson"),
            reason: GameFailure::OperatorQuit,
        };

        assert_eq!(exit_code(&error), 5);
    }
}
