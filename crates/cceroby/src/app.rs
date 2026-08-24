//! Command-line parsing at the process boundary.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::core::{
    Culture, OutputDirectory, QueryText, SearchInputError, SearchSeed, SourceKind, SourceSet,
};

/// Local museum image search.
#[derive(Debug, Parser)]
#[command(name = "cceroby", version, about)]
pub struct App {
    #[command(subcommand)]
    pub command: Command,
}

/// One operation selected by the user.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start a local search page.
    Search(SearchArgs),
    /// Inspect or remove local cached responses.
    Cache(CacheArgs),
}

/// Cache command options.
#[derive(Debug, Args)]
pub struct CacheArgs {
    #[command(subcommand)]
    pub action: CacheAction,
}

/// One cache operation.
#[derive(Debug, Clone, Copy, Subcommand)]
pub enum CacheAction {
    /// Show the cache path and use by freshness.
    Info,
    /// Remove all files in the Cceroby cache.
    Clear,
}

/// Raw command-line values for a local search.
#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Initial search phrase.
    pub query: String,

    /// Collection to search. Repeat this option or use commas.
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        num_args = 1..,
        default_values_t = SourceKind::ALL
    )]
    pub source: Vec<SourceKind>,

    /// Optional culture or region hint.
    #[arg(long)]
    pub culture: Option<String>,

    /// Open the search page in the default browser.
    #[arg(long)]
    pub open: bool,

    /// Directory for later downloaded files.
    #[arg(long, default_value = ".")]
    pub out: PathBuf,

    /// Keep the local search page available after later search work ends.
    #[arg(long)]
    pub serve: bool,
}

impl TryFrom<SearchArgs> for SearchSeed {
    type Error = SearchInputError;

    fn try_from(args: SearchArgs) -> Result<Self, Self::Error> {
        Ok(Self {
            query: QueryText::parse(&args.query)?,
            sources: SourceSet::parse(&args.source)?,
            culture: Culture::parse(args.culture),
            output: parse_output_directory(args.out)?,
            open: args.open,
            serve: args.serve,
        })
    }
}

fn parse_output_directory(path: PathBuf) -> Result<OutputDirectory, SearchInputError> {
    let metadata =
        std::fs::metadata(&path).map_err(|source| SearchInputError::OutputPathUnavailable {
            path: path.clone(),
            source,
        })?;
    if metadata.is_dir() {
        Ok(OutputDirectory::from_verified_path(path))
    } else {
        Err(SearchInputError::OutputPathNotDirectory(path))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use clap::Parser;

    use super::*;

    #[test]
    fn parser_supplies_all_sources_by_default() {
        let app =
            App::try_parse_from(["cceroby", "search", "mask"]).expect("command line is valid");
        let Command::Search(args) = app.command else {
            panic!("expected search command");
        };
        assert_eq!(args.source, SourceKind::ALL);
        assert_eq!(args.out, PathBuf::from("."));
    }

    #[test]
    fn parser_accepts_repeated_and_comma_separated_sources() {
        let app = App::try_parse_from(["cceroby", "search", "mask", "--source", "met,wikimedia"])
            .expect("command line is valid");
        let Command::Search(args) = app.command else {
            panic!("expected search command");
        };
        assert_eq!(
            args.source,
            vec![SourceKind::MetropolitanMuseum, SourceKind::WikimediaCommons]
        );
    }

    #[test]
    fn parser_rejects_source_option_without_values() {
        assert!(App::try_parse_from(["cceroby", "search", "mask", "--source"]).is_err());
    }

    #[test]
    fn parser_accepts_both_cache_actions() {
        let info =
            App::try_parse_from(["cceroby", "cache", "info"]).expect("cache info command is valid");
        assert!(matches!(
            info.command,
            Command::Cache(CacheArgs {
                action: CacheAction::Info
            })
        ));

        let clear = App::try_parse_from(["cceroby", "cache", "clear"])
            .expect("cache clear command is valid");
        assert!(matches!(
            clear.command,
            Command::Cache(CacheArgs {
                action: CacheAction::Clear
            })
        ));
    }

    #[test]
    fn argument_conversion_rejects_empty_sources_and_bad_output_paths() {
        let empty_sources = SearchArgs {
            query: "mask".into(),
            source: Vec::new(),
            culture: None,
            open: false,
            out: std::env::temp_dir(),
            serve: false,
        };
        assert!(matches!(
            SearchSeed::try_from(empty_sources),
            Err(SearchInputError::EmptySources)
        ));

        let bad_path = SearchArgs {
            query: "mask".into(),
            source: SourceKind::ALL.to_vec(),
            culture: None,
            open: false,
            out: std::env::temp_dir().join("cceroby-missing-output-directory"),
            serve: false,
        };
        assert!(matches!(
            SearchSeed::try_from(bad_path),
            Err(SearchInputError::OutputPathUnavailable { .. })
        ));

        let file_path = SearchArgs {
            query: "mask".into(),
            source: SourceKind::ALL.to_vec(),
            culture: None,
            open: false,
            out: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
            serve: false,
        };
        assert!(matches!(
            SearchSeed::try_from(file_path),
            Err(SearchInputError::OutputPathNotDirectory(_))
        ));
    }
}
