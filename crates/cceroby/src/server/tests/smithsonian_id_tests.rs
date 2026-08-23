//! Direct route tests for Smithsonian object identifiers.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use super::*;

#[tokio::test]
async fn smithsonian_routes_accept_dotted_ids_and_reject_hostile_ids() {
    let app = router(state("mask"));
    let invalid_ids = [
        ".",
        "..",
        "0",
        "arbitrary+text",
        "https%3A%2F%2Fevil.test%2Fobject",
        "edanmdm%3A.",
        "edanmdm%3A..",
    ];
    for route in ["/preview", "/detail"] {
        for id in invalid_ids {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("{route}?source=smithsonian&id={id}"))
                        .body(Body::empty())
                        .expect("request is valid"),
                )
                .await
                .expect("request succeeds");
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{route} {id}");
        }
        let valid = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "{route}?source=smithsonian&id=edanmdm%3Afsg_F1900.1"
                    ))
                    .body(Body::empty())
                    .expect("request is valid"),
            )
            .await
            .expect("request succeeds");
        assert_eq!(valid.status(), StatusCode::NOT_FOUND, "{route}");
    }
}
