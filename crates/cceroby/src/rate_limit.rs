//! Per-provider token buckets with pure acquisition decisions.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::core::SourceKind;

/// Shared rate-limit state for each available provider.
#[derive(Debug, Clone)]
pub struct RateLimiters {
    aic: Arc<TokenBucket>,
}

impl RateLimiters {
    #[must_use]
    pub fn new() -> Self {
        Self {
            aic: Arc::new(TokenBucket::new(1.0, 1.0)),
        }
    }

    pub async fn acquire(&self, source: SourceKind) {
        if source == SourceKind::ArtInstituteChicago {
            self.aic.acquire().await;
        }
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
    fn new(capacity: f64, tokens_per_second: f64) -> Self {
        Self {
            capacity,
            tokens_per_second,
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
    use super::*;

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
    async fn non_aic_sources_do_not_wait() {
        RateLimiters::new()
            .acquire(SourceKind::ClevelandMuseum)
            .await;
    }

    #[tokio::test]
    async fn aic_bucket_grants_its_initial_token() {
        RateLimiters::new()
            .acquire(SourceKind::ArtInstituteChicago)
            .await;
    }
}
