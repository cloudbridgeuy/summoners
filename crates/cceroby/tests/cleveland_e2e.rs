//! Deterministic Cleveland search, card, detail, preview, and TIFF download evidence.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::header::{CONTENT_TYPE, HOST, ORIGIN, USER_AGENT};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use cceroby::app::SearchArgs;
use cceroby::cache::Cache;
use cceroby::core::{
    Culture, QueryText, SearchQuery, SearchSeed, SearchSession, SourceKind, SourceSet,
};
use cceroby::http::HttpClient;
use cceroby::providers::aic::AicProvider;
use cceroby::providers::{ProviderEndpoints, ProviderSet};
use cceroby::rate_limit::RateLimiters;
use cceroby::search::SearchServices;
use cceroby::server::{AppState, router};
use cceroby::xmp::XMP_IDENTIFIER;
use img_parts::jpeg::Jpeg;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tower::ServiceExt;
use url::Url;

const USER_AGENT_VALUE: &str = "cceroby/0.0.0 (local public-domain artwork search)";
const LISTENER_AUTHORITY: &str = "127.0.0.1:45123";

#[derive(Clone)]
struct MockState {
    base: String,
    bad_headers: Arc<AtomicUsize>,
    card_requests: Arc<AtomicUsize>,
    full_requests: Arc<AtomicUsize>,
}

fn fixture(raw: &str) -> Vec<u8> {
    raw.trim()
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| match std::str::from_utf8(pair) {
            Ok(text) => match u8::from_str_radix(text, 16) {
                Ok(byte) => byte,
                Err(error) => panic!("fixture must be hexadecimal: {error}"),
            },
            Err(error) => panic!("fixture must be ASCII: {error}"),
        })
        .collect()
}

fn source_tiff() -> Vec<u8> {
    fixture(include_str!("fixtures/images/source-tiff.hex"))
}

fn native_jpeg() -> Vec<u8> {
    fixture(include_str!("fixtures/images/native-jpeg.hex"))
}

fn has_provider_header(headers: &HeaderMap) -> bool {
    headers
        .get(USER_AGENT)
        .is_some_and(|value| value == USER_AGENT_VALUE)
}

fn artwork_json(base: &str) -> serde_json::Value {
    serde_json::json!({
        "id": 126730,
        "accession_number": "1949.158",
        "share_license_status": "CC0",
        "title": "Gigaku Mask",
        "creation_date": "710–94",
        "creators": [{"description": "Workshop of Test"}],
        "culture": ["Japan, Nara period (710–94)"],
        "url": "https://clevelandart.org/art/1949.158",
        "images": {
            "annotation": null,
            "web": {"url": format!("{base}/images/web.jpg"), "width": "2", "height": "2", "filesize": "1", "filename": "web.jpg"},
            "print": {"url": format!("{base}/images/print.jpg"), "width": "4", "height": "4", "filesize": "1", "filename": "print.jpg"},
            "full": {"url": format!("{base}/images/full.tif"), "width": "8", "height": "8", "filesize": "1", "filename": "full.tif"}
        },
        "creditline": "Gift of Test",
        "has_conservation_images": false
    })
}

async fn search(State(state): State<MockState>, headers: HeaderMap) -> Response {
    if !has_provider_header(&headers) {
        state.bad_headers.fetch_add(1, Ordering::SeqCst);
        return StatusCode::FORBIDDEN.into_response();
    }
    axum::Json(serde_json::json!({
        "info": {"total": 1, "parameters": {"skip": 0}},
        "data": [artwork_json(&state.base)]
    }))
    .into_response()
}

async fn detail(State(state): State<MockState>, headers: HeaderMap) -> Response {
    if !has_provider_header(&headers) {
        state.bad_headers.fetch_add(1, Ordering::SeqCst);
        return StatusCode::FORBIDDEN.into_response();
    }
    axum::Json(serde_json::json!({"data": artwork_json(&state.base)})).into_response()
}

async fn jpeg(State(state): State<MockState>, headers: HeaderMap) -> Response {
    if !has_provider_header(&headers) {
        state.bad_headers.fetch_add(1, Ordering::SeqCst);
        return StatusCode::FORBIDDEN.into_response();
    }
    native_jpeg().into_response()
}

async fn card_jpeg(State(state): State<MockState>, headers: HeaderMap) -> Response {
    if !has_provider_header(&headers) {
        state.bad_headers.fetch_add(1, Ordering::SeqCst);
        return StatusCode::FORBIDDEN.into_response();
    }
    state.card_requests.fetch_add(1, Ordering::SeqCst);
    native_jpeg().into_response()
}

async fn tiff(State(state): State<MockState>, headers: HeaderMap) -> Response {
    if !has_provider_header(&headers) {
        state.bad_headers.fetch_add(1, Ordering::SeqCst);
        return StatusCode::FORBIDDEN.into_response();
    }
    state.full_requests.fetch_add(1, Ordering::SeqCst);
    source_tiff().into_response()
}

async fn mock_endpoint() -> (Url, MockState, tokio::task::JoinHandle<()>) {
    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(error) => panic!("mock listener must bind: {error}"),
    };
    let address = match listener.local_addr() {
        Ok(address) => address,
        Err(error) => panic!("mock address must exist: {error}"),
    };
    let base = format!("http://{address}");
    let state = MockState {
        base: base.clone(),
        bad_headers: Arc::new(AtomicUsize::new(0)),
        card_requests: Arc::new(AtomicUsize::new(0)),
        full_requests: Arc::new(AtomicUsize::new(0)),
    };
    let app = Router::new()
        .route("/api/artworks/", get(search))
        .route("/api/artworks/126730", get(detail))
        .route("/images/web.jpg", get(card_jpeg))
        .route("/images/print.jpg", get(jpeg))
        .route("/images/full.tif", get(tiff))
        .with_state(state.clone());
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            panic!("mock server failed: {error}");
        }
    });
    let endpoint = match Url::parse(&format!("{base}/api/artworks/")) {
        Ok(endpoint) => endpoint,
        Err(error) => panic!("mock endpoint must be valid: {error}"),
    };
    (endpoint, state, task)
}

fn output_directory(path: &Path) -> cceroby::core::OutputDirectory {
    match SearchSeed::try_from(SearchArgs {
        query: "mask".into(),
        source: vec![SourceKind::ClevelandMuseum],
        culture: Some("Japan".into()),
        open: false,
        out: path.to_path_buf(),
        serve: false,
    }) {
        Ok(seed) => seed.output,
        Err(error) => panic!("fixture output directory must be valid: {error}"),
    }
}

fn cleveland_query() -> SearchQuery {
    SearchQuery {
        query: QueryText::parse("mask").unwrap_or_else(|error| panic!("query is valid: {error}")),
        sources: SourceSet::parse(&[SourceKind::ClevelandMuseum])
            .unwrap_or_else(|error| panic!("source is valid: {error}")),
        culture: Culture::parse(Some("Japan".into())),
    }
}

async fn body(app: Router, request: Request<Body>) -> (StatusCode, Vec<u8>) {
    let (status, _, bytes) = response_parts(app, request).await;
    (status, bytes)
}

async fn response_parts(app: Router, request: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let response = match app.oneshot(request).await {
        Ok(response) => response,
        Err(error) => panic!("route must respond: {error}"),
    };
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = match to_bytes(response.into_body(), usize::MAX).await {
        Ok(bytes) => bytes.to_vec(),
        Err(error) => panic!("body must be readable: {error}"),
    };
    (status, headers, bytes)
}

fn request(method: &str, uri: &str, body: Body) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(body)
        .unwrap_or_else(|error| panic!("request must be valid: {error}"))
}

fn download_request() -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/download")
        .header(HOST, LISTENER_AUTHORITY)
        .header(ORIGIN, format!("http://{LISTENER_AUTHORITY}"))
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(
            "source=cleveland&id=126730&slug=cleveland-mask&tags=ritual%2C+gold",
        ))
        .unwrap_or_else(|error| panic!("download request must be valid: {error}"))
}

#[tokio::test]
async fn mocked_cleveland_tiff_download_writes_exact_xmp_and_does_not_cache_full_image() {
    let (endpoint, mock, task) = mock_endpoint().await;
    let cache = match tempfile::tempdir() {
        Ok(directory) => directory,
        Err(error) => panic!("temporary cache must exist: {error}"),
    };
    let output = match tempfile::tempdir() {
        Ok(directory) => directory,
        Err(error) => panic!("temporary output must exist: {error}"),
    };
    let aic = AicProvider::official_endpoint()
        .unwrap_or_else(|error| panic!("AIC endpoint must be valid: {error}"));
    let met = cceroby::providers::met::MetProvider::official_endpoint()
        .unwrap_or_else(|error| panic!("Met endpoint must be valid: {error}"));
    let smithsonian = cceroby::providers::smithsonian::SmithsonianProvider::official_endpoint()
        .unwrap_or_else(|error| panic!("Smithsonian endpoint must be valid: {error}"));
    let services = SearchServices::new(
        ProviderSet::with_endpoints(
            ProviderEndpoints {
                aic,
                cleveland: endpoint,
                met,
                smithsonian,
                commons: cceroby::providers::commons::CommonsProvider::official_endpoint()
                    .unwrap_or_else(|error| panic!("Commons endpoint must be valid: {error}")),
            },
            None,
        ),
        Cache::new(cache.path().to_path_buf()),
        HttpClient::new(),
        RateLimiters::new(),
    );
    let (shutdown, _) = broadcast::channel(1);
    let app = router(AppState::new(
        SearchSession::new(cleveland_query()),
        services,
        output_directory(output.path()),
        LISTENER_AUTHORITY
            .parse()
            .unwrap_or_else(|error| panic!("authority must be valid: {error}")),
        shutdown,
    ));

    let (status, search_html) = body(
        app.clone(),
        request(
            "GET",
            "/?query=mask&cleveland=true&culture=Japan",
            Body::empty(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let search_html = String::from_utf8_lossy(&search_html);
    assert!(search_html.contains("Loaded 1 results."), "{search_html}");
    assert!(search_html.contains("Gigaku Mask"));

    let (status, headers, card) = response_parts(
        app.clone(),
        request("GET", "/thumb?source=cleveland&id=126730", Body::empty()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/jpeg")
    );
    assert_eq!(card, native_jpeg());

    let (status, detail_html) = body(
        app.clone(),
        request("GET", "/detail?source=cleveland&id=126730", Body::empty()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let detail_html = String::from_utf8_lossy(&detail_html);
    assert!(detail_html.contains("Back to search results"));
    assert!(detail_html.contains("Gift of Test"));

    let (status, preview) = body(
        app.clone(),
        request("GET", "/preview?source=cleveland&id=126730", Body::empty()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview, native_jpeg());

    let (status, first_html) = body(app.clone(), download_request()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&first_html).contains("Created"));
    let (status, second_html) = body(app, download_request()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&second_html).contains("Replaced"));
    assert_eq!(mock.card_requests.load(Ordering::SeqCst), 1);
    assert_eq!(mock.full_requests.load(Ordering::SeqCst), 2);
    assert_eq!(mock.bad_headers.load(Ordering::SeqCst), 0);

    let written = match std::fs::read(output.path().join("cleveland-mask.jpg")) {
        Ok(bytes) => bytes,
        Err(error) => panic!("downloaded JPEG must exist: {error}"),
    };
    let decoded = image::load_from_memory_with_format(&written, image::ImageFormat::Jpeg)
        .unwrap_or_else(|error| panic!("download must be a JPEG: {error}"));
    assert_eq!((decoded.width(), decoded.height()), (2, 2));
    let jpeg = Jpeg::from_bytes(img_parts::Bytes::from(written))
        .unwrap_or_else(|error| panic!("downloaded JPEG must parse: {error}"));
    let xmp = jpeg
        .segments()
        .iter()
        .find(|segment| segment.contents().starts_with(XMP_IDENTIFIER))
        .unwrap_or_else(|| panic!("downloaded JPEG must contain XMP"));
    let xml = String::from_utf8_lossy(&xmp.contents()[XMP_IDENTIFIER.len()..]);
    let attribution = "“Gigaku Mask” — Workshop of Test. Gift of Test. CC0 (https://creativecommons.org/publicdomain/zero/1.0/). Source: https://clevelandart.org/art/1949.158.";
    assert!(xml.contains(attribution), "{xml}");
    assert!(xml.contains("<photoshop:Credit>Gift of Test</photoshop:Credit>"));
    assert!(xml.contains("<rdf:li>ritual</rdf:li><rdf:li>gold</rdf:li>"));
    task.abort();
}
