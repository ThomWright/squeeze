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

pub struct Limiter<T> {
    limit_algo: T,
    semaphore: Arc<Semaphore>,
    limit: AtomicUsize,
}

impl<T> Limiter<T>
where
    T: LimitAlgo,
{
    pub fn new(limit_algo: T, initial_permits: usize) -> Self {
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

    pub async fn record_reading(&self, reading: Reading<'_>) {
        let new_limit = self.limit_algo.update(reading);

        let old_limit = self.limit.swap(new_limit, Ordering::SeqCst);

        match new_limit.cmp(&old_limit) {
            cmp::Ordering::Greater => {
                self.semaphore.add_permits(new_limit - old_limit);
            }
            cmp::Ordering::Less => {
                let semaphore = self.semaphore.clone();
                tokio::spawn(async move {
                    let permits = semaphore
                        // FIXME: do this async, don't block
                        .acquire_many((old_limit - new_limit) as u32)
                        .await
                        .expect("we own the semaphore, we shouldn't have closed it");

                    permits.forget();
                });
            }
            _ => {}
        }
    }

    #[cfg(test)]
    fn limit(&self) -> usize {
        self.limit.load(Ordering::Acquire)
    }
}

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

    pub fn reading(self, result: ReadingResult) -> Reading<'t> {
        Reading {
            latency: self.start.elapsed(),
            result,
            permit: self.permit,
        }
    }
}

pub struct Reading<'t> {
    latency: Duration,
    result: ReadingResult,
    permit: SemaphorePermit<'t>,
}

pub enum ReadingResult {
    Success,
    Ignore,
    Overload,
}

pub trait LimitAlgo {
    fn update(&self, reading: Reading) -> usize;
}

struct DummyLimitAlgo;
impl LimitAlgo for DummyLimitAlgo {
    fn update(&self, _reading: Reading) -> usize {
        10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn it_works() {
        let limiter = Limiter::new(DummyLimitAlgo, 10);

        let permit = limiter.try_acquire().unwrap();

        limiter
            .record_reading(permit.reading(ReadingResult::Success))
            .await;

        assert_eq!(limiter.limit(), 10);
    }
}
