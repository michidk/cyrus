/*!
This module contains functions that convert from one PCM format to another.

This includes conversion between sample formats, channels or sample rates.
*/

pub use self::channels::ChannelCountConverter;
pub use self::resampling::{set_default_resampling_method, ResamplingMethod, SampleRateConverter};
pub use self::sample::SampleTypeConverter;

mod channels;
pub(crate) mod resampling;
mod sample;
mod sample_rate;
mod sinc;
