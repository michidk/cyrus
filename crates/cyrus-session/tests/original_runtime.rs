//! Public runtime admission of installed original continuations.
#![cfg(feature = "original-save-import")]
use anyhow::{Context, Result, ensure};
use bevy::prelude::{
    App, AssetApp, AssetPlugin, AssetServer, Assets, AudioSource, IntoScheduleConfigs,
    MinimalPlugins, Name, State, Transform, Visibility,
};
use cyrus_assets::{animation::Animations, hud::HudAsset, install::GameInstall, script::Scripts};
use cyrus_audio::sound::{Panned, RestoredDialogueAssets, SoundPlugin, dialogue::DialogueClip};
use cyrus_game::{
    preferences::{PreferenceKey, PreferenceStore, Preferences},
    random::EngineRandom,
};
use cyrus_script::{
    player::{
        Player, data::PlayerScene, input::PlayerToggles, placement::*, runtime::PlayerRuntime,
    },
    script::*,
};
use cyrus_session::*;
use rgpre::import::savegame::{SaveGame, load_save_slot};
use std::{collections::BTreeMap, path::Path};
struct NoWrites;
impl PreferenceStore for NoWrites {
    fn load(&self, _: PreferenceKey) -> Result<Option<String>> {
        Ok(None)
    }
    fn save(&self, _: PreferenceKey, _: &str) -> Result<()> {
        anyhow::bail!("import wrote preferences")
    }
}
fn installed_model_allocations(
    game: &SaveGame,
    animations: &cyrus_assets::animation::Animations,
    install: &GameInstall,
) -> Result<Vec<rgpre::import::savegame::SaveModelAllocation>> {
    let worlds = cyrus_assets::scene::read_worlds(install)?;
    let entry = worlds
        .entries
        .iter()
        .find(|entry| entry.index == game.metadata.world_id)
        .context("saved world not installed")?;
    let filename = entry
        .map
        .rsplit(['/', '\\'])
        .next()
        .context("empty map name")?;
    let path = cyrus_assets::scene::find_file(&install.root.join("maps"), filename)
        .context("installed map absent")?;
    let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(path)?)?;
    let allocations = rgpre::import::savegame::SaveModelAllocations::from_world(
        &map,
        &["CYRUS", "GREMLIN"],
        |source| {
            let file = source.rsplit(['/', '\\']).next().unwrap_or(source);
            let key = file
                .rsplit_once('.')
                .map_or(file, |(stem, _)| stem)
                .to_ascii_uppercase();
            let geometry = animations.models.get(&key).ok_or_else(|| {
                rgpre::error::Error::Parse(format!("installed geometry {key} missing"))
            })?;
            Ok((key, geometry.positions.len()))
        },
    )?;
    Ok(allocations.entries().to_vec())
}

#[test]
fn retained_runtime_import_preserves_late_rejection_and_reaches_first_service_update() -> Result<()>
{
    let (Some(root), Some(slots)) = (
        std::env::var_os("REDGUARD_DIR"),
        std::env::var_os("REDGUARD_ORIGINAL_SAVE_DIR"),
    ) else {
        eprintln!("SKIP retained runtime admission: install and slots not supplied");
        return Ok(());
    };
    let install = GameInstall::locate(Some(Path::new(&root)))?;
    let files = cyrus_assets::native::Files::open(&install)?;
    let exported = cyrus_assets::game::prepare(&files, "hud/overlay.hud")?;
    let hud: HudAsset = serde_json::from_slice(&exported["hud/overlay.hud"])?;
    let preferences = Preferences::new(NoWrites);
    let mut paths = std::fs::read_dir(slots)?.collect::<std::io::Result<Vec<_>>>()?;
    paths.sort_by_key(|entry| entry.path());
    ensure!(!paths.is_empty(), "retained slots absent");
    for path in paths {
        if !path.path().is_dir() {
            continue;
        }
        let slot = load_save_slot(path.path())?;
        let mut scripts = BTreeMap::new();
        let mut animations = BTreeMap::new();
        let mut players = BTreeMap::new();
        let mut player_bytes = BTreeMap::new();
        let mut models = BTreeMap::new();
        let mut walks = BTreeMap::new();
        let mut ropes = BTreeMap::new();
        for game in std::iter::once(&slot.game).chain(slot.maps.values()) {
            let id = game.metadata.world_id;
            let key = format!("{id}.scripts");
            let data = cyrus_assets::game::prepare(&files, &key)?;
            let script: Scripts = serde_json::from_slice(&data[&key])?;
            let animation: Animations = serde_json::from_slice(&data[&format!("{id}.animations")])?;
            let player = PlayerScene::decode(&data[&format!("{id}.player")])?;
            player_bytes.insert(id, data[&format!("{id}.player")].clone());
            models.insert(id, installed_model_allocations(game, &animation, &install)?);
            walks.insert(
                id,
                (0..animation.metadata["walk_node_maps"]
                    .as_array()
                    .context("walk maps absent")?
                    .len())
                    .map(|i| i as u32)
                    .collect::<Vec<_>>(),
            );
            ropes.insert(
                id,
                (1..=animation.metadata["ropes"].as_array().map_or(0, Vec::len))
                    .map(|i| i as u32)
                    .collect::<Vec<_>>(),
            );
            scripts.insert(id, script);
            animations.insert(id, animation);
            players.insert(id, player);
        }
        let sources = scripts
            .iter()
            .map(|(&id, script)| {
                (
                    id,
                    WorldSnapshotSources {
                        scripts: script,
                        animations: &animations[&id],
                        collision: &players[&id].collision,
                        player: Some(&players[&id]),
                        effects: None,
                        identity_memo: None,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let channels = (0u8..16).map(|i| (i, i)).collect();
        let allocations = scripts
            .keys()
            .map(|&id| {
                (
                    id,
                    OriginalWorldAllocations {
                        models: &models[&id],
                        walk_maps: &walks[&id],
                        ropes: &ropes[&id],
                        ambient_channels: &channels,
                    },
                )
            })
            .collect();
        let id = slot.game.metadata.world_id;
        let mut prior = ScriptWorld::prepare(&scripts[&id], 1)?;
        prior.prepare_spatial()?;
        let player = prior.selected_player().context("prior player absent")?;
        prior.initialize_player_spatial(player, [0; 3], [0; 3])?;
        prior.prepare_model_resources(
            &animations[&id],
            players[&id]
                .actor_collision
                .as_ref()
                .context("models absent")?,
        )?;
        prior.initialize_player_animation(player, &animations[&id], 0, &mut |_| Ok(()))?;
        let HostFixture {
            mut app,
            scene,
            animation,
            loaded_worlds,
        } = prepare_host(prior, &sources, &player_bytes, &hud, &preferences, id)?;
        let snapshot = RuntimeSessionSnapshot::capture(app.world(), &sources, &hud)?;
        let before = serde_json::to_value(&snapshot)?;
        let prepare = |snapshot: RuntimeSessionSnapshot| {
            snapshot.prepare_original(
                LogicalSessionRestoreInputs {
                    sources: &sources,
                    last_generation: 20,
                    hud: &hud,
                    preferences: &preferences,
                    bios_tick: 100,
                },
                OriginalSessionImport {
                    slot: &slot,
                    sources: &sources,
                    allocations: &allocations,
                    hud: &hud,
                    preferences: &preferences,
                },
            )
        };
        let admission = prepare(snapshot.clone())?;
        let retained = SessionWorldAssets::from_loaded(
            admission.snapshot(),
            loaded_worlds.clone(),
            BTreeMap::new(),
        )?;
        let completion = AdmissionCompletion {
            sources: &sources,
            hud: &hud,
            preferences: &preferences,
            scene: &scene,
            animation: &animation,
            retained: &retained,
        };
        completion.assert_late_rejection(admission, &mut app, &before)?;
        let admission = prepare(snapshot)?;
        let hud_handle =
            app.world_mut()
                .resource_mut::<Assets<HudAsset>>()
                .add(serde_json::from_slice::<HudAsset>(&serde_json::to_vec(
                    &hud,
                )?)?);
        app.add_plugins(SessionBoundaryPlugin);
        // The first presentation pass must see the complete replacement, never the old host.
        app.add_systems(
            bevy::prelude::PostUpdate,
            (|world: &mut bevy::prelude::World| {
                assert!(world.contains_resource::<SessionPublicationBoundary>());
                let player = world
                    .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<Player>>()
                    .single(world)
                    .unwrap();
                assert!(matches!(
                    world.get::<PlayerEntrySupport>(player),
                    Some(PlayerEntrySupport::RestoredContinuation)
                ));
            })
            .in_set(cyrus_render::animation::PoseRender),
        );
        app.world_mut()
            .resource_mut::<SessionBoundary>()
            .import_original_session(
                admission,
                SessionLoadAssets {
                    sources: retained.clone(),
                    hud: hud_handle,
                    hud_images: None,
                    samples: BTreeMap::new(),
                    dialogue: BTreeMap::new(),
                    music: None,
                },
            )?;
        app.world_mut().run_schedule(bevy::app::PostUpdate);
        let result = app
            .world_mut()
            .resource_mut::<SessionBoundary>()
            .take_result()
            .context("original runtime publication did not finish")??;
        ensure!(
            matches!(result, SessionBoundaryResult::Committed),
            "original runtime returned capture"
        );
        let replacement = app
            .world_mut()
            .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<Player>>()
            .single(app.world())?;
        assert!(matches!(
            app.world().get::<PlayerEntrySupport>(replacement),
            Some(PlayerEntrySupport::RestoredContinuation)
        ));
        let published = app.world().resource::<ScriptActors>();
        let current = published.current.as_ref().unwrap();
        let player_id = current
            .selected_player()
            .context("published player absent")?;
        let model = current
            .model_presentation(player_id)?
            .context("published player model absent")?;
        assert_eq!(
            app.world()
                .get::<bevy::prelude::GlobalTransform>(replacement)
                .context("first rendered affine absent")?
                .to_matrix(),
            bevy::prelude::Mat4::from(cyrus_render::script_transform::scene_transform_at(
                model.pose,
                model.position
            ))
        );
        let checkpoint = RuntimeSessionSnapshot::capture(app.world(), &sources, &hud)?;
        checkpoint.clone().plan(LogicalSessionRestoreInputs {
            sources: &sources,
            last_generation: 100,
            hud: &hud,
            preferences: &preferences,
            bios_tick: 100,
        })?;
        // Repeat a late rejection with the imported native continuations already live.
        let active_before = serde_json::to_value(&checkpoint)?;
        let admission = prepare(checkpoint)?;
        completion.assert_late_rejection(admission, &mut app, &active_before)?;
        assert_first_native_update(&mut app, &scripts[&id])?;
        eprintln!(
            "runtime admitted world {id} through first native update without callback reissue"
        );
    }
    Ok(())
}

type LoadedWorlds = BTreeMap<
    u32,
    (
        cyrus_world::lifecycle::PreparedWorld,
        bevy::prelude::Handle<PlayerScene>,
    ),
>;
struct HostFixture {
    app: App,
    scene: bevy::prelude::Handle<PlayerScene>,
    animation: bevy::prelude::Handle<Animations>,
    loaded_worlds: LoadedWorlds,
}

fn prepare_host(
    prior: ScriptWorld,
    sources: &BTreeMap<u32, WorldSnapshotSources<'_>>,
    player_bytes: &BTreeMap<u32, Vec<u8>>,
    hud: &HudAsset,
    preferences: &Preferences,
    id: u32,
) -> Result<HostFixture> {
    let player = prior.selected_player().context("prior player absent")?;
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), SoundPlugin))
        .init_asset::<Panned>()
        .init_asset::<AudioSource>()
        .init_asset::<DialogueClip>()
        .init_asset::<HudAsset>()
        .init_asset::<Scripts>()
        .init_asset::<Animations>()
        .init_asset::<PlayerScene>();
    let script_handle =
        app.world_mut()
            .resource_mut::<Assets<Scripts>>()
            .add(serde_json::from_slice::<Scripts>(&serde_json::to_vec(
                sources[&id].scripts,
            )?)?);
    let animation =
        app.world_mut()
            .resource_mut::<Assets<Animations>>()
            .add(serde_json::from_slice::<Animations>(&serde_json::to_vec(
                sources[&id].animations,
            )?)?);
    let mut loaded_worlds = BTreeMap::new();
    for (&world, source) in sources {
        let sh = if world == id {
            script_handle.clone()
        } else {
            app.world_mut().resource_mut::<Assets<Scripts>>().add(
                serde_json::from_slice::<Scripts>(&serde_json::to_vec(source.scripts)?)?,
            )
        };
        let ah = if world == id {
            animation.clone()
        } else {
            app.world_mut()
                .resource_mut::<Assets<Animations>>()
                .add(serde_json::from_slice::<Animations>(&serde_json::to_vec(
                    source.animations,
                )?)?)
        };
        let ph = app
            .world_mut()
            .resource_mut::<Assets<PlayerScene>>()
            .add(PlayerScene::decode(&player_bytes[&world])?);
        loaded_worlds.insert(
            world,
            (
                cyrus_world::lifecycle::PreparedWorld {
                    requested_name: world.to_string(),
                    world: Some(world),
                    scripts: sh,
                    animations: ah,
                    player_path: format!("{world}.player").into(),
                    scene_path: format!("{world}.glb").into(),
                    environment_path: format!("{world}.environment").into(),
                },
                ph,
            ),
        );
    }
    let (prepared, scene) = loaded_worlds[&id].clone();
    let mut registry = ScriptActors::default();
    registry.current = Some(prior);
    let logical_hud = ScriptHud::default();
    logical_hud.configure(hud, preferences);
    app.insert_resource(registry)
        .insert_resource(logical_hud)
        .insert_resource(cyrus_script::clock::SimulationClock::new(
            sources[&id].scripts.clock,
        )?)
        .insert_resource(EngineRandom::from_state(12345))
        .insert_resource(PlayerToggles {
            walk_mode: false,
            auto_defend: true,
        })
        .insert_resource(PlayerRuntime::default())
        .insert_resource(MarkerEntry::default())
        .insert_resource(State::new(cyrus_game::AppState::InGame))
        .insert_resource(prepared.clone())
        .insert_resource(cyrus_world::lifecycle::CurrentScene {
            name: id.to_string(),
            world: Some(id),
        })
        .insert_resource(cyrus_world::lifecycle::SceneRequest(id.to_string()))
        .insert_resource(cyrus_script::player::data::PlayerSceneRequest(
            scene.clone(),
        ))
        .insert_resource(cyrus_world::animation::AnimationRequest(animation.clone()))
        .init_resource::<cyrus_audio::music::Music>();
    let owners = app
        .world()
        .resource::<ScriptActors>()
        .current
        .as_ref()
        .unwrap()
        .actors()
        .iter()
        .enumerate()
        .filter(|(_, actor)| !matches!(actor.source, ScriptActorSource::Shell(_)))
        .map(|(index, actor)| {
            (
                index,
                actor.name().to_owned(),
                matches!(actor.source, ScriptActorSource::Map(_)),
            )
        })
        .collect::<Vec<_>>();
    for (index, name, map) in owners {
        let mut entity = app.world_mut().spawn((
            ScriptBinding {
                actor: ScriptActorId {
                    generation: 1,
                    index,
                },
                source: animation.clone(),
            },
            Transform::default(),
            Visibility::Inherited,
        ));
        if map {
            entity.insert(Name::new(format!("B_{index:03}_{name}")));
        }
        if index == player.index {
            entity.insert((
                Player,
                PlayerPose {
                    position: [0; 3],
                    heading: 0,
                },
                PlayerEntrySupport::StaticSettled,
            ));
        }
    }
    Ok(HostFixture {
        app,
        scene,
        animation,
        loaded_worlds,
    })
}

fn assert_first_native_update(app: &mut App, scripts: &Scripts) -> Result<()> {
    let world = app
        .world_mut()
        .resource_mut::<ScriptActors>()
        .into_inner()
        .current
        .as_mut()
        .unwrap();
    let programs = scripts
        .actors
        .values()
        .map(|actor| {
            (
                cyrus_soup::CodeOrigin(actor.code_origin),
                std::sync::Arc::new(actor.program.clone()),
            )
        })
        .collect();
    let waiting = world
        .actors()
        .iter()
        .enumerate()
        .find_map(|(index, actor)| {
            let runtime = actor.runtime()?;
            let vm = runtime.snapshot(&programs).ok()?;
            let calls = vm.task_calls().ok()?;
            (calls.len() == 1
                && calls[0].call.function == cyrus_soup::Native::Wait
                && runtime.task_slots()[0].4 < u16::MAX)
                .then_some(index)
        })
        .context("saved native Wait absent")?;
    let cursor = world.actors()[waiting]
        .runtime()
        .unwrap()
        .program_counter(cyrus_soup::Thread::Main);
    world.advance(
        world.id(waiting)?,
        ScriptStage::Tasks,
        ScriptInputs {
            nominal_ticks: Some(1),
            frame_scale: Some(256),
            ..Default::default()
        },
        Some(&mut |_, _| Ok(())),
        None,
        100,
    )?;
    assert_eq!(
        world.actors()[waiting]
            .runtime()
            .unwrap()
            .program_counter(cyrus_soup::Thread::Main),
        cursor
    );
    Ok(())
}

struct AdmissionCompletion<'a> {
    sources: &'a BTreeMap<u32, WorldSnapshotSources<'a>>,
    hud: &'a HudAsset,
    preferences: &'a Preferences,
    scene: &'a bevy::prelude::Handle<PlayerScene>,
    animation: &'a bevy::prelude::Handle<Animations>,
    retained: &'a SessionWorldAssets,
}
impl AdmissionCompletion<'_> {
    fn assert_late_rejection(
        &self,
        admission: OriginalRuntimeAdmission,
        app: &mut App,
        before: &serde_json::Value,
    ) -> Result<()> {
        let stage = SessionEntityStage::prepare(admission.snapshot(), None, app.world_mut())?;
        let child = app
            .world_mut()
            .spawn((
                Transform::default(),
                bevy::prelude::GlobalTransform::default(),
                bevy::prelude::ChildOf(stage.player()),
            ))
            .id();
        let candidate = self.finish(admission, &stage, app)?;
        app.world_mut().despawn(child);
        let error = candidate
            .commit(app.world_mut())
            .err()
            .context("late projection target disappearance admitted")?;
        ensure!(
            error.to_string().contains("projection target"),
            "rejected before late projection validation: {error:#}"
        );
        assert_eq!(
            &serde_json::to_value(RuntimeSessionSnapshot::capture(
                app.world(),
                self.sources,
                self.hud
            )?)?,
            before
        );
        stage.discard(app.world_mut());
        Ok(())
    }
    fn finish(
        &self,
        admission: OriginalRuntimeAdmission,
        stage: &SessionEntityStage,
        app: &App,
    ) -> Result<RuntimeSessionCandidate> {
        let replacement = stage.player();
        admission
            .plan(LogicalSessionRestoreInputs {
                sources: self.sources,
                last_generation: 20,
                hud: self.hud,
                preferences: self.preferences,
                bios_tick: 100,
            })?
            .finish(
                PlayerReplacement {
                    entity: replacement,
                    scene: self.scene.clone(),
                    animation: self.animation.clone(),
                },
                SessionServices {
                    sources: self.retained.clone(),
                    scene: None,
                    camera_entity: None,
                    entities: stage.bindings(),
                    source_audio: &BTreeMap::new(),
                    dialogue: RestoredDialogueAssets {
                        server: app.world().resource::<AssetServer>(),
                        audio_sources: app.world().resource::<Assets<AudioSource>>(),
                        loaded: app.world().resource::<Assets<DialogueClip>>(),
                        clips: &BTreeMap::new(),
                        requests: &BTreeMap::new(),
                        hud: None,
                    },
                    music_source: None,
                    hud_asset: self.hud,
                    hud_images: None,
                    sky_images: None,
                    render_playback: None,
                    player_model_root: None,
                    actor_model_roots: &BTreeMap::new(),
                    environments: &BTreeMap::new(),
                },
                app.world(),
            )
    }
}
