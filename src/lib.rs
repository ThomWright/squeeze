//! Dynamic congestion-based concurrency limits for controlling backpressure.

pub mod limits;
mod limiter;
mod mov_avgs;
pub mod aggregators;

pub use limiter::{Limiter, LimiterState, Outcome, Token};
