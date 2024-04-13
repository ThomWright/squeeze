//! Dynamic congestion-based concurrency limits for controlling backpressure.

// #![deny(missing_docs)]

#[cfg(doctest)]
use doc_comment::doctest;
#[cfg(doctest)]
doctest!("../README.md");

pub mod aggregators;
mod limiter;
pub mod limits;
mod mov_avgs;

pub use limiter::{Limiter, LimiterState, Outcome, Token};
