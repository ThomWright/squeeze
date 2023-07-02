//! Dynamic congestion-based concurrency limits for controlling backpressure.

pub mod limit;
mod limiter;
mod mov_avg;

pub use limiter::{Limiter, LimiterState, Outcome, Token};
