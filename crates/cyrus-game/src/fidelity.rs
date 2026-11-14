//! The fidelity switch. Every behaviour that may deviate from the original game
//! is registered under an [`Area`] and runs only in the configured [`Mode`].
//! The default of every area is `Faithful`. Areas change only how the game looks and sounds;
//! behaviour such as controls, movement and camera handling changes only through native mods.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Faithful,
    Modern,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    Fog,
    Sky,
    Lighting,
    Textures,
    Music,
    AnisotropicFiltering,
}

impl Area {
    pub const ALL: [Area; 6] = [
        Area::Fog,
        Area::Sky,
        Area::Lighting,
        Area::Textures,
        Area::Music,
        Area::AnisotropicFiltering,
    ];
}

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Fidelity {
    pub fog: Mode,
    pub sky: Mode,
    /// Modern lights the world with physically based shading, shadows and screen-space effects.
    pub lighting: Mode,
    /// Modern point-samples world textures instead of the original's bilinear filtering.
    pub textures: Mode,
    /// Modern mutes CD-ROM data-sector bytes that a disc image places inside an audio track.
    pub music: Mode,
    /// Modern averages up to sixteen samples along world texture footprints.
    pub anisotropic_filtering: Mode,
}

impl Fidelity {
    pub fn mode(&self, area: Area) -> Mode {
        match area {
            Area::Fog => self.fog,
            Area::Sky => self.sky,
            Area::Lighting => self.lighting,
            Area::Textures => self.textures,
            Area::Music => self.music,
            Area::AnisotropicFiltering => self.anisotropic_filtering,
        }
    }
    pub fn set(&mut self, area: Area, mode: Mode) {
        match area {
            Area::Fog => self.fog = mode,
            Area::Sky => self.sky = mode,
            Area::Lighting => self.lighting = mode,
            Area::Textures => self.textures = mode,
            Area::Music => self.music = mode,
            Area::AnisotropicFiltering => self.anisotropic_filtering = mode,
        }
    }
}

/// Run condition: `.run_if(in_mode(Area::Fog, Mode::Modern))`.
pub fn in_mode(area: Area, mode: Mode) -> impl Fn(Res<Fidelity>) -> bool + Clone {
    move |fidelity: Res<Fidelity>| fidelity.mode(area) == mode
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_area_defaults_to_faithful() {
        let fidelity = Fidelity::default();
        for area in Area::ALL {
            assert_eq!(fidelity.mode(area), Mode::Faithful, "{area:?}");
        }
    }
}
