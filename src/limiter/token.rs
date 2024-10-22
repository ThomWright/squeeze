use std::{
    sync::{
        atomic::{self, AtomicUsize},
        Arc,
    },
    time::Duration,
};

use tokio::{sync::OwnedSemaphorePermit, time::Instant};

use super::partitioning::Scheduler;

/// A concurrency token, required to run a job.
///
/// Release the token back to the [Limiter](crate::limiter::Limiter) after the job is complete.
#[derive(Debug)]
pub struct Token {
    inner: Option<TokenInner>,
    partition: Option<Partition>,

    start: Instant,
    #[cfg(test)]
    latency: Duration,
}

#[derive(Debug)]
pub(crate) struct TokenInner {
    _permit: OwnedSemaphorePermit,
    in_flight: Arc<AtomicUsize>,
}

#[derive(Debug)]
pub(crate) struct Partition {
    in_flight: Arc<AtomicUsize>,
    scheduler: Arc<Scheduler>,
}

impl Token {
    pub(crate) fn new(permit: OwnedSemaphorePermit, in_flight: Arc<AtomicUsize>) -> Self {
        in_flight.fetch_add(1, atomic::Ordering::SeqCst);
        Self {
            inner: Some(TokenInner {
                _permit: permit,
                in_flight,
            }),
            partition: None,
            start: Instant::now(),
            #[cfg(test)]
            latency: Duration::ZERO,
        }
    }

    pub(crate) fn new_from_inner(inner: TokenInner) -> Self {
        Self {
            inner: Some(inner),
            partition: None,
            start: Instant::now(),
            #[cfg(test)]
            latency: Duration::ZERO,
        }
    }

    pub(crate) fn for_partition(mut self, partition: Partition) -> Self {
        partition.in_flight.fetch_add(1, atomic::Ordering::SeqCst);
        self.partition = Some(partition);
        self
    }

    #[cfg(test)]
    pub(crate) fn set_latency(&mut self, latency: Duration) {
        use std::ops::Sub;

        use tokio::time::Instant;

        self.start = Instant::now().sub(latency);
        self.latency = latency;
    }

    #[cfg(test)]
    pub(crate) fn latency(&self) -> Duration {
        self.latency
    }

    #[cfg(not(test))]
    pub(crate) fn latency(&self) -> Duration {
        self.start.elapsed()
    }
}

impl Drop for Token {
    /// Reduces the number of jobs in flight and releases the token back to the available pool.
    fn drop(&mut self) {
        if let Some(partition) = self.partition.take() {
            partition.in_flight.fetch_sub(1, atomic::Ordering::SeqCst);
            partition.scheduler.reuse_permit(
                self.inner
                    .take()
                    .expect("TokenInner should always be present until drop"),
            );
        }
    }
}

impl Drop for TokenInner {
    fn drop(&mut self) {
        self.in_flight.fetch_sub(1, atomic::Ordering::SeqCst);
    }
}

impl Partition {
    pub(crate) fn new(in_flight: Arc<AtomicUsize>, scheduler: Arc<Scheduler>) -> Self {
        Self {
            in_flight,
            scheduler,
        }
    }
}
