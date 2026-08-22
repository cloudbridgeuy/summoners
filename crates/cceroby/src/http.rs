//! Provider HTTP shell.

use std::time::Duration;

use thiserror::Error;

use crate::providers::HttpRequest;

/// Cloneable outbound HTTP client.
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct HttpClient {
    client: reqwest::Client,
    timeout: Duration,
}

impl HttpClient {
    #[must_use]
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            timeout: HTTP_TIMEOUT,
        }
    }

    pub async fn execute(&self, request: &HttpRequest) -> Result<Vec<u8>, HttpError> {
        let response = self
            .client
            .get(request.url().clone())
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

    use axum::Router;
    use axum::routing::get;
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
            client: reqwest::Client::new(),
            timeout: Duration::from_millis(10),
        };

        assert_eq!(
            client.execute(&request).await,
            Err(HttpError::RequestFailed)
        );
        task.abort();
    }
}
