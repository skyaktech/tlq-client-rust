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