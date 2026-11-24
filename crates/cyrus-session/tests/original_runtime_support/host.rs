use super::*;

type LoadedWorlds = BTreeMap<
    u32,
    (
        cyrus_world::lifecycle::PreparedWorld,
        bevy::prelude::Handle<PlayerScene>,
    ),
>;
pub struct HostFixture {
    pub app: App,
    pub scene: bevy::prelude::Handle<PlayerScene>,
    pub animation: bevy::prelude::Handle<Animations>,
    pub loaded_worlds: LoadedWorlds,
}

pub fn prepare_host(
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
    let loaded_worlds = register_assets(&mut app, sources, player_bytes, id)?;
    let animation = loaded_worlds[&id].0.animations.clone();
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
    spawn_actors(&mut app, &animation, player);
    Ok(HostFixture {
        app,
        scene,
        animation,
        loaded_worlds,
    })
}

fn register_assets(
    app: &mut App,
    sources: &BTreeMap<u32, WorldSnapshotSources<'_>>,
    player_bytes: &BTreeMap<u32, Vec<u8>>,
    id: u32,
) -> Result<LoadedWorlds> {
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
    Ok(loaded_worlds)
}
fn spawn_actors(
    app: &mut App,
    animation: &bevy::prelude::Handle<Animations>,
    player: ScriptActorId,
) {
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
}
