use super::{sample_rate, sinc};
use crate::common::{ChannelCount, SampleRate};
use crate::Sample;
use std::sync::atomic::{AtomicBool, Ordering};

/// Reconstruction filter used when converting a source to the device rate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResamplingMethod {
    /// Original Rodio conversion, including its downsampling behavior.
    #[default]
    Linear,
    /// Rubato 24-tap Hann-windowed sinc for upsampling only.
    /// Equal-rate and downsampled sources retain the linear path.
    Sinc24,
}

static DEFAULT_SINC: AtomicBool = AtomicBool::new(false);

/// Selects the process audio backend's filter for subsequent source spans.
/// Existing spans retain their filter. Prepare coefficients on the calling thread,
/// before the audio thread first constructs a sinc converter.
pub fn set_default_resampling_method(method: ResamplingMethod) {
    if method == ResamplingMethod::Sinc24 {
        sinc::prepare();
    }
    DEFAULT_SINC.store(method == ResamplingMethod::Sinc24, Ordering::Relaxed);
}

pub(crate) fn default_resampling_method() -> ResamplingMethod {
    if DEFAULT_SINC.load(Ordering::Relaxed) {
        ResamplingMethod::Sinc24
    } else {
        ResamplingMethod::Linear
    }
}

/// Streaming sample-rate conversion with an explicitly selected reconstruction filter.
#[derive(Clone, Debug)]
pub struct SampleRateConverter<I: Iterator<Item = Sample>> {
    inner: Converter<I>,
}

#[derive(Clone, Debug)]
enum Converter<I: Iterator<Item = Sample>> {
    Linear(sample_rate::SampleRateConverter<I>),
    Sinc(sinc::Sinc<I>),
}

impl<I: Iterator<Item = Sample>> SampleRateConverter<I> {
    /// Retains Rodio's original linear conversion, independently of the backend default.
    pub fn new(input: I, from: SampleRate, to: SampleRate, channels: ChannelCount) -> Self {
        Self {
            inner: Converter::Linear(sample_rate::SampleRateConverter::new(
                input, from, to, channels,
            )),
        }
    }

    /// Returns the source iterator, discarding buffered samples.
    pub fn into_inner(self) -> I {
        match self.inner {
            Converter::Linear(inner) => inner.into_inner(),
            Converter::Sinc(inner) => inner.input,
        }
    }

    /// Borrows the underlying source iterator.
    pub fn inner(&self) -> &I {
        match &self.inner {
            Converter::Linear(inner) => inner.inner(),
            Converter::Sinc(inner) => &inner.input,
        }
    }

    /// Mutably borrows the source; buffered samples are unaffected.
    pub fn inner_mut(&mut self) -> &mut I {
        match &mut self.inner {
            Converter::Linear(inner) => inner.inner_mut(),
            Converter::Sinc(inner) => &mut inner.input,
        }
    }
}

impl<I: Iterator<Item = Sample>> SampleRateConverter<I> {
    /// Sinc conversion buffers at most 26 source frames and retains the source duration.
    /// Source edges repeat the nearest frame; interpolation never reads a following cue.
    pub fn new_with_method(
        input: I,
        from: SampleRate,
        to: SampleRate,
        channels: ChannelCount,
        method: ResamplingMethod,
    ) -> Self {
        if method == ResamplingMethod::Sinc24 && from < to {
            Self {
                inner: Converter::Sinc(sinc::Sinc::new(input, from, to, channels)),
            }
        } else {
            Self::new(input, from, to, channels)
        }
    }
}

impl<I: Iterator<Item = Sample>> Iterator for SampleRateConverter<I> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        match &mut self.inner {
            Converter::Linear(inner) => inner.next(),
            Converter::Sinc(inner) => inner.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.inner {
            Converter::Linear(inner) => inner.size_hint(),
            Converter::Sinc(inner) => inner.size_hint(),
        }
    }
}

impl<I: ExactSizeIterator<Item = Sample>> ExactSizeIterator for SampleRateConverter<I> {}
