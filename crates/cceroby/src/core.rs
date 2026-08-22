//! Typed search input and deterministic session transitions.

use std::fmt;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use serde::Deserialize;
use thiserror::Error;

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

/// A license that permits commercial use of an artwork.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommercialLicense {
    /// Creative Commons Zero.
    Cc0,
    /// A public-domain mark or declaration.
    PublicDomain,
    /// Creative Commons Attribution.
    CcBy,
}

impl CommercialLicense {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cc0 => "CC0",
            Self::PublicDomain => "Public domain",
            Self::CcBy => "CC BY",
        }
    }

    #[must_use]
    pub const fn url(self) -> &'static str {
        match self {
            Self::Cc0 => "https://creativecommons.org/publicdomain/zero/1.0/",
            Self::PublicDomain => "https://creativecommons.org/publicdomain/mark/1.0/",
            Self::CcBy => "https://creativecommons.org/licenses/by/4.0/",
        }
    }
}

impl TryFrom<&str> for CommercialLicense {
    type Error = LicensePolicyError;

    fn try_from(raw: &str) -> Result<Self, Self::Error> {
        let normalized = raw.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "cc0"
            | "cc0 1.0"
            | "https://creativecommons.org/publicdomain/zero/1.0"
            | "https://creativecommons.org/publicdomain/zero/1.0/" => Ok(Self::Cc0),
            "public domain"
            | "public domain mark"
            | "public domain mark 1.0"
            | "https://creativecommons.org/publicdomain/mark/1.0"
            | "https://creativecommons.org/publicdomain/mark/1.0/" => Ok(Self::PublicDomain),
            "cc by"
            | "cc by 1.0"
            | "cc by 2.0"
            | "cc by 2.5"
            | "cc by 3.0"
            | "cc by 4.0"
            | "https://creativecommons.org/licenses/by/1.0"
            | "https://creativecommons.org/licenses/by/2.0"
            | "https://creativecommons.org/licenses/by/2.5"
            | "https://creativecommons.org/licenses/by/3.0"
            | "https://creativecommons.org/licenses/by/4.0"
            | "https://creativecommons.org/licenses/by/1.0/"
            | "https://creativecommons.org/licenses/by/2.0/"
            | "https://creativecommons.org/licenses/by/2.5/"
            | "https://creativecommons.org/licenses/by/3.0/"
            | "https://creativecommons.org/licenses/by/4.0/" => Ok(Self::CcBy),
            "cc by-sa"
            | "cc by-sa 4.0"
            | "cc by-nc"
            | "cc by-nc 4.0"
            | "cc by-nd"
            | "cc by-nd 4.0"
            | "cc by-nc-sa"
            | "cc by-nc-sa 4.0"
            | "cc by-nc-nd"
            | "cc by-nc-nd 4.0"
            | "https://creativecommons.org/licenses/by-sa/4.0"
            | "https://creativecommons.org/licenses/by-nc/4.0"
            | "https://creativecommons.org/licenses/by-nd/4.0"
            | "https://creativecommons.org/licenses/by-nc-sa/4.0"
            | "https://creativecommons.org/licenses/by-nc-nd/4.0" => {
                Err(LicensePolicyError::Restricted)
            }
            _ => Err(LicensePolicyError::Unknown),
        }
    }
}

/// A license value that the commercial-use policy rejects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum LicensePolicyError {
    #[error("the license restricts commercial use or changes")]
    Restricted,
    #[error("the license is not known")]
    Unknown,
}

/// Provider-owned image URLs kept for later proxy and download routes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageUrls {
    pub thumbnail: String,
    pub display: String,
    pub original: Option<String>,
}

/// One normalized artwork that passed the license policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artwork {
    pub source: SourceKind,
    pub source_id: String,
    pub title: String,
    pub creator: Option<String>,
    pub date: Option<String>,
    pub culture: Option<String>,
    pub license: CommercialLicense,
    pub image_urls: ImageUrls,
    pub institution: String,
    pub provider_credit: Option<String>,
    pub object_url: String,
}

/// One normalized provider page and its optional next cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderPage {
    pub source: SourceKind,
    pub artworks: Vec<Artwork>,
    pub next_cursor: Option<String>,
}

/// A provider status that can be shown without exposing transport details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderNotice {
    Unavailable { source: SourceKind },
    Failed { source: SourceKind },
}

/// A provider result after transport and parsing finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderOutcome {
    Success(ProviderPage),
    Unavailable { source: SourceKind },
    Failed { source: SourceKind },
}

impl ProviderOutcome {
    #[must_use]
    pub fn from_result<E>(source: SourceKind, result: Result<ProviderPage, E>) -> Self {
        result.map_or(Self::Failed { source }, Self::Success)
    }
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
    artworks: Vec<Artwork>,
}

impl SearchSession {
    #[must_use]
    pub fn new(query: SearchQuery) -> Self {
        Self {
            query,
            notices: Vec::new(),
            artworks: Vec::new(),
        }
    }

    pub fn begin_search(&mut self, query: SearchQuery) -> bool {
        let changed = self.query != query;
        self.query = query;
        self.notices.clear();
        self.artworks.clear();
        changed
    }

    #[must_use]
    pub fn view(&self) -> SearchView<'_> {
        SearchView {
            query: &self.query,
            notices: &self.notices,
            artworks: &self.artworks,
        }
    }
}

/// Merge one provider outcome into the current page in dispatch order.
pub fn merge_page(session: &mut SearchSession, outcome: ProviderOutcome) {
    match outcome {
        ProviderOutcome::Success(page) => session.artworks.extend(page.artworks),
        ProviderOutcome::Unavailable { source } => {
            session.notices.push(ProviderNotice::Unavailable { source });
        }
        ProviderOutcome::Failed { source } => {
            session.notices.push(ProviderNotice::Failed { source });
        }
    }
}

/// Read-only page data.
#[derive(Debug, Clone, Copy)]
pub struct SearchView<'a> {
    pub query: &'a SearchQuery,
    pub notices: &'a [ProviderNotice],
    pub artworks: &'a [Artwork],
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

    fn artwork(source_id: &str) -> Artwork {
        Artwork {
            source: SourceKind::ArtInstituteChicago,
            source_id: source_id.into(),
            title: "Mask".into(),
            creator: None,
            date: None,
            culture: None,
            license: CommercialLicense::PublicDomain,
            image_urls: ImageUrls {
                thumbnail: "https://example.test/thumb.jpg".into(),
                display: "https://example.test/display.jpg".into(),
                original: None,
            },
            institution: "Art Institute of Chicago".into(),
            provider_credit: None,
            object_url: "https://example.test/object".into(),
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
    fn session_starts_each_search_with_empty_results() {
        let original = query("mask", &[SourceKind::ArtInstituteChicago], None);
        let mut session = SearchSession::new(original.clone());
        merge_page(
            &mut session,
            ProviderOutcome::Success(ProviderPage {
                source: SourceKind::ArtInstituteChicago,
                artworks: vec![artwork("1")],
                next_cursor: None,
            }),
        );
        assert!(!session.begin_search(original));
        assert!(session.view().notices.is_empty());
        assert!(session.view().artworks.is_empty());

        let changed = query("mask", &[SourceKind::MetropolitanMuseum], Some("Japan"));
        assert!(session.begin_search(changed));
        assert_eq!(
            session.view().query.culture.as_ref().map(Culture::as_str),
            Some("Japan")
        );
    }

    #[test]
    fn license_policy_accepts_only_the_supported_cc0_forms() {
        for raw in [
            "CC0",
            "CC0 1.0",
            "https://creativecommons.org/publicdomain/zero/1.0",
            "https://creativecommons.org/publicdomain/zero/1.0/",
        ] {
            assert_eq!(CommercialLicense::try_from(raw), Ok(CommercialLicense::Cc0));
        }
    }

    #[test]
    fn license_policy_accepts_only_the_supported_public_domain_forms() {
        for raw in [
            "Public Domain",
            "Public Domain Mark",
            "Public Domain Mark 1.0",
            "https://creativecommons.org/publicdomain/mark/1.0",
            "https://creativecommons.org/publicdomain/mark/1.0/",
        ] {
            assert_eq!(
                CommercialLicense::try_from(raw),
                Ok(CommercialLicense::PublicDomain)
            );
        }
    }

    #[test]
    fn license_policy_accepts_only_the_supported_cc_by_forms() {
        for raw in [
            "CC BY",
            "CC BY 1.0",
            "CC BY 2.0",
            "CC BY 2.5",
            "CC BY 3.0",
            "CC BY 4.0",
            "https://creativecommons.org/licenses/by/1.0",
            "https://creativecommons.org/licenses/by/2.0",
            "https://creativecommons.org/licenses/by/2.5",
            "https://creativecommons.org/licenses/by/3.0",
            "https://creativecommons.org/licenses/by/4.0",
            "https://creativecommons.org/licenses/by/4.0/",
        ] {
            assert_eq!(
                CommercialLicense::try_from(raw),
                Ok(CommercialLicense::CcBy)
            );
        }
    }

    #[test]
    fn license_policy_rejects_share_alike_noncommercial_and_no_derivatives() {
        for raw in [
            "CC BY-SA 1.0",
            "CC BY-SA 2.0",
            "CC BY-SA 2.5",
            "CC BY-SA 3.0",
            "CC BY-SA 4.0",
            "CC BY-NC 1.0",
            "CC BY-NC 2.0",
            "CC BY-NC 2.5",
            "CC BY-NC 3.0",
            "CC BY-NC 4.0",
            "CC BY-ND 1.0",
            "CC BY-ND 2.0",
            "CC BY-ND 2.5",
            "CC BY-ND 3.0",
            "CC BY-ND 4.0",
            "CC BY-NC-SA 1.0",
            "CC BY-NC-SA 2.0",
            "CC BY-NC-SA 2.5",
            "CC BY-NC-SA 3.0",
            "CC BY-NC-SA 4.0",
            "CC BY-NC-ND 1.0",
            "CC BY-NC-ND 2.0",
            "CC BY-NC-ND 2.5",
            "CC BY-NC-ND 3.0",
            "CC BY-NC-ND 4.0",
            "https://creativecommons.org/licenses/by-sa/4.0/",
            "https://creativecommons.org/licenses/by-nc/4.0/",
            "https://creativecommons.org/licenses/by-nd/4.0/",
            "https://creativecommons.org/licenses/by-nc-sa/4.0/",
            "https://creativecommons.org/licenses/by-nc-nd/4.0/",
        ] {
            assert!(CommercialLicense::try_from(raw).is_err(), "accepted {raw}");
        }
    }

    #[test]
    fn license_policy_rejects_unknown_input() {
        for raw in [
            "all rights reserved",
            "not public domain",
            "CC0 BY-NC",
            "CC0 / CC BY-NC 4.0",
            "CC BY 4.0 / CC BY-SA 4.0",
            "CC BY 4.0 with extra restrictions",
            "CC BY 4.0//",
            "https://creativecommons.org/licenses/by/4.0//",
            "https://creativecommons.org/licenses/by/4.0?extra=true",
            "https://creativecommons.org.evil.test/licenses/by/4.0/",
            "public domain-ish",
            "unknown",
            "",
        ] {
            assert_eq!(
                CommercialLicense::try_from(raw),
                Err(LicensePolicyError::Unknown)
            );
        }
    }

    #[test]
    fn license_metadata_has_stable_labels_and_urls() {
        assert_eq!(CommercialLicense::Cc0.label(), "CC0");
        assert!(CommercialLicense::Cc0.url().contains("/zero/"));
        assert_eq!(CommercialLicense::PublicDomain.label(), "Public domain");
        assert!(CommercialLicense::PublicDomain.url().contains("/mark/"));
        assert_eq!(CommercialLicense::CcBy.label(), "CC BY");
        assert!(CommercialLicense::CcBy.url().contains("/licenses/by/"));
    }

    #[test]
    fn merge_page_preserves_results_and_adds_one_notice_per_failure() {
        let mut session = SearchSession::new(query(
            "mask",
            &[SourceKind::ArtInstituteChicago, SourceKind::ClevelandMuseum],
            None,
        ));
        merge_page(
            &mut session,
            ProviderOutcome::Success(ProviderPage {
                source: SourceKind::ArtInstituteChicago,
                artworks: vec![artwork("1"), artwork("2")],
                next_cursor: Some("2".into()),
            }),
        );
        merge_page(
            &mut session,
            ProviderOutcome::Failed {
                source: SourceKind::ClevelandMuseum,
            },
        );
        merge_page(
            &mut session,
            ProviderOutcome::Unavailable {
                source: SourceKind::MetropolitanMuseum,
            },
        );

        assert_eq!(session.view().artworks.len(), 2);
        assert_eq!(
            session.view().notices,
            &[
                ProviderNotice::Failed {
                    source: SourceKind::ClevelandMuseum
                },
                ProviderNotice::Unavailable {
                    source: SourceKind::MetropolitanMuseum
                }
            ]
        );
    }

    #[test]
    fn provider_outcome_converts_success_and_failure_without_transport_details() {
        let page = ProviderPage {
            source: SourceKind::ArtInstituteChicago,
            artworks: vec![artwork("1")],
            next_cursor: None,
        };
        assert_eq!(
            ProviderOutcome::from_result(
                SourceKind::ArtInstituteChicago,
                Ok::<_, &'static str>(page.clone())
            ),
            ProviderOutcome::Success(page)
        );
        assert_eq!(
            ProviderOutcome::from_result(
                SourceKind::ArtInstituteChicago,
                Err::<ProviderPage, _>("private transport error")
            ),
            ProviderOutcome::Failed {
                source: SourceKind::ArtInstituteChicago
            }
        );
    }
}
