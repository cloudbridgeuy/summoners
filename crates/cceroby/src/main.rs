//! Command-line entry point for local museum image search.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use cceroby::app::{App, CacheAction, Command};
use cceroby::cache::{Cache, CacheLayerStats};
use clap::Parser;
use std::process::ExitCode;
use std::time::SystemTime;

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
        Command::Cache(args) => match run_cache(args.action) {
            Ok(()) => Ok(()),
            Err(_) => {
                eprintln!("Cannot clear the cache safely.");
                return ExitCode::FAILURE;
            }
        },
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:?}");
            ExitCode::FAILURE
        }
    }
}

fn run_cache(action: CacheAction) -> std::io::Result<()> {
    let cache = Cache::from_user_cache_dir();
    match action {
        CacheAction::Info => print_cache_info(&cache.stats(SystemTime::now())),
        CacheAction::Clear => {
            let stats = cache.stats(SystemTime::now());
            let removed = cache.clear()?;
            match stats.root {
                Some(root) => println!("Removed {removed} cache files from {}.", root.display()),
                None => println!("The cache is disabled. Removed 0 cache files."),
            }
        }
    }
    Ok(())
}

fn print_cache_info(stats: &cceroby::cache::CacheStats) {
    match &stats.root {
        Some(root) => println!("Cache: {}", root.display()),
        None => println!("Cache: disabled"),
    }
    print_cache_layer("Metadata", stats.metadata);
    print_cache_layer("Thumbnails", stats.thumbnails);
}

fn print_cache_layer(label: &str, stats: CacheLayerStats) {
    println!(
        "{label}: {} fresh files, {} bytes; {} expired files, {} bytes.",
        stats.fresh.files, stats.fresh.bytes, stats.expired.files, stats.expired.bytes
    );
}
