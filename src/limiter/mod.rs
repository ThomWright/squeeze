use std::{
    cmp,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use conv::ValueFrom;
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError},
    time::timeout,
};

pub use partitioning::{create_static_partitions, PartitionedLimiter};
pub use token::Token;

use crate::limits::{LimitAlgorithm, Sample};

mod partitioning;
mod token;

/// Limits the number of concurrent jobs.
///
/// Concurrency is limited through the use of [Token]s. Acquire a token to run a job, and release the
/// token once the job is finished.
///
/// The limit will be automatically adjusted based on observed latency (delay) and/or failures
/// caused by overload (loss).
#[async_trait]
pub trait Limiter {
    /// Try to immediately acquire a concurrency [Token].
    ///
    /// Returns `None` if there are none available.
    async fn try_acquire(&self) -> Option<Token>;

    /// Try to acquire a concurrency [Token], waiting for `duration` if there are none available.
    ///
    /// Returns `None` if there are none available after `duration`.
    async fn acquire_timeout(&self, duration: Duration) -> Option<Token>;

    /// Return the concurrency [Token], along with the outcome of the job.
    ///
    /// The [Outcome] of the job, and the time taken to perform it, may be used
    /// to update the concurrency limit.
    ///
    /// Set the outcome to `None` to ignore the job.
    ///
    /// Returns the new limit.
    /// // TODO: do we need to return the new limit?
    async fn release(&self, token: Token, outcome: Option<Outcome>) -> usize;
}

/// A basic limiter.
///
/// Cheaply cloneable.
#[derive(Debug)]
pub struct DefaultLimiter<T> {
    limit_algo: T,
    semaphore: Arc<Semaphore>,
    limit: AtomicUsize,

    /// Best-effort
    in_flight: Arc<AtomicUsize>,

    // TODO: Turn rejection delay into a wrapper?
    rejection_delay: Option<Duration>,

    #[cfg(test)]
    notifier: Option<Arc<tokio::sync::Notify>>,
}

/// A snapshot of the state of the [Limiter].
///
/// Not guaranteed to be consistent under high concurrency.
#[derive(Debug, Clone, Copy)]
pub struct LimiterState {
    limit: usize,
    available: usize,
    in_flight: usize,
}

/// Whether a job succeeded or failed as a result of congestion/overload.
///
/// Errors not considered to be caused by overload should be ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The job succeeded, or failed in a way unrelated to overload.
    Success,
    /// The job failed because of overload, e.g. it timed out or an explicit backpressure signal
    /// was observed.
    Overload,
}

impl<T> DefaultLimiter<T>
where
    T: LimitAlgorithm,
{
    /// Create a limiter with a given limit control algorithm.
    pub fn new(limit_algo: T) -> Self {
        let initial_permits = limit_algo.limit();
        assert!(initial_permits >= 1);
        Self {
            limit_algo,
            semaphore: Arc::new(Semaphore::new(initial_permits)),
            limit: AtomicUsize::new(initial_permits),
            in_flight: Arc::new(AtomicUsize::new(0)),

            rejection_delay: None,

            #[cfg(test)]
            notifier: None,
        }
    }

    /// When rejecting a request, wait a while before returning the rejection.
    ///
    /// This can help reduce the rate of retries.
    pub fn with_rejection_delay(mut self, delay: Duration) -> Self {
        self.rejection_delay = Some(delay);
        self
    }

    /// In some cases [Token]s are acquired asynchronously when updating the limit.
    #[cfg(test)]
    pub fn with_release_notifier(mut self, n: Arc<tokio::sync::Notify>) -> Self {
        self.notifier.replace(n);
        self
    }

    fn new_sample(&self, latency: Duration, outcome: Outcome) -> Sample {
        Sample {
            latency,
            in_flight: self.in_flight(),
            outcome,
        }
    }

    fn available(&self) -> usize {
        self.semaphore.available_permits()
    }

    pub(crate) fn limit(&self) -> usize {
        self.limit.load(Ordering::Acquire)
    }

    fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }

    pub(crate) fn in_flight_shared(&self) -> Arc<AtomicUsize> {
        self.in_flight.clone()
    }

    /// The current state of the limiter.
    pub fn state(&self) -> LimiterState {
        LimiterState {
            limit: self.limit(),
            available: self.available(),
            in_flight: self.in_flight(),
        }
    }

    pub(crate) async fn on_rejection(&self) {
        if let Some(delay) = self.rejection_delay {
            tokio::time::sleep(delay).await;
        }
    }

    pub(crate) fn mint_token(&self, permit: OwnedSemaphorePermit) -> Token {
        Token::new(permit, self.in_flight.clone())
    }
}

#[async_trait]
impl<T> Limiter for DefaultLimiter<T>
where
    T: LimitAlgorithm + Sync,
{
    async fn try_acquire(&self) -> Option<Token> {
        match Arc::clone(&self.semaphore).try_acquire_owned() {
            Ok(permit) => Some(self.mint_token(permit)),
            Err(TryAcquireError::NoPermits) => {
                self.on_rejection().await;
                None
            }
            Err(TryAcquireError::Closed) => {
                panic!("we own the semaphore, we shouldn't have closed it")
            }
        }
    }

    async fn acquire_timeout(&self, duration: Duration) -> Option<Token> {
        match timeout(duration, Arc::clone(&self.semaphore).acquire_owned()).await {
            Ok(Ok(permit)) => Some(self.mint_token(permit)),
            Err(_) => {
                self.on_rejection().await;
                None
            }

            Ok(Err(_)) => {
                panic!("we own the semaphore, we shouldn't have closed it")
            }
        }
    }

    async fn release(&self, token: Token, outcome: Option<Outcome>) -> usize {
        let limit = if let Some(outcome) = outcome {
            let sample = self.new_sample(token.latency(), outcome);

            let new_limit = self.limit_algo.update(sample).await;

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
                            .acquire_many(
                                u32::value_from(old_limit - new_limit)
                                    .expect("change in limit shouldn't be > u32::MAX"),
                            )
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

            new_limit
        } else {
            self.limit_algo.limit()
        };

        drop(token);

        limit
    }
}

impl LimiterState {
    /// The current concurrency limit.
    pub fn limit(&self) -> usize {
        self.limit
    }
    /// The amount of concurrency available to use.
    pub fn available(&self) -> usize {
        self.available
    }
    /// The number of jobs in flight.
    pub fn in_flight(&self) -> usize {
        self.in_flight
    }
}

impl Outcome {
    pub(crate) fn overloaded_or(self, other: Outcome) -> Outcome {
        use Outcome::*;
        match (self, other) {
            (Success, Overload) => Overload,
            _ => self,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::assert_elapsed;
    use crate::{limits::Fixed, DefaultLimiter, Limiter, Outcome};

    #[tokio::test]
    async fn it_works() {
        let limiter = DefaultLimiter::new(Fixed::new(10));

        let token = limiter.try_acquire().await.unwrap();

        limiter.release(token, Some(Outcome::Success)).await;

        assert_eq!(limiter.limit(), 10);
    }

    #[tokio::test]
    async fn on_rejection_delay_acquire() {
        let delay = Duration::from_millis(50);

        let limiter = DefaultLimiter::new(Fixed::new(1)).with_rejection_delay(delay);

        let _token = limiter.try_acquire().await.unwrap();

        let now = Instant::now();
        let token = limiter.try_acquire().await;

        assert!(token.is_none());
        assert_elapsed!(now, delay, Duration::from_millis(10));
    }

    #[tokio::test]
    async fn on_rejection_delay_acquire_timeout() {
        let delay = Duration::from_millis(50);

        let limiter = DefaultLimiter::new(Fixed::new(1)).with_rejection_delay(delay);

        let _token = limiter.try_acquire().await.unwrap();

        let now = Instant::now();
        let token = limiter.acquire_timeout(Duration::ZERO).await;

        assert!(token.is_none());
        assert_elapsed!(now, delay, Duration::from_millis(10));
    }

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
