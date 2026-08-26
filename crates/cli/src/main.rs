//! Command-line entry point for playing Summoners matches.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use clap::Parser;
use std::process::ExitCode;
use summoners_cli::app::{App, Command};

fn main() -> ExitCode {
    if let Err(error) = color_eyre::install() {
        eprintln!("Cannot install error reporting: {error}");
        return ExitCode::FAILURE;
    }
    let app = App::parse();

    let result = match app.command {
        Command::Serve(_) => summoners_cli::run_serve(),
        Command::Play(args) => summoners_cli::run_play(&args),
        Command::Replay(args) => summoners_cli::run_replay(&args),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
