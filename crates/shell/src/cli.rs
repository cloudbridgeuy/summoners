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
    /// Replay a transcript's recorded actions into a fresh recording, then
    /// compare the observed result against it.
    Replay {
        /// Path to the NDJSON transcript to replay.
        #[arg(long = "from")]
        from: PathBuf,
        /// Path to write the observed transcript to.
        #[arg(long = "output")]
        output: PathBuf,
        /// Overwrite an existing output path.
        #[arg(long)]
        force: bool,
    },
}

/// The strict, engine-facing command this process will run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Verify {
        path: PathBuf,
    },
    Replay {
        from: PathBuf,
        output: PathBuf,
        force: bool,
    },
}

impl From<Cli> for Command {
    fn from(cli: Cli) -> Self {
        match cli.command {
            CliCommand::Verify { expected } => Self::Verify { path: expected },
            CliCommand::Replay {
                from,
                output,
                force,
            } => Self::Replay {
                from,
                output,
                force,
            },
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

    #[test]
    fn replay_maps_its_arguments() {
        let cli = Cli::parse_from([
            "summoners",
            "replay",
            "--from",
            "expected.ndjson",
            "--output",
            "observed.ndjson",
            "--force",
        ]);

        assert_eq!(
            Command::from(cli),
            Command::Replay {
                from: PathBuf::from("expected.ndjson"),
                output: PathBuf::from("observed.ndjson"),
                force: true,
            }
        );
    }

    #[test]
    fn replay_defaults_force_to_false() {
        let cli = Cli::parse_from([
            "summoners",
            "replay",
            "--from",
            "expected.ndjson",
            "--output",
            "observed.ndjson",
        ]);

        assert_eq!(
            Command::from(cli),
            Command::Replay {
                from: PathBuf::from("expected.ndjson"),
                output: PathBuf::from("observed.ndjson"),
                force: false,
            }
        );
    }
}
