//! Guest SDK for cyrus mods: typed handles for every hook in `crates/cyrus-mods/wit/cyrus.wit`,
//! a `Mod` trait and one `export_mod!` line instead of hand-written systems and queries.
//!
//! ```ignore
//! struct Hello;
//!
//! impl cyrus_mod_sdk::Mod for Hello {
//!     const ID: &'static str = "hello";
//!     fn frame(cx: &cyrus_mod_sdk::Cx) {
//!         cx.hud.show("hello");
//!     }
//! }
//!
//! cyrus_mod_sdk::export_mod!(Hello);
//! ```
//!
//! `frame` runs every `Update` with every hook in the context; the host copies answers one frame
//! later, except particle emissions, which render after the current Update. A fresh instance of the mod runs for each call, so keep state in `Cx::memory`.
mod effects;
mod handles;
mod keys;
mod math;
mod state;

pub use effects::{Damage, DamageEvent, Particle, Particles};
pub use handles::{
    Camera, Claim, Collision, Contact, Hud, Input, Memory, Movement, NativeCall, Natives, Player,
    Scene, Settings, Uniforms, published,
};
pub use keys::{Action, Key};
pub use math::{Pose, Quat, Vec3};
pub use state::Persist;

use bindings::cyrus::mods::components as raw;
use bindings::wasvy::ecs::app::{App, Query, QueryFor, Schedule, System};

#[doc(hidden)]
pub mod bindings {
    wasvy_macros::guest_bindings!({
        path: ["../../crates/cyrus-mods/wit", "wit"],
        world: "cyrus:sdk/guest",
        with: {
            "wasvy:ecs/app@0.0.11": generate,
            "cyrus:mods/components@0.1.0": generate,
        },
        pub_export_macro: true,
        default_bindings_module: "cyrus_mod_sdk::bindings",
    });
}

/// A mod: its folder name and what it does each frame.
pub trait Mod {
    /// The mod's folder name under `game-config/mods`; keys settings and memory.
    const ID: &'static str;

    /// One frame of the mod.
    fn frame(cx: &Cx);
}

/// Every hook, for one frame.
pub struct Cx {
    pub camera: Camera,
    pub collision: Collision,
    pub hud: Hud,
    pub input: Input,
    pub memory: Memory,
    pub movement: Movement,
    pub natives: Natives,
    pub player: Player,
    pub scene: Scene,
    pub settings: Settings,
    pub uniforms: Uniforms,
    pub damage: Damage,
    pub particles: Particles,
}

/// Hooks in query order. wasvy panics when a method is called through the other mutability, so
/// the hooks whose host methods take `&mut self` are queried `Mut` and the others `Ref`.
fn query_for() -> [QueryFor; 13] {
    [
        QueryFor::Mut(raw::Camera::type_path()),
        QueryFor::Mut(raw::Collision::type_path()),
        QueryFor::Mut(raw::Hud::type_path()),
        QueryFor::Ref(raw::Input::type_path()),
        QueryFor::Mut(raw::Memory::type_path()),
        QueryFor::Mut(raw::Movement::type_path()),
        QueryFor::Mut(raw::Natives::type_path()),
        QueryFor::Ref(raw::Player::type_path()),
        QueryFor::Ref(raw::Scene::type_path()),
        QueryFor::Ref(raw::Settings::type_path()),
        QueryFor::Mut(raw::Uniforms::type_path()),
        QueryFor::Ref(raw::Damage::type_path()),
        QueryFor::Mut(raw::Particles::type_path()),
    ]
}

#[doc(hidden)]
pub fn register(app: App) {
    let frame = System::new("frame");
    frame.add_query(&query_for());
    app.add_systems(&Schedule::Update, &[&frame]);
}

#[doc(hidden)]
pub fn run<M: Mod>(query: Query) {
    while let Some(row) = query.iter() {
        let cx = Cx {
            camera: Camera(raw::Camera::new(row.component(0))),
            collision: Collision(raw::Collision::new(row.component(1))),
            hud: Hud(raw::Hud::new(row.component(2))),
            input: Input(raw::Input::new(row.component(3))),
            memory: Memory {
                raw: raw::Memory::new(row.component(4)),
                id: M::ID,
            },
            movement: Movement(raw::Movement::new(row.component(5))),
            natives: Natives(raw::Natives::new(row.component(6))),
            player: Player(raw::Player::new(row.component(7))),
            scene: Scene(raw::Scene::new(row.component(8))),
            settings: Settings {
                raw: raw::Settings::new(row.component(9)),
                id: M::ID,
            },
            uniforms: Uniforms(raw::Uniforms::new(row.component(10))),
            damage: Damage(raw::Damage::new(row.component(11))),
            particles: Particles {
                raw: raw::Particles::new(row.component(12)),
                id: M::ID,
            },
        };
        M::frame(&cx);
    }
}

/// Export `$mod` (a [`Mod`]) as the component's `setup` and `frame`.
#[macro_export]
macro_rules! export_mod {
    ($mod:ty) => {
        struct __CyrusModExport;

        impl $crate::bindings::Guest for __CyrusModExport {
            fn setup(app: $crate::bindings::wasvy::ecs::app::App) {
                $crate::register(app);
            }

            fn frame(query: $crate::bindings::wasvy::ecs::app::Query) {
                $crate::run::<$mod>(query);
            }
        }

        $crate::bindings::export!(__CyrusModExport with_types_in $crate::bindings);
    };
}
