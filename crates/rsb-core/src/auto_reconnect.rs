// 智能重连机制实现
use anyhow::Result;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::time::sleep;

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub struct AutoReconnect {
    max_retries: usize,
    initial_backoff: Duration,
    max_backoff: Duration,
    backoff_multiplier: f64,
    jitter: bool,
}

impl AutoReconnect {
    pub fn new() -> Self {
        Self {
            max_retries: 5,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }

    pub fn with_max_retries(mut self, retries: usize) -> Self {
        self.max_retries = retries;
        self
    }

    pub fn with_backoff(mut self, initial: Duration, max: Duration, multiplier: f64) -> Self {
        self.initial_backoff = initial;
        self.max_backoff = max;
        self.backoff_multiplier = multiplier;
        self
    }

    /// 带重试的连接
    pub async fn connect_with_retry<F, Fut, T, E>(&self, mut connect_fn: F) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: std::fmt::Display,
    {
        let mut retries = 0;
        let mut backoff = self.initial_backoff;

        loop {
            match connect_fn().await {
                Ok(result) => {
                    if retries > 0 {
                        tracing::info!(
                            retries = retries,
                            "Connection successful after retries"
                        );
                    }
                    return Ok(result);
                }
                Err(e) => {
                    retries += 1;

                    if retries >= self.max_retries {
                        tracing::error!(
                            retries = retries,
                            error = %e,
                            "Max retries reached, giving up"
                        );
                        return Err(e);
                    }

                    // 计算下一次重试的延迟
                    let delay = if self.jitter {
                        self.calculate_jittered_backoff(backoff)
                    } else {
                        backoff
                    };

                    tracing::warn!(
                        retries = retries,
                        max_retries = self.max_retries,
                        delay_secs = delay.as_secs(),
                        error = %e,
                        "Connection failed, retrying"
                    );

                    sleep(delay).await;

                    // 指数退避
                    backoff = Duration::from_secs_f64(
                        (backoff.as_secs_f64() * self.backoff_multiplier).min(self.max_backoff.as_secs_f64())
                    );
                }
            }
        }
    }

    /// 计算带抖动的退避时间
    fn calculate_jittered_backoff(&self, backoff: Duration) -> Duration {
        use rand::Rng;
        let mut rng = rand::thread_rng();

        // 添加 ±25% 的随机抖动
        let jitter_factor = rng.gen_range(0.75..1.25);
        let jittered = backoff.as_secs_f64() * jitter_factor;

        Duration::from_secs_f64(jittered)
    }

    /// 持续重连（永不放弃）
    pub async fn connect_forever<F, Fut, T, E>(&self, mut connect_fn: F) -> T
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: std::fmt::Display,
    {
        let mut retries = 0;
        let mut backoff = self.initial_backoff;

        loop {
            match connect_fn().await {
                Ok(result) => {
                    if retries > 0 {
                        tracing::info!(
                            retries = retries,
                            "Connection successful after retries"
                        );
                    }
                    return result;
                }
                Err(e) => {
                    retries += 1;

                    let delay = if self.jitter {
                        self.calculate_jittered_backoff(backoff)
                    } else {
                        backoff
                    };

                    tracing::warn!(
                        retries = retries,
                        delay_secs = delay.as_secs(),
                        error = %e,
                        "Connection failed, retrying indefinitely"
                    );

                    sleep(delay).await;

                    // 指数退避
                    backoff = Duration::from_secs_f64(
                        (backoff.as_secs_f64() * self.backoff_multiplier).min(self.max_backoff.as_secs_f64())
                    );
                }
            }
        }
    }
}

impl Default for AutoReconnect {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_auto_reconnect_success() {
        let reconnect = AutoReconnect::new().with_max_retries(3);
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_clone = attempts.clone();

        let result = reconnect
            .connect_with_retry(|| async {
                let count = attempts_clone.fetch_add(1, Ordering::SeqCst);
                if count < 2 {
                    Err("Failed")
                } else {
                    Ok("Success")
                }
            })
            .await;

        assert_eq!(result, Ok("Success"));
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_auto_reconnect_max_retries() {
        let reconnect = AutoReconnect::new()
            .with_max_retries(3)
            .with_backoff(
                Duration::from_millis(10),
                Duration::from_millis(100),
                2.0,
            );

        let result: Result<(), &str> = reconnect
            .connect_with_retry(|| async { Err("Always fail") })
            .await;

        assert!(result.is_err());
    }
}
