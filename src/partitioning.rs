use std::{
    sync::{
        atomic::{self, AtomicUsize},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use conv::ConvAsUtil;

use crate::{limits::LimitAlgorithm, DefaultLimiter, Limiter, Outcome, Token};

/// A limiter which can be partitioned.
///
/// Each partition can use some fraction of the total concurrency limit.
///
/// Each partition can use more than its allotted fraction when there is spare capacity.
///
/// Note that each limiter has a minimum limit of 1. So the total concurrency might exceed the total
/// limit. E.g. when the limit is one and we have two limiters, two jobs can be being processed
/// concurrently.
#[derive(Debug)]
pub struct PartitionedLimiter<L> {
    inner: DefaultLimiter<L>,
}

/// A partition, using some fraction of the concurrency limit.
#[derive(Debug)]
pub struct Partition<L> {
    fraction: f64,
    in_flight: Arc<AtomicUsize>,

    /// The underlying limiter, keeping track of the total limit.
    inner: DefaultLimiter<L>,
}

impl<L: LimitAlgorithm + Clone> PartitionedLimiter<L> {
    /// Create a partitioned limiter with a given limit control algorithm.
    pub fn new(limit_algo: L) -> Self {
        Self {
            inner: DefaultLimiter::new(limit_algo),
        }
    }

    /// Create some partitions with the given relative weights.
    ///
    /// The provided weights will be normalised. E.g. weights of 2, 2 and 4 will result in
    /// partitions of 25%, 25% and 50% of the total limit, respectively.
    ///
    /// `weights` must not be empty.
    pub fn create_static_partitions(&mut self, weights: Vec<f64>) -> Vec<Partition<L>> {
        assert!(!weights.is_empty(), "Must provide at least one weight");

        let total: f64 = weights.iter().sum();

        weights
            .into_iter()
            .map(|weight| {
                let fraction = weight / total;
                Partition {
                    fraction,
                    in_flight: Arc::new(AtomicUsize::new(0)),
                    inner: self.inner.clone(),
                }
            })
            .collect()
    }
}

impl<L: LimitAlgorithm> Partition<L> {
    fn limit(&self) -> usize {
        fractional_limit(self.inner.limit(), self.fraction)
    }

    fn in_flight(&self) -> usize {
        self.in_flight.load(atomic::Ordering::SeqCst)
    }
}

#[async_trait]
impl<L> Limiter for Partition<L>
where
    L: LimitAlgorithm + Sync,
{
    async fn try_acquire(&self) -> Option<Token<'_>> {
        if self.in_flight() < self.limit() {
            let token = self
                .inner
                .try_acquire()
                .await
                .map(|token| token.with_in_flight(self.in_flight.clone()));

            token
        } else {
            self.inner.on_rejection().await;
            None
        }
    }

    async fn acquire_timeout(&self, duration: Duration) -> Option<Token<'_>> {
        if self.in_flight() < self.limit() {
            let token = self
                .inner
                .acquire_timeout(duration)
                .await
                .map(|token| token.with_in_flight(self.in_flight.clone()));

            token
        } else {
            self.inner.on_rejection().await;
            None
        }
    }

    async fn release(&self, token: Token<'_>, outcome: Option<Outcome>) -> usize {
        let new_limit = self.inner.release(token, outcome).await;

        fractional_limit(new_limit, self.fraction)
    }
}

fn fractional_limit(limit: usize, fraction: f64) -> usize {
    let limit_f64 = limit as f64 * fraction;

    limit_f64
        .ceil()
        .approx()
        .expect("should be clamped within usize bounds")
}

#[cfg(test)]
mod tests {
    #[test]
    fn todo() {
        // TODO: write some tests
    }
}
