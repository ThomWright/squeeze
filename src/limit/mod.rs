//! Algorithms for controlling concurrency limits.

mod aimd;
mod gradient2;
mod vegas;

use std::time::Duration;

use crate::Outcome;

pub use aimd::AimdLimit;

pub trait LimitAlgorithm {
    fn initial_limit(&self) -> usize;
    fn update(&self, sample: Sample) -> usize;
}

#[derive(Debug, Clone)]
pub struct Sample {
    pub(crate) latency: Duration,
    pub(crate) outcome: Outcome,
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
    fn update(&self, _reading: Sample) -> usize {
        self.0
    }
}
