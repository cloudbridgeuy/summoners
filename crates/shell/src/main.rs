//! The `summoners` command-line shell binary.
//!
//! Parses the command line, loads the embedded card catalog once,
//! dispatches to a command, and renders whatever the library returns to
//! stdout or stderr. Every decision here is a call into a pure function
//! from `summoners_shell`; this file only owns file, process, and stream
//! effects.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;

use clap::Parser;
use summoners_cards::{CardLibrary, built_in_catalog};
use summoners_shell::cli::{Cli, Command};
use summoners_shell::error::ShellError;
use summoners_shell::exit::exit_code;
use summoners_shell::{report, verify};

fn main() -> ExitCode {
    let command = Command::from(Cli::parse());

    let library = match built_in_catalog() {
        Ok(catalog) => catalog,
        Err(error) => return report_failure(&ShellError::Catalog(error)),
    };

    match run(command, library.library()) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(error) => report_failure(&error),
    }
}

fn run(command: Command, library: &CardLibrary) -> Result<String, ShellError> {
    match command {
        Command::Verify { path } => {
            let summary = verify::run_verify(&path, library)?;
            Ok(report::verify_success(&path, &summary))
        }
    }
}

fn report_failure(error: &ShellError) -> ExitCode {
    eprintln!("{error}");
    ExitCode::from(exit_code(error))
}
