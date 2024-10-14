//! Dynamic congestion-based concurrency limits for controlling backpressure.

#![deny(missing_docs)]

#[cfg(doctest)]
use doc_comment::doctest;
#[cfg(doctest)]
doctest!("../README.md");

pub mod aggregation;
mod limiter;
pub mod limits;
mod moving_avg;

pub use limiter::{
    create_static_partitions, DefaultLimiter, Limiter, LimiterState, Outcome, PartitionedLimiter,
    RejectionDelay, Token,
};
