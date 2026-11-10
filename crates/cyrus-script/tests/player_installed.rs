//! Installed selected-player keyboard route through the production headless scheduler.
use anyhow::{Context, Result, ensure};
use bevy::{
    asset::AssetApp,
    prelude::{
        App, AppExtStates, AssetPlugin, AssetServer, Assets, ButtonInput, Entity, KeyCode,
        MinimalPlugins, NextState, With,
    },
    state::app::StatesPlugin,
};
use cyrus_assets::{animation::Animations, install::GameInstall, script::Scripts};
use cyrus_game::AppState;
use cyrus_script::{
    player::{
        Player, PlayerPlugin,
        data::PlayerScene,
        input::{PlayerBindings, PlayerToggles},
        placement::PlayerPose,
    },
    script::{ScriptActorSource, ScriptActors, ScriptPlugin, ScriptView},
};
use cyrus_world::{animation::AnimationRequest, lifecycle::PreparedWorld, sound::SoundCue};

#[test]
fn installed_island_keyboard_walk_and_release() -> Result<()> {
    installed_keyboard_route(false, false)
}

#[test]
fn installed_modern_jump_returns_to_ground_after_release() -> Result<()> {
    installed_keyboard_route(true, false)
}

#[test]
fn installed_base_running_turns_return_to_upright() -> Result<()> {
    installed_keyboard_route(false, true)
}

fn installed_keyboard_route(modern_jump: bool, banked_turns: bool) -> Result<()> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (installed keyboard walking)");
        return Ok(());
    };
    let (mut app, codes, _assets) = prepare_player_app(&install, modern_jump)?;
    for _ in 0..4 {
        app.update();
    }
    let idle = position(&mut app)?;
    app.update();
    ensure!(
        position(&mut app)? == idle,
        "idle player moved before input"
    );
    if banked_turns {
        banked_turn_route(&mut app, &codes)
    } else if modern_jump {
        standing_jump_route(&mut app)?;
        moving_jump_route(&mut app)
    } else {
        walk_route(&mut app, &codes)
    }
}

/// Matched with the original on ISLAND: a player placed on the steep terrain above the
/// CCGRATE slab slides, lands on the slab, and the backward slide group keeps moving it across
/// the slab after the slope response has cleared the sliding byte.
#[test]
fn installed_island_slide_group_keeps_moving_the_player_across_the_slab() -> Result<()> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (installed slide continuation)");
        return Ok(());
    };
    let (mut app, _codes, _assets) = prepare_player_app(&install, false)?;
    for _ in 0..4 {
        app.update();
    }
    {
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        let world = actors
            .current
            .as_mut()
            .context("logical world unavailable")?;
        world.place_selected_player([40832 * 256, -616 * 256, 40400 * 256], 0)?;
    }
    let mut moving_after_slide = 0;
    let mut previous: Option<[i32; 3]> = None;
    for _ in 0..80 {
        app.update();
        let current = position(&mut app)?;
        let actors = app.world().resource::<ScriptActors>();
        let world = actors
            .current
            .as_ref()
            .context("logical world unavailable")?;
        let id = world
            .selected_player()
            .context("selected player unavailable")?;
        let control = world.actors()[id.index]
            .control_state()
            .context("control")?;
        let group = world.inspect(id.index, 1)["actors"][0]["animation"]["current_group"].as_u64();
        if let Some(before) = previous
            && control.sliding_state == 0
            && group
                == Some(u64::from(
                    cyrus_soup::AnimationGroup::SLIDE_BACKWARD.index(),
                ))
            && current[2] < before[2]
            && current[1] == before[1]
        {
            moving_after_slide += 1;
        }
        previous = Some(current);
    }
    ensure!(
        moving_after_slide >= 3,
        "the slide group moved the player only {moving_after_slide} updates after the slide ended"
    );
    Ok(())
}

fn prepare_player_app(
    install: &GameInstall,
    modern_jump: bool,
) -> Result<(App, [u8; 15], PlayerAssetHandles)> {
    let files = cyrus_assets::native::Files::open(install)?;
    let prepared = cyrus_assets::game::prepare(&files, "ISLAND.player")?;
    let hud_assets = cyrus_assets::game::prepare(&files, "hud/overlay.hud")?;
    let hud_data: cyrus_assets::hud::HudAsset =
        serde_json::from_slice(&hud_assets["hud/overlay.hud"])?;
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(
        &hud_data,
        &cyrus_game::preferences::Preferences::new(EmptyPreferences),
    );
    ensure!(hud.ready(), "installed HUD inventory did not initialize");
    let animations: Animations = serde_json::from_slice(&prepared["ISLAND.animations"])?;
    let scripts: Scripts = serde_json::from_slice(&prepared["ISLAND.scripts"])?;
    let scene = PlayerScene::decode(&prepared["ISLAND.player"])?;
    let marker: [i32; 3] =
        serde_json::from_value(animations.metadata["markers"][0]["map_position"].clone())?;
    let view = marker.map(|value| value * 256);
    let codes = scene
        .control_codes
        .context("installed keyboard bindings unavailable")?;
    let mut app = controller_app(codes, modern_jump)?;
    app.insert_resource(hud);
    let source = register_player_assets(&mut app, scene, scripts, animations, &prepared, view)?;
    Ok((app, codes, source))
}

fn controller_app(codes: [u8; 15], modern_jump: bool) -> Result<App> {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin, AssetPlugin::default()))
        .init_state::<AppState>()
        .init_asset::<Scripts>()
        .init_asset::<Animations>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<cyrus_game::fidelity::Fidelity>()
        .add_message::<SoundCue>()
        .add_message::<cyrus_world::animation::AnimationTick>()
        .add_plugins((PlayerPlugin, ScriptPlugin));
    if modern_jump {
        app.world_mut()
            .resource_mut::<cyrus_game::fidelity::Fidelity>()
            .player_controller = cyrus_game::fidelity::Mode::Modern;
    }
    app.insert_resource(PlayerBindings::from_codes(codes).map_err(anyhow::Error::msg)?);
    app.insert_resource(PlayerToggles {
        walk_mode: true,
        auto_defend: false,
    });
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_millis(55),
    ));
    Ok(app)
}

// Keep manually supplied assets alive throughout the controller route.
struct PlayerAssetHandles {
    _scene: bevy::asset::Handle<PlayerScene>,
    _environment: bevy::asset::Handle<cyrus_assets::environment::Environment>,
}

fn register_player_assets(
    app: &mut App,
    scene: PlayerScene,
    scripts: Scripts,
    animations: Animations,
    prepared: &std::collections::BTreeMap<String, Vec<u8>>,
    view: [i32; 3],
) -> Result<PlayerAssetHandles> {
    let scripts = app
        .world_mut()
        .resource_mut::<Assets<Scripts>>()
        .add(scripts);
    let animations = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(animations);
    let source = app
        .world()
        .resource::<AssetServer>()
        .load::<PlayerScene>("installed-island.player");
    let scene_world = scene.world;
    app.world_mut()
        .resource_mut::<Assets<PlayerScene>>()
        .insert(source.id(), scene)?;
    app.insert_resource(PreparedWorld {
        requested_name: "ISLAND".into(),
        world: Some(scene_world),
        scripts,
        animations: animations.clone(),
        player_path: "installed-island.player".into(),
        scene_path: "island.glb".into(),
        environment_path: "island.environment".into(),
    });
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
    let environment_data = serde_json::from_slice(&prepared["ISLAND.environment"])?;
    app.world_mut()
        .resource_mut::<Assets<cyrus_assets::environment::Environment>>()
        .insert(environment.id(), environment_data)
        .unwrap();
    app.insert_resource(AnimationRequest(animations.clone()));
    app.insert_resource(ScriptView {
        source: animations,
        position: view,
    });
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    Ok(PlayerAssetHandles {
        _scene: source,
        _environment: environment,
    })
}

fn standing_jump_route(app: &mut App) -> Result<()> {
    let ground = position(app)?;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    for _ in 0..4 {
        app.update();
        position(app)?;
    }
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::Space);
    let mut highest = ground[1];
    for _ in 0..120 {
        app.update();
        let current = position(app)?;
        highest = highest.min(current[1]);
    }
    ensure!(highest < ground[1], "Space did not start a jump");
    ensure!(
        position(app)?[1] == ground[1],
        "released jump did not return to ground"
    );
    Ok(())
}

fn moving_jump_route(app: &mut App) -> Result<()> {
    for running in [false, true] {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        if running {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::ShiftLeft);
        }
        for _ in 0..12 {
            app.update();
            position(app)?;
        }
        // Modern walks unless Shift is held.
        let actors = app.world().resource::<ScriptActors>();
        let world = actors.current.as_ref().unwrap();
        let index = world.selected_player().unwrap().index;
        let group = world.inspect(index, 1)["actors"][0]["animation"]["current_group"].as_u64();
        let expected = if running {
            cyrus_soup::AnimationGroup::RUN
        } else {
            cyrus_soup::AnimationGroup::WALK
        };
        ensure!(
            group == Some(u64::from(expected.index())),
            "held forward played group {group:?}, expected {expected:?}"
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);
        let mut entered_airborne = false;
        for _ in 0..12 {
            app.update();
            position(app)?;
            let actors = app.world().resource::<ScriptActors>();
            let world = actors.current.as_ref().unwrap();
            let id = world.selected_player().unwrap();
            let control = world.actors()[id.index].control_state().unwrap();
            entered_airborne |= control.queued_motion != cyrus_script::script::JumpMotion::NONE
                || control.falling_state != cyrus_script::script::JumpMotion::NONE;
        }
        ensure!(entered_airborne, "moving Space input did not start a jump");
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release_all();
        for _ in 0..120 {
            app.update();
            position(app)?;
        }
        let actors = app.world().resource::<ScriptActors>();
        let world = actors.current.as_ref().unwrap();
        let id = world.selected_player().unwrap();
        let control = world.actors()[id.index].control_state().unwrap();
        ensure!(
            control.queued_motion == cyrus_script::script::JumpMotion::NONE
                && control.falling_state == cyrus_script::script::JumpMotion::NONE,
            "moving jump did not land"
        );
    }
    Ok(())
}

fn walk_route(app: &mut App, codes: &[u8]) -> Result<()> {
    let initial = position(app)?;
    let key = match cyrus_script::player::input::Input::try_from(codes[0]) {
        Ok(cyrus_script::player::input::Input::Key(scan)) => scan.key(),
        other => anyhow::bail!("installed forward binding is not a key: {other:?}"),
    };
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    for _ in 0..16 {
        app.update();
        position(app)?;
    }
    let moved = position(app)?;
    ensure!(moved != initial, "held forward did not move: {initial:?}");
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(key);
    for _ in 0..40 {
        app.update();
        position(app)?;
    }
    let stopped = position(app)?;
    for _ in 0..4 {
        app.update();
        ensure!(position(app)? == stopped, "released player did not settle");
    }
    Ok(())
}

struct EmptyPreferences;
impl cyrus_game::preferences::PreferenceStore for EmptyPreferences {
    fn load(&self, _: cyrus_game::preferences::PreferenceKey) -> Result<Option<String>> {
        Ok(None)
    }
    fn save(&self, _: cyrus_game::preferences::PreferenceKey, _: &str) -> Result<()> {
        anyhow::bail!("headless controller test must not write preferences")
    }
}

/// Exercise base running turns while checking script health and presentation each update.
fn banked_turn_route(app: &mut App, codes: &[u8]) -> Result<()> {
    let key = |index: usize| -> Result<KeyCode> {
        match cyrus_script::player::input::Input::try_from(codes[index]) {
            Ok(cyrus_script::player::input::Input::Key(scan)) => Ok(scan.key()),
            other => anyhow::bail!("installed binding {index} is not a key: {other:?}"),
        }
    };
    app.world_mut().resource_mut::<PlayerToggles>().walk_mode = false;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key(0)?);
    let mut banked = false;
    for turn in [key(2)?, key(3)?].into_iter().cycle().take(8) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(turn);
        for _ in 0..8 {
            app.update();
            position(app)?;
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(turn);
        for _ in 0..8 {
            app.update();
            position(app)?;
            let actors = app.world().resource::<ScriptActors>();
            let world = actors.current.as_ref().context("world")?;
            let spatial = world.spatial(world.selected_player().context("player")?)?;
            banked |= spatial.local.angles[2] & 2047 != 0;
            if spatial.local.angles[2] & 2047 == 0 {
                ensure!(
                    spatial.local.matrix.0[1]
                        .iter()
                        .zip([0.0, 1.0, 0.0])
                        .all(|(a, b)| (a - b).abs() < 0.0001),
                    "base running turn retained tilt: {:?}",
                    spatial.local
                );
            }
        }
    }
    ensure!(banked, "base turn route never banked");
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release_all();
    Ok(())
}

/// Validate script health and agreement between logical and presented player positions.
fn position(app: &mut App) -> Result<[i32; 3]> {
    let actors = app.world().resource::<ScriptActors>();
    ensure!(
        !matches!(
            app.world().resource::<cyrus_script::script::ScriptStatus>(),
            cyrus_script::script::ScriptStatus::Failed { .. }
        ),
        "registry: {:?}",
        app.world().resource::<cyrus_script::script::ScriptStatus>()
    );
    let world = actors
        .current
        .as_ref()
        .context("logical world unavailable")?;
    let index = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .context("selected player unavailable")?;
    let actor = &world.actors()[index];
    ensure!(
        actor.failure().is_none(),
        "selected player: {:?}, control: {:?}",
        actor.failure(),
        actor.control_state()
    );
    let position = world.spatial(world.id(index)?)?.local.position;
    let entity = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())?;
    ensure!(
        app.world()
            .get::<PlayerPose>(entity)
            .context("presentation pose unavailable")?
            .position
            == position.map(i64::from),
        "presentation differs from logical player"
    );
    Ok(position)
}
