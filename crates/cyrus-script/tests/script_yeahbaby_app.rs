//! The real keyboard and the Cheats resource drive the `yeahbaby` handler, `magiccarpet` and the
//! Insert camera flag through the installed game's ordinary update (`app.update()`), with the
//! installed START world and its selected player. Skips without a linked install.
use bevy::{asset::AssetApp, input::ButtonInput, prelude::*, state::app::StatesPlugin};
use cyrus_assets::animation::Animations;
use cyrus_assets::cheats::{self, CheatFile};
use cyrus_assets::hud::HudAsset;
use cyrus_assets::script::Scripts;
use cyrus_game::cheats::{CheatStore, Cheats};
use cyrus_game::{AppState, preferences::Preferences};
use cyrus_script::{
    clock::SimulationClock,
    player::{
        Player,
        input::{PlayerInput, PlayerToggles},
        placement::{MarkerEntry, PlayerPose},
    },
    script::{ScriptActors, ScriptHud, ScriptPlugin, ScriptView, ScriptWorld},
};
use cyrus_world::{
    animation::{AnimationRequest, AnimationTick},
    lifecycle::PreparedWorld,
};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Store(Arc<Mutex<Option<Vec<u8>>>>);

impl CheatStore for Store {
    fn load(&self) -> anyhow::Result<CheatFile> {
        self.0
            .lock()
            .unwrap()
            .as_deref()
            .map(cheats::decode)
            .unwrap_or_else(|| {
                // SYSTEM.INI seeds the task and animation switches at startup in every host.
                let mut state = cheats::empty();
                cheats::CheatDefaults {
                    task: true,
                    animation: true,
                    nodemarker: false,
                }
                .apply(&mut state)?;
                Ok(state)
            })
    }
    fn save(&self, state: &CheatFile) -> anyhow::Result<()> {
        *self.0.lock().unwrap() = Some(cheats::encode(state));
        Ok(())
    }
}

struct Fixture {
    app: App,
    _environment: Handle<cyrus_assets::environment::Environment>,
}

/// The installed START world in a real app: the script plugin with the world's scripts,
/// animations, player scene and environment, the script HUD configured from the installed HUD
/// data (without it the ordinary update gives the late stage no selected player), the selected
/// player's entity at the placement the production marker entry computes, the input and cheat
/// resources the ordinary update reads, and a keyboard. The simulation clock is created by the
/// script plugin's world preparation. Only installed data is used; every step is the ordinary
/// `app.update()`.
fn installed_start() -> Option<Fixture> {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    if !root.join("RGFX.EXE").exists() {
        eprintln!("skipped: no install (yeahbaby app fixture)");
        return None;
    }
    let install = cyrus_assets::install::GameInstall::open(&root).unwrap();
    let files = cyrus_assets::native::Files::open(&install).unwrap();
    let prepared = cyrus_assets::game::prepare(&files, "START.player").unwrap();
    let hud_assets = cyrus_assets::game::prepare(&files, "hud/overlay.hud").unwrap();
    let hud_asset: HudAsset = serde_json::from_slice(&hud_assets["hud/overlay.hud"]).unwrap();
    let scripts: Scripts = serde_json::from_slice(&prepared["START.scripts"]).unwrap();
    let data: Animations = serde_json::from_slice(&prepared["START.animations"]).unwrap();
    let scene = cyrus_script::player::data::PlayerScene::decode(&prepared["START.player"]).unwrap();
    let environment_data = serde_json::from_slice(&prepared["START.environment"]).unwrap();
    drop(prepared);
    let world_id = scripts.world;
    // The production marker entry: marker 0 with heading 0 gives the player's pose, exactly as
    // the player placement computes it from the exported marker table.
    let entry = MarkerEntry::default();
    let marker = &data.metadata["markers"][entry.marker];
    assert_eq!(marker["reserved"], 1, "the entry marker is available");
    let position: [i64; 3] = std::array::from_fn(|axis| {
        marker["map_position"][axis]
            .as_i64()
            .expect("marker map position")
            * 256
    });
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin, AssetPlugin::default()))
        .init_state::<AppState>()
        .init_asset::<Scripts>()
        .init_asset::<Animations>()
        .init_asset::<cyrus_script::player::data::PlayerScene>()
        .add_message::<AnimationTick>()
        .add_message::<cyrus_world::sound::SoundCue>()
        .add_plugins(ScriptPlugin);
    let scripts = app
        .world_mut()
        .resource_mut::<Assets<Scripts>>()
        .add(scripts);
    let animations = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_millis(50),
    ));
    app.insert_resource(PreparedWorld {
        requested_name: "START".into(),
        world: Some(world_id),
        scripts,
        animations: animations.clone(),
        player_path: "test.player".into(),
        scene_path: "test.glb".into(),
        environment_path: "test.environment".into(),
    });
    let scene_handle = app
        .world()
        .resource::<AssetServer>()
        .load::<cyrus_script::player::data::PlayerScene>("test.player");
    app.world_mut()
        .resource_mut::<Assets<cyrus_script::player::data::PlayerScene>>()
        .insert(scene_handle.id(), scene)
        .unwrap();
    app.insert_resource(cyrus_script::player::data::PlayerSceneRequest(scene_handle));
    app.init_asset::<cyrus_assets::environment::Environment>();
    let environment = app
        .world()
        .resource::<AssetServer>()
        .load::<cyrus_assets::environment::Environment>(
            app.world()
                .resource::<PreparedWorld>()
                .environment_path
                .clone(),
        );
    app.world_mut()
        .resource_mut::<Assets<cyrus_assets::environment::Environment>>()
        .insert(environment.id(), environment_data)
        .unwrap();
    app.insert_resource(AnimationRequest(animations.clone()));
    app.insert_resource(ScriptView {
        source: animations,
        position: [position[0] as i32, 0, position[2] as i32],
    });
    let hud = ScriptHud::default();
    hud.configure(&hud_asset, &Preferences::default());
    app.insert_resource(hud)
        .insert_resource(entry)
        .insert_resource(PlayerInput::default())
        .insert_resource(PlayerToggles {
            walk_mode: true,
            auto_defend: false,
        })
        .insert_resource(cyrus_game::fidelity::Fidelity::default())
        .insert_resource(Cheats::new(Store::default()))
        .init_resource::<ButtonInput<KeyCode>>();
    app.world_mut().spawn((
        Player,
        PlayerPose {
            position,
            heading: entry.heading as u16,
        },
    ));
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    Some(Fixture {
        app,
        _environment: environment,
    })
}

impl Fixture {
    /// One real frame with the given keys down; key edges are cleared at the end like the input
    /// plugin does between frames.
    fn frame(&mut self, held: &[KeyCode], pressed: &[KeyCode]) {
        {
            let mut keys = self.app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            let down: Vec<KeyCode> = keys.get_pressed().copied().collect();
            for key in down {
                if !held.contains(&key) && !pressed.contains(&key) {
                    keys.release(key);
                }
            }
            for key in held.iter().chain(pressed) {
                keys.press(*key);
            }
        }
        self.app.update();
        assert!(
            self.app
                .world()
                .resource::<ScriptActors>()
                .world_loop_outcome
                .is_none(),
            "world loop ended: {:?}",
            self.app
                .world()
                .resource::<ScriptActors>()
                .world_loop_outcome
        );
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        assert!(
            !matches!(
                self.app
                    .world()
                    .resource::<cyrus_script::script::ScriptStatus>(),
                cyrus_script::script::ScriptStatus::Failed { .. }
            ),
            "{:?}",
            self.app
                .world()
                .resource::<cyrus_script::script::ScriptStatus>()
        );
        if let Some(world) = self.app.world().resource::<ScriptActors>().current.as_ref()
            && let Some(player) = world.selected_player()
        {
            assert!(
                world.actors()[player.index].failure().is_none(),
                "{:?}",
                world.actors()[player.index].failure()
            );
        }
    }

    /// The simulation clock's elapsed game time: it moves only when frames are simulated.
    fn game_seconds(&self) -> f64 {
        self.app
            .world()
            .resource::<SimulationClock>()
            .game_seconds()
    }

    /// The frame scale of the step the last frame used (units of 1/256 nominal tick).
    fn frame_scale(&self) -> u32 {
        self.app
            .world()
            .resource::<SimulationClock>()
            .step()
            .frame_scale
    }

    fn world(&self) -> &ScriptWorld {
        self.app
            .world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .expect("the world is prepared")
    }

    fn position(&self) -> [i32; 3] {
        let world = self.world();
        let player = world.selected_player().expect("selected player");
        world.spatial(player).unwrap().local.position
    }

    fn cheat(&mut self, name: &str, value: u32) {
        self.app
            .world_mut()
            .resource_mut::<Cheats>()
            .execute(name, Some(value))
            .unwrap();
    }

    fn limiter_suspended(&self) -> bool {
        let camera = self.world().camera().expect("camera session");
        camera.records[camera.selected.index()]
            .flags
            .contains(cyrus_script::script::camera::CameraFlags::UNLIMITED_MOVEMENT)
    }

    /// Frames until the selected player's spatial state exists and has settled on the ground.
    fn settle(&mut self) -> [i32; 3] {
        for _ in 0..90 {
            self.frame(&[], &[]);
            let world = self.world();
            if world
                .selected_player()
                .is_some_and(|player| world.spatial(player).is_ok())
            {
                break;
            }
        }
        let before = self.game_seconds();
        for _ in 0..6 {
            self.frame(&[], &[]);
        }
        assert!(
            self.game_seconds() > before,
            "the ordinary update did not advance"
        );
        assert!(
            self.world()
                .selected_player()
                .is_some_and(|player| self.world().spatial(player).is_ok()),
            "the selected player never received spatial state"
        );
        self.position()
    }
}

#[test]
fn page_keys_and_the_home_toggle_move_the_selected_player_only_while_yeahbaby_is_on() {
    let Some(mut fixture) = installed_start() else {
        return;
    };
    let standing = fixture.settle();
    // Cheat off: Page Up has no effect on the standing player.
    fixture.frame(&[KeyCode::PageUp], &[]);
    assert_ne!(fixture.position()[1], standing[1] - 0x1000);
    for _ in 0..3 {
        fixture.frame(&[], &[]);
    }
    assert_eq!(fixture.position(), standing);
    // Cheat on: each held update moves 16 map units and the bypass keeps the support from
    // putting the player back.
    fixture.cheat("yeahbaby", 1);
    fixture.frame(&[KeyCode::PageUp], &[]);
    let lifted = fixture.position();
    assert_eq!(lifted[1], standing[1] - 0x1000);
    assert_eq!((lifted[0], lifted[2]), (standing[0], standing[2]));
    fixture.frame(&[KeyCode::PageUp, KeyCode::PageDown], &[]);
    assert_eq!(fixture.position(), lifted, "both held cancel");
    // Released without the toggle: the ground response returns the player to its support.
    fixture.frame(&[], &[]);
    assert_eq!(fixture.position()[1], standing[1]);
    // Home toggles the bypass on: the lifted player stays up with no key held, also across frames.
    fixture.frame(&[KeyCode::PageUp], &[]);
    let lifted = fixture.position();
    fixture.frame(&[], &[KeyCode::Home]);
    assert!(fixture.world().ground_bypass_toggle());
    for _ in 0..3 {
        fixture.frame(&[], &[]);
        assert_eq!(fixture.position(), lifted);
    }
    // The toggle survives the cheat being turned off and on.
    fixture.cheat("yeahbaby", 0);
    fixture.frame(&[], &[]);
    assert!(fixture.world().ground_bypass_toggle());
    fixture.cheat("yeahbaby", 1);
    // A second press toggles it off and the support puts the player back.
    fixture.frame(&[], &[KeyCode::Home]);
    assert!(!fixture.world().ground_bypass_toggle());
    fixture.frame(&[], &[]);
    assert_eq!(fixture.position()[1], standing[1]);
}

#[test]
fn insert_suspends_the_camera_limiter_and_steps_forward_while_yeahbaby_is_on() {
    let Some(mut fixture) = installed_start() else {
        return;
    };
    let standing = fixture.settle();
    assert!(!fixture.limiter_suspended());
    // Cheat off: Insert does nothing.
    fixture.frame(&[KeyCode::Insert], &[]);
    assert!(!fixture.limiter_suspended());
    assert_eq!(fixture.position(), standing);
    // Cheat on: the first held update suspends the limiter and every held update steps.
    fixture.cheat("yeahbaby", 1);
    // Lift above the entry dock before measuring the Insert step, so its ordinary
    // collision response cannot displace the actor against the dock geometry.
    for _ in 0..4 {
        fixture.frame(&[KeyCode::PageUp], &[]);
    }
    fixture.frame(&[], &[KeyCode::Home]);
    for _ in 0..2 {
        let before = fixture.position();
        let heading = {
            let world = fixture.world();
            world
                .spatial(world.selected_player().unwrap())
                .unwrap()
                .local
                .wrapped_angles()[1]
        };
        fixture.frame(&[KeyCode::Insert], &[]);
        // The step is the frame scale of that update times 128 Q8 along the heading.
        let distance = (fixture.frame_scale() as i32).wrapping_shl(15) >> 8;
        let expected =
            cyrus_world::transform::translate_forward(before, heading, distance).unwrap();
        let now = fixture.position();
        assert_eq!(
            (now[0], now[2]),
            (expected[0], expected[2]),
            "steps forward from {before:?}, heading {heading}, scale {}",
            fixture.frame_scale()
        );
        assert!(fixture.limiter_suspended());
        assert!(fixture.world().yeahbaby_insert_active());
    }
    // Released: the limiter is restored and the player no longer steps.
    fixture.frame(&[], &[]);
    assert!(!fixture.limiter_suspended());
    assert!(!fixture.world().yeahbaby_insert_active());
    let after = fixture.position();
    fixture.frame(&[], &[]);
    assert_eq!(
        (fixture.position()[0], fixture.position()[2]),
        (after[0], after[2])
    );
    // Turning the cheat off while Insert is held restores the limiter and stops the steps.
    fixture.frame(&[KeyCode::Insert], &[]);
    assert!(fixture.limiter_suspended());
    fixture.cheat("yeahbaby", 0);
    let before = fixture.position();
    fixture.frame(&[KeyCode::Insert], &[]);
    assert!(!fixture.limiter_suspended());
    assert_eq!(
        (fixture.position()[0], fixture.position()[2]),
        (before[0], before[2])
    );
}

#[test]
fn the_magic_carpet_ends_the_selected_players_probe_only_for_the_value_one() {
    let Some(mut fixture) = installed_start() else {
        return;
    };
    let standing = fixture.settle();
    fixture.cheat("yeahbaby", 1);
    fixture.frame(&[KeyCode::PageUp], &[]);
    let lifted = fixture.position();
    assert_eq!(lifted[1], standing[1] - 0x1000);
    fixture.cheat("magiccarpet", 1);
    fixture.frame(&[], &[]);
    assert_eq!(fixture.position(), lifted, "the carpet bypasses support");
    fixture.cheat("magiccarpet", 2);
    fixture.frame(&[], &[]);
    assert_eq!(
        fixture.position(),
        standing,
        "two does not enable the carpet"
    );
    fixture.frame(&[KeyCode::PageUp], &[]);
    fixture.cheat("magiccarpet", 0);
    fixture.frame(&[], &[]);
    assert_eq!(
        fixture.position(),
        standing,
        "zero does not enable the carpet"
    );
}
