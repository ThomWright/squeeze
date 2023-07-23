use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use async_trait::async_trait;

use super::{LimitAlgorithm, Sample};

/// Delay-based congestion avoidance.
///
/// Additive increase, additive decrease.
///
/// Estimates queuing delay by comparing the current latency with the minimum observed latency.
///
/// TODO: For more stability consider wrapping with a percentile window sampler. This calculates a
/// percentile (e.g. P90) over a period of time and provides that as a sample. Vegas then compares
/// recent P90 latency with the minimum observed P90. Used this way, Vegas can handle heterogeneous
/// workloads, as long as the percentile latency is fairly stable.
///
/// Can fairly distribute concurrency between independent clients as long as there is enough server
/// capacity to handle the requests. That is: as long as the server isn't overloaded and failing to
/// handle requests as a result.
///
/// Inspired by TCP Vegas.
///
/// - [TCP Vegas: End to End Congestion Avoidance on a Global
///   Internet](https://www.cs.princeton.edu/courses/archive/fall06/cos561/papers/vegas.pdf)
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

    const DEFAULT_INCREASE: usize = 1;
    const DEFAULT_DECREASE: usize = 1;
    const DEFAULT_INCREASE_MIN_UTILISATION: f64 = 0.5;

    const MIN_SAMPLE_LATENCY: Duration = Duration::from_micros(1);

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

    /// Vegas algorithm:
    ///
    /// ```text
    /// d    = estimated min. latency with no queueing
    /// D(t) = observed latency for a job at time t
    /// L(t) = concurrency limit at time t
    /// F(t) = jobs in flight at time t
    ///
    /// L(t) / d    = E = expected rate (no queueing)
    /// L(t) / D(t) = A = actual rate
    ///
    /// E - A = DIFF [>= 0]
    ///
    /// alpha = low rate threshold: too little queueing
    /// beta  = high rate threshold: too much queueing
    ///
    /// L(t+1) = L(t) + (1 / D(t)) if DIFF < alpha and F(t) > L(t) / 2
    ///               - (1 / D(t)) if DIFF > beta
    /// ```
    ///
    /// Or, using queue size instead of rate:
    ///
    /// ```text
    /// queue_size = L(t) * (1 − d / D(T)) [>= 0]
    ///
    /// alpha = low queue threshold
    /// beta  = high queue threshold
    ///
    /// L(t+1) = L(t) + (1 / D(t)) if queue_size < alpha and F(t) > L(t) / 2
    ///               - (1 / D(t)) if queue_size > beta
    /// ```
    ///
    /// Example estimated queue sizes when `L(t)` = 10 and `d` = 10ms, for several changes in
    /// latency:
    ///
    /// ```text
    ///  10x => queue_size = 10 * (1 - 0.01 / 0.1)   =   9 (90%)
    ///   2x => queue_size = 10 * (1 - 0.01 / 0.02)  =   5 (50%)
    /// 1.5x => queue_size = 10 * (1 - 0.01 / 0.015) =   3 (30%)
    ///   1x => queue_size = 10 * (1 - 0.01 / 0.01)  =   0 (0%)
    /// 0.5x => queue_size = 10 * (1 - 0.01 / 0.005) = -10 (0%)
    /// ```
    async fn update(&self, sample: Sample) -> usize {
        if sample.latency < Self::MIN_SAMPLE_LATENCY {
            return self.limit.load(Ordering::Acquire);
        }

        // TODO: periodically reset min. latency measurement.
        0
    }
}
