//! HTTP and shared-state shell for the local search page.

use std::sync::Arc;

use axum::Router;
use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, RwLock, broadcast};

use crate::core::{SearchParams, SearchQuery, SearchSession, merge_page};
use crate::render::render_search_page;
use crate::search::SearchServices;

/// Process resources shared by local HTTP handlers.
#[derive(Debug, Clone)]
pub struct AppState {
    session: Arc<RwLock<SearchSession>>,
    services: SearchServices,
    search_gate: Arc<Mutex<()>>,
    shutdown: broadcast::Sender<()>,
}

impl AppState {
    #[must_use]
    pub fn new(
        session: SearchSession,
        services: SearchServices,
        shutdown: broadcast::Sender<()>,
    ) -> Self {
        Self {
            session: Arc::new(RwLock::new(session)),
            services,
            search_gate: Arc::new(Mutex::new(())),
            shutdown,
        }
    }

    #[must_use]
    pub fn shutdown_sender(&self) -> broadcast::Sender<()> {
        self.shutdown.clone()
    }
}

/// Build the route tree around one application state value.
pub fn router(state: AppState) -> Router {
    Router::new().route("/", get(index)).with_state(state)
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    use crate::cache::Cache;
    use crate::core::{Culture, QueryText, SearchQuery, SourceKind, SourceSet};
    use crate::http::HttpClient;
    use crate::providers::ProviderSet;
    use crate::rate_limit::RateLimiters;

    use super::*;

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
        assert_eq!(html.matches("is not available in this build").count(), 4);
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
}
