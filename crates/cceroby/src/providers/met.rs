//! Metropolitan Museum request construction and response parsing.

use std::time::Duration;

use serde::Deserialize;
use url::{Host, Url};

use crate::artwork::ArtworkKey;
use crate::core::{Artwork, CommercialLicense, ImageUrls, SearchQuery, SourceKind};

use super::{
    ArtworkDropReason, DisplayImageRequest, DisplayImageSize, DisplayMediaType, HttpRequest,
    Provider, ProviderCandidate, ProviderEntry, ProviderError, ProviderSearchPage, RatePolicy,
    TokenBucketPolicy,
};

const OFFICIAL_ENDPOINT: &str = "https://collectionapi.metmuseum.org/public/collection/v1/search";
const PAGE_SIZE: usize = 20;

/// Metropolitan Museum provider configuration.
#[derive(Debug, Clone)]
pub struct MetProvider {
    endpoint: Url,
}

impl MetProvider {
    #[must_use]
    pub fn new(endpoint: Url) -> Self {
        Self { endpoint }
    }

    pub fn official_endpoint() -> Result<Url, url::ParseError> {
        Url::parse(OFFICIAL_ENDPOINT)
    }

    pub fn official() -> Result<Self, url::ParseError> {
        Self::official_endpoint().map(Self::new)
    }

    #[must_use]
    pub const fn kind(&self) -> SourceKind {
        SourceKind::MetropolitanMuseum
    }

    #[must_use]
    pub fn entry(&self) -> ProviderEntry<'_> {
        ProviderEntry::Available(self)
    }
}

impl Provider for MetProvider {
    fn kind(&self) -> SourceKind {
        self.kind()
    }

    fn rate_policy(&self) -> RatePolicy {
        RatePolicy::TokenBucket(TokenBucketPolicy::new(
            std::num::NonZeroU32::MIN,
            Duration::from_micros(12_500),
        ))
    }

    fn validate_search_cursor(&self, cursor: Option<&str>) -> Result<(), ProviderError> {
        parse_offset(cursor).map(|_| ())
    }

    fn search_request(&self, query: &SearchQuery, _cursor: Option<&str>) -> HttpRequest {
        let mut url = self.endpoint.clone();
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("q", query.query.as_str());
            pairs.append_pair("hasImages", "true");
            if let Some(culture) = &query.culture {
                pairs.append_pair("geoLocation", culture.as_str());
            }
        }
        HttpRequest::get(url)
    }

    fn parse_search(
        &self,
        bytes: &[u8],
        cursor: Option<&str>,
    ) -> Result<ProviderSearchPage, ProviderError> {
        let response: MetSearchResponse =
            serde_json::from_slice(bytes).map_err(|_| ProviderError::MalformedResponse)?;
        let offset = parse_offset(cursor)?;
        let object_ids = response.object_ids.unwrap_or_default();
        if offset >= object_ids.len() {
            return Ok(ProviderSearchPage {
                candidates: Vec::new(),
                next_cursor: None,
            });
        }
        let end = offset.saturating_add(PAGE_SIZE).min(object_ids.len());
        let candidates = object_ids[offset..end]
            .iter()
            .copied()
            .map(|object_id| ProviderCandidate {
                raw: serde_json::Value::from(object_id),
                context: None,
            })
            .collect();
        Ok(ProviderSearchPage {
            candidates,
            next_cursor: (end < object_ids.len()).then(|| end.to_string()),
        })
    }

    fn object_request(&self, candidate: &ProviderCandidate) -> Option<HttpRequest> {
        candidate
            .raw
            .as_u64()
            .filter(|object_id| *object_id > 0)
            .map(|object_id| {
                HttpRequest::get(object_endpoint(&self.endpoint, &object_id.to_string()))
            })
    }

    fn parse_artwork(
        &self,
        _candidate: &ProviderCandidate,
        object_bytes: Option<&[u8]>,
    ) -> Result<Artwork, ArtworkDropReason> {
        object_bytes
            .ok_or(ArtworkDropReason::MissingSourceId)
            .and_then(parse_object)
    }

    fn artwork_request(&self, key: &ArtworkKey) -> Result<HttpRequest, ProviderError> {
        Ok(HttpRequest::get(object_endpoint(
            &self.endpoint,
            key.id().as_str(),
        )))
    }

    fn parse_artwork_response(&self, bytes: &[u8]) -> Result<Artwork, ProviderError> {
        parse_object(bytes).map_err(drop_reason_to_provider_error)
    }

    fn display_image_request(
        &self,
        artwork: &Artwork,
        size: DisplayImageSize,
    ) -> Result<DisplayImageRequest, ProviderError> {
        let raw = match size {
            DisplayImageSize::Card => &artwork.image_urls.thumbnail,
            DisplayImageSize::Preview => &artwork.image_urls.display,
        };
        parse_image_request(raw)
            .map(|request| DisplayImageRequest::new(request, DisplayMediaType::Jpeg))
    }

    fn best_image_request(&self, artwork: &Artwork) -> Result<HttpRequest, ProviderError> {
        artwork
            .image_urls
            .original
            .as_deref()
            .ok_or(ProviderError::MissingImageService)
            .and_then(parse_image_request)
    }
}

#[derive(Debug, Deserialize)]
struct MetSearchResponse {
    #[serde(rename = "objectIDs")]
    object_ids: Option<Vec<u64>>,
}

#[derive(Debug, Deserialize)]
struct MetObject {
    #[serde(rename = "objectID")]
    object_id: Option<u64>,
    #[serde(rename = "isPublicDomain", default)]
    is_public_domain: bool,
    title: Option<String>,
    #[serde(rename = "artistDisplayName")]
    artist_display_name: Option<String>,
    #[serde(rename = "objectDate")]
    object_date: Option<String>,
    culture: Option<String>,
    country: Option<String>,
    region: Option<String>,
    subregion: Option<String>,
    locale: Option<String>,
    #[serde(rename = "primaryImage")]
    primary_image: Option<String>,
    #[serde(rename = "primaryImageSmall")]
    primary_image_small: Option<String>,
    #[serde(rename = "creditLine")]
    credit_line: Option<String>,
    #[serde(rename = "objectURL")]
    object_url: Option<String>,
}

fn parse_offset(cursor: Option<&str>) -> Result<usize, ProviderError> {
    match cursor {
        None => Ok(0),
        Some(raw) if !raw.is_empty() && raw.bytes().all(|byte| byte.is_ascii_digit()) => raw
            .parse::<usize>()
            .map_err(|_| ProviderError::MalformedResponse),
        Some(_) => Err(ProviderError::MalformedResponse),
    }
}

fn parse_object(bytes: &[u8]) -> Result<Artwork, ArtworkDropReason> {
    let raw: MetObject =
        serde_json::from_slice(bytes).map_err(|_| ArtworkDropReason::MissingSourceId)?;
    if !raw.is_public_domain {
        return Err(ArtworkDropReason::NotPublicDomain);
    }
    let source_id = raw
        .object_id
        .filter(|object_id| *object_id > 0)
        .map(|object_id| object_id.to_string())
        .ok_or(ArtworkDropReason::MissingSourceId)?;
    let title = nonempty(raw.title).ok_or(ArtworkDropReason::MissingTitle)?;
    let original = nonempty(raw.primary_image)
        .and_then(|value| canonical_trusted_url(&value))
        .ok_or(ArtworkDropReason::MissingImage)?;
    let display = nonempty(raw.primary_image_small)
        .and_then(|value| canonical_trusted_url(&value))
        .ok_or(ArtworkDropReason::MissingImage)?;
    let object_url = nonempty(raw.object_url)
        .and_then(|value| canonical_trusted_url(&value))
        .ok_or(ArtworkDropReason::MissingSourceId)?;
    Ok(Artwork {
        source: SourceKind::MetropolitanMuseum,
        source_id,
        title,
        creator: nonempty(raw.artist_display_name),
        date: nonempty(raw.object_date),
        culture: join_metadata([
            raw.culture,
            raw.country,
            raw.region,
            raw.subregion,
            raw.locale,
        ]),
        license: CommercialLicense::PublicDomain,
        image_urls: ImageUrls {
            thumbnail: display.clone(),
            display,
            original: Some(original),
        },
        institution: SourceKind::MetropolitanMuseum.label().into(),
        provider_credit: nonempty(raw.credit_line),
        object_url,
    })
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.trim().is_empty())
}

fn join_metadata(values: [Option<String>; 5]) -> Option<String> {
    let mut parts = Vec::new();
    for value in values.into_iter().filter_map(nonempty) {
        if !parts
            .iter()
            .any(|part: &String| part.eq_ignore_ascii_case(&value))
        {
            parts.push(value);
        }
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

#[must_use]
fn object_endpoint(search_endpoint: &Url, object_id: &str) -> Url {
    let mut detail = search_endpoint.clone();
    let search_path = detail.path().trim_end_matches('/');
    let collection_path = search_path.strip_suffix("/search").unwrap_or(search_path);
    detail.set_path(&format!("{collection_path}/objects/{object_id}"));
    detail.set_query(None);
    detail
}

fn parse_image_request(raw: &str) -> Result<HttpRequest, ProviderError> {
    trusted_remote_url(raw)
        .map(HttpRequest::get)
        .ok_or(ProviderError::InvalidImageRequest)
}

fn canonical_trusted_url(raw: &str) -> Option<String> {
    trusted_remote_url(raw).map(Into::into)
}

fn trusted_remote_url(raw: &str) -> Option<Url> {
    let url = Url::parse(raw).ok()?;
    let host = url.host()?;
    let allowed = match url.scheme() {
        "https" => true,
        "http" => match host {
            Host::Domain(domain) => domain == "localhost",
            Host::Ipv4(address) => address.is_loopback(),
            Host::Ipv6(address) => address.is_loopback(),
        },
        _ => false,
    };
    allowed.then_some(url)
}

const fn drop_reason_to_provider_error(reason: ArtworkDropReason) -> ProviderError {
    match reason {
        ArtworkDropReason::NotPublicDomain => ProviderError::ArtworkUnavailable,
        ArtworkDropReason::MissingSourceId
        | ArtworkDropReason::MissingTitle
        | ArtworkDropReason::MissingImage => ProviderError::MalformedResponse,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::Router;
    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use axum::routing::get;
    use img_parts::Bytes;
    use img_parts::jpeg::Jpeg;
    use tempfile::tempdir;
    use tokio::net::TcpListener;

    use crate::artwork::format_attribution;
    use crate::cache::Cache;
    use crate::core::{Culture, QueryText, SourceSet};
    use crate::download::{DownloadJob, Slug, Tags};
    use crate::http::HttpClient;
    use crate::providers::ProviderSet;
    use crate::rate_limit::RateLimiters;
    use crate::search::SearchServices;
    use crate::xmp::XMP_IDENTIFIER;

    use super::*;

    const SEARCH: &[u8] = include_bytes!("../../tests/fixtures/met/search.json");
    const AFRICAN: &[u8] = include_bytes!("../../tests/fixtures/met/african.json");
    const ASIAN: &[u8] = include_bytes!("../../tests/fixtures/met/asian.json");
    const NOT_PUBLIC_DOMAIN: &[u8] =
        include_bytes!("../../tests/fixtures/met/not-public-domain.json");
    const MISSING_IMAGE: &[u8] = include_bytes!("../../tests/fixtures/met/missing-image.json");

    fn provider() -> MetProvider {
        MetProvider::official().expect("official endpoint is valid")
    }

    fn query(culture: Option<&str>) -> SearchQuery {
        SearchQuery {
            query: QueryText::parse("ritual mask").expect("query is valid"),
            sources: SourceSet::parse(&[SourceKind::MetropolitanMuseum]).expect("source is valid"),
            culture: Culture::parse(culture.map(str::to_owned)),
        }
    }

    fn query_map(request: &HttpRequest) -> HashMap<String, String> {
        request.url().query_pairs().into_owned().collect()
    }

    #[test]
    fn provider_is_available_and_uses_the_documented_rate() {
        let provider = provider();
        assert!(matches!(provider.entry(), ProviderEntry::Available(_)));
        assert_eq!(
            provider.rate_policy(),
            RatePolicy::TokenBucket(TokenBucketPolicy::new(
                std::num::NonZeroU32::MIN,
                Duration::from_micros(12_500)
            ))
        );
    }

    #[test]
    fn search_request_has_query_image_policy_and_optional_region() {
        let first = query_map(&provider().search_request(&query(None), None));
        assert_eq!(first.get("q").map(String::as_str), Some("ritual mask"));
        assert_eq!(first.get("hasImages").map(String::as_str), Some("true"));
        assert!(!first.contains_key("geoLocation"));
        let regional = query_map(&provider().search_request(&query(Some("Africa")), Some("20")));
        assert_eq!(
            regional.get("geoLocation").map(String::as_str),
            Some("Africa")
        );
        assert!(!regional.contains_key("api_key"));
    }

    #[test]
    fn search_pages_object_ids_with_strict_offset_cursors() {
        let first = provider()
            .parse_search(SEARCH, None)
            .expect("page is valid");
        assert_eq!(first.candidates.len(), 20);
        assert_eq!(first.candidates[0].raw, serde_json::json!(1));
        assert_eq!(first.next_cursor.as_deref(), Some("20"));
        let second = provider()
            .parse_search(SEARCH, Some("20"))
            .expect("second page is valid");
        assert_eq!(second.candidates.len(), 2);
        assert_eq!(second.candidates[0].raw, serde_json::json!(21));
        assert_eq!(second.next_cursor, None);
    }

    #[test]
    fn search_handles_null_empty_end_and_invalid_cursors() {
        let empty = br#"{"total":0,"objectIDs":null}"#;
        assert!(
            provider()
                .parse_search(empty, None)
                .expect("empty page is valid")
                .candidates
                .is_empty()
        );
        assert!(
            provider()
                .parse_search(SEARCH, Some("22"))
                .expect("end page is valid")
                .candidates
                .is_empty()
        );
        for invalid in ["", "-1", "+1", "1.0", "184467440737095516160"] {
            assert_eq!(
                provider().parse_search(SEARCH, Some(invalid)),
                Err(ProviderError::MalformedResponse),
                "{invalid}"
            );
        }
    }

    #[test]
    fn object_request_uses_only_a_typed_positive_object_id() {
        let valid = ProviderCandidate {
            raw: serde_json::json!(45734),
            context: None,
        };
        assert_eq!(
            provider()
                .object_request(&valid)
                .expect("object request exists")
                .url()
                .path(),
            "/public/collection/v1/objects/45734"
        );
        for raw in [serde_json::json!(0), serde_json::json!("45734")] {
            assert_eq!(
                provider().object_request(&ProviderCandidate { raw, context: None }),
                None
            );
        }
    }

    #[test]
    fn african_and_asian_objects_normalize_exact_metadata() {
        let african = parse_object(AFRICAN).expect("African fixture is accepted");
        assert_eq!(african.source_id, "314001");
        assert_eq!(african.creator.as_deref(), Some("Bamana artist"));
        assert_eq!(african.date.as_deref(), Some("19th century"));
        assert_eq!(african.culture.as_deref(), Some("Bamana; Mali; Segou"));
        assert_eq!(
            african.provider_credit.as_deref(),
            Some("Gift of A & B, 1910")
        );
        assert_eq!(
            african.object_url,
            "https://www.metmuseum.org/art/collection/search/314001"
        );
        assert_eq!(
            parse_object(ASIAN)
                .expect("Asian fixture is accepted")
                .culture
                .as_deref(),
            Some("Japan; Kansai")
        );
    }

    #[test]
    fn object_policy_drops_non_public_and_missing_images() {
        assert_eq!(
            parse_object(NOT_PUBLIC_DOMAIN),
            Err(ArtworkDropReason::NotPublicDomain)
        );
        assert_eq!(
            parse_object(MISSING_IMAGE),
            Err(ArtworkDropReason::MissingImage)
        );
        assert_eq!(
            provider().parse_artwork(
                &ProviderCandidate {
                    raw: serde_json::json!(1),
                    context: None
                },
                None
            ),
            Err(ArtworkDropReason::MissingSourceId)
        );

        let base: serde_json::Value = serde_json::from_slice(AFRICAN).expect("fixture is JSON");
        let cases = [
            (
                "objectID",
                serde_json::Value::Null,
                ArtworkDropReason::MissingSourceId,
            ),
            (
                "title",
                serde_json::Value::String(" ".into()),
                ArtworkDropReason::MissingTitle,
            ),
            (
                "primaryImageSmall",
                serde_json::Value::Null,
                ArtworkDropReason::MissingImage,
            ),
            (
                "objectURL",
                serde_json::Value::Null,
                ArtworkDropReason::MissingSourceId,
            ),
        ];
        for (field, value, expected) in cases {
            let mut object = base.clone();
            object[field] = value;
            let bytes = serde_json::to_vec(&object).expect("object is JSON");
            assert_eq!(parse_object(&bytes), Err(expected), "{field}");
        }
        assert_eq!(
            parse_object(br#"{"objectID":1"#),
            Err(ArtworkDropReason::MissingSourceId)
        );
        assert_eq!(
            provider().parse_artwork_response(NOT_PUBLIC_DOMAIN),
            Err(ProviderError::ArtworkUnavailable)
        );

        for (field, expected) in [
            ("primaryImage", ArtworkDropReason::MissingImage),
            ("primaryImageSmall", ArtworkDropReason::MissingImage),
            ("objectURL", ArtworkDropReason::MissingSourceId),
        ] {
            let mut object = base.clone();
            object[field] = serde_json::Value::String("http://example.test/value".into());
            let bytes = serde_json::to_vec(&object).expect("object is JSON");
            assert_eq!(parse_object(&bytes), Err(expected), "{field}");
        }
    }

    #[test]
    fn detail_display_and_best_image_requests_are_provider_owned() {
        let provider = provider();
        let key = ArtworkKey::try_from_parts("met", "314001").expect("key is valid");
        assert_eq!(
            provider
                .artwork_request(&key)
                .expect("request is valid")
                .url()
                .path(),
            "/public/collection/v1/objects/314001"
        );
        let artwork = provider
            .parse_artwork_response(AFRICAN)
            .expect("detail is accepted");
        let card = provider
            .display_image_request(&artwork, DisplayImageSize::Card)
            .expect("card request is valid");
        let preview = provider
            .display_image_request(&artwork, DisplayImageSize::Preview)
            .expect("preview request is valid");
        assert_eq!(card.media_type(), DisplayMediaType::Jpeg);
        assert_eq!(card.request().canonical(), artwork.image_urls.thumbnail);
        assert_eq!(preview.request().canonical(), artwork.image_urls.display);
        assert_eq!(
            provider
                .best_image_request(&artwork)
                .expect("best request is valid")
                .canonical(),
            artwork
                .image_urls
                .original
                .as_deref()
                .expect("original exists")
        );
    }

    #[test]
    fn pure_helpers_cover_empty_duplicate_and_error_branches() {
        assert_eq!(parse_offset(None), Ok(0));
        assert_eq!(parse_offset(Some("0")), Ok(0));
        assert_eq!(nonempty(Some("".into())), None);
        assert_eq!(nonempty(Some(" Mali ".into())), Some(" Mali ".into()));
        assert_eq!(
            join_metadata([
                Some("Japan".into()),
                Some("japan".into()),
                None,
                Some("Kansai".into()),
                Some("".into())
            ]),
            Some("Japan; Kansai".into())
        );
        assert_eq!(
            drop_reason_to_provider_error(ArtworkDropReason::NotPublicDomain),
            ProviderError::ArtworkUnavailable
        );
        assert_eq!(
            drop_reason_to_provider_error(ArtworkDropReason::MissingTitle),
            ProviderError::MalformedResponse
        );
        assert_eq!(
            parse_image_request("not a URL"),
            Err(ProviderError::InvalidImageRequest)
        );
        for raw in [
            "relative/image.jpg",
            "javascript:alert(1)",
            "file:///tmp/image.jpg",
            "http://example.test/image.jpg",
        ] {
            assert_eq!(
                parse_image_request(raw),
                Err(ProviderError::InvalidImageRequest),
                "{raw}"
            );
        }
        for raw in [
            "https://example.test/image.jpg",
            "http://127.0.0.1:4000/image.jpg",
            "http://[::1]:4000/image.jpg",
            "http://localhost:4000/image.jpg",
        ] {
            assert!(parse_image_request(raw).is_ok(), "{raw}");
        }
        assert_eq!(
            canonical_trusted_url("https://EXAMPLE.test/a/../image.jpg"),
            Some("https://example.test/image.jpg".into())
        );
        assert_eq!(
            object_endpoint(
                &Url::parse("https://example.test/base").expect("base URL is valid"),
                "12"
            )
            .path(),
            "/base/objects/12"
        );
        assert_eq!(
            provider().parse_search(br#"{"objectIDs":"bad"}"#, None),
            Err(ProviderError::MalformedResponse)
        );
    }

    #[tokio::test]
    async fn object_responses_are_cached_and_one_failure_does_not_fail_the_page() {
        async fn search(State(requests): State<Arc<AtomicUsize>>) -> &'static str {
            requests.fetch_add(1, Ordering::SeqCst);
            r#"{"total":2,"objectIDs":[314001,314002]}"#
        }
        async fn object(
            State(requests): State<Arc<AtomicUsize>>,
            Path(id): Path<u64>,
        ) -> (StatusCode, &'static str) {
            requests.fetch_add(1, Ordering::SeqCst);
            if id == 314001 {
                (
                    StatusCode::OK,
                    include_str!("../../tests/fixtures/met/african.json"),
                )
            } else {
                (StatusCode::SERVICE_UNAVAILABLE, "object failed")
            }
        }

        let requests = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route("/search", get(search))
            .route("/objects/{id}", get(object))
            .with_state(requests.clone());
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock listener binds");
        let address = listener.local_addr().expect("mock address exists");
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("mock server runs");
        });
        let met_endpoint =
            Url::parse(&format!("http://{address}/search")).expect("mock URL is valid");
        let providers = ProviderSet::with_endpoints(
            crate::providers::aic::AicProvider::official_endpoint().expect("AIC endpoint is valid"),
            crate::providers::cleveland::ClevelandProvider::official_endpoint()
                .expect("Cleveland endpoint is valid"),
            met_endpoint,
            crate::providers::smithsonian::SmithsonianProvider::official_endpoint()
                .expect("Smithsonian endpoint is valid"),
            None,
        );
        let cache = tempdir().expect("temporary cache exists");
        let services = SearchServices::new(
            providers,
            Cache::new(cache.path().to_path_buf()),
            HttpClient::new(),
            RateLimiters::new(),
        );
        let query = query(None);

        for expected_title in ["Ceremonial mask", "Ceremonial mask"] {
            let outcomes = services.search_batch(&query).await;
            let [crate::core::ProviderOutcome::Success(page)] = outcomes.as_slice() else {
                panic!("Met page must succeed");
            };
            assert_eq!(page.artworks.len(), 1);
            assert_eq!(page.artworks[0].title, expected_title);
        }
        assert_eq!(requests.load(Ordering::SeqCst), 4);
        task.abort();
    }

    fn native_jpeg() -> Vec<u8> {
        include_str!("../../tests/fixtures/images/native-jpeg.hex")
            .trim()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let text = std::str::from_utf8(pair).expect("fixture is ASCII");
                u8::from_str_radix(text, 16).expect("fixture is hexadecimal")
            })
            .collect()
    }

    #[derive(Clone)]
    struct V4MockState {
        base: String,
        requests: Arc<AtomicUsize>,
    }

    #[tokio::test]
    async fn common_v4_paths_search_detail_display_and_download_with_xmp() {
        async fn search(State(state): State<V4MockState>) -> &'static str {
            state.requests.fetch_add(1, Ordering::SeqCst);
            r#"{"total":1,"objectIDs":[314001]}"#
        }
        async fn object(State(state): State<V4MockState>) -> String {
            state.requests.fetch_add(1, Ordering::SeqCst);
            serde_json::json!({
                "objectID":314001,
                "isPublicDomain":true,
                "title":"Ceremonial mask",
                "artistDisplayName":"Bamana artist",
                "objectDate":"19th century",
                "culture":"Bamana",
                "country":"Mali",
                "region":"Segou",
                "primaryImage":format!("{}/image", state.base),
                "primaryImageSmall":format!("{}/image?small=true", state.base),
                "creditLine":"Gift of A & B, 1910",
                "objectURL":"https://www.metmuseum.org/art/collection/search/314001"
            })
            .to_string()
        }
        async fn image(State(state): State<V4MockState>) -> Vec<u8> {
            state.requests.fetch_add(1, Ordering::SeqCst);
            native_jpeg()
        }

        let requests = Arc::new(AtomicUsize::new(0));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock listener binds");
        let address = listener.local_addr().expect("mock address exists");
        let base = format!("http://{address}");
        let app = Router::new()
            .route("/search", get(search))
            .route("/objects/314001", get(object))
            .route("/image", get(image))
            .with_state(V4MockState {
                base: base.clone(),
                requests: requests.clone(),
            });
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("mock server runs");
        });
        let providers = ProviderSet::with_endpoints(
            crate::providers::aic::AicProvider::official_endpoint().expect("AIC endpoint is valid"),
            crate::providers::cleveland::ClevelandProvider::official_endpoint()
                .expect("Cleveland endpoint is valid"),
            Url::parse(&format!("{base}/search")).expect("mock endpoint is valid"),
            crate::providers::smithsonian::SmithsonianProvider::official_endpoint()
                .expect("Smithsonian endpoint is valid"),
            None,
        );
        let temporary = tempdir().expect("temporary directory exists");
        let services = SearchServices::new(
            providers,
            Cache::new(temporary.path().join("cache")),
            HttpClient::new(),
            RateLimiters::new(),
        );
        let outcomes = services.search_batch(&query(Some("Africa"))).await;
        let [crate::core::ProviderOutcome::Success(page)] = outcomes.as_slice() else {
            panic!("Met search must succeed");
        };
        let artwork = page.artworks.first().expect("one artwork exists");
        let key =
            ArtworkKey::try_from_parts("met", &artwork.source_id).expect("artwork key is valid");
        let detail = services.load_artwork(&key).await.expect("detail loads");
        let display = services
            .load_display_image(&key, DisplayImageSize::Card)
            .await
            .expect("display image loads");
        assert_eq!(display.media_type, DisplayMediaType::Jpeg);
        let attribution = format_attribution(&detail);
        let slug = Slug::parse("met-mask").expect("slug is valid");
        let tags = Tags::parse("mask, africa");
        let saved = services
            .download(DownloadJob {
                artwork: &detail,
                attribution: &attribution,
                slug: &slug,
                tags: &tags,
                output: temporary.path(),
            })
            .await
            .expect("download succeeds");
        let written = std::fs::read(saved.path).expect("written JPEG is readable");
        let jpeg = Jpeg::from_bytes(Bytes::from(written)).expect("output is JPEG");
        let packet = jpeg
            .segments()
            .iter()
            .find(|segment| segment.contents().starts_with(XMP_IDENTIFIER))
            .expect("XMP exists");
        let xmp = String::from_utf8_lossy(&packet.contents()[XMP_IDENTIFIER.len()..]);
        assert!(xmp.contains("met"));
        assert!(xmp.contains("Gift of A &amp; B, 1910"));
        assert!(xmp.contains("africa"));
        assert_eq!(requests.load(Ordering::SeqCst), 4);
        task.abort();
    }
}
