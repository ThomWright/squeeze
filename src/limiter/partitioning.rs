use std::{
    collections::LinkedList,
    fmt::Debug,
    sync::{atomic, Arc},
    time::Duration,
};

use async_trait::async_trait;
use conv::{ConvAsUtil, ConvUtil};
use tokio::{
    sync::{oneshot, RwLock},
    time::timeout,
};

use crate::{
    limiter::{DefaultLimiter, Limiter, Outcome, Token},
    limits::LimitAlgorithm,
};

use super::{
    token::{self, TokenInner},
    AtomicCapacityUnit, CapacityUnit,
};

type StateIndex = usize;

#[derive(Debug)]
pub(crate) struct Scheduler {
    total_in_flight: Arc<AtomicCapacityUnit>,

    partition_states: Vec<PartitionState>,

    waiters: RwLock<LinkedList<(StateIndex, oneshot::Sender<Token>)>>,
}

#[derive(Debug)]
struct PartitionState {
    fraction: f64,
    /// Shared with [Token]s.
    in_flight: Arc<AtomicCapacityUnit>,
}

/// A partition, using some fraction of the concurrency limit.
#[derive(Debug)]
pub struct PartitionedLimiter<L> {
    /// Partition state used for scheduling is stored at this index in the [Scheduler].
    index: StateIndex,

    scheduler: Arc<Scheduler>,
    limiter: Arc<DefaultLimiter<L>>,
}

impl<L: LimitAlgorithm + Sync> DefaultLimiter<L> {
    /// Divide up this limiter into a set of partitions with the given relative weights.
    ///
    /// The provided weights will be normalised. E.g. weights of 2, 2 and 4 will result in
    /// partitions of 25%, 25% and 50% of the total limit, respectively.
    ///
    /// `weights` must not be empty.
    pub fn create_static_partitions(self, weights: Vec<f64>) -> Vec<PartitionedLimiter<L>> {
        assert!(!weights.is_empty(), "Must provide at least one weight");

        let total: f64 = weights.iter().sum();

        let mut partition_states = Vec::with_capacity(weights.len());

        for weight in weights {
            let fraction = weight / total;

            partition_states.push(PartitionState {
                fraction,
                in_flight: Arc::new(AtomicCapacityUnit::new(0)),
            });
        }

        let shared_limiter = Arc::new(self);
        let scheduler = Arc::new(Scheduler {
            total_in_flight: shared_limiter.in_flight_shared(),
            partition_states,
            waiters: RwLock::default(),
        });

        let mut partitions = Vec::with_capacity(scheduler.partition_states.len());
        for _ in scheduler.partition_states.iter() {
            partitions.push(PartitionedLimiter {
                index: partitions.len(),
                scheduler: scheduler.clone(),
                limiter: shared_limiter.clone(),
            });
        }

        partitions
    }
}

impl Scheduler {
    pub(crate) fn reuse_permit(self: Arc<Scheduler>, token_inner: TokenInner) {
        tokio::spawn(async move {
            // TODO: a better strategy for choosing which waiter to wake
            let waiter = self.waiters.write().await.pop_front();
            match waiter {
                Some((index, waiter)) => {
                    let token =
                        Token::new_from_inner(token_inner).for_partition(token::Partition::new(
                            self.partition_states[index].in_flight.clone(),
                            self.clone(),
                        ));
                    match waiter.send(token) {
                        Ok(()) => {}
                        Err(_) => {
                            // Nothing to do, the token will be dropped
                        }
                    };
                }
                None => drop(token_inner),
            }
        });
    }

    /// Total spare capacity which can be used by any partition.
    fn spare(&self, total_limit: CapacityUnit) -> CapacityUnit {
        self.partition_states
            .iter()
            .fold(0, |total, partition| total + partition.spare(total_limit))
    }
}

impl PartitionState {
    const BUFFER_FRACTION: f64 = 0.1;

    fn limit(&self, total_limit: CapacityUnit) -> CapacityUnit {
        fractional_limit(total_limit, self.fraction)
    }

    fn in_flight(&self) -> CapacityUnit {
        self.in_flight.load(atomic::Ordering::SeqCst)
    }

    /// Spare capacity which can be used by other partitions.
    fn spare(&self, total_limit: CapacityUnit) -> CapacityUnit {
        let partition_limit = self.limit(total_limit);
        let buffer = (partition_limit as f64 * Self::BUFFER_FRACTION)
            .ceil()
            .approx_as::<CapacityUnit>()
            .expect("should be < usize::MAX");
        (partition_limit - self.in_flight()).saturating_sub(buffer)
    }
}

#[async_trait]
impl<L> Limiter for PartitionedLimiter<L>
where
    L: LimitAlgorithm + Sync + Send + Debug,
{
    async fn try_acquire(&self) -> Option<Token> {
        let state = &self.scheduler.partition_states[self.index];

        let total_limit = self.limiter.limit();
        if state.in_flight() < state.limit(total_limit) || self.scheduler.spare(total_limit) > 0 {
            self.limiter.try_acquire().await.map(|token| {
                token.for_partition(token::Partition::new(
                    state.in_flight.clone(),
                    self.scheduler.clone(),
                ))
            })
        } else {
            None
        }
    }

    async fn acquire_timeout(&self, duration: Duration) -> Option<Token> {
        let state = &self.scheduler.partition_states[self.index];
        match timeout(duration, async {
            let total_limit = self.limiter.limit();
            if state.in_flight() < state.limit(total_limit) || self.scheduler.spare(total_limit) > 0
            {
                self.limiter.try_acquire().await
            } else {
                let (snd, rx) = oneshot::channel();
                let mut waiters = self.scheduler.waiters.write().await;
                waiters.push_back((self.index, snd));
                match rx.await {
                    Ok(token) => Some(token),
                    Err(_) => None,
                }
            }
        })
        .await
        {
            Ok(Some(token)) => Some(token.for_partition(token::Partition::new(
                state.in_flight.clone(),
                self.scheduler.clone(),
            ))),
            Err(_) => None,
            Ok(None) => None,
        }
    }

    async fn release(&self, token: Token, outcome: Option<Outcome>) -> CapacityUnit {
        self.limiter.release(token, outcome).await
    }
}

fn fractional_limit(limit: CapacityUnit, fraction: f64) -> CapacityUnit {
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
