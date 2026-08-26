//! Command-line runners for playing Summoners matches.
//!
//! Parsing lives in [`app`]; reported failures live in [`error`]. Each runner
//! below is the single place one subcommand's behavior belongs.
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod app;
pub mod error;

use error::CliError;

use crate::app::{PlayArgs, ReplayArgs};

/// Hosts a match for two players and reports the address to join.
///
/// # Errors
///
/// Returns [`CliError`] while the host behavior is not part of this build.
pub fn run_serve() -> Result<(), CliError> {
    Err(CliError::NotYetAvailable { command: "serve" })
}

/// Joins a hosted match as one player.
///
/// # Errors
///
/// Returns [`CliError`] while the client behavior is not part of this build.
pub fn run_play(_args: &PlayArgs) -> Result<(), CliError> {
    Err(CliError::NotYetAvailable { command: "play" })
}

/// Replays a recorded match transcript.
///
/// # Errors
///
/// Returns [`CliError`] while replay behavior is not part of this build.
pub fn run_replay(_args: &ReplayArgs) -> Result<(), CliError> {
    Err(CliError::NotYetAvailable { command: "replay" })
}
