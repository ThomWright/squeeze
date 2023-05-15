//! Algorithms for controlling concurrency limits.

use crate::limiter::Reading;

mod aimd;
mod gradient2;
mod vegas;

pub use aimd::AimdLimit;

pub trait LimitAlgorithm {
    fn initial_limit(&self) -> usize;
    fn update(&self, reading: Reading) -> usize;
}

pub struct FixedLimit(usize);
impl FixedLimit {
    pub fn limit(limit: usize) -> Self {
        Self(limit)
    }
}
impl LimitAlgorithm for FixedLimit {
    fn initial_limit(&self) -> usize {
        self.0
    }
    fn update(&self, _reading: Reading) -> usize {
        self.0
    }
}
