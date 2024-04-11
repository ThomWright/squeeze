//! Dynamic congestion-based concurrency limits for controlling backpressure.

// #![deny(missing_docs)]

#[cfg(doctest)]
use doc_comment::doctest;
#[cfg(doctest)]
doctest!("../README.md");

pub mod limits;
mod limiter;
mod mov_avgs;
pub mod aggregators;

pub use limiter::{Limiter, LimiterState, Outcome, Token};
