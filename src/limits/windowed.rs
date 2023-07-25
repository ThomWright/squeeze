use std::time::Duration;

use async_trait::async_trait;
use tokio::{sync::Mutex, time::Instant};

use crate::aggregators::Sampler;

use super::{defaults::MIN_SAMPLE_LATENCY, LimitAlgorithm, Sample};

pub struct Windowed<L, S> {
    min_window: Duration,
    max_window: Duration,
    min_samples: usize,
    min_latency: Duration,

    inner: L,

    window: Mutex<Window<S>>,
}

struct Window<S> {
    aggregator: S,
    start: Instant,
    duration: Duration,
}

impl<L: LimitAlgorithm, S: Sampler> Windowed<L, S> {
    pub fn new(inner: L, sampler: S) -> Self {
        let min_window = Duration::from_micros(1);
        Self {
            min_window,
            max_window: Duration::from_secs(1),
            min_samples: 10,
            min_latency: MIN_SAMPLE_LATENCY,

            inner,

            window: Mutex::new(Window {
                aggregator: sampler,
                duration: min_window,
                start: Instant::now(),
            }),
        }
    }

    pub fn with_min_samples(mut self, samples: usize) -> Self {
        assert!(samples > 0, "at least one sample required per window");
        self.min_samples = samples;
        self
    }

    pub fn with_min_window(mut self, min: Duration) -> Self {
        self.min_window = min;
        self
    }

    pub fn with_max_window(mut self, max: Duration) -> Self {
        self.max_window = max;
        self
    }
}

#[async_trait]
impl<L, S> LimitAlgorithm for Windowed<L, S>
where
    L: LimitAlgorithm + Send + Sync,
    S: Sampler + Send + Sync,
{
    fn limit(&self) -> usize {
        self.inner.limit()
    }

    async fn update(&self, sample: Sample) -> usize {
        if sample.latency < self.min_latency {
            return self.inner.limit();
        }

        let mut window = self.window.lock().await;

        let agg_sample = window.aggregator.sample(sample);

        if window.aggregator.samples() >= self.min_samples
            && window.start.elapsed() >= window.duration
        {
            window.aggregator.reset();

            window.start = Instant::now();

            // TODO: the Netflix lib uses min latency, make this configurable?
            window.duration = agg_sample.latency.clamp(self.min_window, self.max_window);

            self.inner.update(agg_sample).await
        } else {
            self.inner.limit()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{aggregators::Average, limits::Vegas, Outcome};

    use super::*;

    #[tokio::test]
    async fn it_works() {
        let samples = 2;

        // Just test with a min sample size for now
        let windowed_vegas = Windowed::new(Vegas::with_initial_limit(10), Average::default())
            .with_min_samples(samples)
            .with_min_window(Duration::ZERO)
            .with_max_window(Duration::ZERO);

        let mut limit = 0;

        for _ in 0..samples {
            limit = windowed_vegas
                .update(Sample {
                    in_flight: 1,
                    latency: Duration::from_millis(10),
                    outcome: Outcome::Success,
                })
                .await;
        }
        assert_eq!(limit, 10, "first window shouldn't change limit for Vegas");

        for _ in 0..samples {
            limit = windowed_vegas
                .update(Sample {
                    in_flight: 1,
                    latency: Duration::from_millis(100),
                    outcome: Outcome::Overload,
                })
                .await;
        }
        assert!(limit < 10, "limit should be reduced");
    }
}
