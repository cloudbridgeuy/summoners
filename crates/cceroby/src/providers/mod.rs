//! Provider contracts, fixed provider set, and source-specific modules.

use std::num::NonZeroU32;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use thiserror::Error;
use url::Url;

use crate::artwork::ArtworkKey;
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
#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    url: Url,
    headers: HeaderMap,
}

impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpRequest")
            .field("scheme", &self.url.scheme())
            .field("host", &self.url.host_str())
            .field("path", &self.url.path())
            .field("header_names", &self.headers.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl HttpRequest {
    #[must_use]
    pub fn get(url: Url) -> Self {
        Self {
            url,
            headers: HeaderMap::new(),
        }
    }

    #[must_use]
    pub fn with_header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.insert(name, value);
        self
    }

    #[must_use]
    pub fn url(&self) -> &Url {
        &self.url
    }

    #[must_use]
    pub fn canonical(&self) -> &str {
        self.url.as_str()
    }

    #[must_use]
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }
}

/// One provider-owned request-rate policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RatePolicy {
    Unlimited,
    TokenBucket(TokenBucketPolicy),
}

/// Valid token-bucket parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenBucketPolicy {
    capacity: NonZeroU32,
    refill_interval: Duration,
}

/// One provider-owned display image purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayImageSize {
    Card,
    Preview,
}

/// One browser-safe display image media type selected by a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMediaType {
    Jpeg,
    Png,
    Webp,
}

impl DisplayMediaType {
    #[must_use]
    pub const fn content_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Webp => "image/webp",
        }
    }
}

/// One provider-derived image request and its expected response type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayImageRequest {
    request: HttpRequest,
    media_type: DisplayMediaType,
}

impl DisplayImageRequest {
    #[must_use]
    pub fn new(request: HttpRequest, media_type: DisplayMediaType) -> Self {
        Self {
            request,
            media_type,
        }
    }

    #[must_use]
    pub fn request(&self) -> &HttpRequest {
        &self.request
    }

    #[must_use]
    pub const fn media_type(&self) -> DisplayMediaType {
        self.media_type
    }
}

impl TokenBucketPolicy {
    #[must_use]
    pub const fn new(capacity: NonZeroU32, refill_interval: Duration) -> Self {
        assert!(
            !refill_interval.is_zero(),
            "refill interval must be positive"
        );
        Self {
            capacity,
            refill_interval,
        }
    }

    #[must_use]
    pub const fn capacity(self) -> NonZeroU32 {
        self.capacity
    }

    #[must_use]
    pub const fn refill_interval(self) -> Duration {
        self.refill_interval
    }
}

/// A pure provider request and parser contract.
pub trait Provider: Send + Sync {
    fn kind(&self) -> SourceKind;
    fn rate_policy(&self) -> RatePolicy;
    fn search_request(&self, query: &SearchQuery, cursor: Option<&str>) -> HttpRequest;
    fn parse_search(&self, bytes: &[u8]) -> Result<ProviderSearchPage, ProviderError>;
    fn parse_artwork(
        &self,
        candidate: &ProviderCandidate,
        object_bytes: Option<&[u8]>,
    ) -> Result<Artwork, ArtworkDropReason>;
    fn artwork_request(&self, key: &ArtworkKey) -> Result<HttpRequest, ProviderError>;
    fn parse_artwork_response(&self, bytes: &[u8]) -> Result<Artwork, ProviderError>;
    fn display_image_request(
        &self,
        artwork: &Artwork,
        size: DisplayImageSize,
    ) -> Result<DisplayImageRequest, ProviderError>;

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
    #[error("the provider artwork is not available")]
    ArtworkUnavailable,
    #[error("the provider image request is invalid")]
    InvalidImageRequest,
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

    use reqwest::header::{AUTHORIZATION, HeaderValue, USER_AGENT};

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
        let request = HttpRequest::get(url.clone())
            .with_header(USER_AGENT, HeaderValue::from_static("cceroby-test/1.0"));
        assert_eq!(request.url(), &url);
        assert_eq!(request.canonical(), url.as_str());
        assert_eq!(
            request.headers().get(USER_AGENT),
            Some(&HeaderValue::from_static("cceroby-test/1.0"))
        );
    }

    #[test]
    fn http_request_diagnostics_hide_query_and_header_secrets() {
        let request = HttpRequest::get(
            Url::parse("https://example.test/search?api_key=query-secret").expect("URL is valid"),
        )
        .with_header(AUTHORIZATION, HeaderValue::from_static("header-secret"));
        let debug = format!("{request:?}");
        assert!(debug.contains("example.test"));
        assert!(debug.contains("authorization"));
        assert!(!debug.contains("query-secret"));
        assert!(!debug.contains("header-secret"));
    }

    #[test]
    fn token_bucket_policy_keeps_nonzero_capacity_and_interval() {
        let policy = TokenBucketPolicy::new(NonZeroU32::MIN, Duration::from_millis(250));
        assert_eq!(policy.capacity(), NonZeroU32::MIN);
        assert_eq!(policy.refill_interval(), Duration::from_millis(250));
    }

    #[test]
    fn display_media_types_have_stable_http_content_types() {
        assert_eq!(DisplayMediaType::Jpeg.content_type(), "image/jpeg");
        assert_eq!(DisplayMediaType::Png.content_type(), "image/png");
        assert_eq!(DisplayMediaType::Webp.content_type(), "image/webp");
    }

    #[test]
    fn display_image_request_keeps_provider_request_and_media_type() {
        let request =
            HttpRequest::get(Url::parse("https://example.test/image.jpg").expect("URL is valid"));
        let display = DisplayImageRequest::new(request.clone(), DisplayMediaType::Jpeg);
        assert_eq!(display.request(), &request);
        assert_eq!(display.media_type(), DisplayMediaType::Jpeg);
    }

    #[test]
    #[should_panic(expected = "refill interval must be positive")]
    fn token_bucket_policy_rejects_a_zero_interval() {
        let _ = TokenBucketPolicy::new(NonZeroU32::MIN, Duration::ZERO);
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
