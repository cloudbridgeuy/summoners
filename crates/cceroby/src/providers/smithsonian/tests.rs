#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use img_parts::Bytes;
use img_parts::jpeg::Jpeg;
use reqwest::header::USER_AGENT;
use tempfile::tempdir;
use tokio::net::TcpListener;

use crate::artwork::format_attribution;
use crate::cache::Cache;
use crate::core::{
    Culture, ProviderOutcome, QueryText, SearchQuery, SearchSession, SourceSet, merge_page,
};
use crate::download::{DownloadJob, Slug, Tags};
use crate::http::HttpClient;
use crate::providers::ProviderSet;
use crate::rate_limit::RateLimiters;
use crate::render::render_search_page;
use crate::search::SearchServices;
use crate::xmp::XMP_IDENTIFIER;

use super::*;

const SEARCH: &[u8] = include_bytes!("../../../tests/fixtures/smithsonian/search.json");
const DETAIL: &[u8] = include_bytes!("../../../tests/fixtures/smithsonian/detail.json");
const MISSING_MEDIA: &[u8] =
    include_bytes!("../../../tests/fixtures/smithsonian/missing-media.json");
const MALFORMED: &[u8] = include_bytes!("../../../tests/fixtures/smithsonian/malformed.json");
const AIC_SEARCH: &[u8] = include_bytes!("../../../tests/fixtures/aic/success.json");
const SECRET: &str = "smithsonian-test-secret-7d2a";

fn provider(key: Option<&str>) -> SmithsonianProvider {
    SmithsonianProvider::new(
        SmithsonianProvider::official_endpoint().expect("official endpoint is valid"),
        key.map(str::to_owned),
    )
}

fn query(culture: Option<&str>) -> SearchQuery {
    SearchQuery {
        query: QueryText::parse("ritual mask").expect("query is valid"),
        sources: SourceSet::parse(&[SourceKind::Smithsonian]).expect("source is valid"),
        culture: Culture::parse(culture.map(str::to_owned)),
    }
}

fn query_map(request: &HttpRequest) -> HashMap<String, String> {
    request.url().query_pairs().into_owned().collect()
}

#[test]
fn key_controls_only_smithsonian_availability() {
    for unavailable in [
        provider(None),
        provider(Some("")),
        provider(Some("bad\nkey")),
    ] {
        assert_eq!(
            unavailable.entry().unavailable_notice(),
            Some(crate::core::ProviderNotice::Unavailable {
                source: SourceKind::Smithsonian
            })
        );
    }
    assert!(matches!(
        provider(Some(SECRET)).entry(),
        ProviderEntry::Available(_)
    ));
}

#[test]
fn provider_and_request_diagnostics_redact_the_key() {
    let provider = provider(Some(SECRET));
    let request = provider.search_request(&query(None), None);
    assert!(
        request
            .headers()
            .get(X_API_KEY)
            .is_some_and(HeaderValue::is_sensitive)
    );
    assert_eq!(
        request.headers().get(USER_AGENT),
        Some(&SMITHSONIAN_USER_AGENT)
    );
    assert!(!request.canonical().contains(SECRET));
    assert!(!format!("{request:?}").contains(SECRET));
    assert!(!format!("{provider:?}").contains(SECRET));
    for error in [
        ProviderError::MalformedResponse,
        ProviderError::ArtworkUnavailable,
        ProviderError::InvalidImageRequest,
    ] {
        assert!(!error.to_string().contains(SECRET));
    }
}

#[test]
fn metadata_cache_filename_does_not_contain_the_key() {
    let directory = tempdir().expect("temporary directory exists");
    let cache = Cache::new(directory.path().to_path_buf());
    let request = provider(Some(SECRET)).search_request(&query(None), None);
    assert!(cache.write_metadata(
        SourceKind::Smithsonian,
        request.canonical(),
        SEARCH,
        SystemTime::now()
    ));
    let metadata = directory.path().join("meta/smithsonian");
    let names = std::fs::read_dir(metadata)
        .expect("cache directory exists")
        .map(|entry| {
            entry
                .expect("cache entry is readable")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 1);
    assert!(!names[0].contains(SECRET));
}

#[test]
fn search_request_maps_all_regions_and_paging() {
    let cases = [
        (
            "Africa",
            vec!["online_media_type:Images", "unit_code:NMAfA"],
        ),
        (
            "Asian",
            vec!["online_media_type:Images", "unit_code:FSG OR unit_code:FSA"],
        ),
        (
            "pre-Columbian",
            vec!["online_media_type:Images", "unit_code:NMAI"],
        ),
    ];
    for (culture, expected_filters) in cases {
        let request = provider(Some(SECRET)).search_request(&query(Some(culture)), Some("40"));
        let pairs = query_map(&request);
        assert_eq!(pairs.get("q").map(String::as_str), Some("ritual mask"));
        assert_eq!(pairs.get("start").map(String::as_str), Some("40"));
        assert_eq!(pairs.get("rows").map(String::as_str), Some("20"));
        assert_eq!(pairs.get("type").map(String::as_str), Some("edanmdm"));
        assert_eq!(pairs.get("row_group").map(String::as_str), Some("objects"));
        let filters: Vec<String> =
            serde_json::from_str(pairs.get("fqs").expect("filter query exists"))
                .expect("filter query is JSON");
        assert_eq!(filters, expected_filters);
        assert!(!pairs.contains_key("api_key"));
    }
}

#[test]
fn unknown_culture_uses_a_field_filter_with_escaped_text() {
    let request = provider(Some(SECRET)).search_request(&query(Some("A \\\"B")), None);
    let filters: Vec<String> =
        serde_json::from_str(query_map(&request).get("fqs").expect("filters exist"))
            .expect("filters are JSON");
    assert_eq!(
        filters,
        vec!["online_media_type:Images", "culture:\"A \\\\\\\"B\""]
    );
}

#[test]
fn search_parser_returns_rows_and_a_strict_next_offset() {
    let page = provider(Some(SECRET))
        .parse_search(SEARCH, Some("0"))
        .expect("search response is valid");
    assert_eq!(page.candidates.len(), 2);
    assert_eq!(page.next_cursor.as_deref(), Some("2"));
    assert_eq!(
        page.candidates[0]
            .raw
            .get("url")
            .and_then(|value| value.as_str()),
        Some("edanmdm:nmafa_2005-6-189")
    );
    for invalid in ["", "-1", "+1", "1.5", "184467440737095516160"] {
        assert_eq!(
            provider(Some(SECRET)).parse_search(SEARCH, Some(invalid)),
            Err(ProviderError::MalformedResponse),
            "{invalid}"
        );
    }
    assert_eq!(
        provider(Some(SECRET)).parse_search(MALFORMED, None),
        Err(ProviderError::MalformedResponse)
    );
    let empty = br#"{"response":{"rowCount":2,"rows":[]}}"#;
    let end = provider(Some(SECRET))
        .parse_search(empty, Some("2"))
        .expect("empty end page is valid");
    assert!(end.candidates.is_empty());
    assert_eq!(end.next_cursor, None);
}

#[test]
fn mixed_media_selects_only_the_cc0_image_and_normalizes_metadata() {
    let page = provider(Some(SECRET))
        .parse_search(SEARCH, None)
        .expect("search response is valid");
    let artwork = provider(Some(SECRET))
        .parse_artwork(&page.candidates[0], None)
        .expect("record is accepted");
    assert_eq!(artwork.source_id, "edanmdm:nmafa_2005-6-189");
    assert_eq!(artwork.title, "Face mask");
    assert_eq!(artwork.creator.as_deref(), Some("Yoruba artist"));
    assert_eq!(artwork.date.as_deref(), Some("early 20th century"));
    assert_eq!(artwork.culture.as_deref(), Some("Yoruba; Nigeria"));
    assert_eq!(artwork.institution, "National Museum of African Art");
    assert_eq!(artwork.provider_credit.as_deref(), Some("Gift of A & B"));
    assert_eq!(
        artwork.object_url,
        "https://www.si.edu/object/face-mask:nmafa_2005-6-189"
    );
    assert_eq!(artwork.license, CommercialLicense::Cc0);
    assert!(artwork.image_urls.thumbnail.contains("NMAfA-2005-6-189"));
    assert!(!artwork.image_urls.thumbnail.contains("RESTRICTED"));
    assert_eq!(
        artwork.image_urls.original.as_deref(),
        Some("https://ids.si.edu/ids/deliveryService?id=NMAfA-2005-6-189")
    );
}

#[test]
fn dotted_official_id_survives_fixture_search_and_normalization() {
    let mut fixture: serde_json::Value =
        serde_json::from_slice(SEARCH).expect("search fixture is JSON");
    let record = &mut fixture["response"]["rows"][0];
    record["url"] = serde_json::Value::String("edanmdm:fsg_F1900.1".into());
    record["content"]["descriptiveNonRepeating"]["record_ID"] =
        serde_json::Value::String("fsg_F1900.1".into());
    let bytes = serde_json::to_vec(&fixture).expect("changed fixture is JSON");

    let page = provider(Some(SECRET))
        .parse_search(&bytes, None)
        .expect("search fixture is valid");
    let artwork = provider(Some(SECRET))
        .parse_artwork(&page.candidates[0], None)
        .expect("dotted official ID is accepted");
    assert_eq!(artwork.source_id, "edanmdm:fsg_F1900.1");
}

#[test]
fn missing_or_restricted_media_drops_the_record() {
    let raw: serde_json::Value = serde_json::from_slice(MISSING_MEDIA).expect("fixture is JSON");
    let candidate = ProviderCandidate { raw, context: None };
    assert_eq!(
        provider(Some(SECRET)).parse_artwork(&candidate, None),
        Err(ArtworkDropReason::MissingImage)
    );
}

#[test]
fn media_selection_requires_exact_image_type_and_cc0_access() {
    fn media(media_type: Option<&str>, access: Option<&str>, id: &str) -> Media {
        Media {
            media_type: media_type.map(str::to_owned),
            content: Some(format!("https://ids.si.edu/ids/deliveryService?id={id}")),
            thumbnail: Some(format!(
                "https://ids.si.edu/ids/deliveryService?id={id}&max=150"
            )),
            usage: access.map(|value| MediaUsage {
                access: Some(value.to_owned()),
            }),
        }
    }

    let rejected = [
        Media {
            media_type: None,
            content: Some("https://ids.si.edu/missing-type".into()),
            thumbnail: Some("https://ids.si.edu/missing-type-thumb".into()),
            usage: Some(MediaUsage {
                access: Some("CC0".into()),
            }),
        },
        Media {
            media_type: Some("Images".into()),
            content: Some("https://ids.si.edu/missing-usage".into()),
            thumbnail: Some("https://ids.si.edu/missing-usage-thumb".into()),
            usage: None,
        },
        Media {
            media_type: Some("Images".into()),
            content: Some("https://ids.si.edu/missing-access".into()),
            thumbnail: Some("https://ids.si.edu/missing-access-thumb".into()),
            usage: Some(MediaUsage { access: None }),
        },
        media(Some("images"), Some("CC0"), "wrong-type-case"),
        media(Some("Images"), Some("cc0"), "wrong-access-case"),
        media(Some("Images"), Some(" CC0 "), "access-whitespace"),
        media(Some("Images"), Some("Usage conditions apply"), "restricted"),
        media(Some("Video"), Some("CC0"), "video"),
    ];
    for item in &rejected {
        assert!(select_image(std::slice::from_ref(item)).is_none());
    }

    let mut mixed = rejected.into_iter().collect::<Vec<_>>();
    mixed.push(media(Some("Images"), Some("CC0"), "valid"));
    assert_eq!(
        select_image(&mixed).map(|image| image.original),
        Some("https://ids.si.edu/ids/deliveryService?id=valid".into())
    );
}

#[test]
fn record_fallbacks_and_required_fields_are_directly_checked() {
    let page = provider(Some(SECRET))
        .parse_search(SEARCH, None)
        .expect("search response is valid");
    let base = page.candidates[0].raw.clone();

    let mut fallback = base.clone();
    fallback["url"] = serde_json::Value::Null;
    fallback["title"] = serde_json::Value::Null;
    fallback["content"]["descriptiveNonRepeating"]["data_source"] = serde_json::Value::Null;
    let artwork = provider(Some(SECRET))
        .parse_artwork(
            &ProviderCandidate {
                raw: fallback,
                context: None,
            },
            None,
        )
        .expect("record fallbacks are accepted");
    assert_eq!(artwork.source_id, "edanmdm:nmafa_2005-6-189");
    assert_eq!(artwork.title, "Face mask");
    assert_eq!(artwork.institution, SourceKind::Smithsonian.label());

    let cases = [
        (
            ["url", "content.descriptiveNonRepeating.record_ID"],
            ArtworkDropReason::MissingSourceId,
        ),
        (
            ["title", "content.descriptiveNonRepeating.title"],
            ArtworkDropReason::MissingTitle,
        ),
        (
            [
                "content.descriptiveNonRepeating.record_link",
                "content.descriptiveNonRepeating.record_link",
            ],
            ArtworkDropReason::MissingSourceId,
        ),
    ];
    for (paths, expected) in cases {
        let mut raw = base.clone();
        for path in paths {
            let mut current = &mut raw;
            for part in path.split('.') {
                current = &mut current[part];
            }
            *current = serde_json::Value::Null;
        }
        assert_eq!(
            provider(Some(SECRET)).parse_artwork(&ProviderCandidate { raw, context: None }, None),
            Err(expected)
        );
    }
    assert_eq!(
        provider(Some(SECRET)).parse_artwork_response(MALFORMED),
        Err(ProviderError::MalformedResponse)
    );
    let mut public_http_object = base;
    public_http_object["content"]["descriptiveNonRepeating"]["record_link"] =
        serde_json::Value::String("http://example.test/object".into());
    assert_eq!(
        provider(Some(SECRET)).parse_artwork(
            &ProviderCandidate {
                raw: public_http_object,
                context: None
            },
            None
        ),
        Err(ArtworkDropReason::MissingSourceId)
    );

    let mut invalid_id: serde_json::Value =
        serde_json::from_slice(SEARCH).expect("search fixture is JSON");
    let invalid_record = &mut invalid_id["response"]["rows"][0];
    invalid_record["url"] = serde_json::Value::String("arbitrary".into());
    invalid_record["content"]["descriptiveNonRepeating"]["record_ID"] =
        serde_json::Value::String("also.invalid".into());
    assert_eq!(
        provider(Some(SECRET)).parse_artwork(
            &ProviderCandidate {
                raw: invalid_record.clone(),
                context: None,
            },
            None,
        ),
        Err(ArtworkDropReason::MissingSourceId)
    );
}

#[test]
fn detail_and_image_requests_keep_provider_owned_headers_and_urls() {
    let provider = provider(Some(SECRET));
    let key =
        ArtworkKey::try_from_parts("smithsonian", "edanmdm:fsg_F1900.1").expect("key is valid");
    let detail_request = provider.artwork_request(&key).expect("request is valid");
    assert_eq!(
        detail_request.url().path(),
        "/openaccess/api/v1.0/content/edanmdm:fsg_F1900.1"
    );
    assert!(
        detail_request
            .headers()
            .get(X_API_KEY)
            .is_some_and(HeaderValue::is_sensitive)
    );
    let artwork = provider
        .parse_artwork_response(DETAIL)
        .expect("detail is accepted");
    let card = provider
        .display_image_request(&artwork, DisplayImageSize::Card)
        .expect("card image is valid");
    let preview = provider
        .display_image_request(&artwork, DisplayImageSize::Preview)
        .expect("preview image is valid");
    assert_eq!(card.media_type(), DisplayMediaType::Jpeg);
    assert_eq!(card.request().headers().get(X_API_KEY), None);
    assert!(card.request().canonical().contains("max=300"));
    assert!(preview.request().canonical().contains("max=1200"));
    assert_eq!(
        provider
            .best_image_request(&artwork)
            .expect("best image is valid")
            .canonical(),
        artwork
            .image_urls
            .original
            .as_deref()
            .expect("original exists")
    );
}

#[tokio::test]
async fn missing_key_does_not_stop_another_source_search_or_expose_secret_text() {
    async fn aic_search() -> &'static [u8] {
        AIC_SEARCH
    }

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock listener binds");
    let address = listener.local_addr().expect("mock address exists");
    let app = Router::new().route("/search", get(aic_search));
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock server runs");
    });
    let query = SearchQuery {
        query: QueryText::parse("mask").expect("query is valid"),
        sources: SourceSet::parse(&[SourceKind::ArtInstituteChicago, SourceKind::Smithsonian])
            .expect("sources are valid"),
        culture: None,
    };
    let temporary = tempdir().expect("temporary directory exists");
    let providers = ProviderSet::with_endpoints(
        Url::parse(&format!("http://{address}/search")).expect("mock AIC endpoint is valid"),
        crate::providers::cleveland::ClevelandProvider::official_endpoint()
            .expect("Cleveland endpoint is valid"),
        crate::providers::met::MetProvider::official_endpoint().expect("Met endpoint is valid"),
        SmithsonianProvider::official_endpoint().expect("Smithsonian endpoint is valid"),
        None,
    );
    let services = SearchServices::new(
        providers,
        Cache::new(temporary.path().join("cache")),
        HttpClient::new(),
        RateLimiters::new(),
    );
    let outcomes = services.search_batch(&query).await;
    assert!(matches!(outcomes[0], ProviderOutcome::Success(_)));
    assert_eq!(
        outcomes[1],
        ProviderOutcome::Unavailable {
            source: SourceKind::Smithsonian
        }
    );
    let mut session = SearchSession::new(query);
    for outcome in outcomes {
        merge_page(&mut session, outcome);
    }
    let html = render_search_page(session.view());
    assert!(html.contains("Smithsonian</strong> is not available"));
    assert!(html.contains("Ceremonial Mask"));
    assert!(!html.contains(API_KEY_ENV));
    assert!(!html.contains(SECRET));
    task.abort();
}

#[tokio::test]
async fn syntactically_valid_rejected_key_fails_after_one_request() {
    async fn reject(State(requests): State<Arc<AtomicUsize>>) -> StatusCode {
        requests.fetch_add(1, Ordering::SeqCst);
        StatusCode::FORBIDDEN
    }

    let requests = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock listener binds");
    let address = listener.local_addr().expect("mock address exists");
    let app = Router::new()
        .route("/search", get(reject))
        .with_state(requests.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock server runs");
    });
    let temporary = tempdir().expect("temporary directory exists");
    let providers = ProviderSet::with_endpoints(
        crate::providers::aic::AicProvider::official_endpoint().expect("AIC endpoint is valid"),
        crate::providers::cleveland::ClevelandProvider::official_endpoint()
            .expect("Cleveland endpoint is valid"),
        crate::providers::met::MetProvider::official_endpoint().expect("Met endpoint is valid"),
        Url::parse(&format!("http://{address}/search")).expect("mock endpoint is valid"),
        Some(SECRET.into()),
    );
    let services = SearchServices::new(
        providers,
        Cache::new(temporary.path().join("cache")),
        HttpClient::new(),
        RateLimiters::new(),
    );

    assert_eq!(
        services.search_batch(&query(None)).await,
        vec![ProviderOutcome::Failed {
            source: SourceKind::Smithsonian
        }]
    );
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    task.abort();
}

#[test]
fn pure_helpers_cover_region_media_url_and_error_branches() {
    assert_eq!(regional_unit_codes(None), &[] as &[&str]);
    assert_eq!(regional_unit_codes(Some(" AFRICAN ")), &["NMAfA"]);
    assert_eq!(regional_unit_codes(Some("Asia")), &["FSG", "FSA"]);
    assert_eq!(regional_unit_codes(Some("precolumbian")), &["NMAI"]);
    assert_eq!(regional_unit_codes(Some("Europe")), &[] as &[&str]);
    assert_eq!(parse_offset(None), Ok(0));
    assert_eq!(parse_offset(Some("0")), Ok(0));
    assert!(parse_api_key("  ".into()).is_none());
    assert!(parse_api_key("bad\nkey".into()).is_none());
    for raw in [
        "relative/image.jpg",
        "javascript:alert(1)",
        "file:///tmp/image.jpg",
        "http://example.test/image.jpg",
    ] {
        assert_eq!(https_url(raw), None, "{raw}");
    }
    for raw in [
        "https://example.test/image.jpg",
        "http://127.0.0.1:4000/image.jpg",
        "http://[::1]:4000/image.jpg",
        "http://localhost:4000/image.jpg",
    ] {
        assert!(https_url(raw).is_some(), "{raw}");
    }
    assert_eq!(
        https_url("https://EXAMPLE.test/a/../image.jpg"),
        Some("https://example.test/image.jpg".into())
    );
    assert_eq!(resized_image_url("not a URL", 300), "not a URL");
    assert_eq!(
        resized_image_url("https://example.test/image.jpg?max=100", 300),
        "https://example.test/image.jpg?max=100"
    );
    assert_eq!(
        join_metadata(["Japan".into(), "japan".into(), "Kansai".into()]),
        Some("Japan; Kansai".into())
    );
    assert_eq!(first_text(&[]), None);
    assert_eq!(nonempty(Some("".into())), None);
    assert_eq!(
        public_image_request("http://example.test/image.jpg"),
        Err(ProviderError::InvalidImageRequest)
    );
    assert_eq!(
        drop_reason_to_provider_error(ArtworkDropReason::MissingImage),
        ProviderError::MalformedResponse
    );
    assert_eq!(
        content_endpoint(
            &Url::parse("https://example.test/base").expect("base URL is valid"),
            "object:1"
        )
        .path(),
        "/base/content/object:1"
    );
    assert_eq!(
        smithsonian_api_request(
            Url::parse("https://example.test/search").expect("search URL is valid"),
            None
        )
        .headers()
        .get(X_API_KEY),
        None
    );
}

fn native_jpeg() -> Vec<u8> {
    include_str!("../../../tests/fixtures/images/native-jpeg.hex")
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
struct MockState {
    base: String,
    search_requests: Arc<AtomicUsize>,
    detail_requests: Arc<AtomicUsize>,
    image_requests: Arc<AtomicUsize>,
}

fn mock_record(base: &str) -> serde_json::Value {
    serde_json::json!({
        "url": "edanmdm:fsg_F1900.1",
        "title": "Face mask",
        "content": {
            "freetext": {
                "name": [{"content":"Yoruba artist"}],
                "date": [{"content":"early 20th century"}],
                "creditLine": [{"content":"Gift of A & B"}]
            },
            "indexedStructured": {"culture":["Yoruba"],"place":["Nigeria"]},
            "descriptiveNonRepeating": {
                "record_ID":"fsg_F1900.1",
                "data_source":"National Museum of African Art",
                "record_link":"https://www.si.edu/object/face-mask:nmafa_2005-6-189",
                "online_media":{"media":[{
                    "type":"Images",
                    "content":format!("{base}/image"),
                    "thumbnail":format!("{base}/image?max=150"),
                    "usage":{"access":"CC0"}
                }]}
            }
        }
    })
}

#[tokio::test]
async fn common_v4_paths_search_detail_display_and_download_with_xmp() {
    async fn search(State(state): State<MockState>) -> String {
        state.search_requests.fetch_add(1, Ordering::SeqCst);
        serde_json::json!({
            "response":{"rowCount":1,"rows":[mock_record(&state.base)]}
        })
        .to_string()
    }
    async fn detail(State(state): State<MockState>) -> String {
        state.detail_requests.fetch_add(1, Ordering::SeqCst);
        serde_json::json!({"response":mock_record(&state.base)}).to_string()
    }
    async fn image(State(state): State<MockState>) -> Vec<u8> {
        state.image_requests.fetch_add(1, Ordering::SeqCst);
        native_jpeg()
    }

    let search_requests = Arc::new(AtomicUsize::new(0));
    let detail_requests = Arc::new(AtomicUsize::new(0));
    let image_requests = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock listener binds");
    let address = listener.local_addr().expect("mock address exists");
    let base = format!("http://{address}");
    let state = MockState {
        base: base.clone(),
        search_requests: search_requests.clone(),
        detail_requests: detail_requests.clone(),
        image_requests: image_requests.clone(),
    };
    let app = Router::new()
        .route("/search", get(search))
        .route("/content/{id}", get(detail))
        .route("/image", get(image))
        .with_state(state);
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock server runs");
    });
    let providers = ProviderSet::with_endpoints(
        crate::providers::aic::AicProvider::official_endpoint().expect("AIC endpoint is valid"),
        crate::providers::cleveland::ClevelandProvider::official_endpoint()
            .expect("Cleveland endpoint is valid"),
        crate::providers::met::MetProvider::official_endpoint().expect("Met endpoint is valid"),
        Url::parse(&format!("{base}/search")).expect("mock endpoint is valid"),
        Some(SECRET.into()),
    );
    let temporary = tempdir().expect("temporary directory exists");
    let services = SearchServices::new(
        providers,
        Cache::new(temporary.path().join("cache")),
        HttpClient::new(),
        RateLimiters::new(),
    );
    let outcomes = services.search_batch(&query(Some("Africa"))).await;
    let [ProviderOutcome::Success(page)] = outcomes.as_slice() else {
        panic!("Smithsonian search must succeed");
    };
    let artwork = page.artworks.first().expect("one artwork exists");
    let key = ArtworkKey::try_from_parts("smithsonian", &artwork.source_id)
        .expect("artwork key is valid");
    let detail_artwork = services.load_artwork(&key).await.expect("detail loads");
    let display = services
        .load_display_image(&key, DisplayImageSize::Card)
        .await
        .expect("display image loads");
    assert_eq!(display.media_type, DisplayMediaType::Jpeg);
    let attribution = format_attribution(&detail_artwork);
    let slug = Slug::parse("smithsonian-mask").expect("slug is valid");
    let tags = Tags::parse("mask, africa");
    let saved = services
        .download(DownloadJob {
            artwork: &detail_artwork,
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
    assert!(xmp.contains("smithsonian"));
    assert!(xmp.contains("Gift of A &amp; B"));
    assert!(xmp.contains("mask"));
    assert_eq!(search_requests.load(Ordering::SeqCst), 1);
    assert_eq!(detail_requests.load(Ordering::SeqCst), 1);
    assert_eq!(image_requests.load(Ordering::SeqCst), 2);
    task.abort();
}
