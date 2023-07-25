use async_trait::async_trait;

use super::{LimitAlgorithm, Sample};

/// A simple, fixed concurrency limit.
pub struct Fixed(usize);
impl Fixed {
    pub fn new(limit: usize) -> Self {
        assert!(limit > 0);

        Self(limit)
    }
}

#[async_trait]
impl LimitAlgorithm for Fixed {
    fn limit(&self) -> usize {
        self.0
    }
    async fn update(&self, _reading: Sample) -> usize {
        self.0
    }
}
