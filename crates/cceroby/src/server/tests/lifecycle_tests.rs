//! Direct route tests for browser liveness and protected shutdown.

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use super::*;

#[tokio::test]
async fn live_route_has_exact_sse_headers_heartbeat_and_shutdown_close() {
    let state = state("mask");
    let live = state.live_connections();
    let shutdown = state.shutdown_sender();
    let response = router(state)
        .oneshot(
            Request::builder()
                .uri("/live")
                .body(Body::empty())
                .expect("request is valid"),
        )
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream")
    );
    assert_eq!(
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-cache")
    );
    assert_eq!(live.snapshot().active, 1);
    let _ = shutdown.send(());
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("SSE body closes after shutdown");
    assert_eq!(body, "event: heartbeat\ndata: connected\n\n");
    assert_eq!(live.snapshot().active, 0);
}

#[tokio::test]
async fn quit_rejects_wrong_authority_before_it_notifies_shutdown() {
    let state = state("mask");
    let quit = state.quit_notifier();
    let app = router(state);
    for (host, origin) in [
        ("127.0.0.1:45123", "https://evil.test"),
        ("evil.test:45123", "http://evil.test:45123"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/quit")
                    .header(header::HOST, host)
                    .header(header::ORIGIN, origin)
                    .body(Body::empty())
                    .expect("request is valid"),
            )
            .await
            .expect("request succeeds");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(
            tokio::time::timeout(Duration::from_millis(1), quit.notified())
                .await
                .is_err()
        );
    }

    for (request, expected) in [
        (
            Request::builder()
                .method("POST")
                .uri("/quit")
                .body(Body::empty())
                .expect("request is valid"),
            StatusCode::FORBIDDEN,
        ),
        (
            Request::builder()
                .method("GET")
                .uri("/quit")
                .header(header::HOST, "127.0.0.1:45123")
                .body(Body::empty())
                .expect("request is valid"),
            StatusCode::METHOD_NOT_ALLOWED,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(request)
            .await
            .expect("request succeeds");
        assert_eq!(response.status(), expected);
        assert!(
            tokio::time::timeout(Duration::from_millis(1), quit.notified())
                .await
                .is_err()
        );
    }

    let accepted = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/quit")
                .header(header::HOST, "127.0.0.1:45123")
                .header(header::ORIGIN, "http://127.0.0.1:45123")
                .body(Body::empty())
                .expect("request is valid"),
        )
        .await
        .expect("request succeeds");
    assert_eq!(accepted.status(), StatusCode::NO_CONTENT);
    tokio::time::timeout(Duration::from_secs(1), quit.notified())
        .await
        .expect("accepted quit notifies shutdown");
}
