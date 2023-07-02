//! Algorithms for controlling concurrency limits.

mod aimd;
mod gradient;
mod vegas;

use std::time::Duration;
use async_trait::async_trait;

use crate::Outcome;

pub use aimd::AimdLimit;
pub use gradient::GradientLimit;

#[async_trait]
pub trait LimitAlgorithm {
    fn limit(&self) -> usize;
    async fn update(&self, sample: Sample) -> usize;
}

#[derive(Debug, Clone)]
pub struct Sample {
    pub(crate) latency: Duration,
    /// Requests in flight when the sample was taken (end of request).
    pub(crate) in_flight: usize,
    pub(crate) outcome: Outcome,
}

pub struct FixedLimit(usize);
impl FixedLimit {
    pub fn limit(limit: usize) -> Self {
        Self(limit)
    }
}
#[async_trait]
impl LimitAlgorithm for FixedLimit {
    fn limit(&self) -> usize {
        self.0
    }
    async fn update(&self, _reading: Sample) -> usize {
        self.0
    }
}
