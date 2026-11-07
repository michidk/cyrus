//! Host components behind `wit/cyrus.wit`, exchanged with the engine once per frame.
//!
//! They live on one host entity and are copied to and from the engine's hook resources once per frame in `PreUpdate`,
//! so a mod's answer takes effect one frame after it was written.
//!
//! wasvy passes a component to its methods with the mutability of the mod's query
//! and panics on a mismatch, so `Water`, `Camera` and `Movement` (queried `Mut`) take
//! `&mut self` throughout and `Input` and `Settings` (queried `Ref`) take `&self`. Method names
//! are single words: wasvy looks them up by their WIT name, which uses dashes.
use crate::settings::{ModSettings, Value};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use cyrus_game::player::input::{Inputs, MovementWish, PlayerInput, Wish};
use cyrus_render::WaterWaves;
use cyrus_render::camera::{CameraTakeover, ViewCamera};
use std::collections::HashMap;
use wasvy::prelude::*;

#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Water {
    generation: u32,
    positions: Vec<f32>,
    parameters: Vec<f32>,
    seconds: f32,
    heights: Option<(u32, Vec<f32>)>,
}

#[wasvy::methods]
impl Water {
    pub fn generation(&mut self) -> u32 {
        self.generation
    }
    pub fn positions(&mut self) -> Vec<f32> {
        self.positions.clone()
    }
    pub fn parameters(&mut self) -> Vec<f32> {
        self.parameters.clone()
    }
    pub fn seconds(&mut self) -> f32 {
        self.seconds
    }
    pub fn answer(&mut self, generation: u32, heights: Vec<f32>) {
        self.heights = Some((generation, heights));
    }
}

#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Camera {
    pose: Vec<f32>,
    takeover: Option<Vec<f32>>,
}

#[wasvy::methods]
impl Camera {
    pub fn pose(&mut self) -> Vec<f32> {
        self.pose.clone()
    }
    pub fn drive(&mut self, pose: Vec<f32>) {
        self.takeover = Some(pose);
    }
}

#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Movement {
    wish: Option<(f32, f32, bool, bool)>,
}

#[wasvy::methods]
impl Movement {
    pub fn steer(&mut self, x: f32, y: f32, jump: bool, wasd: bool) {
        self.wish = Some((x, y, jump, wasd));
    }
}

/// Setting values of every mod, keyed `mod id/setting key`.
#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Settings {
    flags: HashMap<String, bool>,
    numbers: HashMap<String, f32>,
    choices: HashMap<String, String>,
}

#[wasvy::methods]
impl Settings {
    pub fn flag(&self, id: String, key: String) -> bool {
        self.flags
            .get(&format!("{id}/{key}"))
            .copied()
            .unwrap_or_default()
    }
    pub fn number(&self, id: String, key: String) -> f32 {
        self.numbers
            .get(&format!("{id}/{key}"))
            .copied()
            .unwrap_or_default()
    }
    pub fn choice(&self, id: String, key: String) -> String {
        self.choices
            .get(&format!("{id}/{key}"))
            .cloned()
            .unwrap_or_default()
    }
}

#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Input {
    held: Vec<String>,
    pressed: Vec<String>,
    just_pressed: Vec<String>,
    mouse_delta: Vec<f32>,
    delta_seconds: f32,
}

#[wasvy::methods]
impl Input {
    pub fn held(&self, action: String) -> bool {
        self.held.contains(&action)
    }
    pub fn pressed(&self, key: String) -> bool {
        self.pressed.contains(&key)
    }
    pub fn tapped(&self, key: String) -> bool {
        self.just_pressed.contains(&key)
    }
    pub fn mouse(&self) -> Vec<f32> {
        self.mouse_delta.clone()
    }
    pub fn delta(&self) -> f32 {
        self.delta_seconds
    }
}

pub(crate) fn spawn_host(mut commands: Commands) {
    commands.spawn((
        Name::new("cyrus mod API"),
        Water::default(),
        Camera::default(),
        Movement::default(),
        Settings::default(),
        Input::default(),
    ));
}

/// Hands last frame's mod answers to the engine and publishes this frame's state.
#[allow(clippy::too_many_arguments)]
pub(crate) fn exchange(
    mut host: Query<(
        &mut Water,
        &mut Camera,
        &mut Movement,
        &mut Settings,
        &mut Input,
    )>,
    values: Res<ModSettings>,
    mut waves: ResMut<WaterWaves>,
    mut movement: ResMut<MovementWish>,
    mut takeover: ResMut<CameraTakeover>,
    view: Query<&Transform, With<ViewCamera>>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Inputs,
    mouse: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
) {
    let Ok((mut water, mut camera, mut steering, mut settings, mut input)) = host.single_mut()
    else {
        return;
    };
    if values.is_changed() {
        *settings = Settings::default();
        for (id, entries) in &values.values {
            for (key, value) in entries {
                let name = format!("{id}/{key}");
                match value {
                    Value::Bool(value) => {
                        settings.flags.insert(name, *value);
                    }
                    Value::Number(value) => {
                        settings.numbers.insert(name, *value);
                    }
                    Value::Choice(value) => {
                        settings.choices.insert(name, value.clone());
                    }
                }
            }
        }
    }
    waves.heights = water.heights.take();
    if water.generation != waves.generation {
        water.generation = waves.generation;
        water.positions = waves.positions.iter().flatten().copied().collect();
    }
    water.parameters = waves.parameters.to_vec();
    water.seconds = waves.seconds;

    takeover.0 = camera.takeover.take().and_then(|pose| match pose[..] {
        [x, y, z, qx, qy, qz, qw] => Some(
            Transform::from_xyz(x, y, z).with_rotation(Quat::from_xyzw(qx, qy, qz, qw).normalize()),
        ),
        _ => {
            warn_once!("mod camera pose needs 7 values, got {}", pose.len());
            None
        }
    });
    movement.0 = steering.wish.take().map(|(x, y, jump, wasd)| {
        let direction = Vec2::new(x, y);
        Wish {
            // A mod's NaN or infinity must not become a heading.
            direction: if direction.is_finite() {
                direction.clamp_length_max(1.0)
            } else {
                Vec2::ZERO
            },
            jump,
            wasd,
        }
    });
    camera.pose = view.single().map_or_else(
        |_| Vec::new(),
        |t| {
            t.translation
                .to_array()
                .into_iter()
                .chain(t.rotation.to_array())
                .collect()
        },
    );

    input.held = held_actions(&bindings.bindings().sample_devices(bindings.devices()));
    input.pressed = keys.get_pressed().map(|key| format!("{key:?}")).collect();
    input.just_pressed = keys
        .get_just_pressed()
        .map(|key| format!("{key:?}"))
        .collect();
    input.mouse_delta = mouse.delta.to_array().to_vec();
    input.delta_seconds = time.delta_secs();
}

/// The movement actions held under the installed key bindings, by the names `input.held` takes.
fn held_actions(input: &PlayerInput) -> Vec<String> {
    [
        ("forward", input.forward),
        ("back", input.backward),
        ("left", input.left),
        ("right", input.right),
        ("walk", input.walk),
        ("jump", input.jump),
    ]
    .into_iter()
    .filter(|(_, held)| *held)
    .map(|(name, _)| name.to_owned())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_actions_name_the_bound_actions_that_are_down() {
        let input = PlayerInput {
            forward: true,
            walk: true,
            ..default()
        };
        assert_eq!(held_actions(&input), ["forward", "walk"]);
        assert!(held_actions(&PlayerInput::default()).is_empty());
    }
}
