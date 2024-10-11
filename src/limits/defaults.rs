use std::time::Duration;

pub(crate) const MIN_SAMPLE_LATENCY: Duration = Duration::from_micros(1);

pub(crate) const DEFAULT_MIN_LIMIT: usize = 1;
pub(crate) const DEFAULT_MAX_LIMIT: usize = 1000;
