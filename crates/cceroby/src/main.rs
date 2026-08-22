//! Command-line entry point for local museum image search.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use cceroby::app::{App, Command};
use clap::Parser;
use color_eyre::eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let app = App::parse();

    match app.command {
        Command::Search(args) => cceroby::search::run(args.try_into()?).await,
    }
}
