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
    installed_keyboard_route(false)
}

#[test]
fn installed_carrying_player_keyboard_movement_and_release() -> Result<()> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (installed carrying-player movement)");
        return Ok(());
    };
    let (mut app, codes, _assets) = prepare_player_app(&install)?;
    for _ in 0..4 {
        app.update();
    }
    {
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        let world = actors
            .current
            .as_mut()
            .context("logical world unavailable")?;
        let player = world
            .selected_player()
            .context("selected player unavailable")?;
        world.set_attribute_for_test(player, cyrus_soup::Attribute::HoldingWeapon, 1);
    }
    walk_route(&mut app, &codes)
}

#[test]
fn installed_mod_animation_request_interrupts_or_waits_for_the_run() -> Result<()> {
    let Some(interrupted) = installed_mod_route(ModRoute::Stop { interrupt: true })? else {
        return Ok(());
    };
    let queued = installed_mod_route(ModRoute::Stop { interrupt: false })?
        .context("install vanished between routes")?;
    eprintln!("idle replaces the run after {interrupted} update(s) interrupting, {queued} queued");
    ensure!(
        interrupted == 1,
        "an interrupting request took {interrupted} updates to replace the run"
    );
    ensure!(
        queued > interrupted,
        "a queued request ({queued} updates) did not wait for the run's exit"
    );
    Ok(())
}

#[test]
fn installed_animation_tempo_brings_the_jump_take_off_forward() -> Result<()> {
    let Some(original) = installed_mod_route(ModRoute::Jump { tempo: None })? else {
        return Ok(());
    };
    let doubled = installed_mod_route(ModRoute::Jump { tempo: Some(2) })?
        .context("install vanished between routes")?;
    eprintln!("take-off after {original} updates at the original pace, {doubled} at double");
    ensure!(
        doubled < original,
        "double tempo took off after {doubled} updates, the original pace after {original}"
    );
    Ok(())
}

#[test]
fn installed_claim_moves_from_the_first_update_and_sideways_without_turning() -> Result<()> {
    installed_mod_route(ModRoute::Claim).map(drop)
}

/// What a mod does on the installed island once the player stands idle.
#[derive(Clone, Copy)]
enum ModRoute {
    /// A mod runs the player forward, then stops and asks for the idle group; `interrupt` asks
    /// for it to replace the run at once.
    Stop { interrupt: bool },
    /// A mod claims the player's movement from standing, forward and then to the left.
    Claim,
    /// A mod jumps from standing at this animation tempo.
    Jump { tempo: Option<u32> },
}

/// Runs `route`; a stop or jump returns the updates it took, `None` without an install.
fn installed_mod_route(route: ModRoute) -> Result<Option<u32>> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (installed mod route)");
        return Ok(None);
    };
    let (mut app, _codes, _assets) = prepare_player_app(&install)?;
    for _ in 0..4 {
        app.update();
    }
    match route {
        ModRoute::Stop { interrupt } => mod_stop(&mut app, interrupt).map(Some),
        ModRoute::Claim => claim(&mut app).map(|()| None),
        ModRoute::Jump { tempo } => jump(&mut app, tempo.map(|tempo| tempo as f32)).map(Some),
    }
}

#[test]
fn installed_base_running_turns_return_to_upright() -> Result<()> {
    installed_keyboard_route(true)
}

fn installed_keyboard_route(banked_turns: bool) -> Result<()> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (installed keyboard walking)");
        return Ok(());
    };
    let (mut app, codes, _assets) = prepare_player_app(&install)?;
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
    let (mut app, _codes, _assets) = prepare_player_app(&install)?;
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
    let mut saw_natural_slide = false;
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
        if !saw_natural_slide && control.sliding_distance != 0 {
            ensure!(
                control.move_counter == 0,
                "natural slope response retained uncalled movement counters: {}",
                control.move_counter
            );
            saw_natural_slide = true;
        }
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
    ensure!(saw_natural_slide, "fixture did not enter natural sliding");
    ensure!(
        moving_after_slide >= 3,
        "the slide group moved the player only {moving_after_slide} updates after the slide ended"
    );
    Ok(())
}

fn prepare_player_app(install: &GameInstall) -> Result<(App, [u8; 15], PlayerAssetHandles)> {
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
    let mut app = controller_app(codes)?;
    app.insert_resource(hud);
    let source = register_player_assets(&mut app, scene, scripts, animations, &prepared, view)?;
    Ok((app, codes, source))
}

fn controller_app(codes: [u8; 15]) -> Result<App> {
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

/// A mod runs the player forward, then stops steering and asks every update for the idle group
/// and a quarter-turn heading; returns the updates until the run gave way.
fn mod_stop(app: &mut App, interrupt: bool) -> Result<u32> {
    use cyrus_script::player::input::{MovementWish, PlayAnimation, PlayerRequests, Wish};
    let wish = |direction| {
        MovementWish(Some(Wish {
            direction,
            jump: false,
            wasd: false,
        }))
    };
    let group = |app: &App| {
        let world = app.world().resource::<ScriptActors>().current.as_ref()?;
        let index = world.selected_player()?.index;
        world.inspect(index, 1)["actors"][0]["animation"]["current_group"].as_u64()
    };
    let run = Some(u64::from(cyrus_soup::AnimationGroup::RUN.index()));
    app.insert_resource(wish(bevy::math::Vec2::Y));
    for _ in 0..12 {
        app.update();
    }
    ensure!(
        group(app) == run,
        "a full mod tilt played {:?}, not the run",
        group(app)
    );
    let status = |app: &App| {
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .and_then(|world| world.locomotion_status())
    };
    let idle = status(app)
        .and_then(|status| status.idle_animation)
        .context("the player has no idle group")?;
    app.insert_resource(wish(bevy::math::Vec2::ZERO));
    for updates in 1..=60 {
        app.insert_resource(PlayerRequests {
            animation: Some(PlayAnimation {
                group: idle,
                interrupt,
            }),
            heading: Some(512),
            tempo: None,
        });
        app.update();
        let heading = status(app).context("player status")?.heading;
        ensure!(
            heading == 512,
            "requested heading 512, the player faces {heading}"
        );
        if group(app) != run {
            return Ok(updates);
        }
    }
    anyhow::bail!("the run never gave way after the mod stopped")
}

/// The player's locomotion status.
fn player_status(app: &App) -> Result<cyrus_script::script::LocomotionStatus> {
    app.world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .and_then(|world| world.locomotion_status())
        .context("player status")
}

/// The player's bank angle (roll), wrapped to one turn.
fn player_roll(app: &App) -> Result<i32> {
    let world = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .context("world")?;
    let id = world.selected_player().context("player")?;
    Ok(world.spatial(id)?.local.angles[2] & 2047)
}

/// The player's model matrix must equal the orientation of its absolute angles.
fn ensure_orientation(app: &App) -> Result<()> {
    let world = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .context("world")?;
    let spatial = world.spatial(world.selected_player().context("player")?)?;
    let expected = cyrus_world::transform::Matrix::orientation(spatial.local.wrapped_angles());
    for (actual, expected) in spatial
        .local
        .matrix
        .0
        .iter()
        .flatten()
        .zip(expected.0.iter().flatten())
    {
        ensure!(
            (actual - expected).abs() < 0.0001,
            "claimed orientation accumulated tilt: {:?}, angles {:?}",
            spatial.local.matrix,
            spatial.local.angles
        );
    }
    Ok(())
}

/// A running claim facing `heading`, travelling along `travel` at `speed`.
fn run_claim(
    heading: u16,
    travel: u16,
    speed: f32,
) -> cyrus_script::player::input::LocomotionClaimed {
    use cyrus_script::player::input::{Gait, LocomotionClaim, LocomotionClaimed};
    LocomotionClaimed(Some(LocomotionClaim {
        heading,
        travel,
        speed,
        gait: Gait::Run,
        jump: false,
        tuning: Default::default(),
    }))
}

/// Map units covered on the ground plane between two Q8 positions.
fn horizontal(a: [i32; 3], b: [i32; 3]) -> f64 {
    let (dx, dz) = (f64::from(b[0] - a[0]), f64::from(b[2] - a[2]));
    (dx * dx + dz * dz).sqrt() / 256.0
}

/// A mod claims the standing player's movement: a run covers ground on the first update, and a
/// travel heading a quarter turn to the left moves the player that way without turning it.
fn claim(app: &mut App) -> Result<()> {
    let start = player_status(app)?;
    let heading = start.heading;
    let speed = f32::from(start.run_speed);
    app.insert_resource(run_claim(heading, heading, speed));
    app.update();
    let first = player_status(app)?;
    let moved = horizontal(start.position, first.position);
    eprintln!("first claimed update covered {moved:.2} map units at run speed {speed}");
    ensure!(
        moved > 0.0,
        "a claimed run did not move on its first update"
    );
    claimed_turns_bank_the_player(app, heading)?;
    // Jump directly out of a banked running turn. The airborne controller must relax the
    // bank as well as keep the model matrix consistent with its absolute facing.
    jump(app, None)?;
    for _ in 0..20 {
        app.update();
        ensure_orientation(app)?;
    }
    ensure!(player_roll(app)? == 0, "jump retained the running bank");
    // Stand, then travel to the left of the facing.
    app.insert_resource(run_claim(heading, heading, 0.0));
    for _ in 0..20 {
        app.update();
    }
    ensure!(
        player_roll(app)? == 0,
        "a standing player stayed banked at {}",
        player_roll(app)?
    );
    claimed_sidestep(app, heading, speed)?;
    claimed_obstacle_route(app, f32::from(start.walk_speed))
}

/// Turning while running banks the player, every update keeps the matrix consistent.
fn claimed_turns_bank_the_player(app: &mut App, heading: u16) -> Result<()> {
    let mut banked = 0;
    for turn in 1..=12u16 {
        let facing = cyrus_world::heading::wrap(heading.wrapping_add(turn * 40));
        app.insert_resource(run_claim(facing, facing, 0.0));
        app.update();
        banked = banked.max(player_roll(app)?);
        ensure_orientation(app)?;
    }
    ensure!(banked != 0, "a running turn did not bank the player");
    Ok(())
}

/// A claim travelling a quarter turn left of the facing moves along the travel heading and
/// leaves the facing alone.
fn claimed_sidestep(app: &mut App, heading: u16, speed: f32) -> Result<()> {
    let before = player_status(app)?;
    let left = cyrus_world::heading::wrap(heading.wrapping_add(512));
    app.insert_resource(run_claim(heading, left, speed));
    for _ in 0..4 {
        app.update();
    }
    let after = player_status(app)?;
    ensure!(after.heading == heading, "a sidestep turned the player");
    let moved = horizontal(before.position, after.position);
    ensure!(moved > 0.0, "a sideways claim did not move");
    // The displacement points along the travel heading, not the facing.
    let travelled = cyrus_world::transform::translate_forward(before.position, left, 256 * 100)?;
    let toward = horizontal(after.position, travelled);
    let away = horizontal(before.position, travelled);
    ensure!(
        toward < away,
        "the sidestep did not move toward the travel heading"
    );
    Ok(())
}

/// Runtime reproduction: this route crosses nearby obstacle contacts, whose wall response
/// adjusts the original controller's heading. A mod owns facing separately from displacement.
fn claimed_obstacle_route(app: &mut App, walk_speed: f32) -> Result<()> {
    use cyrus_script::player::input::{Gait, LocomotionClaim, LocomotionClaimed};
    app.world_mut()
        .resource_mut::<ScriptActors>()
        .current
        .as_mut()
        .context("world")?
        .place_selected_player([40816, -744, 41024].map(|value| value * 256), 0)?;
    let mut grounded = 0;
    for _ in 0..40 {
        app.insert_resource(LocomotionClaimed(Some(LocomotionClaim {
            heading: 883,
            travel: 883,
            speed: walk_speed,
            gait: Gait::Walk,
            jump: false,
            tuning: Default::default(),
        })));
        app.update();
        let current = player_status(app)?;
        if !current.airborne {
            ensure!(
                current.heading == 883,
                "wall response changed claimed facing to {}",
                current.heading
            );
            grounded += 1;
        }
    }
    ensure!(
        grounded > 20,
        "obstacle route did not retain grounded movement"
    );
    Ok(())
}

/// A mod jumps the standing player at `tempo`; returns the updates until it leaves the ground.
fn jump(app: &mut App, tempo: Option<f32>) -> Result<u32> {
    use cyrus_script::player::input::{Gait, LocomotionClaim, LocomotionClaimed, PlayerRequests};
    let status = |app: &App| {
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .and_then(|world| world.locomotion_status())
            .context("player status")
    };
    let start = status(app)?;
    for updates in 1..=60 {
        app.insert_resource(LocomotionClaimed(Some(LocomotionClaim {
            heading: start.heading,
            travel: start.heading,
            speed: 0.0,
            gait: Gait::Walk,
            jump: updates == 1,
            tuning: Default::default(),
        })));
        app.insert_resource(PlayerRequests {
            tempo,
            ..Default::default()
        });
        app.update();
        // Up lowers the map Y.
        if status(app)?.position[1] < start.position[1] {
            return Ok(updates);
        }
    }
    anyhow::bail!("the claimed jump never left the ground")
}
