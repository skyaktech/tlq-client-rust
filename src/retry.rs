use std::time::Duration;
use tokio::time::sleep;

pub struct RetryStrategy {
    max_retries: u32,
    base_delay: Duration,
}

impl RetryStrategy {
    pub fn new(max_retries: u32, base_delay: Duration) -> Self {
        Self {
            max_retries,
            base_delay,
        }
    }

    pub async fn execute<F, Fut, T, E>(&self, mut operation: F) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
        E: std::fmt::Debug,
    {
        let mut attempt = 0;

        loop {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(err) if attempt >= self.max_retries => {
                    return Err(err);
                }
                Err(_) if attempt < self.max_retries => {
                    let delay = self.calculate_delay(attempt);
                    sleep(delay).await;
                    attempt += 1;
                }
                Err(err) => return Err(err),
            }
        }
    }

    fn calculate_delay(&self, attempt: u32) -> Duration {
        let multiplier = 2_u32.pow(attempt);
        self.base_delay * multiplier
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        
        assert_eq!(strategy.calculate_delay(0), Duration::from_millis(0));
        assert_eq!(strategy.calculate_delay(1), Duration::from_millis(0));
        assert_eq!(strategy.calculate_delay(5), Duration::from_millis(0));
    }

    #[tokio::test]
    async fn test_immediate_success() {
        let strategy = RetryStrategy::new(3, Duration::from_millis(10));
        let call_count = Arc::new(AtomicU32::new(0));
        
        let call_count_clone = call_count.clone();
        let result = strategy.execute(|| {
            let counter = call_count_clone.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok::<&str, &str>("success")
            }
        }).await;

        assert_eq!(result, Ok("success"));
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_success_after_retries() {
        let strategy = RetryStrategy::new(3, Duration::from_millis(1));
        let call_count = Arc::new(AtomicU32::new(0));
        
        let call_count_clone = call_count.clone();
        let result = strategy.execute(|| {
            let counter = call_count_clone.clone();
            async move {
                let count = counter.fetch_add(1, Ordering::SeqCst);
                if count < 2 {
                    Err("temporary failure")
                } else {
                    Ok("success after retries")
                }
            }
        }).await;

        assert_eq!(result, Ok("success after retries"));
        assert_eq!(call_count.load(Ordering::SeqCst), 3); // Failed twice, succeeded on 3rd
    }

    #[tokio::test]
    async fn test_max_retries_exceeded() {
        let strategy = RetryStrategy::new(2, Duration::from_millis(1));
        let call_count = Arc::new(AtomicU32::new(0));
        
        let call_count_clone = call_count.clone();
        let result = strategy.execute(|| {
            let counter = call_count_clone.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Err::<&str, &str>("always fails")
            }
        }).await;

        assert_eq!(result, Err("always fails"));
        assert_eq!(call_count.load(Ordering::SeqCst), 3); // Initial attempt + 2 retries
    }

    #[tokio::test]
    async fn test_zero_max_retries() {
        let strategy = RetryStrategy::new(0, Duration::from_millis(1));
        let call_count = Arc::new(AtomicU32::new(0));
        
        let call_count_clone = call_count.clone();
        let result = strategy.execute(|| {
            let counter = call_count_clone.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Err::<&str, &str>("fails immediately")
            }
        }).await;

        assert_eq!(result, Err("fails immediately"));
        assert_eq!(call_count.load(Ordering::SeqCst), 1); // Only initial attempt, no retries
    }

    #[tokio::test]
    async fn test_retry_timing() {
        let strategy = RetryStrategy::new(2, Duration::from_millis(50));
        let start_time = Instant::now();
        
        let result = strategy.execute(|| async {
            Err::<&str, &str>("always fails")
        }).await;

        let elapsed = start_time.elapsed();
        
        // Should have waited at least 50ms (first retry) + 100ms (second retry) = 150ms
        // Add some tolerance for timing variations
        assert!(elapsed >= Duration::from_millis(140));
        assert!(elapsed < Duration::from_millis(200));
        assert_eq!(result, Err("always fails"));
    }

    #[tokio::test]
    async fn test_retry_with_different_error_types() {
        #[derive(Debug, PartialEq)]
        enum TestError {
            Recoverable,
            Fatal,
        }

        let strategy = RetryStrategy::new(3, Duration::from_millis(1));
        let call_count = Arc::new(AtomicU32::new(0));
        
        let call_count_clone = call_count.clone();
        let result = strategy.execute(|| {
            let counter = call_count_clone.clone();
            async move {
                let count = counter.fetch_add(1, Ordering::SeqCst);
                match count {
                    0 => Err(TestError::Recoverable),
                    1 => Err(TestError::Fatal),
                    2 => Ok("success on third attempt"),
                    _ => panic!("Should not reach more than 3 attempts"),
                }
            }
        }).await;

        // The strategy should continue retrying through all error types
        // and succeed on the third attempt
        assert_eq!(result, Ok("success on third attempt"));
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }
}
