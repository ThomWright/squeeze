use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;

use super::{LimitAlgorithm, Sample};

/// Delay-based congestion avoidance.
///
/// Additive increase, additive decrease. (?)
///
/// Estimates queuing delay by comparing current latency with a minimum observed latency.
///
/// Can fairly distribute concurrency between independent clients as long as there is enough server
/// capacity to handle the requests. That is: as long as the server isn't overloaded and failing to
/// handle requests as a result.
///
/// Inspired by TCP Vegas.
///
/// - [TCP Vegas: End to End Congestion Avoidance on a Global Internet](https://www.cs.princeton.edu/courses/archive/fall06/cos561/papers/vegas.pdf)
/// - [Understanding TCP Vegas: Theory and
/// Practice](https://www.cs.princeton.edu/research/techreps/TR-628-00)
pub struct VegasLimit {
    limit: AtomicUsize,
    _min_limit: usize,
    _max_limit: usize,
}

impl VegasLimit {
    const DEFAULT_MIN_LIMIT: usize = 1;
    const DEFAULT_MAX_LIMIT: usize = 100;

    pub fn new_with_initial_limit(initial_limit: usize) -> Self {
        Self {
            limit: AtomicUsize::new(initial_limit),
            _min_limit: Self::DEFAULT_MIN_LIMIT,
            _max_limit: Self::DEFAULT_MAX_LIMIT,
        }
    }

    pub fn with_max_limit(self, max: usize) -> Self {
        assert!(max > 0);
        Self {
            _max_limit: max,
            ..self
        }
    }
}

#[async_trait]
impl LimitAlgorithm for VegasLimit {
    fn limit(&self) -> usize {
        self.limit.load(Ordering::Acquire)
    }

    /// TODO: Calculated once per RTT.
    ///
    /// ```text
    /// d    = estimated min. latency with no queueing
    /// D(t) = observed latency for a job at time t
    /// L(t) = concurrency limit at time t
    ///
    /// L(t) / d    = E = expected rate (no queueing)
    /// L(t) / D(t) = A = actual rate
    ///
    /// E - A = DIFF (>= 0)
    ///
    /// alpha = low rate threshold: too little queueing
    /// beta  = high rate threshold: too much queueing
    ///
    /// L(t+1) = L(t) + (1 / D(t)) if DIFF < alpha
    ///               - (1 / D(t)) if DIFF > beta
    /// ```
    async fn update(&self, _sample: Sample) -> usize {
        0
    }
}
