//! Host components behind `wit/cyrus.wit`, exchanged with the engine once per frame.
//!
//! They live on one host entity and are copied to and from the engine's hook resources once per frame in `PreUpdate`,
//! so a mod's answer takes effect one frame after it was written.
//!
//! wasvy passes a component to its methods with the mutability of the mod's query
//! and panics on a mismatch, so `Water` and `Camera` (queried `Mut`) take
//! `&mut self` throughout and `Input` (queried `Ref`) takes `&self`. Method names
//! are single words: wasvy looks them up by their WIT name, which uses dashes.
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use cyrus_render::WaterWaves;
use cyrus_render::camera::{CameraTakeover, ViewCamera};
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
pub struct Input {
    pressed: Vec<String>,
    just_pressed: Vec<String>,
    mouse_delta: Vec<f32>,
    delta_seconds: f32,
}

#[wasvy::methods]
impl Input {
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
        Input::default(),
    ));
}

/// Hands last frame's mod answers to the engine and publishes this frame's state.
#[allow(clippy::too_many_arguments)]
pub(crate) fn exchange(
    mut host: Query<(&mut Water, &mut Camera, &mut Input)>,
    mut waves: ResMut<WaterWaves>,
    mut takeover: ResMut<CameraTakeover>,
    view: Query<&Transform, With<ViewCamera>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
) {
    let Ok((mut water, mut camera, mut input)) = host.single_mut() else {
        return;
    };
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

    input.pressed = keys.get_pressed().map(|key| format!("{key:?}")).collect();
    input.just_pressed = keys
        .get_just_pressed()
        .map(|key| format!("{key:?}"))
        .collect();
    input.mouse_delta = mouse.delta.to_array().to_vec();
    input.delta_seconds = time.delta_secs();
}
