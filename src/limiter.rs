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

use crate::limit::LimitAlgorithm;

pub struct Limiter<T> {
    limit_algo: T,
    semaphore: Arc<Semaphore>,
    limit: AtomicUsize,
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
        }
    }

    pub fn try_acquire(&self) -> Option<Timer<'_>> {
        match self.semaphore.try_acquire() {
            Ok(permit) => Some(Timer::new(permit)),
            Err(TryAcquireError::NoPermits) => None,
            Err(TryAcquireError::Closed) => {
                panic!("we own the semaphore, we shouldn't have closed it")
            }
        }
    }

    pub async fn acquire_timeout(&self, duration: Duration) -> Option<Timer<'_>> {
        match timeout(duration, self.semaphore.acquire()).await {
            Ok(Ok(permit)) => Some(Timer::new(permit)),
            Err(_) => None,

            Ok(Err(_)) => {
                panic!("we own the semaphore, we shouldn't have closed it")
            }
        }
    }

    pub async fn record_reading(&self, timer: Timer<'_>, result: ReadingResult) {
        let reading = Reading {
            latency: timer.start.elapsed(),
            result,
        };

        let new_limit = self.limit_algo.update(reading);

        let old_limit = self.limit.swap(new_limit, Ordering::SeqCst);

        drop(timer.permit);

        match new_limit.cmp(&old_limit) {
            cmp::Ordering::Greater => {
                self.semaphore.add_permits(new_limit - old_limit);
            }
            cmp::Ordering::Less => {
                let semaphore = self.semaphore.clone();
                tokio::spawn(async move {
                    let permits = semaphore
                        .acquire_many((old_limit - new_limit) as u32)
                        .await
                        .expect("we own the semaphore, we shouldn't have closed it");

                    permits.forget();
                });
            }
            _ => {}
        }
    }

    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }

    pub fn limit(&self) -> usize {
        self.limit.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
pub struct Timer<'t> {
    permit: SemaphorePermit<'t>,
    start: Instant,
}

impl<'t> Timer<'t> {
    fn new(permit: SemaphorePermit<'t>) -> Self {
        Self {
            permit,
            start: Instant::now(),
        }
    }
}

pub struct Reading {
    pub(crate) latency: Duration,
    pub(crate) result: ReadingResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReadingResult {
    Success,
    Ignore,
    Overload,
}

#[cfg(test)]
mod tests {
    use crate::{limit::FixedLimit, Limiter, ReadingResult};

    #[tokio::test]
    async fn it_works() {
        let limiter = Limiter::new(FixedLimit::limit(10));

        let timer = limiter.try_acquire().unwrap();

        limiter.record_reading(timer, ReadingResult::Success).await;

        assert_eq!(limiter.limit(), 10);
    }
}
