//! Token-bucket rate limiter for per-user bandwidth control.

use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct RateLimiter {
    bytes_per_sec: u64,
    state: Mutex<(Instant, f64)>,
}

impl RateLimiter {
    pub fn new(bytes_per_sec: u64) -> Self {
        Self {
            bytes_per_sec,
            state: Mutex::new((Instant::now(), bytes_per_sec as f64)),
        }
    }

    pub async fn throttle(&self, nbytes: u64) {
        if self.bytes_per_sec == 0 || nbytes == 0 {
            return;
        }
        loop {
            let wait = self.acquire_wait(nbytes);
            if wait.is_zero() {
                return;
            }
            tokio::time::sleep(wait).await;
        }
    }

    /// Returns how long to wait before `nbytes` can be sent (zero = ready now).
    pub fn acquire_wait(&self, nbytes: u64) -> Duration {
        if self.bytes_per_sec == 0 || nbytes == 0 {
            return Duration::ZERO;
        }
        let mut guard = self.state.lock().expect("rate limiter lock");
        let now = Instant::now();
        let elapsed = now.duration_since(guard.0).as_secs_f64();
        guard.1 += elapsed * self.bytes_per_sec as f64;
        let cap = self.bytes_per_sec as f64 * 2.0;
        if guard.1 > cap {
            guard.1 = cap;
        }
        guard.0 = now;
        if guard.1 >= nbytes as f64 {
            guard.1 -= nbytes as f64;
            Duration::ZERO
        } else {
            let deficit = nbytes as f64 - guard.1;
            guard.1 = 0.0;
            Duration::from_secs_f64(deficit / self.bytes_per_sec as f64)
        }
    }
}
