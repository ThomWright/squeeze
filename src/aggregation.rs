//! Sample aggregators.

use std::time::Duration;

use crate::{limits::Sample, Outcome};

/// Aggregates multiple samples into one.
///
/// Additional samples can be added to update the aggregated sample. As such, the sample window can
/// be expanded, but only contracted again by resetting.
pub trait Aggregator {
    /// Add a sample to the aggregation.
    ///
    /// Returns the current aggregated sample.
    fn sample(&mut self, sample: Sample) -> Sample;
    fn sample_size(&self) -> usize;
    fn reset(&mut self);
}

/// Average latency.
pub struct Average {
    latency_sum: Duration,
    max_in_flight: usize,
    overload: Outcome,
    samples: usize,
}

/// A latency percentile.
pub struct Percentile {
    percentile: f64,
    max_in_flight: usize,
    overload: Outcome,
    latencies: Vec<Duration>,
}

impl Aggregator for Average {
    fn sample(&mut self, sample: Sample) -> Sample {
        self.latency_sum += sample.latency;
        self.max_in_flight = self.max_in_flight.max(sample.in_flight);
        self.overload = self.overload.overloaded_or(sample.outcome);
        self.samples += 1;
        Sample {
            in_flight: self.max_in_flight,
            latency: self.latency_sum.div_f64(self.samples as f64),
            outcome: self.overload,
        }
    }

    fn sample_size(&self) -> usize {
        self.samples
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

impl Default for Average {
    fn default() -> Self {
        Self {
            latency_sum: Duration::ZERO,
            max_in_flight: 0,
            overload: Outcome::Success,
            samples: 0,
        }
    }
}

impl Percentile {
    pub fn new(percentile: f64) -> Self {
        assert!(
            percentile > 0. && percentile < 1.,
            "percentiles must be between 0 and 1 exclusive"
        );
        Self {
            percentile,
            ..Default::default()
        }
    }
}

impl Aggregator for Percentile {
    fn sample(&mut self, sample: Sample) -> Sample {
        self.latencies.push(sample.latency);
        self.max_in_flight = self.max_in_flight.max(sample.in_flight);
        self.overload = self.overload.overloaded_or(sample.outcome);

        let index = (self.latencies.len() as f64 * self.percentile).ceil() as usize;
        Sample {
            in_flight: self.max_in_flight,
            latency: *self.latencies.get(index - 1).expect("index should exist"),
            outcome: self.overload,
        }
    }

    fn sample_size(&self) -> usize {
        self.latencies.len()
    }

    fn reset(&mut self) {
        *self = Self {
            percentile: self.percentile,
            ..Default::default()
        };
    }
}

impl Default for Percentile {
    fn default() -> Self {
        Self {
            percentile: 0.5,
            latencies: Vec::new(),
            max_in_flight: 0,
            overload: Outcome::Success,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn average() {
        let mut aggregator = Average::default();

        aggregator.sample(Sample {
            in_flight: 1,
            latency: Duration::from_millis(1),
            outcome: Outcome::Success,
        });

        aggregator.sample(Sample {
            in_flight: 5,
            latency: Duration::from_millis(3),
            outcome: Outcome::Overload,
        });

        let sample = aggregator.sample(Sample {
            in_flight: 3,
            latency: Duration::from_millis(5),
            outcome: Outcome::Success,
        });

        assert_eq!(
            sample,
            Sample {
                in_flight: 5,
                latency: Duration::from_millis(3),
                outcome: Outcome::Overload,
            }
        );
    }

    #[tokio::test]
    async fn average_reset() {
        let mut aggregator = Average::default();

        aggregator.sample(Sample {
            in_flight: 1,
            latency: Duration::from_millis(1),
            outcome: Outcome::Success,
        });

        aggregator.reset();

        let sample = aggregator.sample(Sample {
            in_flight: 3,
            latency: Duration::from_millis(5),
            outcome: Outcome::Success,
        });

        assert_eq!(
            sample,
            Sample {
                in_flight: 3,
                latency: Duration::from_millis(5),
                outcome: Outcome::Success,
            },
            "should be equal to new sample after reset"
        )
    }

    #[tokio::test]
    async fn percentile_p01() {
        let mut aggregator = Percentile::new(0.01);

        aggregator.sample(Sample {
            in_flight: 1,
            latency: Duration::from_millis(1),
            outcome: Outcome::Success,
        });

        aggregator.sample(Sample {
            in_flight: 5,
            latency: Duration::from_millis(3),
            outcome: Outcome::Overload,
        });

        let sample = aggregator.sample(Sample {
            in_flight: 3,
            latency: Duration::from_millis(5),
            outcome: Outcome::Success,
        });

        assert_eq!(
            sample,
            Sample {
                in_flight: 5,
                latency: Duration::from_millis(1),
                outcome: Outcome::Overload,
            }
        );
    }

    #[tokio::test]
    async fn percentile_p99() {
        let mut aggregator = Percentile::new(0.99);

        aggregator.sample(Sample {
            in_flight: 1,
            latency: Duration::from_millis(1),
            outcome: Outcome::Success,
        });

        aggregator.sample(Sample {
            in_flight: 5,
            latency: Duration::from_millis(3),
            outcome: Outcome::Overload,
        });

        let sample = aggregator.sample(Sample {
            in_flight: 3,
            latency: Duration::from_millis(5),
            outcome: Outcome::Success,
        });

        assert_eq!(
            sample,
            Sample {
                in_flight: 5,
                latency: Duration::from_millis(5),
                outcome: Outcome::Overload,
            }
        );
    }

    #[tokio::test]
    async fn percentile_reset() {
        let mut aggregator = Percentile::new(0.99);

        aggregator.sample(Sample {
            in_flight: 1,
            latency: Duration::from_millis(1),
            outcome: Outcome::Success,
        });

        aggregator.reset();

        let sample = aggregator.sample(Sample {
            in_flight: 3,
            latency: Duration::from_millis(5),
            outcome: Outcome::Success,
        });

        assert_eq!(
            sample,
            Sample {
                in_flight: 3,
                latency: Duration::from_millis(5),
                outcome: Outcome::Success,
            },
            "should be equal to new sample after reset"
        );

        assert_eq!(
            aggregator.percentile, 0.99,
            "percentile shouldn't change after reset"
        );
    }
}
