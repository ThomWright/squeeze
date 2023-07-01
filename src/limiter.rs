use std::{
    cmp,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use tokio::{
    sync::{Semaphore, SemaphorePermit, TryAcquireError},
    time::{timeout, Instant},
};

use crate::limit::{LimitAlgorithm, Sample};

/// Limits the number of concurrent jobs.
///
/// The limit will be automatically adjusted based on observed latency (delay) and/or failures
/// caused by overload (loss).
///
#[derive(Debug)]
pub struct Limiter<T> {
    limit_algo: T,
    semaphore: Arc<Semaphore>,
    limit: AtomicUsize,

    /// Best-effort
    in_flight: Arc<AtomicUsize>,

    #[cfg(test)]
    notifier: Option<Arc<tokio::sync::Notify>>,
}

#[derive(Debug)]
pub struct Timer<'t> {
    _permit: SemaphorePermit<'t>,
    start: Instant,
    in_flight: Arc<AtomicUsize>,
}

/// A snapshot of the state of the limiter.
///
/// Not guaranteed to be consistent under high concurrency.
#[derive(Debug, Clone, Copy)]
pub struct LimiterState {
    limit: usize,
    available: usize,
    in_flight: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Success,
    Overload,
}

impl<T> Limiter<T>
where
    T: LimitAlgorithm,
{
    pub fn new(limit_algo: T) -> Self {
        let initial_permits = limit_algo.initial_limit();
        assert!(initial_permits > 0);
        Self {
            limit_algo,
            semaphore: Arc::new(Semaphore::new(initial_permits)),
            limit: AtomicUsize::new(initial_permits),
            in_flight: Arc::new(AtomicUsize::new(0)),

            #[cfg(test)]
            notifier: None,
        }
    }

    /// In some cases permits are acquired asynchronously when updating the limit.
    #[cfg(test)]
    pub fn with_release_notifier(mut self, n: Arc<tokio::sync::Notify>) -> Self {
        self.notifier = Some(n);
        self
    }

    /// Try to immediately acquire a concurrency token.
    ///
    /// Returns `None` if there are none available.
    pub fn try_acquire(&self) -> Option<Timer<'_>> {
        match self.semaphore.try_acquire() {
            Ok(permit) => {
                self.in_flight.fetch_add(1, Ordering::AcqRel);
                Some(Timer::new(permit, self.in_flight.clone()))
            }
            Err(TryAcquireError::NoPermits) => None,
            Err(TryAcquireError::Closed) => {
                panic!("we own the semaphore, we shouldn't have closed it")
            }
        }
    }

    /// Try to acquire a concurrency token, waiting for `duration` if there are none available.
    ///
    /// Returns `None` if there are none available for `duration`.
    pub async fn acquire_timeout(&self, duration: Duration) -> Option<Timer<'_>> {
        match timeout(duration, self.semaphore.acquire()).await {
            Ok(Ok(permit)) => Some(Timer::new(permit, self.in_flight.clone())),
            Err(_) => None,

            Ok(Err(_)) => {
                panic!("we own the semaphore, we shouldn't have closed it")
            }
        }
    }

    /// Return the concurrency token, along with the outcome of the job.
    ///
    /// The outcome of the job, and the time taken to perform it, may be used
    /// to update the concurrency limit.
    ///
    /// Set the outcome to `None` to ignore the job.
    pub async fn release(&self, timer: Timer<'_>, outcome: Option<Outcome>) {
        if let Some(outcome) = outcome {
            let sample = Sample {
                latency: timer.start.elapsed(),
                outcome,
            };

            let new_limit = self.limit_algo.update(sample);

            let old_limit = self.limit.swap(new_limit, Ordering::SeqCst);

            match new_limit.cmp(&old_limit) {
                cmp::Ordering::Greater => {
                    self.semaphore.add_permits(new_limit - old_limit);

                    #[cfg(test)]
                    if let Some(n) = &self.notifier {
                        n.notify_one();
                    }
                }
                cmp::Ordering::Less => {
                    let semaphore = self.semaphore.clone();
                    #[cfg(test)]
                    let notifier = self.notifier.clone();

                    tokio::spawn(async move {
                        // If there aren't enough permits available then this will wait until enough
                        // become available. This could take a while, so we do this in the background.
                        let permits = semaphore
                            .acquire_many((old_limit - new_limit) as u32)
                            .await
                            .expect("we own the semaphore, we shouldn't have closed it");

                        // Acquiring some permits and throwing them away reduces the available limit.
                        permits.forget();

                        #[cfg(test)]
                        if let Some(n) = notifier {
                            n.notify_one();
                        }
                    });
                }
                _ =>
                {
                    #[cfg(test)]
                    if let Some(n) = &self.notifier {
                        n.notify_one();
                    }
                }
            }
        }

        drop(timer);
    }

    pub(crate) fn available(&self) -> usize {
        self.semaphore.available_permits()
    }

    pub(crate) fn limit(&self) -> usize {
        self.limit.load(Ordering::Acquire)
    }

    pub(crate) fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }

    pub fn state(&self) -> LimiterState {
        LimiterState {
            limit: self.limit(),
            available: self.available(),
            in_flight: self.in_flight(),
        }
    }
}

impl<'t> Timer<'t> {
    fn new(permit: SemaphorePermit<'t>, in_flight: Arc<AtomicUsize>) -> Self {
        Self {
            _permit: permit,
            start: Instant::now(),
            in_flight,
        }
    }
}

impl Drop for Timer<'_> {
    fn drop(&mut self) {
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

impl LimiterState {
    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn available(&self) -> usize {
        self.available
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight
    }
}

#[cfg(test)]
mod tests {
    use crate::{limit::FixedLimit, Limiter, Outcome};

    #[tokio::test]
    async fn it_works() {
        let limiter = Limiter::new(FixedLimit::limit(10));

        let timer = limiter.try_acquire().unwrap();

        limiter.release(timer, Some(Outcome::Success)).await;

        assert_eq!(limiter.limit(), 10);
    }
}
