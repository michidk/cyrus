//! The fidelity switch. Every behaviour that may deviate from the original game
//! is registered under an [`Area`] and runs only in the configured [`Mode`].
//! The default of every area is `Faithful`.

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
    PlayerController,
    Camera,
    Water,
    Fog,
    Sky,
    Lighting,
}

impl Area {
    pub const ALL: [Area; 6] = [
        Area::PlayerController,
        Area::Camera,
        Area::Water,
        Area::Fog,
        Area::Sky,
        Area::Lighting,
    ];
}

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Fidelity {
    pub player_controller: Mode,
    pub camera: Mode,
    pub water: Mode,
    pub fog: Mode,
    pub sky: Mode,
    /// Modern lighting currently reuses the faithful terrain implementation.
    pub lighting: Mode,
}

impl Fidelity {
    pub fn mode(&self, area: Area) -> Mode {
        match area {
            Area::PlayerController => self.player_controller,
            Area::Camera => self.camera,
            Area::Water => self.water,
            Area::Fog => self.fog,
            Area::Sky => self.sky,
            Area::Lighting => self.lighting,
        }
    }
    pub fn set(&mut self, area: Area, mode: Mode) {
        match area {
            Area::PlayerController => self.player_controller = mode,
            Area::Camera => self.camera = mode,
            Area::Water => self.water = mode,
            Area::Fog => self.fog = mode,
            Area::Sky => self.sky = mode,
            Area::Lighting => self.lighting = mode,
        }
    }
}

/// Run condition: `.run_if(in_mode(Area::Camera, Mode::Modern))`.
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
