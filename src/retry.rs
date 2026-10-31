use crate::error::Result;
use std::time::Duration;
use tokio::time::sleep;

const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);

/// Internal retry strategy with exponential backoff for TLQ client operations.
///
/// This struct implements an exponential backoff retry mechanism that automatically
/// retries failed operations up to a maximum number of attempts. The delay between
/// retries doubles with each attempt, capped at 30 seconds:
/// `min(base_delay × 2^attempt_number, 30s)`.
///
/// Used internally by [`TlqClient`](crate::TlqClient) to handle transient failures
/// like network connectivity issues and timeouts.
pub struct RetryStrategy {
    max_retries: u32,
    base_delay: Duration,
}

impl RetryStrategy {
    /// Creates a new retry strategy with the specified parameters.
    ///
    /// # Arguments
    ///
    /// * `max_retries` - Maximum number of retry attempts (0 disables retries)
    /// * `base_delay` - Base delay for exponential backoff calculation
    pub fn new(max_retries: u32, base_delay: Duration) -> Self {
        Self {
            max_retries,
            base_delay,
        }
    }

    /// Executes an async operation with automatic retry on transient failure.
    ///
    /// This method retries errors classified by [`TlqError::is_retryable`](crate::TlqError::is_retryable)
    /// up to `max_retries` times. Other errors are returned immediately.
    /// Between retries, it waits for an exponentially increasing delay.
    ///
    /// # Arguments
    ///
    /// * `operation` - A closure that returns a Future yielding `Result<T, TlqError>`
    ///
    /// # Returns
    ///
    /// Returns the first successful result, or the last error if all attempts fail.
    ///
    /// # Retry Behavior
    ///
    /// - Attempt 0: No delay, execute immediately
    /// - Attempt 1: Wait `base_delay × 2^0` = base_delay
    /// - Attempt 2: Wait `base_delay × 2^1` = base_delay × 2
    /// - Attempt 3: Wait `base_delay × 2^2` = base_delay × 4
    /// - Later delays are capped at 30 seconds, including when the base delay exceeds the cap.
    pub async fn execute<F, Fut, T>(&self, mut operation: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut attempt = 0;

        loop {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(err) if err.is_retryable() && attempt < self.max_retries => {
                    let delay = self.calculate_delay(attempt);
                    sleep(delay).await;
                    attempt += 1;
                }
                Err(err) => return Err(err),
            }
        }
    }

    /// Calculates the delay duration for a given retry attempt using exponential backoff.
    ///
    /// The delay formula is: `min(base_delay × 2^attempt_number, 30s)`.
    ///
    /// # Arguments
    ///
    /// * `attempt` - The retry attempt number (0-based)
    ///
    /// # Returns
    ///
    /// The duration to wait before the next retry attempt.
    ///
    /// # Examples
    ///
    /// With `base_delay = 100ms`:
    /// - Attempt 0: 100ms × 2^0 = 100ms
    /// - Attempt 1: 100ms × 2^1 = 200ms  
    /// - Attempt 2: 100ms × 2^2 = 400ms
    fn calculate_delay(&self, attempt: u32) -> Duration {
        if self.base_delay.is_zero() {
            return Duration::ZERO;
        }

        let cap = MAX_RETRY_DELAY.as_nanos();
        let nanos = 1_u128
            .checked_shl(attempt)
            .and_then(|multiplier| self.base_delay.as_nanos().checked_mul(multiplier))
            .unwrap_or(cap)
            .min(cap);
        // The 30-second cap fits in u64 nanoseconds.
        Duration::from_nanos(nanos as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::TlqError;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use tokio::time::Instant;

    #[test]
    fn test_exponential_backoff_calculation() {
        let strategy = RetryStrategy::new(3, Duration::from_millis(100));

        // Test exponential backoff: 100ms, 200ms, 400ms, 800ms
        assert_eq!(strategy.calculate_delay(0), Duration::from_millis(100)); // 2^0 * 100
        assert_eq!(strategy.calculate_delay(1), Duration::from_millis(200)); // 2^1 * 100
        assert_eq!(strategy.calculate_delay(2), Duration::from_millis(400)); // 2^2 * 100
        assert_eq!(strategy.calculate_delay(3), Duration::from_millis(800)); // 2^3 * 100
    }

    #[test]
    fn test_zero_base_delay() {
        let strategy = RetryStrategy::new(2, Duration::from_millis(0));

        for attempt in [0, 1, 5, 32, 128, u32::MAX] {
            assert_eq!(strategy.calculate_delay(attempt), Duration::ZERO);
        }
    }

    #[test]
    fn test_backoff_caps_large_attempts_and_base_delays() {
        let strategy = RetryStrategy::new(u32::MAX, Duration::from_secs(1));
        assert_eq!(strategy.calculate_delay(4), Duration::from_secs(16));
        for attempt in [5, 31, 32, 40, 128, u32::MAX] {
            assert_eq!(strategy.calculate_delay(attempt), Duration::from_secs(30));
        }

        let strategy = RetryStrategy::new(u32::MAX, Duration::MAX);
        for attempt in [0, 1, 32, 128, u32::MAX] {
            assert_eq!(strategy.calculate_delay(attempt), Duration::from_secs(30));
        }
    }

    #[test]
    fn test_subsecond_backoff_does_not_cap_before_reaching_limit() {
        let strategy = RetryStrategy::new(u32::MAX, Duration::from_nanos(1));
        for attempt in [31, 32, 34] {
            assert_eq!(
                strategy.calculate_delay(attempt),
                Duration::from_nanos(1_u64 << attempt)
            );
        }
        assert_eq!(strategy.calculate_delay(35), Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_more_than_32_retries_with_zero_delay() {
        let strategy = RetryStrategy::new(40, Duration::ZERO);
        let calls = AtomicU32::new(0);
        let result = strategy
            .execute(|| {
                calls.fetch_add(1, Ordering::SeqCst);
                std::future::ready(Err::<(), _>(TlqError::Connection(
                    "unavailable".to_string(),
                )))
            })
            .await;

        assert!(matches!(result, Err(TlqError::Connection(_))));
        assert_eq!(calls.load(Ordering::SeqCst), 41);
    }

    #[tokio::test]
    async fn test_immediate_success() {
        let strategy = RetryStrategy::new(3, Duration::from_millis(10));
        let call_count = Arc::new(AtomicU32::new(0));

        let call_count_clone = call_count.clone();
        let result = strategy
            .execute(|| {
                let counter = call_count_clone.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok("success")
                }
            })
            .await;

        assert_eq!(result.unwrap(), "success");
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_success_after_retries() {
        let strategy = RetryStrategy::new(3, Duration::from_millis(1));
        let call_count = Arc::new(AtomicU32::new(0));

        let call_count_clone = call_count.clone();
        let result = strategy
            .execute(|| {
                let counter = call_count_clone.clone();
                async move {
                    let count = counter.fetch_add(1, Ordering::SeqCst);
                    match count {
                        0 => Err(TlqError::Connection("temporary failure".to_string())),
                        1 => Err(TlqError::Timeout(100)),
                        2 => Err(TlqError::Io(std::io::ErrorKind::ConnectionReset.into())),
                        _ => Ok("success after retries"),
                    }
                }
            })
            .await;

        assert_eq!(result.unwrap(), "success after retries");
        assert_eq!(call_count.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn test_max_retries_exceeded() {
        let strategy = RetryStrategy::new(2, Duration::from_millis(1));
        let call_count = Arc::new(AtomicU32::new(0));

        let call_count_clone = call_count.clone();
        let result = strategy
            .execute(|| {
                let counter = call_count_clone.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Err::<(), _>(TlqError::Connection("always fails".to_string()))
                }
            })
            .await;

        assert!(matches!(result, Err(TlqError::Connection(message)) if message == "always fails"));
        assert_eq!(call_count.load(Ordering::SeqCst), 3); // Initial attempt + 2 retries
    }

    #[tokio::test]
    async fn test_zero_max_retries() {
        let strategy = RetryStrategy::new(0, Duration::from_millis(1));
        let call_count = Arc::new(AtomicU32::new(0));

        let call_count_clone = call_count.clone();
        let result = strategy
            .execute(|| {
                let counter = call_count_clone.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Err::<(), _>(TlqError::Connection("fails immediately".to_string()))
                }
            })
            .await;

        assert!(
            matches!(result, Err(TlqError::Connection(message)) if message == "fails immediately")
        );
        assert_eq!(call_count.load(Ordering::SeqCst), 1); // Only initial attempt, no retries
    }

    #[tokio::test]
    async fn test_retry_timing() {
        let strategy = RetryStrategy::new(2, Duration::from_millis(50));
        let start_time = Instant::now();

        let result = strategy
            .execute(|| async { Err::<(), _>(TlqError::Timeout(100)) })
            .await;

        let elapsed = start_time.elapsed();

        // Should have waited at least 50ms (first retry) + 100ms (second retry) = 150ms
        // Add some tolerance for timing variations
        assert!(elapsed >= Duration::from_millis(140));
        assert!(elapsed < Duration::from_millis(200));
        assert!(matches!(result, Err(TlqError::Timeout(100))));
    }

    #[tokio::test]
    async fn test_stops_retrying_after_non_retryable_error() {
        let strategy = RetryStrategy::new(3, Duration::from_millis(1));
        let call_count = Arc::new(AtomicU32::new(0));

        let call_count_clone = call_count.clone();
        let result = strategy
            .execute(|| {
                let counter = call_count_clone.clone();
                async move {
                    let count = counter.fetch_add(1, Ordering::SeqCst);
                    match count {
                        0 => Err(TlqError::Connection("temporary failure".to_string())),
                        1 => Err(TlqError::Validation("invalid request".to_string())),
                        2 => Ok("success on third attempt"),
                        _ => panic!("Should not reach more than 3 attempts"),
                    }
                }
            })
            .await;

        assert!(
            matches!(result, Err(TlqError::Validation(message)) if message == "invalid request")
        );
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }
}
