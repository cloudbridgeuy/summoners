#![allow(
    clippy::expect_used,
    reason = "test code asserts with expect/unwrap; a failure is the test failure"
)]

use std::collections::HashMap;

use reqwest::header::USER_AGENT;

use crate::core::{Culture, QueryText, SourceSet};

use super::test_fixtures::*;
use super::*;

fn provider() -> ClevelandProvider {
    ClevelandProvider::official().expect("built-in endpoint is valid")
}

fn query(culture: Option<&str>) -> SearchQuery {
    SearchQuery {
        query: QueryText::parse("ritual mask").expect("query is valid"),
        sources: SourceSet::parse(&[SourceKind::ClevelandMuseum]).expect("source is valid"),
        culture: Culture::parse(culture.map(str::to_owned)),
    }
}

fn query_map(request: &HttpRequest) -> HashMap<String, String> {
    request.url().query_pairs().into_owned().collect()
}

fn image(url: &str, width: &str, height: &str, filename: &str) -> ClevelandImage {
    ClevelandImage {
        url: url.into(),
        width: Some(width.into()),
        height: Some(height.into()),
        _filesize: None,
        filename: Some(filename.into()),
    }
}

fn remote_url(raw: &str) -> Url {
    parse_remote_url(raw).expect("test URL is valid")
}

#[test]
fn cleveland_provider_is_available() {
    let provider = provider();
    assert_eq!(provider.kind(), SourceKind::ClevelandMuseum);
    assert!(matches!(provider.entry(), ProviderEntry::Available(_)));
}

#[test]
fn request_contains_cc0_query_default_pagination_fields_and_policy() {
    let provider = provider();
    let request = provider.search_request(&query(None), None);
    let pairs = query_map(&request);
    assert_eq!(pairs.get("q").map(String::as_str), Some("ritual mask"));
    assert_eq!(pairs.get("cc0").map(String::as_str), Some(""));
    assert_eq!(pairs.get("has_image").map(String::as_str), Some("1"));
    assert_eq!(pairs.get("skip").map(String::as_str), Some("0"));
    assert_eq!(pairs.get("limit").map(String::as_str), Some("20"));
    assert_eq!(pairs.get("fields").map(String::as_str), Some(SEARCH_FIELDS));
    assert!(!pairs.contains_key("culture"));
    let raw_query = request.url().query().expect("query is present");
    assert!(raw_query.split('&').any(|segment| segment == "cc0"));
    assert!(!raw_query.split('&').any(|segment| segment == "cc0="));
    assert_eq!(
        provider.rate_policy(),
        RatePolicy::TokenBucket(TokenBucketPolicy::new(
            std::num::NonZeroU32::MIN,
            std::time::Duration::from_secs(1)
        ))
    );
}

#[test]
fn request_contains_optional_culture_and_stable_offset_cursor() {
    let request = provider().search_request(&query(Some("Japan")), Some("40"));
    let pairs = query_map(&request);
    assert_eq!(pairs.get("culture").map(String::as_str), Some("Japan"));
    assert_eq!(pairs.get("skip").map(String::as_str), Some("40"));
    let invalid = provider().search_request(&query(None), Some("not-an-offset"));
    assert_eq!(
        query_map(&invalid).get("skip").map(String::as_str),
        Some("0")
    );
}

#[test]
fn request_helper_applies_the_exact_stable_user_agent() {
    let url = Url::parse("https://example.test/resource").expect("URL is valid");
    assert_eq!(
        cleveland_request(url).headers().get(USER_AGENT),
        Some(&CLEVELAND_USER_AGENT)
    );
}

#[test]
fn detail_endpoint_appends_the_numeric_id_and_removes_query_data() {
    let endpoint = Url::parse("https://example.test/api/artworks/?limit=20").expect("URL is valid");
    assert_eq!(
        detail_endpoint(&endpoint, "126730").as_str(),
        "https://example.test/api/artworks/126730"
    );
}

#[test]
fn cursor_helper_stops_on_empty_last_and_overflow_pages() {
    assert_eq!(next_cursor(0, 2, 3), Some("2".into()));
    assert_eq!(next_cursor(2, 1, 3), None);
    assert_eq!(next_cursor(0, 0, 3), None);
    assert_eq!(next_cursor(u64::MAX, 1, u64::MAX), None);
}

#[test]
fn json_offset_helper_accepts_integer_and_string_forms() {
    assert_eq!(json_u64(&serde_json::json!(20)), Some(20));
    assert_eq!(json_u64(&serde_json::json!("40")), Some(40));
    assert_eq!(json_u64(&serde_json::json!("bad")), None);
}

#[test]
fn captured_pages_parse_with_stable_pagination() {
    let first = provider()
        .parse_search(SEARCH_PAGE_1, None)
        .expect("page is valid");
    assert_eq!(first.candidates.len(), 2);
    assert_eq!(first.next_cursor.as_deref(), Some("2"));
    let second = provider()
        .parse_search(SEARCH_PAGE_2, None)
        .expect("page is valid");
    assert_eq!(second.candidates.len(), 1);
    assert_eq!(second.next_cursor, None);
}

#[test]
fn captured_tiff_record_normalizes_exact_metadata_and_urls() {
    let page = provider()
        .parse_search(SEARCH_PAGE_1, None)
        .expect("page is valid");
    let artwork = provider()
        .parse_artwork(&page.candidates[0], None)
        .expect("record is accepted");
    assert_eq!(artwork.source_id, "126730");
    assert_eq!(artwork.title, "Gigaku Mask of Young Persian Boy (Taikōji)");
    assert_eq!(artwork.creator, None);
    assert_eq!(artwork.date.as_deref(), Some("710–94"));
    assert_eq!(
        artwork.culture.as_deref(),
        Some("Japan, Nara period (710–94)")
    );
    assert_eq!(artwork.license, CommercialLicense::Cc0);
    assert_eq!(artwork.institution, "Cleveland Museum of Art");
    assert_eq!(
        artwork.provider_credit.as_deref(),
        Some("John L. Severance Fund")
    );
    assert_eq!(artwork.object_url, "https://clevelandart.org/art/1949.158");
    assert!(artwork.image_urls.thumbnail.ends_with("_web.jpg"));
    assert!(artwork.image_urls.display.ends_with("_print.jpg"));
    assert!(
        artwork
            .image_urls
            .original
            .as_deref()
            .is_some_and(|url| url.ends_with("_full.tif"))
    );
}

#[test]
fn captured_jpeg_only_record_uses_best_jpeg_and_joins_creator_and_culture() {
    let page = provider()
        .parse_search(SEARCH_PAGE_1, None)
        .expect("page is valid");
    let artwork = provider()
        .parse_artwork(&page.candidates[1], None)
        .expect("record is accepted");
    assert_eq!(
        artwork.creator.as_deref(),
        Some("Song Xu (Chinese, 1525-c. 1606)")
    );
    assert_eq!(
        artwork.culture.as_deref(),
        Some("China, Ming dynasty (1368–1644); Wanli reign (1573–1620)")
    );
    assert_eq!(
        artwork.image_urls.display,
        artwork
            .image_urls
            .original
            .clone()
            .expect("best image exists")
    );
    assert!(artwork.image_urls.display.ends_with("_print.jpg"));
}

#[test]
fn captured_missing_creator_and_single_jpeg_are_supported() {
    let page = provider()
        .parse_search(SEARCH_PAGE_2, None)
        .expect("page is valid");
    let artwork = provider()
        .parse_artwork(&page.candidates[0], None)
        .expect("record is accepted");
    assert_eq!(artwork.creator, None);
    assert_eq!(artwork.provider_credit, None);
    assert_eq!(artwork.image_urls.thumbnail, artwork.image_urls.display);
    assert_eq!(
        artwork.image_urls.original.as_deref(),
        Some(artwork.image_urls.display.as_str())
    );
}

#[test]
fn captured_restricted_unknown_and_missing_records_are_rejected() {
    assert_eq!(
        provider().parse_artwork_response(CC0_FALSE),
        Err(ProviderError::ArtworkUnavailable)
    );
    assert_eq!(
        provider().parse_artwork_response(UNKNOWN_LICENSE),
        Err(ProviderError::ArtworkUnavailable)
    );
    assert_eq!(
        provider().parse_artwork_response(MISSING_FIELDS),
        Err(ProviderError::MalformedResponse)
    );
}

#[test]
fn missing_data_malformed_json_and_unknown_schema_fields_are_rejected() {
    assert_eq!(
        provider().parse_search(MISSING_DATA, None),
        Err(ProviderError::MalformedResponse)
    );
    assert_eq!(
        provider().parse_search(MALFORMED, None),
        Err(ProviderError::MalformedResponse)
    );
    assert_eq!(
        provider().parse_search(
            br#"{"info":{"total":0,"parameters":{}},"data":[],"extra":true}"#,
            None,
        ),
        Err(ProviderError::MalformedResponse)
    );
}

#[test]
fn license_helper_accepts_only_the_exact_supported_state() {
    assert_eq!(parse_license(Some("CC0")), Ok(CommercialLicense::Cc0));
    for raw in [
        Some("Copyrighted"),
        Some("Other"),
        Some("cc0"),
        Some("Future"),
        None,
    ] {
        assert_eq!(parse_license(raw), Err(ArtworkDropReason::NotPublicDomain));
    }
}

#[test]
fn optional_text_and_list_helpers_remove_only_absent_or_blank_values() {
    assert_eq!(nonempty(Some(" exact ".into())), Some(" exact ".into()));
    assert_eq!(nonempty(Some("  ".into())), None);
    let creators = [
        ClevelandCreator {
            description: Some("Maker One".into()),
        },
        ClevelandCreator {
            description: Some("".into()),
        },
        ClevelandCreator {
            description: Some("Maker Two".into()),
        },
    ];
    assert_eq!(
        join_creators(&creators).as_deref(),
        Some("Maker One; Maker Two")
    );
    assert_eq!(join_creators(&[]), None);
    assert_eq!(
        join_culture(&["Japan".into(), "Nara period".into()]).as_deref(),
        Some("Japan; Nara period")
    );
    assert_eq!(join_nonempty(["", "  "].into_iter()), None);
}

#[test]
fn remote_url_parser_normalizes_http_and_rejects_other_values() {
    assert_eq!(
        parse_remote_url(" HTTPS://Example.TEST:443/a/../image.jpg ").map(|url| url.to_string()),
        Some("https://example.test/image.jpg".into())
    );
    assert_eq!(
        parse_remote_url("http://example.test/image.jpg").map(|url| url.to_string()),
        Some("http://example.test/image.jpg".into())
    );
    for invalid in [
        "",
        "not a URL",
        "/relative.jpg",
        "file:///tmp/image.jpg",
        "data:image/jpeg;base64,AA==",
        "javascript:alert(1)",
        "ftp://example.test/image.jpg",
    ] {
        assert_eq!(parse_remote_url(invalid), None, "URL: {invalid}");
    }
}

#[test]
fn image_helpers_classify_formats_measure_pixels_and_reject_blank_urls() {
    let jpeg = image("https://example.test/image", "40", "50", "IMAGE.JPEG");
    assert_eq!(
        jpeg.usable_url().map(|url| url.to_string()),
        Some("https://example.test/image".into())
    );
    assert_eq!(jpeg.pixel_area(), 2_000);
    assert_eq!(jpeg.media_type(), Some(RemoteMediaType::Jpeg));
    let tiff = image("https://example.test/full.tiff", "bad", "50", "");
    assert_eq!(tiff.pixel_area(), 0);
    assert_eq!(tiff.media_type(), Some(RemoteMediaType::Tiff));
    let blank = image(" ", "1", "1", "image.png");
    assert_eq!(blank.usable_url(), None);
    assert_eq!(blank.media_type(), None);
}

#[test]
fn selection_prefers_web_for_cards_best_jpeg_for_preview_and_largest_tiff_for_download() {
    let images = ClevelandImages {
        _annotation: None,
        web: Some(image(
            "https://example.test/web.jpg",
            "900",
            "600",
            "web.jpg",
        )),
        print: Some(image(
            "https://example.test/print.jpg",
            "3400",
            "2200",
            "print.jpg",
        )),
        full: Some(image(
            "https://example.test/full.tif",
            "6000",
            "4000",
            "full.tif",
        )),
    };
    assert_eq!(
        select_images(&images),
        Some(SelectedImages {
            thumbnail: remote_url("https://example.test/web.jpg"),
            display: remote_url("https://example.test/print.jpg"),
            original: remote_url("https://example.test/full.tif"),
        })
    );
    let no_display = ClevelandImages {
        _annotation: None,
        web: None,
        print: None,
        full: Some(image(
            "https://example.test/full.tif",
            "6000",
            "4000",
            "full.tif",
        )),
    };
    assert_eq!(select_images(&no_display), None);
}

#[test]
fn selection_rejects_invalid_card_preview_and_original_urls() {
    let invalid_card = ClevelandImages {
        _annotation: None,
        web: Some(image("/web.jpg", "900", "600", "web.jpg")),
        print: Some(image(
            "https://example.test/print.jpg",
            "3400",
            "2200",
            "print.jpg",
        )),
        full: Some(image(
            "https://example.test/full.tif",
            "6000",
            "4000",
            "full.tif",
        )),
    };
    assert_eq!(
        select_images(&invalid_card),
        Some(SelectedImages {
            thumbnail: remote_url("https://example.test/print.jpg"),
            display: remote_url("https://example.test/print.jpg"),
            original: remote_url("https://example.test/full.tif"),
        })
    );

    let invalid_preview = ClevelandImages {
        _annotation: None,
        web: Some(image(
            "https://example.test/web.jpg",
            "900",
            "600",
            "web.jpg",
        )),
        print: Some(image("file:///print.jpg", "3400", "2200", "print.jpg")),
        full: None,
    };
    assert_eq!(
        select_images(&invalid_preview),
        Some(SelectedImages {
            thumbnail: remote_url("https://example.test/web.jpg"),
            display: remote_url("https://example.test/web.jpg"),
            original: remote_url("https://example.test/web.jpg"),
        })
    );

    let invalid_tiff = ClevelandImages {
        _annotation: None,
        web: Some(image(
            "https://example.test/web.jpg",
            "900",
            "600",
            "web.jpg",
        )),
        print: None,
        full: Some(image("javascript:full.tif", "6000", "4000", "full.tif")),
    };
    assert_eq!(
        select_images(&invalid_tiff),
        Some(SelectedImages {
            thumbnail: remote_url("https://example.test/web.jpg"),
            display: remote_url("https://example.test/web.jpg"),
            original: remote_url("https://example.test/web.jpg"),
        })
    );

    let invalid_jpeg = ClevelandImages {
        _annotation: None,
        web: Some(image(
            "data:image/jpeg;base64,AA==",
            "900",
            "600",
            "web.jpg",
        )),
        print: None,
        full: None,
    };
    assert_eq!(select_images(&invalid_jpeg), None);
}

#[test]
fn detail_display_and_best_image_requests_are_provider_owned() {
    let provider = provider();
    let key = ArtworkKey::try_from_parts("cleveland", "126730").expect("key is valid");
    let detail = provider
        .artwork_request(&key)
        .expect("detail request is valid");
    assert_eq!(detail.url().path(), "/api/artworks/126730");
    let page = provider
        .parse_search(SEARCH_PAGE_1, None)
        .expect("page is valid");
    let artwork = provider
        .parse_artwork(&page.candidates[0], None)
        .expect("artwork is valid");
    let card = provider
        .display_image_request(&artwork, DisplayImageSize::Card)
        .expect("card request is valid");
    let preview = provider
        .display_image_request(&artwork, DisplayImageSize::Preview)
        .expect("preview request is valid");
    let best = provider
        .best_image_request(&artwork)
        .expect("best request is valid");
    assert_eq!(card.media_type(), DisplayMediaType::Jpeg);
    assert_eq!(preview.media_type(), DisplayMediaType::Jpeg);
    assert!(card.request().url().path().ends_with("_web.jpg"));
    assert!(preview.request().url().path().ends_with("_print.jpg"));
    assert!(best.url().path().ends_with("_full.tif"));
    for request in [
        detail,
        card.request().clone(),
        preview.request().clone(),
        best,
    ] {
        assert_eq!(
            request.headers().get(USER_AGENT),
            Some(&CLEVELAND_USER_AGENT)
        );
    }
}

#[test]
fn invalid_and_missing_image_requests_return_typed_errors() {
    let artwork = provider().parse_artwork_response(b"{\"data\":null}");
    assert_eq!(artwork, Err(ProviderError::MalformedResponse));
    let page = provider()
        .parse_search(SEARCH_PAGE_1, None)
        .expect("page is valid");
    let mut artwork = provider()
        .parse_artwork(&page.candidates[0], None)
        .expect("artwork is valid");
    artwork.image_urls.thumbnail = "file:///tmp/card.jpg".into();
    assert_eq!(
        provider().display_image_request(&artwork, DisplayImageSize::Card),
        Err(ProviderError::InvalidImageRequest)
    );
    artwork.image_urls.display = "data:image/jpeg;base64,AA==".into();
    assert_eq!(
        provider().display_image_request(&artwork, DisplayImageSize::Preview),
        Err(ProviderError::InvalidImageRequest)
    );
    artwork.image_urls.original = Some("javascript:download()".into());
    assert_eq!(
        provider().best_image_request(&artwork),
        Err(ProviderError::InvalidImageRequest)
    );
    artwork.image_urls.original = None;
    assert_eq!(
        provider().best_image_request(&artwork),
        Err(ProviderError::MissingImageService)
    );
}

#[test]
fn normalization_helper_directly_rejects_each_missing_required_field() {
    let page = provider()
        .parse_search(SEARCH_PAGE_1, None)
        .expect("fixture page is valid");
    let raw = page.candidates[0].raw.clone();

    let mut missing_id: ClevelandArtwork =
        serde_json::from_value(raw.clone()).expect("fixture shape is valid");
    missing_id.id = None;
    assert_eq!(
        normalize_artwork(missing_id),
        Err(ArtworkDropReason::MissingSourceId)
    );

    let mut missing_title: ClevelandArtwork =
        serde_json::from_value(raw.clone()).expect("fixture shape is valid");
    missing_title.title = None;
    assert_eq!(
        normalize_artwork(missing_title),
        Err(ArtworkDropReason::MissingTitle)
    );

    let mut missing_images: ClevelandArtwork =
        serde_json::from_value(raw).expect("fixture shape is valid");
    missing_images.images = None;
    assert_eq!(
        normalize_artwork(missing_images),
        Err(ArtworkDropReason::MissingImage)
    );
}

#[test]
fn normalization_rejects_missing_blank_malformed_and_non_http_object_urls() {
    let page = provider()
        .parse_search(SEARCH_PAGE_1, None)
        .expect("fixture page is valid");
    let raw = page.candidates[0].raw.clone();
    for invalid in [
        None,
        Some("   "),
        Some("not a URL"),
        Some("/art/1949.158"),
        Some("file:///art/1949.158"),
        Some("data:text/plain,art"),
        Some("javascript:alert(1)"),
        Some("ftp://clevelandart.org/art/1949.158"),
    ] {
        let mut artwork: ClevelandArtwork =
            serde_json::from_value(raw.clone()).expect("fixture shape is valid");
        artwork.url = invalid.map(str::to_owned);
        assert_eq!(
            normalize_artwork(artwork),
            Err(ArtworkDropReason::MissingSourceId),
            "object URL must be rejected: {invalid:?}"
        );
    }
}
