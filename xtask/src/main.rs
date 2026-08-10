//! Repository automation.
//!
//! See <https://github.com/matklad/cargo-xtask/>. This binary holds the
//! development commands that plain `cargo` cannot express. The `lint` command
//! runs every quality check in order and manages the Git pre-commit hook.
#![deny(clippy::unwrap_used, clippy::expect_used)]

mod lint;

use clap::{Parser, Subcommand};
use color_eyre::eyre::Result;

#[derive(Parser)]
#[command(name = "xtask", about = "Summoners development tasks")]
struct App {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run every quality check: fmt, check, clippy, test, file length, and the
    /// `#[allow(clippy::too_many_arguments)]` ban
    Lint(lint::LintArgs),
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let app = App::parse();

    match app.command {
        Commands::Lint(args) => lint::run(&args),
    }
}
