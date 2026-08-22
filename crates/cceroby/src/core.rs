//! Typed search input and deterministic session transitions.

use std::fmt;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use serde::Deserialize;
use thiserror::Error;

use crate::providers::{ProviderNotice, ProviderRegistry};

/// One museum or collection that can supply image records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum)]
pub enum SourceKind {
    /// Art Institute of Chicago.
    #[value(name = "aic")]
    ArtInstituteChicago,
    /// Cleveland Museum of Art.
    #[value(name = "cleveland")]
    ClevelandMuseum,
    /// Metropolitan Museum of Art.
    #[value(name = "met")]
    MetropolitanMuseum,
    /// Smithsonian Institution.
    #[value(name = "smithsonian")]
    Smithsonian,
    /// Wikimedia Commons.
    #[value(name = "wikimedia")]
    WikimediaCommons,
}

impl SourceKind {
    pub const ALL: [Self; 5] = [
        Self::ArtInstituteChicago,
        Self::ClevelandMuseum,
        Self::MetropolitanMuseum,
        Self::Smithsonian,
        Self::WikimediaCommons,
    ];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::ArtInstituteChicago => "aic",
            Self::ClevelandMuseum => "cleveland",
            Self::MetropolitanMuseum => "met",
            Self::Smithsonian => "smithsonian",
            Self::WikimediaCommons => "wikimedia",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ArtInstituteChicago => "Art Institute of Chicago",
            Self::ClevelandMuseum => "Cleveland Museum of Art",
            Self::MetropolitanMuseum => "The Met",
            Self::Smithsonian => "Smithsonian",
            Self::WikimediaCommons => "Wikimedia Commons",
        }
    }
}

/// A canonical, non-empty search phrase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryText(String);

impl QueryText {
    pub fn parse(raw: &str) -> Result<Self, SearchInputError> {
        let canonical = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if canonical.is_empty() {
            Err(SearchInputError::EmptyQuery)
        } else {
            Ok(Self(canonical))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A canonical optional culture or region hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Culture(String);

impl Culture {
    #[must_use]
    pub fn parse(raw: Option<String>) -> Option<Self> {
        raw.map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .map(Self)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A source collection that is non-empty by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSet(Vec<SourceKind>);

impl SourceSet {
    pub fn parse(raw: &[SourceKind]) -> Result<Self, SearchInputError> {
        let sources = SourceKind::ALL
            .into_iter()
            .filter(|source| raw.contains(source))
            .collect::<Vec<_>>();
        if sources.is_empty() {
            Err(SearchInputError::EmptySources)
        } else {
            Ok(Self(sources))
        }
    }

    #[must_use]
    pub fn as_slice(&self) -> &[SourceKind] {
        &self.0
    }

    #[must_use]
    pub fn contains(&self, source: SourceKind) -> bool {
        self.0.contains(&source)
    }
}

/// An output directory that existed when command-line input was parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDirectory(PathBuf);

impl OutputDirectory {
    #[must_use]
    pub(crate) fn from_verified_path(path: PathBuf) -> Self {
        Self(path)
    }

    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// Fully parsed command-line search input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSeed {
    pub query: QueryText,
    pub sources: SourceSet,
    pub culture: Option<Culture>,
    pub output: OutputDirectory,
    pub open: bool,
    pub serve: bool,
}

/// Fully parsed form input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub query: QueryText,
    pub sources: SourceSet,
    pub culture: Option<Culture>,
}

impl SearchQuery {
    #[must_use]
    pub fn from_seed(seed: &SearchSeed) -> Self {
        Self {
            query: seed.query.clone(),
            sources: seed.sources.clone(),
            culture: seed.culture.clone(),
        }
    }
}

/// Raw values accepted by the local search form.
#[derive(Debug, Default, Deserialize)]
pub struct SearchParams {
    pub query: String,
    #[serde(default)]
    pub aic: bool,
    #[serde(default)]
    pub cleveland: bool,
    #[serde(default)]
    pub met: bool,
    #[serde(default)]
    pub smithsonian: bool,
    #[serde(default)]
    pub wikimedia: bool,
    pub culture: Option<String>,
}

impl TryFrom<SearchParams> for SearchQuery {
    type Error = SearchInputError;

    fn try_from(params: SearchParams) -> Result<Self, Self::Error> {
        let selected: Vec<SourceKind> = [
            (params.aic, SourceKind::ArtInstituteChicago),
            (params.cleveland, SourceKind::ClevelandMuseum),
            (params.met, SourceKind::MetropolitanMuseum),
            (params.smithsonian, SourceKind::Smithsonian),
            (params.wikimedia, SourceKind::WikimediaCommons),
        ]
        .into_iter()
        .filter_map(|(is_selected, source)| is_selected.then_some(source))
        .collect();

        Ok(Self {
            query: QueryText::parse(&params.query)?,
            sources: SourceSet::parse(&selected)?,
            culture: Culture::parse(params.culture),
        })
    }
}

/// Current deterministic state of the local search page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSession {
    query: SearchQuery,
    notices: Vec<ProviderNotice>,
}

impl SearchSession {
    #[must_use]
    pub fn new(query: SearchQuery) -> Self {
        Self {
            query,
            notices: Vec::new(),
        }
    }

    pub fn reset_if_changed(&mut self, query: SearchQuery, providers: &ProviderRegistry) -> bool {
        let changed = self.query != query;
        if changed {
            self.query = query;
        }
        self.notices = providers.unavailable_for(self.query.sources.as_slice());
        changed
    }

    #[must_use]
    pub fn view(&self) -> SearchView<'_> {
        SearchView {
            query: &self.query,
            notices: &self.notices,
        }
    }
}

/// Read-only page data.
#[derive(Debug, Clone, Copy)]
pub struct SearchView<'a> {
    pub query: &'a SearchQuery,
    pub notices: &'a [ProviderNotice],
}

/// A typed boundary error shown before I/O starts or as a bad form request.
#[derive(Debug, Error)]
pub enum SearchInputError {
    #[error("the search query must contain text")]
    EmptyQuery,
    #[error("select at least one source")]
    EmptySources,
    #[error("cannot use output path {path}: {source}")]
    OutputPathUnavailable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("output path is not a directory: {0}")]
    OutputPathNotDirectory(PathBuf),
}

impl fmt::Display for SourceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.key())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn query(text: &str, sources: &[SourceKind], culture: Option<&str>) -> SearchQuery {
        SearchQuery {
            query: QueryText::parse(text).expect("query is valid"),
            sources: SourceSet::parse(sources).expect("sources are valid"),
            culture: Culture::parse(culture.map(str::to_owned)),
        }
    }

    #[test]
    fn source_metadata_is_complete() {
        let metadata =
            SourceKind::ALL.map(|source| (source.key(), source.label(), source.to_string()));
        assert_eq!(metadata.len(), 5);
        assert_eq!(
            metadata[0],
            ("aic", "Art Institute of Chicago", "aic".into())
        );
        assert_eq!(metadata[4].0, "wikimedia");
    }

    #[test]
    fn query_text_collapses_whitespace_and_rejects_empty_input() {
        let parsed = QueryText::parse("  blue\n  mask ").expect("query is valid");
        assert_eq!(parsed.as_str(), "blue mask");
        assert!(matches!(
            QueryText::parse(" \t "),
            Err(SearchInputError::EmptyQuery)
        ));
    }

    #[test]
    fn culture_trims_text_and_removes_empty_values() {
        assert_eq!(
            Culture::parse(Some("  Japan ".into())).map(|value| value.0),
            Some("Japan".into())
        );
        assert_eq!(Culture::parse(Some("  ".into())), None);
        assert_eq!(Culture::parse(None), None);
    }

    #[test]
    fn source_set_deduplicates_in_registry_order_and_rejects_empty_input() {
        let parsed = SourceSet::parse(&[
            SourceKind::WikimediaCommons,
            SourceKind::ArtInstituteChicago,
            SourceKind::WikimediaCommons,
        ])
        .expect("sources are valid");
        assert_eq!(
            parsed.as_slice(),
            &[
                SourceKind::ArtInstituteChicago,
                SourceKind::WikimediaCommons
            ]
        );
        assert!(parsed.contains(SourceKind::WikimediaCommons));
        assert!(matches!(
            SourceSet::parse(&[]),
            Err(SearchInputError::EmptySources)
        ));
    }

    #[test]
    fn output_directory_preserves_a_verified_path() {
        let directory = std::env::temp_dir();
        let parsed = OutputDirectory::from_verified_path(directory.clone());
        assert_eq!(parsed.as_path(), directory);
    }

    #[test]
    fn search_query_copies_seed_values() {
        let seed = SearchSeed {
            query: QueryText::parse("mask").expect("query is valid"),
            sources: SourceSet::parse(&SourceKind::ALL).expect("sources are valid"),
            culture: Culture::parse(Some("Japan".into())),
            output: OutputDirectory::from_verified_path(std::env::temp_dir()),
            open: true,
            serve: false,
        };
        let copied = SearchQuery::from_seed(&seed);
        assert_eq!(copied.query.as_str(), "mask");
        assert_eq!(copied.culture.as_ref().map(Culture::as_str), Some("Japan"));
    }

    #[test]
    fn search_params_parse_all_fields_and_reject_empty_sources() {
        let parsed = SearchQuery::try_from(SearchParams {
            query: " mask ".into(),
            met: true,
            culture: Some(" Europe ".into()),
            ..SearchParams::default()
        })
        .expect("form is valid");
        assert_eq!(parsed.query.as_str(), "mask");
        assert_eq!(parsed.sources.as_slice(), &[SourceKind::MetropolitanMuseum]);
        assert_eq!(parsed.culture.as_ref().map(Culture::as_str), Some("Europe"));

        let error = SearchQuery::try_from(SearchParams {
            query: "mask".into(),
            ..SearchParams::default()
        });
        assert!(matches!(error, Err(SearchInputError::EmptySources)));
    }

    #[test]
    fn session_only_resets_when_query_changes() {
        let providers = ProviderRegistry::new();
        let original = query("mask", &[SourceKind::ArtInstituteChicago], None);
        let mut session = SearchSession::new(original.clone());
        assert!(!session.reset_if_changed(original, &providers));
        assert_eq!(session.view().notices.len(), 1);

        let changed = query("mask", &[SourceKind::MetropolitanMuseum], Some("Japan"));
        assert!(session.reset_if_changed(changed, &providers));
        assert_eq!(session.view().notices.len(), 1);
        assert_eq!(
            session.view().query.culture.as_ref().map(Culture::as_str),
            Some("Japan")
        );
    }
}
