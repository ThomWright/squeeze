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

use crate::limits::{LimitAlgorithm, Sample};

/// Limits the number of concurrent jobs.
///
/// Concurrency is limited through the use of [Token]s. Acquire a token to run a job, and release the
/// token once the job is finished.
///
/// The limit will be automatically adjusted based on observed latency (delay) and/or failures
/// caused by overload (loss).
#[derive(Debug)]
pub struct Limiter<T> {
    limit_algo: T,
    semaphore: Arc<Semaphore>,
    limit: AtomicUsize,

    /// Best-effort
    in_flight: Arc<AtomicUsize>,

    rejection_delay: Option<Duration>,

    #[cfg(test)]
    notifier: Option<Arc<tokio::sync::Notify>>,
}

/// A concurrency token, required to run a job.
///
/// Release the token back to the [Limiter] after the job is complete.
#[derive(Debug)]
pub struct Token<'t> {
    _permit: SemaphorePermit<'t>,
    start: Instant,
    #[cfg(test)]
    latency: Duration,
    in_flight: Arc<AtomicUsize>,
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

impl<T> Limiter<T>
where
    T: LimitAlgorithm,
{
    /// Create a limiter with a given limit control algorithm.
    pub fn new(limit_algo: T) -> Self {
        let initial_permits = limit_algo.limit();
        assert!(initial_permits > 0);
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
    /// This can help reduce ...
    pub fn with_rejection_delay(mut self, delay: Duration) -> Self {
        self.rejection_delay = Some(delay);
        self
    }

    /// In some cases [Token]s are acquired asynchronously when updating the limit.
    #[cfg(test)]
    pub fn with_release_notifier(mut self, n: Arc<tokio::sync::Notify>) -> Self {
        self.notifier = Some(n);
        self
    }

    /// Try to immediately acquire a concurrency [Token].
    ///
    /// Returns `None` if there are none available.
    pub async fn try_acquire(&self) -> Option<Token<'_>> {
        match self.semaphore.try_acquire() {
            Ok(permit) => {
                self.in_flight.fetch_add(1, Ordering::AcqRel);
                Some(Token::new(permit, self.in_flight.clone()))
            }
            Err(TryAcquireError::NoPermits) => {
                if let Some(delay) = self.rejection_delay {
                    tokio::time::sleep(delay).await;
                }
                None
            }
            Err(TryAcquireError::Closed) => {
                panic!("we own the semaphore, we shouldn't have closed it")
            }
        }
    }

    /// Try to acquire a concurrency [Token], waiting for `duration` if there are none available.
    ///
    /// Returns `None` if there are none available after `duration`.
    pub async fn acquire_timeout(&self, duration: Duration) -> Option<Token<'_>> {
        match timeout(duration, self.semaphore.acquire()).await {
            Ok(Ok(permit)) => Some(Token::new(permit, self.in_flight.clone())),
            Err(_) => {
                if let Some(delay) = self.rejection_delay {
                    tokio::time::sleep(delay).await;
                }
                None
            }

            Ok(Err(_)) => {
                panic!("we own the semaphore, we shouldn't have closed it")
            }
        }
    }

    /// Return the concurrency [Token], along with the outcome of the job.
    ///
    /// The [Outcome] of the job, and the time taken to perform it, may be used
    /// to update the concurrency limit.
    ///
    /// Set the outcome to `None` to ignore the job.
    pub async fn release(&self, token: Token<'_>, outcome: Option<Outcome>) {
        if let Some(outcome) = outcome {
            let sample = self.new_sample(&token, outcome);

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

        drop(token);
    }

    #[cfg(test)]
    fn new_sample(&self, token: &Token<'_>, outcome: Outcome) -> Sample {
        Sample {
            latency: token.latency,
            in_flight: self.in_flight(),
            outcome,
        }
    }

    #[cfg(not(test))]
    fn new_sample(&self, token: &Token<'_>, outcome: Outcome) -> Sample {
        Sample {
            latency: token.start.elapsed(),
            in_flight: self.in_flight(),
            outcome,
        }
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

    /// The current state of the limiter.
    pub fn state(&self) -> LimiterState {
        LimiterState {
            limit: self.limit(),
            available: self.available(),
            in_flight: self.in_flight(),
        }
    }
}

impl<'t> Token<'t> {
    fn new(permit: SemaphorePermit<'t>, in_flight: Arc<AtomicUsize>) -> Self {
        Self {
            _permit: permit,
            start: Instant::now(),
            #[cfg(test)]
            latency: Duration::ZERO,
            in_flight,
        }
    }

    #[cfg(test)]
    pub fn set_latency(&mut self, latency: Duration) {
        use std::ops::Sub;

        self.start = Instant::now().sub(latency);
        self.latency = latency;
    }
}

impl Drop for Token<'_> {
    /// Reduces the number of jobs in flight and releases the token back to the available pool.
    fn drop(&mut self) {
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
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
    use crate::{limits::Fixed, Limiter, Outcome};

    #[tokio::test]
    async fn it_works() {
        let limiter = Limiter::new(Fixed::new(10));

        let token = limiter.try_acquire().await.unwrap();

        limiter.release(token, Some(Outcome::Success)).await;

        assert_eq!(limiter.limit(), 10);
    }

    #[tokio::test]
    async fn on_rejection_delay_acquire() {
        let delay = Duration::from_millis(50);

        let limiter = Limiter::new(Fixed::new(1)).with_rejection_delay(delay);

        let _token = limiter.try_acquire().await.unwrap();

        let now = Instant::now();
        let token = limiter.try_acquire().await;

        assert!(token.is_none());
        assert_elapsed!(now, delay, Duration::from_millis(10));
    }

    #[tokio::test]
    async fn on_rejection_delay_acquire_timeout() {
        let delay = Duration::from_millis(50);

        let limiter = Limiter::new(Fixed::new(1)).with_rejection_delay(delay);

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
