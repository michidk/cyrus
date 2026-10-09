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
    Graphics,
}

impl Area {
    pub const ALL: [Area; 3] = [Area::PlayerController, Area::Camera, Area::Graphics];
}

#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Fidelity {
    pub player_controller: Mode,
    pub camera: Mode,
    pub graphics: Mode,
}

impl Fidelity {
    pub fn mode(&self, area: Area) -> Mode {
        match area {
            Area::PlayerController => self.player_controller,
            Area::Camera => self.camera,
            Area::Graphics => self.graphics,
        }
    }
    pub fn set(&mut self, area: Area, mode: Mode) {
        match area {
            Area::PlayerController => self.player_controller = mode,
            Area::Camera => self.camera = mode,
            Area::Graphics => self.graphics = mode,
        }
    }
}

/// Run condition: `.run_if(in_mode(Area::Camera, Mode::Modern))`.
pub fn in_mode(area: Area, mode: Mode) -> impl Fn(Res<Fidelity>) -> bool + Clone {
    move |fidelity: Res<Fidelity>| fidelity.mode(area) == mode
}
