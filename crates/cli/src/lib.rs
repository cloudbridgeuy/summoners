#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod app;
pub mod client;
pub mod error;
pub mod protocol;
pub mod replay;
pub mod server;
pub mod setup;

use error::CliError;

use crate::app::{PlayArgs, ReplayArgs, ServeArgs};

pub fn run_serve(args: &ServeArgs) -> Result<(), CliError> {
    server::serve(args).map_err(CliError::Serve)
}

pub fn run_play(args: &PlayArgs) -> Result<(), CliError> {
    client::play(args).map_err(CliError::Play)
}

pub fn run_replay(args: &ReplayArgs) -> Result<(), CliError> {
    let summary = replay::run_replay(&args.transcript)?;
    println!("{}", summary.confirmation_line());
    Ok(())
}
