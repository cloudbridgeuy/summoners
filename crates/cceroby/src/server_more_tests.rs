#![allow(clippy::expect_used)]

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{RawQuery, State};
use axum::http::Request;
use axum::routing::get;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tower::ServiceExt;
use url::Url;

use crate::cache::Cache;
use crate::core::{
    Artwork, CommercialLicense, Culture, ImageUrls, OutputDirectory, ProviderOutcome, ProviderPage,
    QueryText, SearchQuery, SearchSession, SourceKind, SourceSet,
};
use crate::http::HttpClient;
use crate::providers::{ProviderEndpoints, ProviderSet};
use crate::rate_limit::RateLimiters;
use crate::search::SearchServices;

use super::*;

async fn paginated_aic_search(
    State(requests): State<Arc<Mutex<Vec<String>>>>,
    RawQuery(raw_query): RawQuery,
) -> String {
    let page = raw_query
        .as_deref()
        .and_then(|query| {
            url::form_urlencoded::parse(query.as_bytes())
                .find(|(name, _)| name == "page")
                .and_then(|(_, value)| value.parse::<u32>().ok())
        })
        .unwrap_or(1);
    requests
        .lock()
        .expect("request log lock is available")
        .push(page.to_string());
    serde_json::json!({
        "pagination": { "current_page": page, "total_pages": 3 },
        "config": { "iiif_url": "https://images.example.test/iiif/2" },
        "data": [{
            "id": 1000 + page,
            "title": format!("Page {page} mask"),
            "artist_display": "Test artist",
            "date_display": "1900",
            "place_of_origin": "Uruguay",
            "image_id": format!("image-{page}"),
            "is_public_domain": true,
            "credit_line": "Test credit"
        }]
    })
    .to_string()
}

fn initial_artwork() -> Artwork {
    Artwork {
        source: SourceKind::ArtInstituteChicago,
        source_id: "1001".into(),
        title: "Page 1 mask".into(),
        creator: None,
        date: None,
        culture: None,
        license: CommercialLicense::PublicDomain,
        image_urls: ImageUrls {
            thumbnail: "https://images.example.test/thumb.jpg".into(),
            display: "https://images.example.test/display.jpg".into(),
            original: Some("https://images.example.test/original.jpg".into()),
        },
        institution: "Art Institute of Chicago".into(),
        provider_credit: None,
        object_url: "https://www.artic.edu/artworks/1001".into(),
    }
}

async fn paginated_state(
    requests: Arc<Mutex<Vec<String>>>,
) -> (
    AppState,
    tokio::task::JoinHandle<()>,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock listener binds");
    let address = listener.local_addr().expect("mock address exists");
    let mock = Router::new()
        .route("/api/v1/artworks/search", get(paginated_aic_search))
        .with_state(requests);
    let task = tokio::spawn(async move {
        axum::serve(listener, mock).await.expect("mock server runs");
    });
    let query = SearchQuery {
        query: QueryText::parse("mask").expect("query is valid"),
        sources: SourceSet::parse(&[SourceKind::ArtInstituteChicago]).expect("source is valid"),
        culture: Culture::parse(None),
    };
    let mut session = SearchSession::new(query);
    let _ = session.merge_batch(vec![ProviderOutcome::Success(ProviderPage {
        source: SourceKind::ArtInstituteChicago,
        artworks: vec![initial_artwork()],
        next_cursor: Some("2".into()),
    })]);
    let cache = tempdir().expect("temporary cache exists");
    let output = tempdir().expect("temporary output exists");
    let endpoints = ProviderEndpoints {
        aic: Url::parse(&format!("http://{address}/api/v1/artworks/search"))
            .expect("mock endpoint is valid"),
        cleveland: crate::providers::cleveland::ClevelandProvider::official_endpoint()
            .expect("Cleveland endpoint is valid"),
        met: crate::providers::met::MetProvider::official_endpoint()
            .expect("Met endpoint is valid"),
        smithsonian: crate::providers::smithsonian::SmithsonianProvider::official_endpoint()
            .expect("Smithsonian endpoint is valid"),
        commons: crate::providers::commons::CommonsProvider::official_endpoint()
            .expect("Commons endpoint is valid"),
    };
    let (shutdown, _) = broadcast::channel(1);
    let state = AppState::new(
        session,
        SearchServices::new(
            ProviderSet::with_endpoints(endpoints, None),
            Cache::new(cache.path().to_path_buf()),
            HttpClient::new(),
            RateLimiters::new(),
        ),
        OutputDirectory::from_verified_path(output.path().to_path_buf()),
        std::net::SocketAddr::from(([127, 0, 0, 1], 45_123)),
        shutdown,
    );
    (state, task, cache, output)
}

async fn more_response(app: Router) -> (String, String) {
    let response = app
        .oneshot(
            Request::builder()
                .uri("/more")
                .body(Body::empty())
                .expect("request is valid"),
        )
        .await
        .expect("request succeeds");
    let has_more = response
        .headers()
        .get("X-Has-More")
        .and_then(|value| value.to_str().ok())
        .expect("has-more header is valid")
        .to_owned();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body is readable");
    let fragment = String::from_utf8(body.to_vec()).expect("body is UTF-8");
    (has_more, fragment)
}

#[tokio::test]
async fn more_route_propagates_cursors_and_changes_header_after_exhaustion() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let (state, task, _cache, _output) = paginated_state(Arc::clone(&requests)).await;
    let app = router(state);

    let (first_header, first_fragment) = more_response(app.clone()).await;
    assert_eq!(first_header, "true");
    assert!(first_fragment.contains("id=1002"));
    assert!(!first_fragment.contains("id=1001"));
    assert!(!first_fragment.contains("<!doctype"));
    assert!(!first_fragment.contains("<script"));

    let (second_header, second_fragment) = more_response(app).await;
    assert_eq!(second_header, "false");
    assert!(second_fragment.contains("id=1003"));
    assert!(!second_fragment.contains("id=1002"));
    assert!(!second_fragment.contains("<!doctype"));
    assert!(!second_fragment.contains("<script"));
    assert_eq!(
        *requests.lock().expect("request log lock is available"),
        vec!["2", "3"]
    );

    task.abort();
}

#[test]
fn has_more_header_changes_from_true_to_false() {
    assert_eq!(has_more_header(true), "true");
    assert_eq!(has_more_header(false), "false");
}
