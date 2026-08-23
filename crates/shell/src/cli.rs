//! The raw command line and its pure mapping to an engine-facing command.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// The raw command line, parsed by clap.
#[derive(Debug, Parser)]
#[command(
    name = "summoners",
    version,
    about = "Summoners match transcript tools"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: CliCommand,
}

/// The raw subcommands clap accepts.
#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Replay a transcript and confirm it reproduces itself exactly.
    Verify {
        /// Path to the NDJSON transcript to verify.
        expected: PathBuf,
    },
}

/// The strict, engine-facing command this process will run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Verify { path: PathBuf },
}

impl From<Cli> for Command {
    fn from(cli: Cli) -> Self {
        match cli.command {
            CliCommand::Verify { expected } => Self::Verify { path: expected },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_maps_its_path_argument() {
        let cli = Cli::parse_from(["summoners", "verify", "match.ndjson"]);

        assert_eq!(
            Command::from(cli),
            Command::Verify {
                path: PathBuf::from("match.ndjson"),
            }
        );
    }
}
