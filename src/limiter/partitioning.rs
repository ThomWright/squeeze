use std::{
    collections::LinkedList,
    sync::{
        atomic::{self, AtomicUsize},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use conv::{ConvAsUtil, ConvUtil};
use tokio::{
    sync::{oneshot, RwLock},
    time::timeout,
};

use crate::{limits::LimitAlgorithm, DefaultLimiter, Limiter, Outcome, Token};

use super::token::{self, TokenInner};

/// Creates a set of partitioned limiters, sharing the total capacity between them.
///
/// Each partition can use some fraction of the total concurrency limit.
///
/// Each partition can use more than its allotted fraction when there is spare capacity.
///
/// Note that each limiter has a minimum limit of 1. So the total concurrency might exceed the total
/// limit. E.g. when the limit is one and we have two limiters, two jobs can be being processed
/// concurrently.
// #[derive(Debug)]
// pub struct Partitioner<L> {
//     limiter: DefaultLimiter<L>,

//     thing: Scheduler,
// }

#[derive(Debug)]
pub(crate) struct Scheduler {
    total_in_flight: Arc<AtomicUsize>,

    partition_states: Vec<PartitionState>,

    waiters: RwLock<LinkedList<(usize, oneshot::Sender<Token>)>>,
}

#[derive(Debug)]
struct PartitionState {
    fraction: f64,
    /// Shared with [Token]s.
    in_flight: Arc<AtomicUsize>,
}

/// A partition, using some fraction of the concurrency limit.
#[derive(Debug)]
pub struct PartitionedLimiter<L> {
    /// Partition state used for scheduling is stored at this index in the [Scheduler].
    index: usize,

    scheduler: Arc<Scheduler>,
    limiter: Arc<DefaultLimiter<L>>,
}

/// Create some partitions with the given relative weights.
///
/// The provided weights will be normalised. E.g. weights of 2, 2 and 4 will result in
/// partitions of 25%, 25% and 50% of the total limit, respectively.
///
/// `weights` must not be empty.
pub fn create_static_partitions<L: LimitAlgorithm + Sync>(
    limit_algo: L,
    weights: Vec<f64>,
) -> Vec<PartitionedLimiter<L>> {
    assert!(!weights.is_empty(), "Must provide at least one weight");

    let total: f64 = weights.iter().sum();

    let mut partition_states = Vec::with_capacity(weights.len());

    for weight in weights {
        let fraction = weight / total;

        partition_states.push(PartitionState {
            fraction,
            in_flight: Arc::new(AtomicUsize::new(0)),
        });
    }

    let shared_limiter = Arc::new(DefaultLimiter::new(limit_algo));
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
    fn spare(&self, total_limit: usize) -> usize {
        self.partition_states
            .iter()
            .fold(0, |total, partition| total + partition.spare(total_limit))
    }
}

// impl<L: LimitAlgorithm + Sync> Partitioner<L> {
//     /// Create a partitioned limiter with a given limit control algorithm.
//     pub fn new(limit_algo: L) -> Self {
//         Self {
//             limiter: DefaultLimiter::new(limit_algo),
//             thing: Scheduler {
//                 partition_states: vec![],
//                 waiters: RwLock::new(LinkedList::new()),
//             },
//         }
//     }

//     // async fn try_acquire(&self, index: usize) -> Option<Token> {
//     //     let state = &self.partition_states[index];

//     //     let total_limit = self.limiter.limit();
//     //     if state.in_flight() < state.limit(total_limit) || self.spare() > 0 {
//     //         self.limiter
//     //             .try_acquire()
//     //             .await
//     //             .map(|token| token.with_in_flight(state.in_flight.clone()))
//     //     } else {
//     //         self.limiter.on_rejection().await;
//     //         None
//     //     }
//     // }

//     // async fn acquire_timeout(&self, duration: Duration, index: usize) -> Option<Token> {
//     //     let state = &self.partition_states[index];
//     //     match timeout(duration, async {
//     //         let total_limit = self.limiter.limit();
//     //         if state.in_flight() < state.limit(total_limit) || self.spare() > 0 {
//     //             self.limiter
//     //                 .try_acquire()
//     //                 .await
//     //                 .map(|token| token.with_in_flight(state.in_flight.clone()))
//     //         } else {
//     //             let (snd, rx) = oneshot::channel();
//     //             let mut waiters = self.waiters.write().await;
//     //             waiters.push_back(snd);
//     //             match rx.await {
//     //                 Ok(token) => Some(token),
//     //                 Err(_) => None,
//     //             }
//     //         }
//     //     })
//     //     .await
//     //     {
//     //         Ok(Some(token)) => Some(token.with_in_flight(state.in_flight.clone())),
//     //         Err(_) => {
//     //             self.limiter.on_rejection().await;
//     //             None
//     //         }
//     //         Ok(None) => {
//     //             self.limiter.on_rejection().await;
//     //             None
//     //         }
//     //     }
//     // }

//     // async fn release(&self, token: Token, outcome: Option<Outcome>) -> usize {
//     //     self.limiter.release(token, outcome).await
//     // }

//     fn fraction(&self, index: usize) -> f64 {
//         self.partition_states[index].fraction
//     }

// }

impl PartitionState {
    const BUFFER_FRACTION: f64 = 0.1;

    fn limit(&self, total_limit: usize) -> usize {
        fractional_limit(total_limit, self.fraction)
    }

    fn in_flight(&self) -> usize {
        self.in_flight.load(atomic::Ordering::SeqCst)
    }

    /// Spare capacity which can be used by other partitions.
    fn spare(&self, total_limit: usize) -> usize {
        let partition_limit = self.limit(total_limit);
        let buffer = (partition_limit as f64 * Self::BUFFER_FRACTION)
            .ceil()
            .approx_as::<usize>()
            .expect("should be < usize::MAX");
        (partition_limit - self.in_flight()).saturating_sub(buffer)
    }
}

#[async_trait]
impl<L> Limiter for PartitionedLimiter<L>
where
    L: LimitAlgorithm + Sync + Send,
{
    // async fn try_acquire(&self) -> Option<Token> {
    //     self.inner.try_acquire(self.index).await
    // }

    // async fn acquire_timeout(&self, duration: Duration) -> Option<Token> {
    //     self.inner.acquire_timeout(duration, self.index).await
    // }

    // async fn release(&self, token: Token, outcome: Option<Outcome>) -> usize {
    //     let new_limit = self.inner.release(token, outcome).await;

    //     fractional_limit(new_limit, self.inner.fraction(self.index))
    // }

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
            self.limiter.on_rejection().await;
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
            Err(_) => {
                self.limiter.on_rejection().await;
                None
            }
            Ok(None) => {
                self.limiter.on_rejection().await;
                None
            }
        }
    }

    async fn release(&self, token: Token, outcome: Option<Outcome>) -> usize {
        self.limiter.release(token, outcome).await
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
