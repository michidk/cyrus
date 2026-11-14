//! Typed wrappers over the host's hook resources (`crates/cyrus-mods/wit/cyrus.wit` documents
//! each hook; the wrappers keep its units and timing). Every wrapper takes `&self`: the raw
//! resources already mutate through the host.
use crate::bindings::cyrus::mods::components as raw;
use crate::keys::{Action, Key};
use crate::math::{Pose, Vec3};
use crate::state::Persist;

/// The view camera.
pub struct Camera(pub(crate) raw::Camera);

impl Camera {
    /// The current view pose.
    pub fn pose(&self) -> Option<Pose> {
        Pose::from_list(&self.0.pose())
    }

    /// Drive the camera this frame, which suppresses player input. Stop calling to hand it back.
    pub fn drive(&self, pose: Pose) {
        self.0.drive(&pose.to_list());
    }

    /// The mouse and right stick turn the follow camera around the player this frame.
    pub fn orbit(&self) {
        self.0.orbit();
    }

    /// Turn the orbit camera by `amount` heading units (2048 per turn, positive left).
    pub fn turn(&self, amount: f32) {
        self.0.turn(amount);
    }

    /// Manual mouse or right-stick camera input was recently sampled.
    pub fn manual(&self) -> bool {
        self.0.manual()
    }
}

/// How the player's ground locomotion is claimed for a frame (`Movement::claim`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Claim {
    /// Where the player faces, in engine units (2048 to a turn).
    pub heading: u32,
    /// The direction it moves along, in the same units.
    pub travel: u32,
    /// Map units per nominal update (see `Player::speed`); zero stands.
    pub speed: f32,
    pub run: bool,
    /// Start the original jump.
    pub jump: bool,
}

/// The player's movement.
pub struct Movement(pub(crate) raw::Movement);

impl Movement {
    /// Steer toward `x` (right) and `y` (forward) relative to the camera; set `wasd` when the mod
    /// reads W, A, S and D.
    pub fn steer(&self, x: f32, y: f32, jump: bool, wasd: bool) {
        self.0.steer(x, y, jump, wasd);
    }

    /// Play animation `group` on the player; with `interrupt` the current group gives way now.
    pub fn play(&self, group: u32, interrupt: bool) {
        self.0.play(group, interrupt);
    }

    /// Face `heading` (2048 to a turn) this frame.
    pub fn face(&self, heading: u32) {
        self.0.face(heading);
    }

    /// Play the player's animation `scale` times as fast this frame (at most 8).
    pub fn tempo(&self, scale: f32) {
        self.0.tempo(scale);
    }

    /// Take over ground locomotion for this frame; call it every frame.
    pub fn claim(&self, claim: Claim) {
        self.0.claim(
            claim.heading,
            claim.travel,
            claim.speed,
            claim.run,
            claim.jump,
        );
    }

    /// Scale a claimed player's jump height and forward jump speed (1 and 1 are the original).
    pub fn tune(&self, jump_height: f32, jump_speed: f32) {
        self.0.tune(jump_height, jump_speed);
    }
}

/// The selected player's state; readings are `None` or default before a world has one.
pub struct Player(pub(crate) raw::Player);

impl Player {
    /// Map units, original axes; `None` while there is no player (menus, loading).
    pub fn position(&self) -> Option<Vec3> {
        let &[x, y, z] = &self.0.position()[..] else {
            return None;
        };
        Some(Vec3::new(x, y, z))
    }

    /// Facing, in engine units (2048 to a turn).
    pub fn heading(&self) -> u32 {
        self.0.heading()
    }

    /// The direction the camera looks, in heading units.
    pub fn view(&self) -> u32 {
        self.0.view()
    }

    pub fn airborne(&self) -> bool {
        self.0.airborne()
    }

    /// A weapon is drawn or the combat stance is on.
    pub fn armed(&self) -> bool {
        self.0.armed()
    }

    /// Walking or running speed in map units per nominal update.
    pub fn speed(&self, run: bool) -> f32 {
        self.0.speed(run)
    }

    /// The playing animation group.
    pub fn animation(&self) -> Option<u32> {
        self.0.animation().first().copied()
    }

    /// The group waiting to follow the playing one.
    pub fn pending(&self) -> Option<u32> {
        self.0.pending().first().copied()
    }

    /// The group the player returns to when nothing is requested.
    pub fn idle(&self) -> Option<u32> {
        self.0.idle().first().copied()
    }
}

/// Where a probe segment first met geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    pub point: Vec3,
    /// Unit length.
    pub normal: Vec3,
}

/// Collision probes for a mod that claims the player's movement.
pub struct Collision(pub(crate) raw::Collision);

impl Collision {
    /// Queue a segment (map units, original axes). Queue the same probes every frame.
    pub fn cast(&self, from: Vec3, to: Vec3) {
        self.0.cast(from.x, from.y, from.z, to.x, to.y, to.z);
    }

    /// Probe number `index` of the last answered batch (answers arrive two frames after a cast);
    /// `None` when the segment hit nothing or nothing was answered.
    pub fn hit(&self, index: u32) -> Option<Contact> {
        let &[px, py, pz, nx, ny, nz] = &self.0.hit(index)[..] else {
            return None;
        };
        Some(Contact {
            point: Vec3::new(px, py, pz),
            normal: Vec3::new(nx, ny, nz),
        })
    }
}

/// One script native call made while it was watched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeCall {
    pub function: String,
    /// The script name of the actor that called it.
    pub actor: String,
    /// The integer arguments (actor and string arguments are left out).
    pub arguments: Vec<i32>,
}

/// Script natives a mod can watch.
pub struct Natives(pub(crate) raw::Natives);

impl Natives {
    /// Watch a native by its installed `SOUP386.DEF` name; call every frame you want its calls.
    pub fn watch(&self, function: &str) {
        self.0.watch(function);
    }

    /// The calls made last frame to what was watched, oldest first.
    pub fn calls(&self) -> Vec<NativeCall> {
        (0..self.0.count())
            .map(|index| NativeCall {
                function: self.0.function(index),
                actor: self.0.actor(index),
                arguments: self.0.arguments(index),
            })
            .collect()
    }
}

/// Whether the Lighting area draws the original way or the modern one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lighting {
    Faithful,
    Modern,
}

/// How the loaded world's water moves, as its installed environment describes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Water {
    /// Wave amplitude, speed and frequency in the world file's units.
    pub amplitude: f32,
    pub speed: f32,
    pub frequency: f32,
    /// The terrain grid cell size in scene units.
    pub cell: f32,
}

/// Facts about the loaded world and the presentation, refreshed every frame.
pub struct Environment(pub(crate) raw::Environment);

impl Environment {
    /// Presentation clock in seconds.
    pub fn seconds(&self) -> f32 {
        self.0.seconds()
    }

    /// Changes whenever a world is loaded or reloaded; 0 while none is. Prefer `Cx::world_changed`.
    pub fn generation(&self) -> u32 {
        self.0.generation()
    }

    /// The loaded world's WORLD.INI index; `None` while no world is loaded.
    pub fn world(&self) -> Option<u32> {
        self.0.index().first().copied()
    }

    pub fn lighting(&self) -> Lighting {
        if self.0.lighting() == "modern" {
            Lighting::Modern
        } else {
            Lighting::Faithful
        }
    }

    /// The loaded world's water; `None` when it has none.
    pub fn water(&self) -> Option<Water> {
        let &[amplitude, speed, frequency, cell] = &self.0.water()[..] else {
            return None;
        };
        Some(Water {
            amplitude,
            speed,
            frequency,
            cell,
        })
    }
}

/// One actor of the loaded world.
#[derive(Clone, Debug, PartialEq)]
pub struct Actor {
    /// Valid for the current world generation only.
    pub index: u32,
    /// The actor's script name.
    pub name: String,
    /// Map units, original axes; `None` before the actor has a position.
    pub position: Option<Vec3>,
    pub health: u8,
    /// A script hid the actor, so it is not drawn.
    pub hidden: bool,
}

/// The actors of the loaded world, read-only and refreshed every frame.
pub struct Actors(pub(crate) raw::Actors);

impl Actors {
    pub fn count(&self) -> u32 {
        self.0.count()
    }

    pub fn get(&self, index: u32) -> Option<Actor> {
        (index < self.0.count()).then(|| Actor {
            index,
            name: self.0.name(index),
            position: match self.0.position(index)[..] {
                [x, y, z] => Some(Vec3::new(x, y, z)),
                _ => None,
            },
            health: self.0.health(index).min(255) as u8,
            hidden: self.0.hidden(index),
        })
    }

    /// Every actor, in actor order.
    pub fn all(&self) -> impl Iterator<Item = Actor> + '_ {
        (0..self.0.count()).filter_map(|index| self.get(index))
    }

    /// The nearest actor to `from` within `range` map units that `keep` accepts, with its
    /// distance. Positions are read first and only the nearest candidates are read in full, so
    /// this stays cheap in a world with many actors.
    pub fn nearest(
        &self,
        from: Vec3,
        range: f32,
        keep: impl Fn(&Actor) -> bool,
    ) -> Option<(Actor, f32)> {
        let mut candidates: Vec<(u32, f32)> = (0..self.0.count())
            .filter_map(|index| {
                let &[x, y, z] = &self.0.position(index)[..] else {
                    return None;
                };
                let distance = (Vec3::new(x, y, z) - from).length();
                (distance <= range).then_some((index, distance))
            })
            .collect();
        candidates.sort_by(|a, b| a.1.total_cmp(&b.1));
        candidates.into_iter().find_map(|(index, distance)| {
            self.get(index).filter(&keep).map(|actor| (actor, distance))
        })
    }

    /// The first actor with this script name (case does not matter).
    pub fn find(&self, name: &str) -> Option<Actor> {
        (0..self.0.count())
            .find(|&index| self.0.name(index).eq_ignore_ascii_case(name))
            .and_then(|index| self.get(index))
    }
}

/// The terrain surface slot: what the surface shader library does with the terrain's water.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainSurface {
    /// Turns on the library's shore foam, wet sand and ripples.
    pub effects: bool,
    /// Wave amplitude in scene units; above zero the terrain shader moves the waves.
    pub amplitude: f32,
    /// The terrain grid cell size in scene units (`Water::cell`); needed for the waves.
    pub cell: f32,
}

/// Parameter blocks for the game's shaders. A mod replaces a shader library (`[[shader]]` in
/// mod.toml) and fills the slot that library reads; fill it every frame, and when the mod stops
/// the shaders return to the original.
pub struct Effects(pub(crate) raw::Effects);

impl Effects {
    pub fn terrain_surface(&self, surface: TerrainSurface) {
        self.0.set(
            "terrain-surface",
            &[
                f32::from(u8::from(surface.effects)),
                0.0,
                surface.amplitude,
                surface.cell,
            ],
        );
    }
}

/// Text lines in the top-left corner; call `show` every frame the text should stay.
pub struct Hud(pub(crate) raw::Hud);

impl Hud {
    pub fn show(&self, line: &str) {
        self.0.show(line);
    }
}

/// Keyboard and mouse this frame.
pub struct Input(pub(crate) raw::Input);

impl Input {
    /// The key is held.
    pub fn down(&self, key: Key) -> bool {
        self.0.pressed(key.name())
    }

    /// The key went down this frame.
    pub fn tapped(&self, key: Key) -> bool {
        self.0.tapped(key.name())
    }

    /// The player holds `action` under the game's own bindings.
    pub fn action(&self, action: Action) -> bool {
        self.0.held(action.name())
    }

    /// Mouse motion since the last frame in pixels: x, y.
    pub fn mouse(&self) -> [f32; 2] {
        match self.0.mouse()[..] {
            [x, y] => [x, y],
            _ => [0.0; 2],
        }
    }

    /// Seconds since the last frame.
    pub fn delta(&self) -> f32 {
        self.0.delta()
    }
}

/// The player's values for this mod's `[[setting]]` entries; a key that is not declared reads
/// false, 0 or "". Read every frame so a change in the Mods window takes effect at once.
pub struct Settings {
    pub(crate) raw: raw::Settings,
    pub(crate) id: &'static str,
}

impl Settings {
    pub fn flag(&self, key: &str) -> bool {
        self.raw.flag(self.id, key)
    }

    pub fn number(&self, key: &str) -> f32 {
        self.raw.number(self.id, key)
    }

    pub fn choice(&self, key: &str) -> String {
        self.raw.choice(self.id, key)
    }
}

/// What the mod keeps between frames, for the session, under keys private to this mod.
pub struct Memory {
    pub(crate) raw: raw::Memory,
    pub(crate) id: &'static str,
}

impl Memory {
    fn key(&self, name: &str) -> String {
        format!("{}/{name}", self.id)
    }

    /// The value kept under `name`; `None` when nothing is or it does not unpack.
    pub fn load<T: Persist>(&self, name: &str) -> Option<T> {
        T::unpack(&self.raw.load(&self.key(name)))
    }

    /// Keep `value` under `name`; `None` forgets it.
    pub fn save<T: Persist>(&self, name: &str, value: Option<&T>) {
        let values = value.map_or_else(Vec::new, T::pack);
        self.raw.save(&self.key(name), &values);
    }
}
