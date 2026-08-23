//! Deterministic provider, cache, and page integration evidence.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::header::USER_AGENT;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::routing::get;
use cceroby::app::SearchArgs;
use cceroby::cache::Cache;
use cceroby::core::{
    Culture, OutputDirectory, QueryText, SearchQuery, SearchSeed, SearchSession, SourceKind,
    SourceSet,
};
use cceroby::http::HttpClient;
use cceroby::providers::ProviderSet;
use cceroby::rate_limit::RateLimiters;
use cceroby::search::SearchServices;
use cceroby::server::{AppState, router};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tower::ServiceExt;
use url::Url;

const SUCCESS: &str = include_str!("fixtures/aic/success.json");
const AIC_USER_AGENT: &str = "cceroby/0.0.0 (local public-domain artwork search)";

fn output_directory(path: &Path) -> OutputDirectory {
    let seed = match SearchSeed::try_from(SearchArgs {
        query: "mask".into(),
        source: vec![SourceKind::ArtInstituteChicago],
        culture: None,
        open: false,
        out: path.to_path_buf(),
        serve: false,
    }) {
        Ok(seed) => seed,
        Err(error) => panic!("fixture output directory must be valid: {error}"),
    };
    seed.output
}

fn aic_query() -> SearchQuery {
    let query = match QueryText::parse("mask") {
        Ok(query) => query,
        Err(error) => panic!("fixture query must be valid: {error}"),
    };
    let sources = match SourceSet::parse(&[SourceKind::ArtInstituteChicago]) {
        Ok(sources) => sources,
        Err(error) => panic!("fixture source must be valid: {error}"),
    };
    SearchQuery {
        query,
        sources,
        culture: Culture::parse(None),
    }
}

fn services(endpoint: Url, cache_root: &TempDir) -> SearchServices {
    let cleveland = match cceroby::providers::cleveland::ClevelandProvider::official_endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => panic!("built-in Cleveland endpoint must be valid: {error}"),
    };
    SearchServices::new(
        ProviderSet::with_endpoints(
            endpoint,
            cleveland,
            cceroby::providers::met::MetProvider::official_endpoint()
                .unwrap_or_else(|error| panic!("Met endpoint must be valid: {error}")),
            cceroby::providers::smithsonian::SmithsonianProvider::official_endpoint()
                .unwrap_or_else(|error| panic!("Smithsonian endpoint must be valid: {error}")),
            None,
        ),
        Cache::new(cache_root.path().to_path_buf()),
        HttpClient::new(),
        RateLimiters::new(),
    )
}

async fn success(
    State(requests): State<Arc<AtomicUsize>>,
    headers: HeaderMap,
) -> (StatusCode, &'static str) {
    requests.fetch_add(1, Ordering::SeqCst);
    if headers
        .get(USER_AGENT)
        .is_some_and(|value| value == AIC_USER_AGENT)
    {
        (StatusCode::OK, SUCCESS)
    } else {
        (StatusCode::FORBIDDEN, "missing AIC User-Agent")
    }
}

async fn failure(State(requests): State<Arc<AtomicUsize>>) -> StatusCode {
    requests.fetch_add(1, Ordering::SeqCst);
    StatusCode::SERVICE_UNAVAILABLE
}

async fn mock_endpoint(
    handler: axum::routing::MethodRouter<Arc<AtomicUsize>>,
) -> (Url, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let requests = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/search", handler)
        .with_state(requests.clone());
    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(error) => panic!("mock listener must bind: {error}"),
    };
    let address = match listener.local_addr() {
        Ok(address) => address,
        Err(error) => panic!("mock address must exist: {error}"),
    };
    let task = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            panic!("mock server failed: {error}");
        }
    });
    let endpoint = match Url::parse(&format!("http://{address}/search")) {
        Ok(endpoint) => endpoint,
        Err(error) => panic!("mock endpoint must be valid: {error}"),
    };
    (endpoint, requests, task)
}

async fn response_html(app: Router, uri: &str) -> String {
    let response = match app
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .unwrap_or_else(|error| panic!("request must be valid: {error}")),
        )
        .await
    {
        Ok(response) => response,
        Err(error) => panic!("route must respond: {error}"),
    };
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = match to_bytes(response.into_body(), usize::MAX).await {
        Ok(bytes) => bytes,
        Err(error) => panic!("response body must be readable: {error}"),
    };
    match String::from_utf8(bytes.to_vec()) {
        Ok(html) => html,
        Err(error) => panic!("response body must be UTF-8: {error}"),
    }
}

#[tokio::test]
async fn first_search_fetches_and_second_search_uses_metadata_cache() {
    let (endpoint, requests, task) = mock_endpoint(get(success)).await;
    let cache_root = match tempfile::tempdir() {
        Ok(directory) => directory,
        Err(error) => panic!("temporary cache must exist: {error}"),
    };
    let services = services(endpoint, &cache_root);
    let query = aic_query();
    let (shutdown, _) = broadcast::channel(1);
    let app = router(AppState::new(
        SearchSession::new(query),
        services,
        output_directory(cache_root.path()),
        std::net::SocketAddr::from(([127, 0, 0, 1], 45_123)),
        shutdown,
    ));

    let html = response_html(app.clone(), "/?query=mask&aic=true").await;
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert!(html.contains("Loaded 2 results."));
    assert!(html.contains("Ceremonial Mask"));
    assert!(html.contains("Dance Mask"));
    assert!(!html.contains("Restricted Mask"));

    let second_html = response_html(app, "/?query=mask&aic=true").await;
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert!(second_html.contains("Loaded 2 results."));
    task.abort();
}

#[tokio::test]
async fn provider_failure_becomes_one_notice_and_keeps_the_page_alive() {
    let (endpoint, requests, task) = mock_endpoint(get(failure)).await;
    let cache_root = match tempfile::tempdir() {
        Ok(directory) => directory,
        Err(error) => panic!("temporary cache must exist: {error}"),
    };
    let (shutdown, _) = broadcast::channel(1);
    let app = router(AppState::new(
        SearchSession::new(aic_query()),
        services(endpoint, &cache_root),
        output_directory(cache_root.path()),
        std::net::SocketAddr::from(([127, 0, 0, 1], 45_123)),
        shutdown,
    ));
    let html = response_html(app, "/?query=mask&aic=true").await;

    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert!(html.contains("Loaded 0 results."));
    assert_eq!(html.matches("could not complete the search").count(), 1);
    task.abort();
}
