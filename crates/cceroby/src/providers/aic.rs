//! Art Institute of Chicago request construction and response parsing.

use serde::Deserialize;
use url::Url;

use crate::core::{Artwork, CommercialLicense, ImageUrls, SearchQuery, SourceKind};

use super::{
    ArtworkDropReason, HttpRequest, Provider, ProviderCandidate, ProviderEntry, ProviderError,
    ProviderSearchPage, RatePolicy, TokenBucketPolicy,
};

const OFFICIAL_ENDPOINT: &str = "https://api.artic.edu/api/v1/artworks/search";
const SEARCH_FIELDS: &str =
    "id,title,artist_display,date_display,place_of_origin,image_id,is_public_domain,credit_line";

/// AIC provider configuration.
#[derive(Debug, Clone)]
pub struct AicProvider {
    endpoint: Url,
}

impl AicProvider {
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
    pub fn entry(&self) -> ProviderEntry<'_> {
        ProviderEntry::Available(self)
    }
}

impl Provider for AicProvider {
    fn kind(&self) -> SourceKind {
        SourceKind::ArtInstituteChicago
    }

    fn rate_policy(&self) -> RatePolicy {
        RatePolicy::TokenBucket(TokenBucketPolicy::new(
            std::num::NonZeroU32::MIN,
            std::time::Duration::from_secs(1),
        ))
    }

    fn search_request(&self, query: &SearchQuery, cursor: Option<&str>) -> HttpRequest {
        let page = cursor
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(1);
        let mut url = self.endpoint.clone();
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("q", query.query.as_str());
            pairs.append_pair("query[term][is_public_domain]", "true");
            if let Some(culture) = &query.culture {
                pairs.append_pair("query[term][place_of_origin]", culture.as_str());
            }
            pairs.append_pair("page", &page.to_string());
            pairs.append_pair("limit", "20");
            pairs.append_pair("fields", SEARCH_FIELDS);
        }
        HttpRequest::get(url)
    }

    fn parse_search(&self, bytes: &[u8]) -> Result<ProviderSearchPage, ProviderError> {
        let response: AicResponse =
            serde_json::from_slice(bytes).map_err(|_| ProviderError::MalformedResponse)?;
        let image_base = response
            .config
            .iiif_url
            .filter(|value| !value.trim().is_empty())
            .ok_or(ProviderError::MissingImageService)?;
        let candidates = response
            .data
            .into_iter()
            .map(|raw| ProviderCandidate {
                raw,
                context: Some(image_base.clone()),
            })
            .collect();

        Ok(ProviderSearchPage {
            candidates,
            next_cursor: next_cursor(
                response.pagination.current_page,
                response.pagination.total_pages,
            ),
        })
    }

    fn parse_artwork(
        &self,
        candidate: &ProviderCandidate,
        _object_bytes: Option<&[u8]>,
    ) -> Result<Artwork, ArtworkDropReason> {
        let image_base = candidate
            .context
            .as_deref()
            .ok_or(ArtworkDropReason::MissingImage)?;
        let raw: AicArtwork = serde_json::from_value(candidate.raw.clone())
            .map_err(|_| ArtworkDropReason::MissingSourceId)?;
        if !raw.is_public_domain {
            return Err(ArtworkDropReason::NotPublicDomain);
        }
        let source_id = raw
            .id
            .map(|value| value.to_string())
            .ok_or(ArtworkDropReason::MissingSourceId)?;
        let title = raw
            .title
            .filter(|value| !value.trim().is_empty())
            .ok_or(ArtworkDropReason::MissingTitle)?;
        let image_id = raw
            .image_id
            .filter(|value| !value.trim().is_empty())
            .ok_or(ArtworkDropReason::MissingImage)?;
        let license = CommercialLicense::try_from("Public Domain")
            .map_err(|_| ArtworkDropReason::NotPublicDomain)?;

        Ok(Artwork {
            source: self.kind(),
            source_id: source_id.clone(),
            title,
            creator: raw.artist_display.filter(|value| !value.trim().is_empty()),
            date: raw.date_display.filter(|value| !value.trim().is_empty()),
            culture: raw.place_of_origin.filter(|value| !value.trim().is_empty()),
            license,
            image_urls: ImageUrls {
                thumbnail: build_iiif_url(image_base, &image_id, "200,"),
                display: build_iiif_url(image_base, &image_id, "843,"),
                original: Some(build_iiif_url(image_base, &image_id, "full")),
            },
            institution: SourceKind::ArtInstituteChicago.label().into(),
            provider_credit: raw.credit_line.filter(|value| !value.trim().is_empty()),
            object_url: format!("https://www.artic.edu/artworks/{source_id}"),
        })
    }
}

#[derive(Debug, Deserialize)]
struct AicResponse {
    pagination: AicPagination,
    data: Vec<serde_json::Value>,
    config: AicConfig,
}

#[derive(Debug, Deserialize)]
struct AicPagination {
    current_page: u32,
    total_pages: u32,
}

#[derive(Debug, Deserialize)]
struct AicConfig {
    iiif_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AicArtwork {
    id: Option<i64>,
    title: Option<String>,
    artist_display: Option<String>,
    date_display: Option<String>,
    place_of_origin: Option<String>,
    image_id: Option<String>,
    #[serde(default)]
    is_public_domain: bool,
    credit_line: Option<String>,
}

#[must_use]
fn next_cursor(current_page: u32, total_pages: u32) -> Option<String> {
    (current_page < total_pages).then(|| (current_page + 1).to_string())
}

#[must_use]
pub fn build_iiif_url(image_base: &str, image_id: &str, width: &str) -> String {
    format!(
        "{}/{image_id}/full/{width}/0/default.jpg",
        image_base.trim_end_matches('/')
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::collections::HashMap;

    use crate::core::{Culture, QueryText, SourceSet};

    use super::*;

    const SUCCESS: &[u8] = include_bytes!("../../tests/fixtures/aic/success.json");
    const MISSING_FIELDS: &[u8] = include_bytes!("../../tests/fixtures/aic/missing-fields.json");
    const MALFORMED: &[u8] = include_bytes!("../../tests/fixtures/aic/malformed.json");
    const MISSING_IMAGE_SERVICE: &[u8] =
        include_bytes!("../../tests/fixtures/aic/missing-image-service.json");
    const NON_PUBLIC_DOMAIN: &[u8] =
        include_bytes!("../../tests/fixtures/aic/non-public-domain.json");

    fn query(culture: Option<&str>) -> SearchQuery {
        SearchQuery {
            query: QueryText::parse("ritual mask").expect("query is valid"),
            sources: SourceSet::parse(&[SourceKind::ArtInstituteChicago]).expect("source is valid"),
            culture: Culture::parse(culture.map(str::to_owned)),
        }
    }

    fn provider() -> AicProvider {
        AicProvider::official().expect("built-in endpoint is valid")
    }

    fn query_map(request: &HttpRequest) -> HashMap<String, String> {
        request.url().query_pairs().into_owned().collect()
    }

    #[test]
    fn request_contains_query_policy_fields_and_first_page() {
        let provider = provider();
        let request = provider.search_request(&query(None), None);
        assert_eq!(
            provider.rate_policy(),
            RatePolicy::TokenBucket(TokenBucketPolicy::new(
                std::num::NonZeroU32::MIN,
                std::time::Duration::from_secs(1)
            ))
        );
        let pairs = query_map(&request);
        assert_eq!(pairs.get("q").map(String::as_str), Some("ritual mask"));
        assert_eq!(
            pairs
                .get("query[term][is_public_domain]")
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(pairs.get("page").map(String::as_str), Some("1"));
        assert_eq!(pairs.get("limit").map(String::as_str), Some("20"));
        let fields = pairs.get("fields").expect("fields exist");
        for field in [
            "id",
            "title",
            "artist_display",
            "date_display",
            "place_of_origin",
            "image_id",
            "is_public_domain",
            "credit_line",
        ] {
            assert!(fields.split(',').any(|candidate| candidate == field));
        }
        assert!(!pairs.contains_key("query[term][place_of_origin]"));
    }

    #[test]
    fn request_contains_culture_and_explicit_page() {
        let request = provider().search_request(&query(Some("Japan")), Some("3"));
        let pairs = query_map(&request);
        assert_eq!(
            pairs
                .get("query[term][place_of_origin]")
                .map(String::as_str),
            Some("Japan")
        );
        assert_eq!(pairs.get("page").map(String::as_str), Some("3"));
    }

    #[test]
    fn invalid_cursor_returns_to_the_first_page() {
        let request = provider().search_request(&query(None), Some("not-a-page"));
        assert_eq!(
            query_map(&request).get("page").map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn iiif_builder_supports_thumbnail_display_and_full_sizes() {
        let base = "https://www.artic.edu/iiif/2/";
        assert_eq!(
            build_iiif_url(base, "image-one", "200,"),
            "https://www.artic.edu/iiif/2/image-one/full/200,/0/default.jpg"
        );
        assert_eq!(
            build_iiif_url(base, "image-one", "843,"),
            "https://www.artic.edu/iiif/2/image-one/full/843,/0/default.jpg"
        );
        assert_eq!(
            build_iiif_url(base, "image-one", "full"),
            "https://www.artic.edu/iiif/2/image-one/full/full/0/default.jpg"
        );
    }

    #[test]
    fn pagination_returns_only_a_real_next_page() {
        assert_eq!(next_cursor(1, 3), Some("2".into()));
        assert_eq!(next_cursor(3, 3), None);
        assert_eq!(next_cursor(4, 3), None);
    }

    #[test]
    fn success_fixture_parses_normalized_public_domain_artwork() {
        let provider = provider();
        let page = provider.parse_search(SUCCESS).expect("fixture is valid");
        assert_eq!(page.next_cursor, Some("2".into()));
        assert_eq!(page.candidates.len(), 3);
        let first = provider
            .parse_artwork(&page.candidates[0], None)
            .expect("candidate is valid");
        assert_eq!(first.source_id, "1001");
        assert_eq!(first.title, "Ceremonial Mask");
        assert_eq!(first.creator.as_deref(), Some("Maker unknown"));
        assert_eq!(first.date.as_deref(), Some("1900-1920"));
        assert_eq!(first.culture.as_deref(), Some("Côte d’Ivoire"));
        assert_eq!(first.license, CommercialLicense::PublicDomain);
        assert!(first.image_urls.display.contains("/full/843,/"));
        assert_eq!(first.object_url, "https://www.artic.edu/artworks/1001");
        assert_eq!(
            page.candidates
                .iter()
                .filter_map(|candidate| provider.parse_artwork(candidate, None).ok())
                .count(),
            2
        );
    }

    #[test]
    fn missing_required_fields_are_typed_drops() {
        let response: AicResponse =
            serde_json::from_slice(MISSING_FIELDS).expect("fixture is valid JSON");
        let provider = provider();
        let candidate = |raw| ProviderCandidate {
            raw,
            context: Some("https://www.artic.edu/iiif/2".into()),
        };
        assert_eq!(
            provider.parse_artwork(&candidate(response.data[0].clone()), None),
            Err(ArtworkDropReason::MissingTitle)
        );
        assert_eq!(
            provider.parse_artwork(&candidate(response.data[1].clone()), None),
            Err(ArtworkDropReason::MissingImage)
        );
        assert_eq!(
            provider.parse_artwork(
                &candidate(serde_json::json!({
                    "title": "Mask",
                    "image_id": "image-one",
                    "is_public_domain": true
                })),
                None
            ),
            Err(ArtworkDropReason::MissingSourceId)
        );
        assert_eq!(
            provider.parse_artwork(
                &ProviderCandidate {
                    raw: serde_json::json!({
                        "id": 1,
                        "title": "Mask",
                        "image_id": "image-one",
                        "is_public_domain": true
                    }),
                    context: None
                },
                None
            ),
            Err(ArtworkDropReason::MissingImage)
        );
    }

    #[test]
    fn malformed_json_returns_a_typed_provider_error() {
        assert_eq!(
            provider().parse_search(MALFORMED),
            Err(ProviderError::MalformedResponse)
        );
    }

    #[test]
    fn missing_image_service_returns_a_typed_provider_error() {
        assert_eq!(
            provider().parse_search(MISSING_IMAGE_SERVICE),
            Err(ProviderError::MissingImageService)
        );
    }

    #[test]
    fn non_public_domain_record_is_rejected_even_after_query_filtering() {
        let response: AicResponse =
            serde_json::from_slice(NON_PUBLIC_DOMAIN).expect("fixture is valid JSON");
        let provider = provider();
        let candidate = ProviderCandidate {
            raw: response.data[0].clone(),
            context: Some("https://www.artic.edu/iiif/2".into()),
        };
        assert_eq!(
            provider.parse_artwork(&candidate, None),
            Err(ArtworkDropReason::NotPublicDomain)
        );
    }
}
