#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod app;
pub mod error;
pub mod replay;
pub mod server;
pub mod setup;

use error::CliError;

use crate::app::{PlayArgs, ReplayArgs, ServeArgs};

pub fn run_serve(args: &ServeArgs) -> Result<(), CliError> {
    server::serve(args).map_err(CliError::Serve)
}

pub fn run_play(_args: &PlayArgs) -> Result<(), CliError> {
    Err(CliError::NotYetAvailable { command: "play" })
}

pub fn run_replay(args: &ReplayArgs) -> Result<(), CliError> {
    let summary = replay::run_replay(&args.transcript)?;
    println!("{}", summary.confirmation_line());
    Ok(())
}
