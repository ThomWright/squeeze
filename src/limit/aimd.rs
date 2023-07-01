//! Loss-based limit that does an additive increment as long as there are no errors
//! and a multiplicative decrement when there is an error.
//!
//! - Loss-based
//! - Additive increase, multiplicative decrease

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{limit::Sample, Outcome};

use super::LimitAlgorithm;

pub struct AimdLimit {
    limit: AtomicUsize,
    decrease_factor: f32,
    increase_by: usize,

    min_limit: usize,
    max_limit: usize,
}

impl AimdLimit {
    const DEFAULT_DECREASE_FACTOR: f32 = 0.9;
    const DEFAULT_INCREASE: usize = 1;
    const DEFAULT_MIN_LIMIT: usize = 1;
    const DEFAULT_MAX_LIMIT: usize = 100;

    pub fn new_with_initial_limit(initial_limit: usize) -> Self {
        Self {
            limit: AtomicUsize::new(initial_limit),
            decrease_factor: Self::DEFAULT_DECREASE_FACTOR,
            increase_by: Self::DEFAULT_INCREASE,
            min_limit: Self::DEFAULT_MIN_LIMIT,
            max_limit: Self::DEFAULT_MAX_LIMIT,
        }
    }

    pub fn decrease_factor(self, factor: f32) -> Self {
        assert!((0.5..1.0).contains(&factor));
        Self {
            decrease_factor: factor,
            ..self
        }
    }

    pub fn increase_by(self, increase: usize) -> Self {
        assert!(increase > 0);
        Self {
            increase_by: increase,
            ..self
        }
    }

    pub fn with_max_limit(self, max: usize) -> Self {
        assert!(max > 0);
        Self {
            max_limit: max,
            ..self
        }
    }
}

impl LimitAlgorithm for AimdLimit {
    fn initial_limit(&self) -> usize {
        self.limit.load(Ordering::Acquire)
    }

    fn update(&self, sample: Sample) -> usize {
        use Outcome::*;
        match sample.outcome {
            Success => {
                self.limit
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |limit| {
                        let limit = limit + self.increase_by;
                        Some(limit.clamp(self.min_limit, self.max_limit))
                    })
                    .expect("we always return Some(limit)");
            }
            Overload => {
                self.limit
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |limit| {
                        let limit = limit as f32 * self.decrease_factor;

                        // Floor instead of round, so the limit reduces even with small numbers.
                        // E.g. round(2 * 0.9) = 2, but floor(2 * 0.9) = 1
                        let limit = limit.floor() as usize;

                        Some(limit.clamp(self.min_limit, self.max_limit))
                    })
                    .expect("we always return Some(limit)");
            }
        }
        self.limit.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::Notify;

    use crate::Limiter;

    use super::*;

    #[tokio::test]
    async fn it_works() {
        let aimd = AimdLimit::new_with_initial_limit(10)
            .decrease_factor(0.5)
            .increase_by(1);

        let release_notifier = Arc::new(Notify::new());

        let limiter = Limiter::new(aimd).with_release_notifier(release_notifier.clone());

        let timer = limiter.try_acquire().unwrap();
        limiter.release(timer, Some(Outcome::Overload)).await;
        release_notifier.notified().await;
        assert_eq!(limiter.limit(), 5, "decrease");

        let timer = limiter.try_acquire().unwrap();
        limiter.release(timer, Some(Outcome::Success)).await;
        assert_eq!(limiter.limit(), 6, "increase");

        let timer = limiter.try_acquire().unwrap();
        limiter.release(timer, None).await;
        assert_eq!(limiter.limit(), 6, "ignore");
    }
}
