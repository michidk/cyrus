use crate::common::{ChannelCount, SampleRate};
use crate::Sample;
use std::sync::OnceLock;

use rubato::interpolation::{get_nearest_times_2, interp_lin};
use rubato::sinc_interpolator::{make_interpolator, AnyInterpolator, SincInterpolator};
use rubato::WindowFunction;

const FILTER_TAPS: usize = 24;
const FRAMES: usize = FILTER_TAPS + 2;
const LEFT: usize = FILTER_TAPS / 2;
const PHASES: usize = 1024;
static INTERPOLATOR: OnceLock<AnyInterpolator<f32>> = OnceLock::new();

fn interpolator() -> &'static AnyInterpolator<f32> {
    INTERPOLATOR
        .get_or_init(|| make_interpolator(FILTER_TAPS, 1.0, None, PHASES, WindowFunction::Hann))
}

pub(super) fn prepare() {
    let _ = interpolator();
}

/// Adapts frame-interleaved Rodio sources to Rubato's public fractional-reader API.
/// Rubato owns the window, cutoff, sinc generation, convolution and interpolation.
/// The ring includes the adjacent phase's frame; no DSP coefficients are authored here.
#[derive(Clone, Debug)]
pub(super) struct Sinc<I: Iterator> {
    pub(super) input: I,
    samples: Vec<Sample>,
    output: Vec<Sample>,
    channels: usize,
    from: u32,
    to: u32,
    head: usize,
    center: u64,
    frames_read: u64,
    exhausted: bool,
    output_frame: u64,
    channel: usize,
}

fn read_frame(input: &mut impl Iterator<Item = Sample>, frame: &mut [Sample]) -> bool {
    for sample in frame {
        let Some(value) = input.next() else {
            return false;
        };
        *sample = value;
    }
    true
}

impl<I: Iterator<Item = Sample>> Sinc<I> {
    pub(super) fn new(
        mut input: I,
        from: SampleRate,
        to: SampleRate,
        channels: ChannelCount,
    ) -> Self {
        prepare();
        let channels = usize::from(channels.get());
        let mut samples = vec![0.0; FRAMES * channels];
        let present = read_frame(&mut input, &mut samples[..channels]);
        for slot in 1..=LEFT {
            samples.copy_within(..channels, slot * channels);
        }
        let mut result = Self {
            input,
            samples,
            output: vec![0.0; channels],
            channels,
            from: from.get(),
            to: to.get(),
            head: 0,
            center: 0,
            frames_read: u64::from(present),
            exhausted: !present,
            output_frame: 0,
            channel: 0,
        };
        for slot in LEFT + 1..FRAMES {
            result.read_slot(slot, slot - 1);
        }
        result
    }

    fn read_slot(&mut self, slot: usize, previous: usize) {
        let start = slot * self.channels;
        if !self.exhausted
            && read_frame(
                &mut self.input,
                &mut self.samples[start..start + self.channels],
            )
        {
            self.frames_read += 1;
        } else {
            self.exhausted = true;
            self.samples.copy_within(
                previous * self.channels..(previous + 1) * self.channels,
                start,
            );
        }
    }

    fn finished(&self) -> bool {
        self.exhausted
            && u128::from(self.output_frame) * u128::from(self.from)
                >= u128::from(self.frames_read) * u128::from(self.to)
    }

    fn remaining_samples(&self, unread: usize) -> u128 {
        let frames = u128::from(self.frames_read) + (unread / self.channels) as u128;
        let output_frames = (frames * u128::from(self.to)).div_ceil(u128::from(self.from));
        (output_frames * self.channels as u128).saturating_sub(
            u128::from(self.output_frame) * self.channels as u128 + self.channel as u128,
        )
    }
}

impl<I: Iterator<Item = Sample>> Iterator for Sinc<I> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if self.channel == 0 {
            if self.finished() {
                return None;
            }
            let position = u128::from(self.output_frame) * u128::from(self.from);
            let center = (position / u128::from(self.to)) as u64;
            while self.center < center {
                let previous = (self.head + FRAMES - 1) % FRAMES;
                self.read_slot(self.head, previous);
                self.head = (self.head + 1) % FRAMES;
                self.center += 1;
            }
            if self.finished() {
                return None;
            }
            let fraction = (position % u128::from(self.to)) as f64 / f64::from(self.to);
            // Alignment is the fractional-reader convention documented by Rubato.
            let time =
                LEFT as f64 + fraction - FILTER_TAPS as f64 / 2.0 + 1.0 - 1.0 / PHASES as f64;
            let mut nearest = [(0, 0); 2];
            get_nearest_times_2(time, PHASES as isize, &mut nearest);
            let fraction = (time * PHASES as f64).fract() as f32;
            let interpolator = interpolator();
            let mut wave = [0.0; FRAMES];
            for (channel, output) in self.output.iter_mut().enumerate() {
                for (offset, value) in wave.iter_mut().enumerate() {
                    let slot = (self.head + offset) % FRAMES;
                    *value = self.samples[slot * self.channels + channel];
                }
                let points = nearest.map(|(index, phase)| {
                    interpolator.get_sinc_interpolated(&wave, index as usize, phase as usize)
                });
                *output = interp_lin(fraction, &points);
            }
        }
        let sample = self.output[self.channel];
        self.channel += 1;
        if self.channel == self.channels {
            self.channel = 0;
            self.output_frame += 1;
        }
        Some(sample)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (lower, upper) = if self.exhausted {
            (0, Some(0))
        } else {
            self.input.size_hint()
        };
        let lower = self
            .remaining_samples(lower)
            .try_into()
            .unwrap_or(usize::MAX);
        let upper = upper.and_then(|upper| self.remaining_samples(upper).try_into().ok());
        (lower, upper)
    }
}
