//! Host components behind `wit/cyrus.wit`, exchanged with the engine once per frame.
//!
//! They live on one host entity and are copied to and from the engine's hook resources once per frame in `PreUpdate`,
//! so a mod's answer takes effect one frame after it was written.
//!
//! wasvy passes a component to its methods with the mutability of the mod's query
//! and panics on a mismatch, so `Camera`, `Movement`, `Memory`, `Uniforms` and `Hud` (queried `Mut`) take
//! `&mut self` throughout and `Input`, `Settings`, `Player` and `Scene` (queried `Ref`) take `&self`. Method names
//! are single words: wasvy looks them up by their WIT name, which uses dashes.
use crate::settings::{ModSettings, Value};
use bevy::ecs::system::SystemParam;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use cyrus_render::camera::{CameraTakeover, ViewCamera};
use cyrus_render::hooks::{SceneValues, ShaderInputs};
use cyrus_script::player::input::{
    CameraOrbit, CollisionProbes, Gait, Inputs, LocomotionClaim, LocomotionClaimed, ModernLook,
    MovementWish, PlayAnimation, PlayerInput, PlayerRequests, PlayerTuning, Probe, Wish,
};
use cyrus_script::script::{CameraFrameState, ScriptActors};
use cyrus_soup::{AnimationGroup, Native};
use std::collections::{BTreeSet, HashMap};
use wasvy::prelude::*;

/// Named values the presentation publishes and its clock ([`cyrus_render::hooks`]).
#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Scene {
    values: HashMap<String, Vec<f32>>,
    seconds: f32,
}

#[wasvy::methods]
impl Scene {
    pub fn values(&self, key: String) -> Vec<f32> {
        self.values.get(&key).cloned().unwrap_or_default()
    }
    pub fn seconds(&self) -> f32 {
        self.seconds
    }
}

#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Camera {
    pose: Vec<f32>,
    takeover: Option<Vec<f32>>,
    orbit: bool,
    turn: f32,
    manual: bool,
}

#[wasvy::methods]
impl Camera {
    pub fn pose(&mut self) -> Vec<f32> {
        self.pose.clone()
    }
    pub fn drive(&mut self, pose: Vec<f32>) {
        self.takeover = Some(pose);
    }
    pub fn orbit(&mut self) {
        self.orbit = true;
    }
    pub fn turn(&mut self, amount: f32) {
        if amount.is_finite() {
            self.turn += amount;
        }
    }
    pub fn manual(&mut self) -> bool {
        self.manual
    }
}

#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Movement {
    wish: Option<(f32, f32, bool, bool)>,
    play: Option<(u32, bool)>,
    face: Option<u32>,
    tempo: Option<f32>,
    claim: Option<(u32, u32, f32, bool, bool)>,
    tuning: Option<(f32, f32)>,
}

#[wasvy::methods]
impl Movement {
    pub fn steer(&mut self, x: f32, y: f32, jump: bool, wasd: bool) {
        self.wish = Some((x, y, jump, wasd));
    }
    pub fn play(&mut self, group: u32, interrupt: bool) {
        self.play = Some((group, interrupt));
    }
    pub fn face(&mut self, heading: u32) {
        self.face = Some(heading);
    }
    pub fn tempo(&mut self, scale: f32) {
        self.tempo = Some(scale);
    }
    pub fn claim(&mut self, heading: u32, travel: u32, speed: f32, run: bool, jump: bool) {
        self.claim = Some((heading, travel, speed, run, jump));
    }
    pub fn tune(&mut self, jump_height: f32, jump_speed: f32) {
        self.tuning = Some((jump_height, jump_speed));
    }
}

/// Script natives a mod watches and the calls made to them.
#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Natives {
    watched: Vec<String>,
    /// Calls to the natives watched last frame, one `function|actor|a,b,c` string per call.
    calls: Vec<String>,
}

#[wasvy::methods]
impl Natives {
    pub fn watch(&mut self, function: String) {
        self.watched.push(function);
    }
    pub fn count(&mut self) -> u32 {
        self.calls.len() as u32
    }
    pub fn function(&mut self, index: u32) -> String {
        self.field(index, 0)
    }
    pub fn actor(&mut self, index: u32) -> String {
        self.field(index, 1)
    }
    pub fn arguments(&mut self, index: u32) -> Vec<i32> {
        self.field(index, 2)
            .split(',')
            .filter_map(|value| value.parse().ok())
            .collect()
    }
}

impl Natives {
    fn field(&self, index: u32, part: usize) -> String {
        self.calls
            .get(index as usize)
            .and_then(|call| call.splitn(3, '|').nth(part))
            .unwrap_or_default()
            .to_owned()
    }
}

/// Collision probes: queued by the mod, answered by the engine while it claims locomotion.
#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Collision {
    /// Six values per queued probe: start then end.
    queued: Vec<f32>,
    /// One entry per probe of the last answered batch: six values, or none for a miss.
    hits: Vec<Vec<f32>>,
}

#[wasvy::methods]
impl Collision {
    pub fn cast(&mut self, sx: f32, sy: f32, sz: f32, ex: f32, ey: f32, ez: f32) {
        self.queued.extend([sx, sy, sz, ex, ey, ez]);
    }
    pub fn hit(&mut self, index: u32) -> Vec<f32> {
        self.hits.get(index as usize).cloned().unwrap_or_default()
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
    /// Current, pending and idle animation group; each empty when absent.
    animation: Vec<u32>,
    pending: Vec<u32>,
    idle: Vec<u32>,
}

#[wasvy::methods]
impl Player {
    pub fn animation(&self) -> Vec<u32> {
        self.animation.clone()
    }
    pub fn pending(&self) -> Vec<u32> {
        self.pending.clone()
    }
    pub fn idle(&self) -> Vec<u32> {
        self.idle.clone()
    }
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

/// State mods keep between frames, since each system call runs in a fresh mod instance.
#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Memory {
    values: HashMap<String, Vec<f32>>,
}

#[wasvy::methods]
impl Memory {
    pub fn load(&mut self, key: String) -> Vec<f32> {
        self.values.get(&key).cloned().unwrap_or_default()
    }
    pub fn save(&mut self, key: String, values: Vec<f32>) {
        if values.is_empty() {
            self.values.remove(&key);
        } else {
            self.values.insert(key, values);
        }
    }
}

/// Shader inputs mods wrote this frame; the engine takes them every frame (see [`ShaderInputs`]).
#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Uniforms {
    values: HashMap<String, Vec<f32>>,
}

#[wasvy::methods]
impl Uniforms {
    pub fn set(&mut self, key: String, values: Vec<f32>) {
        self.values.insert(key, values);
    }
}

/// Text lines mods wrote this frame; drawn over the game and then cleared.
#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Hud {
    lines: Vec<String>,
}

#[wasvy::methods]
impl Hud {
    pub fn show(&mut self, line: String) {
        self.lines.push(line);
    }
}

impl Hud {
    /// The lines written since the last call, oldest first.
    pub(crate) fn take(&mut self) -> Vec<String> {
        std::mem::take(&mut self.lines)
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
        Scene::default(),
        Camera::default(),
        Movement::default(),
        Settings::default(),
        Player::default(),
        Collision::default(),
        Natives::default(),
        Memory::default(),
        Uniforms::default(),
        Hud::default(),
        Input::default(),
    ));
}

/// Every component of the host entity.
type Host = (
    &'static mut Scene,
    &'static mut Camera,
    &'static mut Movement,
    &'static mut Settings,
    &'static mut Player,
    &'static mut Collision,
    &'static mut Natives,
    &'static mut Uniforms,
    &'static mut Input,
);
/// Engine-side resources the mod answers are handed to.
#[derive(SystemParam)]
pub(crate) struct Outputs<'w> {
    probes: ResMut<'w, CollisionProbes>,
    claimed: ResMut<'w, LocomotionClaimed>,
    movement: ResMut<'w, MovementWish>,
    takeover: ResMut<'w, CameraTakeover>,
    requests: ResMut<'w, PlayerRequests>,
    orbit: ResMut<'w, CameraOrbit>,
}

/// Engine state published to mods each frame.
#[derive(SystemParam)]
pub(crate) struct Sources<'w, 's> {
    actors: Option<ResMut<'w, ScriptActors>>,
    camera_frame: Option<Res<'w, CameraFrameState>>,
    values: Res<'w, ModSettings>,
    published: Res<'w, SceneValues>,
    look: Option<Res<'w, ModernLook>>,
    console: Option<Res<'w, cyrus_console::Console>>,
    logbook: Option<Res<'w, cyrus_script::logbook::LogbookScreen>>,
    view: Query<'w, 's, &'static Transform, With<ViewCamera>>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    bindings: Inputs<'w>,
    mouse: Res<'w, AccumulatedMouseMotion>,
    time: Res<'w, Time>,
}

/// Hands last frame's mod answers to the engine and publishes this frame's state.
pub(crate) fn exchange(
    mut host: Query<Host>,
    mut inputs: ResMut<ShaderInputs>,
    mut out: Outputs,
    mut src: Sources,
) {
    let Ok((
        mut scene,
        mut camera,
        mut steering,
        mut settings,
        mut player,
        mut collision,
        mut natives,
        mut uniforms,
        mut input,
    )) = host.single_mut()
    else {
        return;
    };
    if src.values.is_changed() {
        publish_settings(&mut settings, &src.values);
    }
    if src.published.is_changed() {
        scene.values = src.published.values.clone();
    }
    scene.seconds = src.published.seconds;
    // Shader inputs last only for the frame after they were written.
    inputs.0 = take_shader_inputs(&mut uniforms);

    out.takeover.0 = camera.takeover.take().and_then(|pose| match pose[..] {
        [x, y, z, qx, qy, qz, qw] => Some(
            Transform::from_xyz(x, y, z).with_rotation(Quat::from_xyzw(qx, qy, qz, qw).normalize()),
        ),
        _ => {
            warn_once!("mod camera pose needs 7 values, got {}", pose.len());
            None
        }
    });
    out.movement.0 = steering.wish.take().map(|(x, y, jump, wasd)| {
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
    out.orbit.enabled = std::mem::take(&mut camera.orbit);
    out.orbit.turn = std::mem::take(&mut camera.turn);
    camera.manual =
        src.mouse.delta != Vec2::ZERO || src.look.as_deref().is_some_and(|look| look.manual);
    *out.requests = take_player_requests(&mut steering);
    out.claimed.0 = take_claim(&mut steering);
    exchange_collision(&mut out.probes, &mut collision);
    natives.calls = exchange_natives(&mut natives, src.actors.as_deref_mut());
    publish_player(&mut player, src.actors.as_deref());
    camera.pose = src.view.single().map_or_else(
        |_| Vec::new(),
        |t| {
            t.translation
                .to_array()
                .into_iter()
                .chain(t.rotation.to_array())
                .collect()
        },
    );
    // The same heading the game derives for camera-relative movement: the view yaw, mirrored.
    player.view = src
        .camera_frame
        .as_deref()
        .and_then(|frame| frame.published.as_ref())
        .map_or(player.heading, |camera| {
            u32::from(camera.view_angles[1].wrapping_neg() & 2047)
        });

    // Keys typed into the console or read in the logbook are not the mods'.
    let typing = src.console.as_deref().is_some_and(|console| console.open)
        || src
            .logbook
            .as_deref()
            .is_some_and(|screen| screen.suspended());
    if typing {
        input.held.clear();
        input.pressed.clear();
        input.just_pressed.clear();
        input.mouse_delta = vec![0.0, 0.0];
    } else {
        input.held = held_actions(
            &src.bindings
                .bindings()
                .sample_devices(src.bindings.devices()),
        );
        input.pressed = src
            .keys
            .get_pressed()
            .map(|key| format!("{key:?}"))
            .collect();
        input.just_pressed = src
            .keys
            .get_just_pressed()
            .map(|key| format!("{key:?}"))
            .collect();
        input.mouse_delta = src.mouse.delta.to_array().to_vec();
    }
    input.delta_seconds = src.time.delta_secs();
}

/// Rebuilds the settings a mod reads from the configured values, keyed `mod/key`.
fn publish_settings(settings: &mut Settings, values: &ModSettings) {
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

/// The four-value shader inputs mods wrote; any other length is dropped with a warning.
fn take_shader_inputs(uniforms: &mut Uniforms) -> HashMap<String, [f32; 4]> {
    std::mem::take(&mut uniforms.values)
        .into_iter()
        .filter_map(
            |(key, values)| match <[f32; 4]>::try_from(values.as_slice()) {
                Ok(values) => Some((key, values)),
                Err(_) => {
                    warn_once!("mods: uniforms: {key} needs 4 values, got {}", values.len());
                    None
                }
            },
        )
        .collect()
}

/// The animation, heading and tempo requests a mod queued this frame.
fn take_player_requests(steering: &mut Movement) -> PlayerRequests {
    PlayerRequests {
        animation: steering.play.take().and_then(|(group, interrupt)| {
            let group = u16::try_from(group).ok();
            if group.is_none() {
                warn_once!("mods: play: animation group out of range");
            }
            group.map(|group| PlayAnimation {
                group: AnimationGroup::from_index(group),
                interrupt,
            })
        }),
        heading: steering.face.take().map(|heading| (heading & 2047) as u16),
        tempo: steering.tempo.take(),
    }
}

/// The locomotion claim a mod made this frame, with its jump tuning.
fn take_claim(steering: &mut Movement) -> Option<LocomotionClaim> {
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
    steering
        .claim
        .take()
        .map(|(heading, travel, speed, run, jump)| LocomotionClaim {
            heading: (heading & 2047) as u16,
            travel: (travel & 2047) as u16,
            speed,
            gait: if run { Gait::Run } else { Gait::Walk },
            jump,
            tuning,
        })
}

/// Queues the segment probes mods asked for and hands back last frame's answers.
fn exchange_collision(probes: &mut CollisionProbes, collision: &mut Collision) {
    probes.requests = std::mem::take(&mut collision.queued)
        .as_chunks::<6>()
        .0
        .iter()
        .map(|&[sx, sy, sz, ex, ey, ez]| Probe {
            start: [sx, sy, sz],
            end: [ex, ey, ez],
        })
        .collect();
    if let Some(answers) = probes.take_answers() {
        collision.hits = answers
            .into_iter()
            .map(|hit| hit.map_or_else(Vec::new, |hit| [hit.point, hit.normal].concat()))
            .collect();
    }
}

/// Watches the natives mods named this frame and returns the calls made to last frame's.
fn exchange_natives(natives: &mut Natives, actors: Option<&mut ScriptActors>) -> Vec<String> {
    let watched = std::mem::take(&mut natives.watched);
    let known: BTreeSet<Native> = watched
        .iter()
        .filter_map(|name| {
            let native = Native::from_name(name);
            if native.is_none() {
                warn_once!("mods: watch: no script native named {name}");
            }
            native
        })
        .collect();
    actors
        .and_then(|actors| actors.current.as_mut())
        .map(|world| {
            let calls = world.take_native_calls();
            world.watch_natives(known);
            calls
        })
        .unwrap_or_default()
        .into_iter()
        .map(|call| {
            let integers: Vec<String> = call.integers.iter().map(i32::to_string).collect();
            format!("{}|{}|{}", call.function, call.actor, integers.join(","))
        })
        .collect()
}

/// Publishes the player's locomotion state, or the empty default outside a script world.
fn publish_player(player: &mut Player, actors: Option<&ScriptActors>) {
    let Some(status) = actors.and_then(|actors| actors.current.as_ref()?.locomotion_status())
    else {
        *player = Player::default();
        return;
    };
    player.position = status.position.map(|q8| q8 as f32 / 256.0).to_vec();
    player.heading = u32::from(status.heading);
    player.airborne = status.airborne;
    player.armed = status.armed;
    let group = |group: Option<cyrus_soup::AnimationGroup>| {
        group
            .map(|group| u32::from(group.index()))
            .into_iter()
            .collect()
    };
    player.animation = group(status.animation);
    player.pending = group(status.pending_animation);
    player.idle = group(status.idle_animation);
    (player.walk_speed, player.run_speed) = if status.armed {
        (
            f32::from(status.fight_walk_speed),
            f32::from(status.fight_run_speed),
        )
    } else {
        (f32::from(status.walk_speed), f32::from(status.run_speed))
    };
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
    fn native_calls_read_back_by_field_and_out_of_range_is_empty() {
        let mut natives = Natives {
            calls: vec![
                "Jump|LOGIC|".to_owned(),
                "SetAttribute|CYRUS|3,-7".to_owned(),
            ],
            ..default()
        };
        assert_eq!(natives.count(), 2);
        assert_eq!(natives.function(1), "SetAttribute");
        assert_eq!(natives.actor(1), "CYRUS");
        assert_eq!(natives.arguments(1), [3, -7]);
        assert!(natives.arguments(0).is_empty());
        assert_eq!(natives.function(9), "");
    }

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
