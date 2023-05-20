// Straight from Stack Overflow: https://stackoverflow.com/questions/43921436/extend-iterator-with-a-mean-method

use std::time::Duration;

pub trait MeanExt: Iterator {
    fn mean<M>(self) -> M
    where
        M: Mean<Self::Item>,
        Self: Sized,
    {
        M::mean(self)
    }
}

impl<I: Iterator> MeanExt for I {}

pub trait Mean<A = Self> {
    fn mean<I>(iter: I) -> Self
    where
        I: Iterator<Item = A>;
}

impl Mean for Duration {
    fn mean<I>(iter: I) -> Self
    where
        I: Iterator<Item = Duration>,
    {
        let mut sum = Duration::ZERO;
        let mut count: usize = 0;

        for v in iter {
            sum += v;
            count += 1;
        }

        if count > 0 {
            sum / count as u32
        } else {
            sum
        }
    }
}

impl<'a> Mean<&'a Duration> for Duration {
    fn mean<I>(iter: I) -> Self
    where
        I: Iterator<Item = &'a Duration>,
    {
        iter.mean()
    }
}
