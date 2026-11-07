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
use cyrus_render::WaterWaves;
use cyrus_render::camera::{CameraTakeover, ViewCamera};
use cyrus_script::player::input::{
    Gait, Inputs, LocomotionClaim, LocomotionClaimed, MovementWish, PlayerInput, PlayerTuning, Wish,
};
use cyrus_script::script::{CameraFrameState, ScriptActors};
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
    claim: Option<(u32, f32, bool, bool)>,
    tuning: Option<(f32, f32)>,
}

#[wasvy::methods]
impl Movement {
    pub fn steer(&mut self, x: f32, y: f32, jump: bool, wasd: bool) {
        self.wish = Some((x, y, jump, wasd));
    }
    pub fn claim(&mut self, heading: u32, speed: f32, run: bool, jump: bool) {
        self.claim = Some((heading, speed, run, jump));
    }
    pub fn tune(&mut self, jump_height: f32, jump_speed: f32) {
        self.tuning = Some((jump_height, jump_speed));
    }
}

/// The selected player as the last update left it.
#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Player {
    position: Vec<f32>,
    heading: u32,
    view: u32,
    airborne: bool,
    walk_speed: f32,
    run_speed: f32,
    armed: bool,
}

#[wasvy::methods]
impl Player {
    pub fn armed(&self) -> bool {
        self.armed
    }
    pub fn position(&self) -> Vec<f32> {
        self.position.clone()
    }
    pub fn heading(&self) -> u32 {
        self.heading
    }
    pub fn view(&self) -> u32 {
        self.view
    }
    pub fn airborne(&self) -> bool {
        self.airborne
    }
    pub fn speed(&self, run: bool) -> f32 {
        if run { self.run_speed } else { self.walk_speed }
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
        Player::default(),
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
        &mut Player,
        &mut Input,
    )>,
    actors: Option<Res<ScriptActors>>,
    camera_frame: Option<Res<CameraFrameState>>,
    mut claimed: ResMut<LocomotionClaimed>,
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
    let Ok((mut water, mut camera, mut steering, mut settings, mut player, mut input)) =
        host.single_mut()
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
    let tuning = steering
        .tuning
        .take()
        .map_or_else(Default::default, |(height, speed)| PlayerTuning {
            // A mod's NaN or infinity must not become a jump.
            jump_height: if height.is_finite() {
                height.max(0.0)
            } else {
                1.0
            },
            jump_speed: if speed.is_finite() {
                speed.max(0.0)
            } else {
                1.0
            },
        });
    claimed.0 = steering
        .claim
        .take()
        .map(|(heading, speed, run, jump)| LocomotionClaim {
            heading: (heading & 2047) as u16,
            speed,
            gait: if run { Gait::Run } else { Gait::Walk },
            jump,
            tuning,
        });
    if let Some(status) = actors
        .as_deref()
        .and_then(|actors| actors.current.as_ref()?.locomotion_status())
    {
        player.position = status.position.map(|q8| q8 as f32 / 256.0).to_vec();
        player.heading = u32::from(status.heading);
        player.airborne = status.airborne;
        player.armed = status.armed;
        (player.walk_speed, player.run_speed) = if status.armed {
            (
                f32::from(status.fight_walk_speed),
                f32::from(status.fight_run_speed),
            )
        } else {
            (f32::from(status.walk_speed), f32::from(status.run_speed))
        };
    } else {
        *player = Player::default();
    }
    // The same heading the game derives for camera-relative movement: the view yaw, mirrored.
    player.view = camera_frame
        .as_deref()
        .and_then(|frame| frame.published.as_ref())
        .map_or(player.heading, |camera| {
            u32::from(camera.view_angles[1].wrapping_neg() & 2047)
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
