//! Provider contracts, fixed provider set, and source-specific modules.

use thiserror::Error;
use url::Url;

use crate::core::{Artwork, ProviderNotice, SearchQuery, SourceKind};

pub mod aic;
pub mod cleveland;
pub mod commons;
pub mod met;
pub mod smithsonian;

use aic::AicProvider;
use cleveland::ClevelandProvider;
use commons::CommonsProvider;
use met::MetProvider;
use smithsonian::SmithsonianProvider;

/// One immutable outbound provider request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    url: Url,
}

impl HttpRequest {
    #[must_use]
    pub fn get(url: Url) -> Self {
        Self { url }
    }

    #[must_use]
    pub fn url(&self) -> &Url {
        &self.url
    }

    #[must_use]
    pub fn canonical(&self) -> &str {
        self.url.as_str()
    }
}

/// A pure provider request and parser contract.
pub trait Provider: Send + Sync {
    fn kind(&self) -> SourceKind;
    fn search_request(&self, query: &SearchQuery, cursor: Option<&str>) -> HttpRequest;
    fn parse_search(&self, bytes: &[u8]) -> Result<ProviderSearchPage, ProviderError>;
    fn parse_artwork(
        &self,
        candidate: &ProviderCandidate,
        object_bytes: Option<&[u8]>,
    ) -> Result<Artwork, ArtworkDropReason>;

    fn object_request(&self, _candidate: &ProviderCandidate) -> Option<HttpRequest> {
        None
    }
}

/// One provider-owned search item and the context required to parse it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderCandidate {
    pub raw: serde_json::Value,
    pub context: Option<String>,
}

/// One parsed provider search page before optional object requests finish.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderSearchPage {
    pub candidates: Vec<ProviderCandidate>,
    pub next_cursor: Option<String>,
}

/// A response cannot be parsed as one provider page.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProviderError {
    #[error("the provider response is malformed")]
    MalformedResponse,
    #[error("the provider response has no image service")]
    MissingImageService,
}

/// Built-in provider configuration cannot be constructed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProviderConfigError {
    #[error("the built-in AIC endpoint is invalid")]
    InvalidAicEndpoint(#[from] url::ParseError),
}

/// A provider record is valid JSON but cannot become an accepted artwork.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ArtworkDropReason {
    #[error("the artwork is not public domain")]
    NotPublicDomain,
    #[error("the artwork has no source ID")]
    MissingSourceId,
    #[error("the artwork has no title")]
    MissingTitle,
    #[error("the artwork has no image")]
    MissingImage,
}

/// One fixed source slot, either available or a typed stub.
#[derive(Clone, Copy)]
pub enum ProviderEntry<'a> {
    Available(&'a dyn Provider),
    Unavailable { source: SourceKind },
}

impl std::fmt::Debug for ProviderEntry<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Available(provider) => formatter
                .debug_tuple("Available")
                .field(&provider.kind())
                .finish(),
            Self::Unavailable { source } => formatter
                .debug_struct("Unavailable")
                .field("source", source)
                .finish(),
        }
    }
}

impl ProviderEntry<'_> {
    #[must_use]
    pub fn kind(self) -> SourceKind {
        match self {
            Self::Available(provider) => provider.kind(),
            Self::Unavailable { source } => source,
        }
    }

    #[must_use]
    pub fn unavailable_notice(self) -> Option<ProviderNotice> {
        match self {
            Self::Available(_) => None,
            Self::Unavailable { source } => Some(ProviderNotice::Unavailable { source }),
        }
    }
}

/// Ordered application provider configuration.
#[derive(Debug, Clone)]
pub struct ProviderSet {
    aic: AicProvider,
    cleveland: ClevelandProvider,
    met: MetProvider,
    smithsonian: SmithsonianProvider,
    commons: CommonsProvider,
}

impl ProviderSet {
    pub fn from_env() -> Result<Self, ProviderConfigError> {
        AicProvider::official_endpoint()
            .map(Self::with_aic_endpoint)
            .map_err(ProviderConfigError::from)
    }

    #[must_use]
    pub fn with_aic_endpoint(endpoint: Url) -> Self {
        Self {
            aic: AicProvider::new(endpoint),
            cleveland: ClevelandProvider::new(),
            met: MetProvider::new(),
            smithsonian: SmithsonianProvider::new(),
            commons: CommonsProvider::new(),
        }
    }

    #[must_use]
    pub fn get(&self, source: SourceKind) -> ProviderEntry<'_> {
        match source {
            SourceKind::ArtInstituteChicago => self.aic.entry(),
            SourceKind::ClevelandMuseum => self.cleveland.entry(),
            SourceKind::MetropolitanMuseum => self.met.entry(),
            SourceKind::Smithsonian => self.smithsonian.entry(),
            SourceKind::WikimediaCommons => self.commons.entry(),
        }
    }

    #[must_use]
    pub fn selected<'a>(&'a self, sources: &'a [SourceKind]) -> Vec<ProviderEntry<'a>> {
        SourceKind::ALL
            .into_iter()
            .filter(|source| sources.contains(source))
            .map(|source| self.get(source))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use crate::core::{Culture, QueryText, SourceSet};

    use super::*;

    fn query() -> SearchQuery {
        SearchQuery {
            query: QueryText::parse("mask").expect("query is valid"),
            sources: SourceSet::parse(&SourceKind::ALL).expect("sources are valid"),
            culture: Culture::parse(None),
        }
    }

    fn provider_set() -> ProviderSet {
        ProviderSet::from_env().expect("built-in provider configuration is valid")
    }

    #[test]
    fn http_request_keeps_one_canonical_get_url() {
        let url = Url::parse("https://example.test/search?q=mask").expect("URL is valid");
        let request = HttpRequest::get(url.clone());
        assert_eq!(request.url(), &url);
        assert_eq!(request.canonical(), url.as_str());
    }

    #[test]
    fn provider_default_object_request_is_no_op() {
        let provider = AicProvider::official().expect("built-in endpoint is valid");
        let candidate = ProviderCandidate {
            raw: serde_json::json!({ "id": 123 }),
            context: None,
        };
        assert_eq!(provider.object_request(&candidate), None);
    }

    #[test]
    fn provider_entries_return_kinds_and_only_stub_notices() {
        let providers = provider_set();
        let aic = providers.get(SourceKind::ArtInstituteChicago);
        let met = providers.get(SourceKind::MetropolitanMuseum);
        assert_eq!(aic.kind(), SourceKind::ArtInstituteChicago);
        assert_eq!(aic.unavailable_notice(), None);
        assert_eq!(met.kind(), SourceKind::MetropolitanMuseum);
        assert_eq!(
            met.unavailable_notice(),
            Some(ProviderNotice::Unavailable {
                source: SourceKind::MetropolitanMuseum
            })
        );
    }

    #[test]
    fn selected_providers_keep_fixed_source_order() {
        let providers = provider_set();
        let selected = providers.selected(&[
            SourceKind::WikimediaCommons,
            SourceKind::ArtInstituteChicago,
            SourceKind::Smithsonian,
        ]);
        assert_eq!(
            selected
                .into_iter()
                .map(ProviderEntry::kind)
                .collect::<Vec<_>>(),
            vec![
                SourceKind::ArtInstituteChicago,
                SourceKind::Smithsonian,
                SourceKind::WikimediaCommons
            ]
        );
    }

    #[test]
    fn injected_endpoint_is_used_by_the_aic_slot() {
        let endpoint = Url::parse("http://127.0.0.1:4000/search").expect("URL is valid");
        let providers = ProviderSet::with_aic_endpoint(endpoint.clone());
        let ProviderEntry::Available(provider) = providers.get(SourceKind::ArtInstituteChicago)
        else {
            panic!("AIC is available");
        };
        assert!(
            provider
                .search_request(&query(), None)
                .canonical()
                .starts_with(endpoint.as_str())
        );
    }

    #[test]
    fn built_in_configuration_uses_the_typed_official_aic_endpoint() {
        let providers = ProviderSet::from_env().expect("provider configuration is valid");
        let ProviderEntry::Available(provider) = providers.get(SourceKind::ArtInstituteChicago)
        else {
            panic!("AIC is available");
        };
        let request = provider.search_request(&query(), None);
        assert_eq!(request.url().scheme(), "https");
        assert_eq!(request.url().host_str(), Some("api.artic.edu"));
        assert_eq!(request.url().path(), "/api/v1/artworks/search");
    }
}
