use super::*;

#[test]
fn trusted_authority_accepts_the_exact_http_origin_and_host() {
    let authority = ListenerAuthority::from_address(SocketAddr::from(([127, 0, 0, 1], 45_123)));
    let mut headers = HeaderMap::new();
    headers.insert(
        header::HOST,
        "127.0.0.1:45123".parse().expect("host is valid"),
    );
    headers.insert(
        header::ORIGIN,
        "http://127.0.0.1:45123".parse().expect("origin is valid"),
    );
    assert!(is_same_origin(&authority, &headers));
}

#[test]
fn trusted_authority_rejects_missing_mismatched_and_matching_hostile_headers() {
    let authority = ListenerAuthority::from_address(SocketAddr::from(([127, 0, 0, 1], 45_123)));
    let mut headers = HeaderMap::new();
    headers.insert(
        header::HOST,
        "127.0.0.1:45123".parse().expect("host is valid"),
    );

    for origin in [
        "https://127.0.0.1:45123",
        "http://127.0.0.1:45124",
        "https://evil.test",
        "null",
    ] {
        headers.insert(header::ORIGIN, origin.parse().expect("origin is valid"));
        assert!(!is_same_origin(&authority, &headers), "{origin}");
    }
    headers.remove(header::ORIGIN);
    assert!(!is_same_origin(&authority, &headers));
    headers.remove(header::HOST);
    assert!(!is_same_origin(&authority, &headers));

    headers.insert(
        header::HOST,
        "evil.test:45123".parse().expect("host is valid"),
    );
    headers.insert(
        header::ORIGIN,
        "http://evil.test:45123".parse().expect("origin is valid"),
    );
    assert!(!is_same_origin(&authority, &headers));
}

#[tokio::test]
async fn download_handler_rejects_all_browser_controlled_trust_fields_before_io() {
    let harness = mock_harness().await;
    let app = router(harness.state.clone());
    let hostile = [
        "source=aic&id=1001&slug=mask&tags=&url=https%3A%2F%2Fevil.test",
        "source=aic&id=1001&slug=mask&tags=&license=CC0",
        "source=aic&id=1001&slug=mask&tags=&attribution=evil",
        "source=aic&id=1001&slug=mask&tags=&output_path=%2Ftmp%2Fevil",
        "source=aic&id=1001&slug=mask&tags=&remote_request=evil",
        "source=aic&id=1001&slug=mask&slug=other&tags=",
        "source=aic&id=1001&slug=mask&tags=&tags=other",
        "source=aic&id=1001&slug=mask",
    ];
    for body in hostile {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/download")
                    .header(header::HOST, "127.0.0.1:45123")
                    .header(header::ORIGIN, "http://127.0.0.1:45123")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(body))
                    .expect("request is valid"),
            )
            .await
            .expect("request succeeds");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body is readable");
        assert_eq!(bytes.as_ref(), b"the download form is invalid");
    }
    assert_no_download_io(&harness);
}

#[tokio::test]
async fn download_handler_rejects_malformed_form_encoding_before_io() {
    let harness = mock_harness().await;
    let app = router(harness.state.clone());
    for body in [
        b"source=aic&id=1001&slug=mask%&tags=".as_slice(),
        b"source=aic&id=1001&slug=mask%GG&tags=".as_slice(),
        b"source=aic&id=1001&slug=mask%ff&tags=".as_slice(),
        b"source=aic&id=1001&slug=mask&tags=\xff".as_slice(),
    ] {
        let response = app
            .clone()
            .oneshot(same_origin_form_request(Body::from(body)))
            .await
            .expect("request succeeds");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert_no_download_io(&harness);
}

#[tokio::test]
async fn download_handler_rejects_hostile_origin_before_io() {
    let harness = mock_harness().await;
    let response = router(harness.state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/download")
                .header(header::HOST, "127.0.0.1:45123")
                .header(header::ORIGIN, "https://evil.test")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(valid_body()))
                .expect("request is valid"),
        )
        .await
        .expect("request succeeds");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_no_download_io(&harness);
}

#[tokio::test]
async fn download_handler_rejects_matching_hostile_authority_before_io() {
    let harness = mock_harness().await;
    let response = router(harness.state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/download")
                .header(header::HOST, "evil.test:45123")
                .header(header::ORIGIN, "http://evil.test:45123")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(valid_body()))
                .expect("request is valid"),
        )
        .await
        .expect("request succeeds");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_no_download_io(&harness);
}

#[tokio::test]
async fn download_handler_requires_urlencoded_content_type_before_io() {
    let harness = mock_harness().await;
    let response = router(harness.state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/download")
                .header(header::HOST, "127.0.0.1:45123")
                .header(header::ORIGIN, "http://127.0.0.1:45123")
                .header(header::CONTENT_TYPE, "text/plain")
                .body(Body::from(valid_body()))
                .expect("request is valid"),
        )
        .await
        .expect("request succeeds");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_no_download_io(&harness);
}

#[tokio::test]
async fn same_origin_mocked_download_refetches_and_replaces_one_self_contained_jpeg() {
    let harness = mock_harness().await;
    let app = router(harness.state.clone());
    let body = "source=aic&id=1001&slug=ceremonial-mask&tags=ritual%2Cblue";
    for expected in ["Created", "Replaced"] {
        let response = app
            .clone()
            .oneshot(same_origin_form_request(Body::from(body)))
            .await
            .expect("request succeeds");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body is readable");
        let html = String::from_utf8(bytes.to_vec()).expect("body is UTF-8");
        assert!(html.contains(expected));
        assert!(html.contains("Ceremonial Mask"));
        assert!(html.contains("Gift of A &amp; B"));
    }

    let target = harness.output.path().join("ceremonial-mask.jpg");
    let bytes = std::fs::read(&target).expect("asset reads");
    let jpeg =
        img_parts::jpeg::Jpeg::from_bytes(img_parts::Bytes::from(bytes)).expect("asset is a JPEG");
    let xmp = jpeg
        .segments()
        .iter()
        .filter(|segment| {
            segment.marker() == img_parts::jpeg::markers::APP1
                && segment.contents().starts_with(crate::xmp::XMP_IDENTIFIER)
        })
        .collect::<Vec<_>>();
    assert_eq!(xmp.len(), 1);
    let xml = std::str::from_utf8(&xmp[0].contents()[crate::xmp::XMP_IDENTIFIER.len()..])
        .expect("XMP is UTF-8");
    assert!(xml.contains("<rdf:li>ritual</rdf:li><rdf:li>blue</rdf:li>"));
    assert!(xml.contains("Gift of A &amp; B"));
    assert_eq!(harness.requests.load(Ordering::SeqCst), 3);
    let entries = std::fs::read_dir(harness.output.path())
        .expect("output reads")
        .collect::<Result<Vec<_>, _>>()
        .expect("entries read");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path(), target);
}

#[tokio::test]
async fn invalid_slug_is_rejected_before_trusted_detail_io() {
    let harness = mock_harness().await;
    let response = router(harness.state.clone())
        .oneshot(same_origin_form_request(Body::from(
            "source=aic&id=1001&slug=..%2Fescape&tags=mask",
        )))
        .await
        .expect("request succeeds");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body is readable");
    assert_eq!(bytes.as_ref(), b"the download form is invalid");
    assert_no_download_io(&harness);
}

fn same_origin_form_request(body: Body) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/download")
        .header(header::HOST, "127.0.0.1:45123")
        .header(header::ORIGIN, "http://127.0.0.1:45123")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(body)
        .expect("request is valid")
}

fn assert_no_download_io(harness: &MockHarness) {
    assert_eq!(harness.requests.load(Ordering::SeqCst), 0);
    assert_eq!(
        std::fs::read_dir(harness.output.path())
            .expect("output reads")
            .count(),
        0
    );
}

fn valid_body() -> &'static str {
    "source=aic&id=1001&slug=ceremonial-mask&tags=ritual"
}
