//! Applies the game's independent audio fidelity policy to subsequent backend source spans.
use bevy::prelude::*;
use cyrus_game::fidelity::{Area, Fidelity, Mode};
use rodio::conversions::{ResamplingMethod, set_default_resampling_method};

fn method(fidelity: Option<&Fidelity>) -> ResamplingMethod {
    if fidelity.is_some_and(|fidelity| fidelity.mode(Area::AudioResampling) == Mode::Modern) {
        ResamplingMethod::Sinc24
    } else {
        ResamplingMethod::Linear
    }
}

pub(crate) fn apply(fidelity: Option<Res<Fidelity>>) {
    if fidelity
        .as_ref()
        .is_none_or(|fidelity| fidelity.is_changed())
    {
        set_default_resampling_method(method(fidelity.as_deref()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_filter_is_independent_of_music_and_defaults_to_linear() {
        assert_eq!(method(None), ResamplingMethod::Linear);
        let mut fidelity = Fidelity::default();
        fidelity.set(Area::Music, Mode::Modern);
        assert_eq!(method(Some(&fidelity)), ResamplingMethod::Linear);
        fidelity.set(Area::AudioResampling, Mode::Modern);
        assert_eq!(method(Some(&fidelity)), ResamplingMethod::Sinc24);
        fidelity.set(Area::Music, Mode::Faithful);
        assert_eq!(method(Some(&fidelity)), ResamplingMethod::Sinc24);
        fidelity.set(Area::AudioResampling, Mode::Faithful);
        assert_eq!(method(Some(&fidelity)), ResamplingMethod::Linear);
    }
}
