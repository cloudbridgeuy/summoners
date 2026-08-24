//! Provider HTTP shell.

use std::time::Duration;

use thiserror::Error;

use crate::providers::HttpRequest;

/// Cloneable outbound HTTP client.
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct HttpClient {
    client: Option<reqwest::Client>,
    timeout: Duration,
}

impl HttpClient {
    #[must_use]
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .ok(),
            timeout: HTTP_TIMEOUT,
        }
    }

    pub async fn execute(&self, request: &HttpRequest) -> Result<Vec<u8>, HttpError> {
        let client = self.client.as_ref().ok_or(HttpError::RequestFailed)?;
        let response = client
            .get(request.url().clone())
            .headers(request.headers().clone())
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|_| HttpError::RequestFailed)?;
        if !response.status().is_success() {
            return Err(HttpError::UnsuccessfulStatus(response.status().as_u16()));
        }
        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|_| HttpError::ResponseFailed)
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

/// A short transport failure safe for provider-level degradation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HttpError {
    #[error("the provider request failed")]
    RequestFailed,
    #[error("the provider returned HTTP {0}")]
    UnsuccessfulStatus(u16),
    #[error("the provider response could not be read")]
    ResponseFailed,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::convert::Infallible;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::Router;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode, header::LOCATION};
    use axum::routing::get;
    use reqwest::header::{HeaderName, HeaderValue};
    use tokio::net::TcpListener;
    use url::Url;

    use super::*;

    #[test]
    fn client_has_an_explicit_fifteen_second_timeout() {
        assert_eq!(HttpClient::new().timeout, Duration::from_secs(15));
    }

    #[tokio::test]
    async fn request_timeout_becomes_a_short_transport_error() {
        async fn slow_response() -> Result<&'static str, Infallible> {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok("late")
        }

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock listener binds");
        let address = listener.local_addr().expect("mock address exists");
        let task = tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/slow", get(slow_response)))
                .await
                .expect("mock server runs");
        });
        let request = HttpRequest::get(
            Url::parse(&format!("http://{address}/slow")).expect("mock URL is valid"),
        );
        let client = HttpClient {
            client: Some(reqwest::Client::new()),
            timeout: Duration::from_millis(10),
        };

        assert_eq!(
            client.execute(&request).await,
            Err(HttpError::RequestFailed)
        );
        task.abort();
    }

    #[tokio::test]
    async fn request_applies_provider_owned_headers() {
        async fn read_provider_header(headers: HeaderMap) -> String {
            headers
                .get("x-provider-test")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("missing")
                .to_owned()
        }

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock listener binds");
        let address = listener.local_addr().expect("mock address exists");
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route("/header", get(read_provider_header)),
            )
            .await
            .expect("mock server runs");
        });
        let request = HttpRequest::get(
            Url::parse(&format!("http://{address}/header")).expect("mock URL is valid"),
        )
        .with_header(
            HeaderName::from_static("x-provider-test"),
            HeaderValue::from_static("provider-owned"),
        );

        assert_eq!(
            HttpClient::new().execute(&request).await,
            Ok(b"provider-owned".to_vec())
        );
        task.abort();
    }

    #[tokio::test]
    async fn cross_origin_redirect_does_not_forward_a_custom_secret_header() {
        async fn read_secret_header(
            State(requests): State<Arc<AtomicUsize>>,
            headers: HeaderMap,
        ) -> String {
            requests.fetch_add(1, Ordering::SeqCst);
            headers
                .get("x-api-key")
                .map_or_else(|| "missing".to_owned(), |_| "received".to_owned())
        }

        let target_requests = Arc::new(AtomicUsize::new(0));
        let target_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("target listener binds");
        let target_address = target_listener.local_addr().expect("target address exists");
        let target_state = target_requests.clone();
        let target_task = tokio::spawn(async move {
            axum::serve(
                target_listener,
                Router::new()
                    .route("/target", get(read_secret_header))
                    .with_state(target_state),
            )
            .await
            .expect("target server runs");
        });

        let redirect_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("redirect listener binds");
        let redirect_address = redirect_listener
            .local_addr()
            .expect("redirect address exists");
        let location = format!("http://{target_address}/target");
        let redirect_task = tokio::spawn(async move {
            let app = Router::new().route(
                "/redirect",
                get(move || async move {
                    (
                        StatusCode::FOUND,
                        [(LOCATION, location.clone())],
                        "redirect",
                    )
                }),
            );
            axum::serve(redirect_listener, app)
                .await
                .expect("redirect server runs");
        });

        let request = HttpRequest::get(
            Url::parse(&format!("http://{redirect_address}/redirect"))
                .expect("redirect URL is valid"),
        )
        .with_header(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_static("must-not-cross-origin"),
        );

        assert_eq!(
            HttpClient::new().execute(&request).await,
            Err(HttpError::UnsuccessfulStatus(302))
        );
        assert_eq!(target_requests.load(Ordering::SeqCst), 0);
        redirect_task.abort();
        target_task.abort();
    }
}
