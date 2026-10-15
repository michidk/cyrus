use std::cmp;
use std::time::Duration;

use super::SeekError;
use crate::common::{ChannelCount, SampleRate};
use crate::conversions::{ChannelCountConverter, SampleRateConverter};
use crate::Source;

/// An iterator that reads from a `Source` and converts the samples to a
/// specific type, sample-rate and channels count.
///
/// It implements `Source` as well, but all the data is guaranteed to be in a
/// single span whose channels and samples rate have been passed to `new`.
#[derive(Clone)]
pub struct UniformSourceIterator<I>
where
    I: Source,
{
    inner: Option<ChannelCountConverter<SampleRateConverter<Take<I>>>>,
    pending: Option<I>,
    target_channels: ChannelCount,
    target_sample_rate: SampleRate,
    total_duration: Option<Duration>,
}

impl<I> UniformSourceIterator<I>
where
    I: Source,
{
    /// Wrap a `Source` and lazily convert its samples to a specific type,
    /// sample-rate and channels count.
    #[inline]
    pub fn new(
        input: I,
        target_channels: ChannelCount,
        target_sample_rate: SampleRate,
    ) -> UniformSourceIterator<I> {
        let total_duration = input.total_duration();

        UniformSourceIterator {
            inner: None,
            pending: Some(input),
            target_channels,
            target_sample_rate,
            total_duration,
        }
    }

    #[inline]
    fn bootstrap(
        mut input: I,
        target_channels: ChannelCount,
        target_sample_rate: SampleRate,
    ) -> ChannelCountConverter<SampleRateConverter<Take<I>>> {
        // Pull the first sample before fixing metadata: a queue or control may change it.
        let first = input.next();
        let remaining = input.current_span_len().or_else(|| {
            let (lower, upper) = input.size_hint();
            (upper == Some(lower)).then_some(lower)
        });
        let span_len = remaining.map(|n| n.saturating_add(usize::from(first.is_some())));

        let from_channels = input.channels();
        let from_sample_rate = input.sample_rate();

        let input = Take {
            iter: input,
            first,
            n: span_len,
        };
        let input =
            SampleRateConverter::new(input, from_sample_rate, target_sample_rate, from_channels);
        ChannelCountConverter::new(input, from_channels, target_channels)
    }
}

impl<I> Iterator for UniformSourceIterator<I>
where
    I: Source,
{
    type Item = I::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(input) = self.pending.take() {
            self.inner = Some(Self::bootstrap(
                input,
                self.target_channels,
                self.target_sample_rate,
            ));
        }
        if let Some(value) = self.inner.as_mut().unwrap().next() {
            return Some(value);
        }

        let input = self.inner.take().unwrap().into_inner().into_inner().iter;

        let mut input =
            UniformSourceIterator::bootstrap(input, self.target_channels, self.target_sample_rate);

        let value = input.next();
        self.inner = Some(input);
        value
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (
            self.inner.as_ref().map_or(0, |inner| inner.size_hint().0),
            None,
        )
    }
}

impl<I> Source for UniformSourceIterator<I>
where
    I: Iterator + Source,
{
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    #[inline]
    fn channels(&self) -> ChannelCount {
        self.target_channels
    }

    #[inline]
    fn sample_rate(&self) -> SampleRate {
        self.target_sample_rate
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    #[inline]
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        if let Some(input) = self.inner.as_mut() {
            input.inner_mut().inner_mut().inner_mut().try_seek(pos)
        } else if let Some(input) = self.pending.as_mut() {
            input.try_seek(pos)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug)]
struct Take<I: Iterator> {
    iter: I,
    first: Option<I::Item>,
    n: Option<usize>,
}

impl<I: Iterator> Take<I> {
    #[inline]
    pub fn inner_mut(&mut self) -> &mut I {
        &mut self.iter
    }
}

impl<I> Iterator for Take<I>
where
    I: Iterator,
{
    type Item = <I as Iterator>::Item;

    #[inline]
    fn next(&mut self) -> Option<<I as Iterator>::Item> {
        if let Some(n) = &mut self.n {
            if *n != 0 {
                *n -= 1;
                self.first.take().or_else(|| self.iter.next())
            } else {
                None
            }
        } else {
            self.first.take().or_else(|| self.iter.next())
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        if let Some(n) = self.n {
            let (lower, upper) = self.iter.size_hint();
            let pending = usize::from(self.first.is_some());
            let lower = lower.saturating_add(pending);
            let upper = upper.and_then(|upper| upper.checked_add(pending));

            let lower = cmp::min(lower, n);

            let upper = match upper {
                Some(x) if x < n => Some(x),
                _ => Some(n),
            };

            (lower, upper)
        } else {
            let (lower, upper) = self.iter.size_hint();
            let pending = usize::from(self.first.is_some());
            (
                lower.saturating_add(pending),
                upper.and_then(|upper| upper.checked_add(pending)),
            )
        }
    }
}

impl<I> ExactSizeIterator for Take<I> where I: ExactSizeIterator {}
