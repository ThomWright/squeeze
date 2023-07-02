//! Algorithms for controlling concurrency limits using congestion detection.

mod aimd;
mod gradient;
mod vegas;

use async_trait::async_trait;
use std::time::Duration;

use crate::Outcome;

pub use aimd::AimdLimit;
pub use gradient::GradientLimit;

#[async_trait]
pub trait LimitAlgorithm {
    /// The current concurrency limit.
    fn limit(&self) -> usize;

    /// Update the concurrency limit in response to a new job completion.
    async fn update(&self, sample: Sample) -> usize;
}

/// The result of a job, including the [Outcome] (loss) and latency (delay).
#[derive(Debug, Clone)]
pub struct Sample {
    pub(crate) latency: Duration,
    /// Jobs in flight when the sample was taken.
    pub(crate) in_flight: usize,
    pub(crate) outcome: Outcome,
}

/// A simple, fixed concurrency limit.
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
