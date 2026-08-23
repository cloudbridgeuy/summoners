use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use super::*;

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
                .uri("/detail?source=smithsonian&id=edanmdm%3ANMAFA_1")
                .body(Body::empty())
                .expect("request is valid"),
        )
        .await
        .expect("request succeeds");
    assert_eq!(unavailable.status(), StatusCode::NOT_FOUND);
}
