use crate::client::PlayError;
use crate::replay::ReplayCliError;
use crate::server::ServeError;

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(transparent)]
    Play(#[from] PlayError),
    #[error(transparent)]
    Replay(#[from] ReplayCliError),
    #[error(transparent)]
    Serve(#[from] ServeError),
}
