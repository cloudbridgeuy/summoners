//! Errors the command line reports to its user.

use crate::replay::ReplayCliError;

/// One failure a subcommand runner can report.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// The invoked subcommand has no behavior behind it.
    #[error("`{command}` is not part of this build yet.")]
    NotYetAvailable { command: &'static str },
    /// The replay runner could not verify the transcript.
    #[error(transparent)]
    Replay(#[from] ReplayCliError),
}
