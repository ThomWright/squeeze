//! Algorithms for controlling concurrency limits.

mod aimd;
mod defaults;
mod fixed;
mod gradient;
mod vegas;
mod windowed;

use async_trait::async_trait;
use std::time::Duration;

use crate::Outcome;

pub use aimd::Aimd;
pub use fixed::Fixed;
pub use gradient::Gradient;
pub use vegas::Vegas;
pub use windowed::Windowed;

/// An algorithm for controlling a concurrency limit.
#[async_trait]
pub trait LimitAlgorithm {
    /// The current limit.
    fn limit(&self) -> usize;

    /// Update the concurrency limit in response to a new job completion.
    async fn update(&self, sample: Sample) -> usize;
}

/// The result of a job (or jobs), including the [Outcome] (loss) and latency (delay).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    pub(crate) latency: Duration,
    /// Jobs in flight when the sample was taken.
    pub(crate) in_flight: usize,
    pub(crate) outcome: Outcome,
}
