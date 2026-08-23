//! Per-provider token buckets with pure acquisition decisions.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::core::SourceKind;
use crate::providers::{Provider, RatePolicy, TokenBucketPolicy};

/// Shared rate-limit state for each available provider.
#[derive(Debug, Clone)]
pub struct RateLimiters {
    buckets: Arc<Mutex<HashMap<SourceKind, Arc<TokenBucket>>>>,
}

impl RateLimiters {
    #[must_use]
    pub fn new() -> Self {
        Self {
            buckets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn acquire(&self, provider: &dyn Provider) {
        let RatePolicy::TokenBucket(policy) = provider.rate_policy() else {
            return;
        };
        let bucket = {
            let mut buckets = self.buckets.lock().await;
            buckets
                .entry(provider.kind())
                .or_insert_with(|| Arc::new(TokenBucket::new(policy)))
                .clone()
        };
        bucket.acquire().await;
    }
}

impl Default for RateLimiters {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
struct TokenBucket {
    capacity: f64,
    tokens_per_second: f64,
    state: Mutex<BucketState>,
}

impl TokenBucket {
    fn new(policy: TokenBucketPolicy) -> Self {
        let capacity = f64::from(policy.capacity().get());
        Self {
            capacity,
            tokens_per_second: 1.0 / policy.refill_interval().as_secs_f64(),
            state: Mutex::new(BucketState {
                available: capacity,
                updated_at: Instant::now(),
            }),
        }
    }

    async fn acquire(&self) {
        loop {
            let wait = {
                let now = Instant::now();
                let mut state = self.state.lock().await;
                let elapsed = now.saturating_duration_since(state.updated_at);
                match decide_acquire(
                    state.available,
                    elapsed,
                    self.capacity,
                    self.tokens_per_second,
                ) {
                    TokenDecision::Granted { remaining } => {
                        state.available = remaining;
                        state.updated_at = now;
                        None
                    }
                    TokenDecision::Wait {
                        available,
                        duration,
                    } => {
                        state.available = available;
                        state.updated_at = now;
                        Some(duration)
                    }
                }
            };
            match wait {
                Some(duration) => tokio::time::sleep(duration).await,
                None => return,
            }
        }
    }
}

#[derive(Debug)]
struct BucketState {
    available: f64,
    updated_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TokenDecision {
    Granted { remaining: f64 },
    Wait { available: f64, duration: Duration },
}

#[must_use]
fn decide_acquire(
    available: f64,
    elapsed: Duration,
    capacity: f64,
    tokens_per_second: f64,
) -> TokenDecision {
    let replenished = (available + elapsed.as_secs_f64() * tokens_per_second).min(capacity);
    if replenished >= 1.0 {
        TokenDecision::Granted {
            remaining: replenished - 1.0,
        }
    } else {
        TokenDecision::Wait {
            available: replenished,
            duration: Duration::from_secs_f64((1.0 - replenished) / tokens_per_second),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::num::NonZeroU32;

    use url::Url;

    use crate::artwork::ArtworkKey;
    use crate::core::{Artwork, SearchQuery};
    use crate::providers::{
        ArtworkDropReason, DisplayImageRequest, DisplayImageSize, HttpRequest, ProviderCandidate,
        ProviderError, ProviderSearchPage,
    };

    use super::*;

    struct TestProvider {
        source: SourceKind,
        policy: RatePolicy,
    }

    impl Provider for TestProvider {
        fn kind(&self) -> SourceKind {
            self.source
        }

        fn rate_policy(&self) -> RatePolicy {
            self.policy
        }

        fn search_request(&self, _query: &SearchQuery, _cursor: Option<&str>) -> HttpRequest {
            HttpRequest::get(Url::parse("https://example.test/search").expect("URL is valid"))
        }

        fn parse_search(
            &self,
            _bytes: &[u8],
            _cursor: Option<&str>,
        ) -> Result<ProviderSearchPage, ProviderError> {
            Err(ProviderError::MalformedResponse)
        }

        fn parse_artwork(
            &self,
            _candidate: &ProviderCandidate,
            _object_bytes: Option<&[u8]>,
        ) -> Result<Artwork, ArtworkDropReason> {
            Err(ArtworkDropReason::MissingSourceId)
        }

        fn artwork_request(&self, _key: &ArtworkKey) -> Result<HttpRequest, ProviderError> {
            Err(ProviderError::ArtworkUnavailable)
        }

        fn parse_artwork_response(&self, _bytes: &[u8]) -> Result<Artwork, ProviderError> {
            Err(ProviderError::ArtworkUnavailable)
        }

        fn display_image_request(
            &self,
            _artwork: &Artwork,
            _size: DisplayImageSize,
        ) -> Result<DisplayImageRequest, ProviderError> {
            Err(ProviderError::InvalidImageRequest)
        }
    }

    #[test]
    fn available_token_is_granted_and_consumed() {
        assert_eq!(
            decide_acquire(1.0, Duration::ZERO, 1.0, 1.0),
            TokenDecision::Granted { remaining: 0.0 }
        );
    }

    #[test]
    fn empty_bucket_returns_the_exact_wait() {
        assert_eq!(
            decide_acquire(0.0, Duration::ZERO, 1.0, 1.0),
            TokenDecision::Wait {
                available: 0.0,
                duration: Duration::from_secs(1)
            }
        );
    }

    #[test]
    fn elapsed_time_refills_without_exceeding_capacity() {
        assert_eq!(
            decide_acquire(0.0, Duration::from_secs(10), 1.0, 1.0),
            TokenDecision::Granted { remaining: 0.0 }
        );
        assert_eq!(
            decide_acquire(0.25, Duration::from_millis(250), 1.0, 1.0),
            TokenDecision::Wait {
                available: 0.5,
                duration: Duration::from_millis(500)
            }
        );
    }

    #[tokio::test]
    async fn unlimited_provider_does_not_create_a_bucket() {
        let limiters = RateLimiters::new();
        let provider = TestProvider {
            source: SourceKind::WikimediaCommons,
            policy: RatePolicy::Unlimited,
        };
        limiters.acquire(&provider).await;
        assert!(limiters.buckets.lock().await.is_empty());
    }

    #[tokio::test]
    async fn provider_policy_creates_and_reuses_a_bucket_by_source() {
        let limiters = RateLimiters::new();
        let provider = TestProvider {
            source: SourceKind::ArtInstituteChicago,
            policy: RatePolicy::TokenBucket(TokenBucketPolicy::new(
                NonZeroU32::new(2).expect("capacity is non-zero"),
                Duration::from_millis(1),
            )),
        };
        limiters.acquire(&provider).await;
        limiters.acquire(&provider).await;
        let buckets = limiters.buckets.lock().await;
        assert_eq!(buckets.len(), 1);
        assert!(buckets.contains_key(&SourceKind::ArtInstituteChicago));
    }
}
