use std::time::Duration;

use async_trait::async_trait;

use super::{Limiter, Outcome, Token};

/// A wrapper which adds rejection delay.
///
/// When a job is rejected because there is no available capacity, a delay is added before
/// returning.
///
/// This can help reduce the rate of retries, especially when they are too eager and lack
/// appropriate backoff.
#[derive(Debug)]
pub struct RejectionDelay {
    delay: Duration,
    inner: Box<dyn Limiter>,
}

impl RejectionDelay {
    #[allow(missing_docs)]
    pub fn new(delay: Duration, limiter: impl Limiter + 'static) -> Self {
        Self {
            delay,
            inner: Box::new(limiter),
        }
    }
}

#[async_trait]
impl Limiter for RejectionDelay {
    async fn try_acquire(&self) -> Option<Token> {
        let token = self.inner.try_acquire().await;

        if token.is_none() {
            tokio::time::sleep(self.delay).await;
        }

        token
    }

    async fn acquire_timeout(&self, duration: Duration) -> Option<Token> {
        let token = self.inner.acquire_timeout(duration).await;

        if token.is_none() {
            tokio::time::sleep(self.delay).await;
        }

        token
    }

    async fn release(&self, token: Token, outcome: Option<Outcome>) -> usize {
        self.inner.release(token, outcome).await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::{self, Instant};

    use crate::assert_elapsed;
    use crate::{
        limiter::{DefaultLimiter, Limiter, RejectionDelay},
        limits::Fixed,
    };

    #[tokio::test]
    async fn on_rejection_delay_acquire() {
        time::pause();

        let delay = Duration::from_millis(5000);

        let limiter = RejectionDelay::new(delay, DefaultLimiter::new(Fixed::new(1)));

        let _token = limiter.try_acquire().await.unwrap();

        let before_acquire = Instant::now();
        let token = limiter.try_acquire().await;

        assert!(token.is_none());
        assert_elapsed!(before_acquire, delay, Duration::from_millis(10));
    }

    #[tokio::test]
    async fn on_rejection_delay_acquire_timeout() {
        time::pause();

        let delay = Duration::from_millis(5000);

        let limiter = RejectionDelay::new(delay, DefaultLimiter::new(Fixed::new(1)));

        let _token = limiter.try_acquire().await.unwrap();

        let before_acquire = Instant::now();
        let token = limiter.acquire_timeout(delay).await;

        assert!(token.is_none());
        assert_elapsed!(before_acquire, delay * 2, Duration::from_millis(10));
    }

    /// Assert that a given duration has elapsed since `start`, within the given tolerance.
    #[macro_export]
    #[cfg(test)]
    macro_rules! assert_elapsed {
        ($start:expr, $dur:expr, $tolerance:expr) => {{
            let elapsed = $start.elapsed();
            let lower: std::time::Duration = $dur;

            // Handles ms rounding
            assert!(
                elapsed >= lower && elapsed <= lower + $tolerance,
                "actual = {:?}, expected = {:?}",
                elapsed,
                lower
            );
        }};
    }
}
