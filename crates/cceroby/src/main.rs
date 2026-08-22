//! Command-line entry point for local museum image search.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use cceroby::app::{App, Command};
use clap::Parser;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    if let Err(error) = color_eyre::install() {
        eprintln!("Cannot install error reporting: {error}");
        return ExitCode::FAILURE;
    }
    let app = App::parse();

    let result = match app.command {
        Command::Search(args) => {
            let seed = match args.try_into() {
                Ok(seed) => seed,
                Err(error) => {
                    eprintln!("{error}");
                    return ExitCode::FAILURE;
                }
            };
            cceroby::search::run(seed).await
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:?}");
            ExitCode::FAILURE
        }
    }
}
