use crate::replay::ReplayCliError;
use crate::server::ServeError;

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("`{command}` is not part of this build yet.")]
    NotYetAvailable { command: &'static str },
    #[error(transparent)]
    Replay(#[from] ReplayCliError),
    #[error(transparent)]
    Serve(#[from] ServeError),
}
