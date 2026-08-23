//! HTTP and shared-state shell for the local search page.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use futures::Stream;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify, RwLock, broadcast, watch};

use crate::artwork::{ArtworkKey, ArtworkKeyError, format_attribution};
use crate::core::{OutputDirectory, SearchParams, SearchQuery, SearchSession, merge_page};
use crate::download::{DownloadError, DownloadJob, DownloadNotice, DownloadRequest, Slug};
use crate::providers::DisplayImageSize;
use crate::render::{DetailView, render_detail_page, render_search_page};
use crate::search::{ArtworkLoadError, SearchServices};

/// Process resources shared by local HTTP handlers.
#[derive(Debug, Clone)]
pub struct AppState {
    session: Arc<RwLock<SearchSession>>,
    services: SearchServices,
    output: OutputDirectory,
    authority: ListenerAuthority,
    search_gate: Arc<Mutex<()>>,
    shutdown: broadcast::Sender<()>,
    live: LiveConnections,
    quit: Arc<Notify>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListenerAuthority {
    host: String,
    origin: String,
}

impl ListenerAuthority {
    fn from_address(address: SocketAddr) -> Self {
        let host = address.to_string();
        let origin = format!("http://{host}");
        Self { host, origin }
    }
}

impl AppState {
    #[must_use]
    pub fn new(
        session: SearchSession,
        services: SearchServices,
        output: OutputDirectory,
        authority: SocketAddr,
        shutdown: broadcast::Sender<()>,
    ) -> Self {
        Self {
            session: Arc::new(RwLock::new(session)),
            services,
            output,
            authority: ListenerAuthority::from_address(authority),
            search_gate: Arc::new(Mutex::new(())),
            shutdown,
            live: LiveConnections::new(),
            quit: Arc::new(Notify::new()),
        }
    }

    #[must_use]
    pub fn shutdown_sender(&self) -> broadcast::Sender<()> {
        self.shutdown.clone()
    }

    #[must_use]
    pub fn live_connections(&self) -> LiveConnections {
        self.live.clone()
    }

    #[must_use]
    pub fn quit_notifier(&self) -> Arc<Notify> {
        Arc::clone(&self.quit)
    }
}

/// One consistent view of browser connection activity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LiveSnapshot {
    pub active: usize,
    pub ever_connected: bool,
    pub(crate) generation: u64,
}

/// Shared browser connection state with lossless change notification.
#[derive(Debug, Clone)]
pub struct LiveConnections {
    sender: watch::Sender<LiveSnapshot>,
}

impl LiveConnections {
    #[must_use]
    pub fn new() -> Self {
        Self {
            sender: watch::channel(LiveSnapshot::default()).0,
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> LiveSnapshot {
        *self.sender.borrow()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<LiveSnapshot> {
        self.sender.subscribe()
    }
}

impl Default for LiveConnections {
    fn default() -> Self {
        Self::new()
    }
}

/// One live SSE connection. Dropping it records the disconnect exactly once.
#[derive(Debug)]
pub struct LiveGuard {
    sender: watch::Sender<LiveSnapshot>,
}

impl LiveGuard {
    #[must_use]
    pub fn new(live: &LiveConnections) -> Self {
        live.sender.send_modify(|snapshot| {
            snapshot.active = snapshot.active.saturating_add(1);
            snapshot.ever_connected = true;
            snapshot.generation = snapshot.generation.wrapping_add(1);
        });
        Self {
            sender: live.sender.clone(),
        }
    }
}

impl Drop for LiveGuard {
    fn drop(&mut self) {
        self.sender.send_modify(|snapshot| {
            snapshot.active = snapshot.active.saturating_sub(1);
            snapshot.generation = snapshot.generation.wrapping_add(1);
        });
    }
}

/// Build the route tree around one application state value.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/thumb", get(thumbnail))
        .route("/preview", get(preview))
        .route("/detail", get(detail))
        .route("/download", post(download))
        .route("/live", get(live))
        .route("/quit", post(quit))
        .with_state(state)
}

/// Serve routes until the shutdown channel receives a message.
pub async fn run(
    listener: TcpListener,
    state: AppState,
    mut shutdown: broadcast::Receiver<()>,
) -> std::io::Result<()> {
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async move {
            let _ = shutdown.recv().await;
        })
        .await
}

async fn index(State(state): State<AppState>, RawQuery(raw_query): RawQuery) -> Response {
    if let Some(raw_query) = raw_query {
        let params = match serde_urlencoded::from_str::<SearchParams>(&raw_query) {
            Ok(params) => params,
            Err(error) => {
                return (
                    StatusCode::BAD_REQUEST,
                    format!("invalid search form: {error}"),
                )
                    .into_response();
            }
        };
        let query = match SearchQuery::try_from(params) {
            Ok(query) => query,
            Err(error) => return (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
        };
        let _search = state.search_gate.lock().await;
        {
            let mut session = state.session.write().await;
            session.begin_search(query.clone());
        }
        let outcomes = state.services.search_batch(&query).await;
        let mut session = state.session.write().await;
        for outcome in outcomes {
            merge_page(&mut session, outcome);
        }
    }

    let session = state.session.read().await;
    Html(render_search_page(session.view())).into_response()
}

async fn thumbnail(State(state): State<AppState>, RawQuery(raw_query): RawQuery) -> Response {
    display_image(state, raw_query, DisplayImageSize::Card).await
}

async fn preview(State(state): State<AppState>, RawQuery(raw_query): RawQuery) -> Response {
    display_image(state, raw_query, DisplayImageSize::Preview).await
}

async fn display_image(
    state: AppState,
    raw_query: Option<String>,
    size: DisplayImageSize,
) -> Response {
    let key = match parse_artwork_query(raw_query.as_deref()) {
        Ok(key) => key,
        Err(error) => return (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    };
    match state.services.load_display_image(&key, size).await {
        Ok(image) => (
            [(header::CONTENT_TYPE, image.media_type.content_type())],
            image.bytes,
        )
            .into_response(),
        Err(error) => artwork_load_response(error),
    }
}

async fn detail(State(state): State<AppState>, RawQuery(raw_query): RawQuery) -> Response {
    let key = match parse_artwork_query(raw_query.as_deref()) {
        Ok(key) => key,
        Err(error) => return (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    };
    match state.services.load_artwork(&key).await {
        Ok(artwork) => {
            let attribution = format_attribution(&artwork);
            let slug = Slug::from_title(&artwork.title);
            Html(render_detail_page(DetailView {
                artwork: &artwork,
                key: &key,
                attribution: &attribution,
                slug: slug.as_str(),
                tags: "",
                notice: None,
            }))
            .into_response()
        }
        Err(error) => artwork_load_response(error),
    }
}

async fn download(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if !is_same_origin(&state.authority, &headers) {
        return (StatusCode::FORBIDDEN, DownloadError::Validation.to_string()).into_response();
    }
    if headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        != Some("application/x-www-form-urlencoded")
    {
        return (
            StatusCode::BAD_REQUEST,
            DownloadError::Validation.to_string(),
        )
            .into_response();
    };
    let Ok(request) = DownloadRequest::parse_urlencoded(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            DownloadError::Validation.to_string(),
        )
            .into_response();
    };
    let artwork = match state.services.load_artwork(&request.key).await {
        Ok(artwork) => artwork,
        Err(error) => return artwork_load_response(error),
    };
    let attribution = format_attribution(&artwork);
    let result = state
        .services
        .download(DownloadJob {
            artwork: &artwork,
            attribution: &attribution,
            slug: &request.slug,
            tags: &request.tags,
            output: state.output.as_path(),
        })
        .await;
    let notice = DownloadNotice::from_result(result);
    Html(render_detail_page(DetailView {
        artwork: &artwork,
        key: &request.key,
        attribution: &attribution,
        slug: request.slug.as_str(),
        tags: &request.tags.as_slice().join(", "),
        notice: Some(&notice),
    }))
    .into_response()
}

async fn live(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let guard = LiveGuard::new(&state.live);
    let shutdown = state.shutdown.subscribe();
    let stream = futures::stream::unfold(
        (guard, shutdown, true),
        |(guard, mut shutdown, first)| async move {
            if first {
                return Some((
                    Ok(Event::default().event("heartbeat").data("connected")),
                    (guard, shutdown, false),
                ));
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(30)) => Some((
                    Ok(Event::default().event("heartbeat").data("connected")),
                    (guard, shutdown, false),
                )),
                _ = shutdown.recv() => None,
            }
        },
    );
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(5))
            .text("heartbeat"),
    )
}

async fn quit(State(state): State<AppState>, headers: HeaderMap) -> StatusCode {
    if !is_same_origin(&state.authority, &headers) {
        return StatusCode::FORBIDDEN;
    }
    state.quit.notify_one();
    StatusCode::NO_CONTENT
}

#[must_use]
fn is_same_origin(authority: &ListenerAuthority, headers: &HeaderMap) -> bool {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    host == authority.host && origin == authority.origin
}

fn parse_artwork_query(raw_query: Option<&str>) -> Result<ArtworkKey, ArtworkRouteError> {
    let raw_query = raw_query
        .filter(|query| !query.is_empty())
        .ok_or(ArtworkRouteError::InvalidQuery)?;
    let mut source = None;
    let mut id = None;
    for field in raw_query.split('&') {
        let (raw_name, raw_value) = field
            .split_once('=')
            .filter(|(name, _)| !name.is_empty())
            .ok_or(ArtworkRouteError::InvalidQuery)?;
        let name = decode_form_component(raw_name)?;
        let value = decode_form_component(raw_value)?;
        match name.as_str() {
            "source" if source.is_none() => source = Some(value),
            "id" if id.is_none() => id = Some(value),
            "source" | "id" => return Err(ArtworkRouteError::DuplicateField),
            _ => return Err(ArtworkRouteError::UnknownField),
        }
    }
    ArtworkKey::try_from_parts(
        source.as_deref().ok_or(ArtworkRouteError::InvalidQuery)?,
        id.as_deref().ok_or(ArtworkRouteError::InvalidQuery)?,
    )
    .map_err(ArtworkRouteError::from)
}

fn decode_form_component(raw: &str) -> Result<String, ArtworkRouteError> {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut position = 0;
    while position < bytes.len() {
        match bytes[position] {
            b'+' => {
                decoded.push(b' ');
                position += 1;
            }
            b'%' => {
                let high = bytes
                    .get(position + 1)
                    .copied()
                    .and_then(hex_value)
                    .ok_or(ArtworkRouteError::InvalidQuery)?;
                let low = bytes
                    .get(position + 2)
                    .copied()
                    .and_then(hex_value)
                    .ok_or(ArtworkRouteError::InvalidQuery)?;
                decoded.push((high << 4) | low);
                position += 3;
            }
            byte => {
                decoded.push(byte);
                position += 1;
            }
        }
    }
    String::from_utf8(decoded).map_err(|_| ArtworkRouteError::InvalidQuery)
}

#[must_use]
const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn artwork_load_response(error: ArtworkLoadError) -> Response {
    let status = match error {
        ArtworkLoadError::SourceUnavailable => StatusCode::NOT_FOUND,
        ArtworkLoadError::ArtworkUnavailable | ArtworkLoadError::ImageUnavailable => {
            StatusCode::BAD_GATEWAY
        }
    };
    (status, error.to_string()).into_response()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
enum ArtworkRouteError {
    #[error("the artwork query is invalid")]
    InvalidQuery,
    #[error("the artwork query contains a duplicate field")]
    DuplicateField,
    #[error("the artwork query contains an unknown field")]
    UnknownField,
    #[error(transparent)]
    InvalidKey(#[from] ArtworkKeyError),
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use axum::body::{Body, to_bytes};
    use axum::http::{Request, header};
    use axum::routing::get;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;
    use tokio::task::JoinHandle;
    use tower::ServiceExt;
    use url::Url;

    use crate::cache::Cache;
    use crate::core::{
        Artwork, CommercialLicense, Culture, ImageUrls, ProviderOutcome, ProviderPage, QueryText,
        SearchQuery, SourceKind, SourceSet, merge_page,
    };
    use crate::http::HttpClient;
    use crate::providers::ProviderSet;
    use crate::rate_limit::RateLimiters;

    use super::*;

    struct MockHarness {
        state: AppState,
        requests: Arc<AtomicUsize>,
        task: JoinHandle<()>,
        _cache: TempDir,
        output: TempDir,
    }

    impl Drop for MockHarness {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn counted_detail(State(state): State<(Arc<AtomicUsize>, String)>) -> String {
        state.0.fetch_add(1, Ordering::SeqCst);
        serde_json::json!({
            "data": {
                "id": 1001,
                "title": "Ceremonial Mask",
                "artist_display": "Maker unknown",
                "date_display": "1900-1920",
                "place_of_origin": "Côte d’Ivoire",
                "image_id": "image-one",
                "is_public_domain": true,
                "credit_line": "Gift of A & B"
            },
            "config": { "iiif_url": format!("{}/iiif/2", state.1) }
        })
        .to_string()
    }

    async fn counted_image(State(requests): State<Arc<AtomicUsize>>) -> Vec<u8> {
        requests.fetch_add(1, Ordering::SeqCst);
        vec![0xff, 0xd8, 0x01, 0xff, 0xd9]
    }

    async fn counted_original(State(requests): State<Arc<AtomicUsize>>) -> Vec<u8> {
        requests.fetch_add(1, Ordering::SeqCst);
        include_str!("../tests/fixtures/images/native-jpeg.hex")
            .trim()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                std::str::from_utf8(pair)
                    .ok()
                    .and_then(|text| u8::from_str_radix(text, 16).ok())
                    .unwrap_or_default()
            })
            .collect()
    }

    async fn mock_harness() -> MockHarness {
        let requests = Arc::new(AtomicUsize::new(0));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock listener binds");
        let address = listener.local_addr().expect("mock address exists");
        let base = format!("http://{address}");
        let app = Router::new()
            .route("/api/v1/artworks/1001", get(counted_detail))
            .with_state((requests.clone(), base.clone()))
            .merge(
                Router::new()
                    .route(
                        "/iiif/2/image-one/full/200,/0/default.jpg",
                        get(counted_image),
                    )
                    .route(
                        "/iiif/2/image-one/full/843,/0/default.jpg",
                        get(counted_image),
                    )
                    .route(
                        "/iiif/2/image-one/full/full/0/default.jpg",
                        get(counted_original),
                    )
                    .with_state(requests.clone()),
            );
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("mock server runs");
        });
        let query = SearchQuery {
            query: QueryText::parse("mask").expect("query is valid"),
            sources: SourceSet::parse(&SourceKind::ALL).expect("sources are valid"),
            culture: Culture::parse(None),
        };
        let cache = tempfile::tempdir().expect("temporary cache exists");
        let output = tempfile::tempdir().expect("temporary output exists");
        let endpoint =
            Url::parse(&format!("{base}/api/v1/artworks/search")).expect("mock endpoint is valid");
        let cleveland = crate::providers::cleveland::ClevelandProvider::official_endpoint()
            .expect("built-in Cleveland endpoint is valid");
        let mut session = SearchSession::new(query);
        merge_page(
            &mut session,
            ProviderOutcome::Success(ProviderPage {
                source: SourceKind::ArtInstituteChicago,
                artworks: vec![Artwork {
                    source: SourceKind::ArtInstituteChicago,
                    source_id: "1001".into(),
                    title: "Accumulated Mask".into(),
                    creator: None,
                    date: None,
                    culture: None,
                    license: CommercialLicense::PublicDomain,
                    image_urls: ImageUrls {
                        thumbnail: "https://trusted.test/thumb.jpg".into(),
                        display: "https://trusted.test/display.jpg".into(),
                        original: None,
                    },
                    institution: "Art Institute of Chicago".into(),
                    provider_credit: None,
                    object_url: "https://www.artic.edu/artworks/1001".into(),
                }],
                next_cursor: None,
            }),
        );
        let (shutdown, _) = broadcast::channel(1);
        let state = AppState::new(
            session,
            SearchServices::new(
                ProviderSet::with_endpoints(endpoint, cleveland),
                Cache::new(cache.path().to_path_buf()),
                HttpClient::new(),
                RateLimiters::new(),
            ),
            OutputDirectory::from_verified_path(output.path().to_path_buf()),
            SocketAddr::from(([127, 0, 0, 1], 45_123)),
            shutdown,
        );
        MockHarness {
            state,
            requests,
            task,
            _cache: cache,
            output,
        }
    }

    fn state(query: &str) -> AppState {
        let query = SearchQuery {
            query: QueryText::parse(query).expect("query is valid"),
            sources: SourceSet::parse(&SourceKind::ALL).expect("sources are valid"),
            culture: Culture::parse(None),
        };
        let (shutdown, _) = broadcast::channel(1);
        AppState::new(
            SearchSession::new(query),
            SearchServices::new(
                ProviderSet::from_env().expect("provider configuration is valid"),
                Cache::new(std::env::temp_dir().join("cceroby-server-tests")),
                HttpClient::new(),
                RateLimiters::new(),
            ),
            OutputDirectory::from_verified_path(std::env::temp_dir()),
            SocketAddr::from(([127, 0, 0, 1], 45_123)),
            shutdown,
        )
    }

    #[test]
    fn application_state_exposes_its_shutdown_channel() {
        let state = state("mask");
        assert_eq!(state.shutdown_sender().receiver_count(), 0);
        let _receiver = state.shutdown_sender().subscribe();
        assert_eq!(state.shutdown_sender().receiver_count(), 1);
    }

    #[tokio::test]
    async fn root_route_renders_the_seeded_query() {
        let response = router(state("seeded mask"))
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .expect("request is valid"),
            )
            .await
            .expect("request succeeds");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body is readable");
        let html = String::from_utf8(body.to_vec()).expect("body is utf-8");
        assert!(html.contains("value=\"seeded mask\""));
    }

    #[tokio::test]
    async fn root_route_preserves_valid_form_values() {
        let response = router(state("seed"))
            .oneshot(
                Request::builder()
                    .uri("/?query=bronze+mask&met=true&culture=Japan")
                    .body(Body::empty())
                    .expect("request is valid"),
            )
            .await
            .expect("request succeeds");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body is readable");
        let html = String::from_utf8(body.to_vec()).expect("body is utf-8");
        assert!(html.contains("value=\"bronze mask\""));
        assert!(html.contains("name=\"met\" value=\"true\" checked"));
        assert!(html.contains("value=\"Japan\""));
    }

    #[tokio::test]
    async fn root_route_returns_notices_for_unavailable_sources() {
        let response = router(state("mask"))
            .oneshot(
                Request::builder()
                    .uri("/?query=mask&cleveland=true&met=true&smithsonian=true&wikimedia=true")
                    .body(Body::empty())
                    .expect("request is valid"),
            )
            .await
            .expect("request succeeds");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body is readable");
        let html = String::from_utf8(body.to_vec()).expect("body is utf-8");
        assert_eq!(html.matches("is not available in this build").count(), 3);
    }

    #[tokio::test]
    async fn root_route_rejects_a_form_without_sources() {
        let response = router(state("seed"))
            .oneshot(
                Request::builder()
                    .uri("/?query=mask")
                    .body(Body::empty())
                    .expect("request is valid"),
            )
            .await
            .expect("request succeeds");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn artwork_query_parser_accepts_exact_fields_in_any_order() {
        let first = parse_artwork_query(Some("source=aic&id=1001")).expect("query is valid");
        let second = parse_artwork_query(Some("id=1001&source=aic")).expect("query is valid");
        assert_eq!(first, second);
        assert_eq!(first.source(), SourceKind::ArtInstituteChicago);
        assert_eq!(first.id().as_str(), "1001");
    }

    #[test]
    fn form_component_decoder_handles_valid_encoding_and_rejects_malformed_bytes() {
        assert_eq!(decode_form_component("aic"), Ok("aic".into()));
        assert_eq!(
            decode_form_component("File%3AMask+One"),
            Ok("File:Mask One".into())
        );
        for raw in ["%", "%0", "%GG", "%ff"] {
            assert_eq!(
                decode_form_component(raw),
                Err(ArtworkRouteError::InvalidQuery),
                "{raw}"
            );
        }
    }

    #[test]
    fn hexadecimal_decoder_accepts_both_cases_and_rejects_non_hexadecimal_bytes() {
        assert_eq!(hex_value(b'0'), Some(0));
        assert_eq!(hex_value(b'9'), Some(9));
        assert_eq!(hex_value(b'a'), Some(10));
        assert_eq!(hex_value(b'F'), Some(15));
        assert_eq!(hex_value(b'g'), None);
        assert_eq!(hex_value(b'/'), None);
    }

    #[test]
    fn artwork_query_parser_rejects_missing_duplicate_unknown_and_url_fields() {
        let cases = [
            (None, ArtworkRouteError::InvalidQuery),
            (Some(""), ArtworkRouteError::InvalidQuery),
            (Some("source=aic"), ArtworkRouteError::InvalidQuery),
            (Some("source=aic&&id=1001"), ArtworkRouteError::InvalidQuery),
            (Some("source=aic&id=1001&"), ArtworkRouteError::InvalidQuery),
            (Some("source=aic&id"), ArtworkRouteError::InvalidQuery),
            (Some("source=aic&id=%"), ArtworkRouteError::InvalidQuery),
            (Some("source=aic&id=%ff"), ArtworkRouteError::InvalidQuery),
            (
                Some("source=aic&source=met&id=1001"),
                ArtworkRouteError::DuplicateField,
            ),
            (
                Some("source=aic&id=1001&id=1002"),
                ArtworkRouteError::DuplicateField,
            ),
            (
                Some("source=aic&id=1001&url=https%3A%2F%2Fevil.test"),
                ArtworkRouteError::UnknownField,
            ),
        ];
        for (query, expected) in cases {
            assert_eq!(parse_artwork_query(query), Err(expected));
        }
        assert!(matches!(
            parse_artwork_query(Some("source=unknown&id=1001")),
            Err(ArtworkRouteError::InvalidKey(
                ArtworkKeyError::UnknownSource
            ))
        ));
        assert!(matches!(
            parse_artwork_query(Some("source=aic&id=https%3A%2F%2Fevil.test%2Fimage.jpg")),
            Err(ArtworkRouteError::InvalidKey(ArtworkKeyError::MalformedId))
        ));
    }

    #[tokio::test]
    async fn artwork_load_errors_map_directly_to_short_safe_responses() {
        let cases = [
            (
                ArtworkLoadError::SourceUnavailable,
                StatusCode::NOT_FOUND,
                "the artwork source is not available",
            ),
            (
                ArtworkLoadError::ArtworkUnavailable,
                StatusCode::BAD_GATEWAY,
                "the artwork is not available",
            ),
            (
                ArtworkLoadError::ImageUnavailable,
                StatusCode::BAD_GATEWAY,
                "the artwork image is not available",
            ),
        ];
        for (error, status, expected_body) in cases {
            let response = artwork_load_response(error);
            assert_eq!(response.status(), status);
            let body = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body is readable");
            assert_eq!(body.as_ref(), expected_body.as_bytes());
        }
    }

    #[tokio::test]
    async fn thumbnail_and_preview_routes_fetch_provider_images_and_use_the_byte_cache() {
        let harness = mock_harness().await;
        let app = router(harness.state.clone());
        for path in [
            "/thumb?source=aic&id=1001",
            "/thumb?source=aic&id=1001",
            "/preview?source=aic&id=1001",
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .body(Body::empty())
                        .expect("request is valid"),
                )
                .await
                .expect("request succeeds");
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers().get(header::CONTENT_TYPE),
                Some(&header::HeaderValue::from_static("image/jpeg"))
            );
            let body = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body is readable");
            assert_eq!(body.as_ref(), [0xff, 0xd8, 0x01, 0xff, 0xd9]);
        }
        assert_eq!(harness.requests.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn detail_route_reconstructs_trusted_artwork_and_renders_local_preview() {
        let harness = mock_harness().await;
        let response = router(harness.state.clone())
            .oneshot(
                Request::builder()
                    .uri("/detail?source=aic&id=1001")
                    .body(Body::empty())
                    .expect("request is valid"),
            )
            .await
            .expect("request succeeds");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body is readable");
        let html = String::from_utf8(body.to_vec()).expect("body is UTF-8");
        assert!(html.contains("Ceremonial Mask"));
        assert!(html.contains("Gift of A &amp; B"));
        assert!(html.contains("/preview?source=aic&amp;id=1001"));
        assert!(!html.contains("/iiif/2/image-one"));
        assert_eq!(harness.requests.load(Ordering::SeqCst), 1);

        let back = router(harness.state.clone())
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .expect("request is valid"),
            )
            .await
            .expect("request succeeds");
        let back_body = to_bytes(back.into_body(), usize::MAX)
            .await
            .expect("body is readable");
        let back_html = String::from_utf8(back_body.to_vec()).expect("body is UTF-8");
        assert!(back_html.contains("Accumulated Mask"));
        assert!(back_html.contains("/detail?source=aic&amp;id=1001"));
    }

    #[tokio::test]
    async fn artwork_handlers_reject_unknown_malformed_extra_duplicate_and_url_input() {
        let app = router(state("mask"));
        let invalid_queries = [
            "source=unknown&id=1",
            "source=aic&id=bad",
            "source=aic&&id=1",
            "source=aic&id=1&",
            "source=aic&id",
            "source=aic&id=1&extra=x",
            "source=aic&id=1&id=2",
            "source=aic&id=https%3A%2F%2Fevil.test%2Fimage.jpg",
            "source=aic&id=1&url=https%3A%2F%2Fevil.test%2Fimage.jpg",
        ];
        for route in ["/thumb", "/preview", "/detail"] {
            for query in invalid_queries {
                let response = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .uri(format!("{route}?{query}"))
                            .body(Body::empty())
                            .expect("request is valid"),
                    )
                    .await
                    .expect("request succeeds");
                assert_eq!(
                    response.status(),
                    StatusCode::BAD_REQUEST,
                    "{route}?{query}"
                );
                let body = to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("body is readable");
                let message = String::from_utf8(body.to_vec()).expect("body is UTF-8");
                assert!(!message.contains("evil.test"));
                assert!(!message.contains("https://"));
            }
        }

        let unavailable = app
            .oneshot(
                Request::builder()
                    .uri("/detail?source=met&id=1")
                    .body(Body::empty())
                    .expect("request is valid"),
            )
            .await
            .expect("request succeeds");
        assert_eq!(unavailable.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn live_guard_counts_each_connection_and_cannot_underflow() {
        let live = LiveConnections::new();
        assert_eq!(live.snapshot(), LiveSnapshot::default());
        let first = LiveGuard::new(&live);
        let second = LiveGuard::new(&live);
        assert_eq!(live.snapshot().active, 2);
        assert!(live.snapshot().ever_connected);
        drop(first);
        assert_eq!(live.snapshot().active, 1);
        drop(second);
        assert_eq!(live.snapshot().active, 0);
    }

    #[path = "download_tests.rs"]
    mod download_tests;

    #[path = "lifecycle_tests.rs"]
    mod lifecycle_tests;
}
