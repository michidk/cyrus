//! Read-only installed ground/animation/mixer probe. See the crate README for the fixture contract.
use anyhow::{Context, Result, ensure};
use bevy::{
    audio::{AudioPlayer, AudioSource, PlaybackSettings},
    prelude::{
        App, AssetApp, AssetPlugin, Assets, Entity, GlobalTransform, MinimalPlugins, Vec3, With,
    },
};
use cyrus_assets::{
    animation::Animations,
    install::GameInstall,
    native::Files,
    sound::{SoundSettings, SurfaceSounds},
    source::ReadAssets,
};
use cyrus_audio::{
    AudioListener,
    sound::{Effect, Panned, PlayingSound, SoundEffects, SoundPlugin},
};
use cyrus_collision::ground::SupportHistory;
use cyrus_game::player::{
    data::PlayerScene,
    grounded::{self, ForwardWalk, GroundedUpdate, GroundedWalk, StaticGroundQuery, SurfaceUpdate},
    motion::upright_rotation,
    placement::initial_animation,
    walk::WalkingStartup,
};
use cyrus_world::{
    animation::{ActorAnimation, AnimationSound},
    sound::{SoundCue, SoundSurface},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    world: u32,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    initial_surface: u8,
    collision_attribute: u8,
    group: u16,
    #[serde(default)]
    grounded: bool,
    #[serde(default)]
    entry_ticks: u32,
    steps: Vec<Step>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    position_q8: [i64; 3],
    view: [i32; 2],
    heading: u16,
    ticks: u32,
    /// Deliberately change ECS state after emission to exercise the snapshot contract.
    later_surface: Option<u8>,
    movement: Option<Movement>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Movement {
    frame_scale: u32,
    walk: bool,
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 4,
        "usage: surface_audio INSTALL PLAYER_CACHE RGM_OR_ANIMATION_CACHE FIXTURE_JSON"
    );
    let install = GameInstall::resolve(Path::new(&args[0]))?;
    let read = |path: &std::ffi::OsStr| {
        std::fs::read(path).with_context(|| format!("{}: read", Path::new(path).display()))
    };
    let scene = PlayerScene::decode(&read(&args[1])?)
        .with_context(|| format!("{}: player cache", Path::new(&args[1]).display()))?;
    let data = if Path::new(&args[2])
        .extension()
        .is_some_and(|ext| ext == "animations")
    {
        let data: Animations = serde_json::from_slice(&read(&args[2])?)
            .with_context(|| format!("{}: animation cache", Path::new(&args[2]).display()))?;
        data.validate()?;
        ensure!(
            data.world == scene.world,
            "animation world {} differs from player world {}",
            data.world,
            scene.world
        );
        data
    } else {
        let map = rgpre::import::rgm::parse_rgm_file(&read(&args[2])?)
            .with_context(|| format!("{}: RGM", Path::new(&args[2]).display()))?;
        Animations {
            version: 2,
            world: scene.world,
            metadata: rgpre::import::rgm::export_rgm_metadata_json(&map, None),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        }
    };
    let fixture: Fixture = serde_json::from_slice(&read(&args[3])?)
        .with_context(|| format!("{}: fixture", Path::new(&args[3]).display()))?;
    ensure!(
        fixture.world == scene.world,
        "fixture world {} differs from cache world {}",
        fixture.world,
        scene.world
    );
    ensure!(!fixture.cases.is_empty(), "fixture has no cases");
    let files = Files::open(&install)?;
    let surfaces = SurfaceSounds::parse(&files.text("SURFACE.INI")?).map_err(anyhow::Error::msg)?;
    let settings = SoundSettings::parse(&files.text("SYSTEM.INI")?);
    let sfx = rgpre::import::sfx::parse_sfx_file(&files.read("sound/MAIN.SFX")?)?;
    let bank: Vec<_> = sfx
        .effects
        .iter()
        .map(|effect| {
            cyrus_assets::audio::pcm_wav(effect.audio_type, effect.sample_rate, &effect.pcm_data)
                .map_err(anyhow::Error::msg)
        })
        .collect::<Result<_>>()?;
    let mut reports = Vec::new();
    let mut names = HashSet::new();
    for case in fixture.cases {
        ensure!(
            names.insert(case.name.clone()),
            "duplicate case {}",
            case.name
        );
        ensure!(!case.steps.is_empty(), "case {} has no steps", case.name);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), SoundPlugin))
            .init_asset::<AudioSource>()
            .init_asset::<Panned>()
            .insert_resource(settings.clone())
            .insert_resource(surfaces.clone())
            .insert_resource(AudioListener(GlobalTransform::default()));
        let effects =
            sfx.effects
                .iter()
                .zip(&bank)
                .map(|(effect, bytes)| {
                    Some(Effect {
                        audio: app.world_mut().resource_mut::<Assets<AudioSource>>().add(
                            AudioSource {
                                bytes: bytes.clone().into(),
                            },
                        ),
                        looping: effect.loop_flag != 0,
                        base_volume: effect.base_volume,
                        sample_rate: effect.sample_rate,
                    })
                })
                .collect();
        app.insert_resource(SoundEffects(effects));
        let actor = app
            .world_mut()
            .spawn((
                cyrus_game::player::Player,
                GlobalTransform::from_translation(Vec3::Z),
                SoundSurface(case.initial_surface),
            ))
            .id();
        if case.grounded {
            let steps = grounded_steps(&case, &scene, &data, (&mut app, actor, &bank))
                .with_context(|| format!("case {} grounded updates", case.name))?;
            reports.push(json!({"name": case.name, "grounded": true, "steps": steps}));
            continue;
        }
        ensure!(
            case.entry_ticks == 0,
            "case {}: entry_ticks requires grounded mode",
            case.name
        );
        let mut animation = ActorAnimation::from_asset(&data, "CYRUS", 0)?;
        let mut history = SupportHistory::new(0, 0);
        let mut surface = case.initial_surface;
        let mut veto = 0;
        let mut steps = Vec::new();
        for (index, step) in case.steps.into_iter().enumerate() {
            ensure!(
                step.movement.is_none(),
                "case {} step {index}: movement requires grounded mode",
                case.name
            );
            ensure!(
                step.ticks <= 1024,
                "case {} step {index}: ticks exceed 1024",
                case.name
            );
            let candidates = scene.candidates(step.view, step.position_q8, None)?;
            let query = StaticGroundQuery {
                scene: &scene,
                candidates: &candidates,
                rotation: upright_rotation(step.heading),
                collision_attribute: case.collision_attribute,
                alternate_collision: false,
                sidedness: false,
                surface_update: SurfaceUpdate::Assign,
            };
            let prior = surface;
            let support = query
                .support(step.position_q8, &mut history, &mut surface, &mut veto)
                .with_context(|| format!("case {} step {index}", case.name))?;
            let mut cues = Vec::new();
            if index == 0 {
                animation.force_request_with_sounds(case.group, 0, 0, &mut |sound| {
                    cues.push(sound);
                    Ok(())
                })?;
            }
            animation.advance_with_sounds(step.ticks, &mut |sound| {
                cues.push(sound);
                Ok(())
            })?;
            let mut events = Vec::new();
            for sound in cues {
                events.push(play(
                    &mut app,
                    actor,
                    sound,
                    surface,
                    step.later_surface,
                    &bank,
                )?);
            }
            steps.push(json!({"position_q8": step.position_q8, "prior_surface": prior,
                "surface": surface, "veto": veto, "support": support.map(|s| format!("{:?}", s.source())), "events": events}));
        }
        reports.push(json!({"name": case.name, "steps": steps}));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"version": 1, "world": scene.world, "cases": reports})
        )?
    );
    Ok(())
}

fn grounded_steps(
    case: &Case,
    scene: &PlayerScene,
    data: &Animations,
    mixer: (&mut App, Entity, &[Vec<u8>]),
) -> Result<Vec<Value>> {
    ensure!(case.group == 0, "grounded entry requires group 0");
    ensure!(case.entry_ticks <= 1024, "entry_ticks exceeds 1024");
    let first = case.steps.first().context("no grounded steps")?;
    let definition = scene
        .definition
        .as_ref()
        .context("player definition missing")?;
    let mut walking = GroundedWalk {
        position: first.position_q8,
        surface: case.initial_surface,
        startup: WalkingStartup::new(definition.step_type)
            .context("unsupported walking step type")?,
    };
    let (app, actor, bank) = mixer;
    let translation = |position: [i64; 3]| {
        let [x, y, z] = position.map(|v| v as f32 / (256.0 * cyrus_game::ENGINE_UNIT_SCALE));
        Vec3::new(-x, -y, z)
    };
    app.insert_resource(AudioListener(GlobalTransform::from_translation(
        translation(walking.position),
    )));
    let mut pending = Vec::new();
    let (mut animation, mut frame) = initial_animation(data, case.entry_ticks, &mut |sound| {
        pending.push((sound, walking.surface, "entry"));
        Ok(())
    })?;
    let mut history = SupportHistory::new(0, 0);
    let mut veto = 0;
    let mut heading = first.heading;
    let mut reports = Vec::new();
    for (index, step) in case.steps.iter().enumerate() {
        let context = format!("step {index}");
        ensure!(step.ticks <= 1024, "{context}: ticks exceeds 1024");
        ensure!(
            step.position_q8 == walking.position,
            "{context}: expected input position {:?}, actual {:?}",
            step.position_q8,
            walking.position
        );
        ensure!(
            step.heading == heading && heading < 2048,
            "{context}: expected input heading {}, actual {heading}",
            step.heading
        );
        let movement = step
            .movement
            .as_ref()
            .with_context(|| format!("{context}: movement missing"))?;
        let prior = walking.surface;
        frame = grounded::advance_animation(
            scene,
            step.ticks,
            &mut history,
            &mut animation,
            &mut |sound| {
                pending.push((sound, prior, "ordinary_animation"));
                Ok(())
            },
        )
        .with_context(|| context.clone())?
        .or(frame);
        let model_frame = frame
            .map(|reference| data.resolve_frame("CYRUS", reference))
            .transpose()
            .with_context(|| {
                format!("{context}: active model requires a complete animation cache")
            })?;
        let candidates = scene.candidates(
            step.view,
            walking.position,
            model_frame.as_ref().map(|f| f.model.as_str()),
        )?;
        let query = StaticGroundQuery {
            scene,
            candidates: &candidates,
            rotation: upright_rotation(heading),
            collision_attribute: case.collision_attribute,
            alternate_collision: false,
            sidedness: false,
            surface_update: SurfaceUpdate::Assign,
        };
        let mut input = GroundedUpdate {
            motion: ForwardWalk {
                heading,
                frame_scale: movement.frame_scale,
                nominal_ticks: step.ticks,
            },
            walk: movement.walk,
        };
        frame = walking
            .move_grounded(
                &query,
                &mut input,
                (&mut history, &mut veto),
                &mut animation,
                &mut |sound, surface| {
                    pending.push((sound, surface, "forward_animation"));
                    Ok(())
                },
            )
            .with_context(|| context.clone())?
            .or(frame);
        heading = input.motion.heading;
        app.world_mut()
            .entity_mut(actor)
            .insert(GlobalTransform::from_translation(translation(
                walking.position,
            )));
        let mut events = Vec::new();
        for (sound, surface, stage) in pending.drain(..) {
            let mut event = play(app, actor, sound, surface, step.later_surface, bank)?;
            event["stage"] = json!(stage);
            // Player runtime emits no position snapshot and resolves its final scene transform.
            event["captured_position_q8"] = Value::Null;
            events.push(event);
        }
        reports.push(json!({"position_q8": walking.position, "input_position_q8": step.position_q8,
            "heading": heading, "input_heading": step.heading, "ticks": step.ticks,
            "frame_scale": movement.frame_scale, "walk": movement.walk,
            "prior_surface": prior, "surface": walking.surface, "veto": veto,
            "frame_reference": frame, "group": animation.current_group(), "pending_group": animation.pending_group(), "events": events}));
    }
    Ok(reports)
}

fn play(
    app: &mut App,
    actor: Entity,
    sound: AnimationSound,
    surface: u8,
    later: Option<u8>,
    bank: &[Vec<u8>],
) -> Result<Value> {
    let before: HashSet<_> = app
        .world_mut()
        .query_filtered::<Entity, With<PlayingSound>>()
        .iter(app.world())
        .collect();
    let mut event = match sound {
        AnimationSound::Play {
            effect,
            rate_offset_hz,
        } => json!({"kind": "play", "authored": effect, "rate_offset_hz": rate_offset_hz}),
        AnimationSound::Stop => json!({"kind": "stop"}),
        AnimationSound::Forget => json!({"kind": "forget"}),
    };
    app.world_mut().write_message(SoundCue::Animation {
        position: None,
        actor,
        sound,
        surface: Some(surface),
    });
    app.world_mut()
        .entity_mut(actor)
        .insert(SoundSurface(later.unwrap_or(surface)));
    app.update();
    let players: Vec<_> = app
        .world_mut()
        .query::<(Entity, &AudioPlayer<Panned>, &PlaybackSettings)>()
        .iter(app.world())
        .filter(|(entity, _, _)| !before.contains(entity))
        .map(|(_, player, settings)| (player.0.clone(), settings.speed))
        .collect();
    let mut started = Vec::new();
    for (handle, speed) in players {
        let pcm = &app
            .world()
            .resource::<Assets<Panned>>()
            .get(&handle)
            .context("started PCM missing")?
            .source
            .bytes;
        let targets: Vec<_> = bank
            .iter()
            .enumerate()
            .filter(|(_, bytes)| bytes.as_slice() == pcm.as_ref())
            .map(|(id, _)| id)
            .collect();
        ensure!(
            !targets.is_empty(),
            "mixer PCM does not match any installed bank entry"
        );
        started.push(json!({"target_candidates": targets, "speed": speed}));
    }
    let remaining = app
        .world_mut()
        .query_filtered::<Entity, With<PlayingSound>>()
        .iter(app.world())
        .count();
    event["captured_surface"] = json!(surface);
    event["started"] = json!(started);
    event["remaining"] = json!(remaining);
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrus_assets::{collision::CollisionScene, player::MovementSettings};
    use cyrus_collision::{collision_world::StaticCollision, ground::TerrainGround};

    #[test]
    fn grounded_report_keeps_each_stage_surface_and_rejects_a_discontinuous_path() {
        let data: Animations = serde_json::from_value(json!({
            "version":2, "world":1, "sections":{}, "unsupported":{},
            "models":{"TEST":{"radius":1.0,"base_positions":[],"base_normals":[],"faces":[],
                "encoding":"static_3d","positions":[[],[]],"normals":[[],[]]}},
            "metadata":{"actors":[{"script_name":"CYRUS","attributes":{
                "attr_forward_walk":8,"attr_forward_speed":24,"attr_step_type":1,"attr_panic_group":3},
                "mpsz_bounds_primary":0,"raex":{"unknown_12":256},
                "raan_entries":[{"frame_count":2,"file_path":"TEST.3D"}],
                "animation_groups":[
                    {"group_index":0,"anim_type":0,"anim_id":1,"commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]},
                    {"group_index":3,"anim_type":0,"anim_id":1,"commands":[{"opcode":4,"sound_param":200,"volume_shift":0},{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]},
                    {"group_index":20,"anim_type":0,"anim_id":1,"commands":[{"opcode":4,"sound_param":200,"volume_shift":0},{"opcode":0,"frame_reference":1},{"opcode":2,"value":0}]}
                ]}],"mpsz_entries":[{"total_extent":[48,128,49],"pos_extent":[24,62,11],
                    "neg_extent":[24,66,38],"center_offset":[0,0,0],"flags":0}]}
        })).unwrap();
        data.validate().unwrap();
        let collision = CollisionScene {
            texture_surfaces: Some(std::collections::BTreeMap::from([((302 << 7) | 25, 6)])),
            ..Default::default()
        };
        let scene = PlayerScene {
            world: 1,
            model_radii: std::collections::BTreeMap::from([("TEST".into(), 1.0)]),
            keyboard_codes: None,
            actor_collision: None,
            unavailable: None,
            settings: MovementSettings {
                standing_height: -2,
                post_collide_height: 50,
                slide_range: -60000,
                turn_speed: 12,
                turn_max_speed: 48,
                poly_push_units: 16,
            },
            definition: Some(
                cyrus_game::player::definition::PlayerDefinition::from_metadata(&data.metadata)
                    .unwrap(),
            ),
            terrain: Some(
                TerrainGround::new(
                    vec![0; 256 * 256],
                    vec![false; 256 * 256],
                    Some(vec![25; 256 * 256]),
                )
                .unwrap(),
            ),
            static_collision: StaticCollision::new(&collision).unwrap(),
            collision,
        };
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), SoundPlugin))
            .init_asset::<AudioSource>()
            .init_asset::<Panned>()
            .insert_resource(SoundSettings::parse("[system]\nsound_distance=64"))
            .insert_resource(SurfaceSounds::parse("[surfaces]\n[sand]\n200=1").unwrap());
        let bank: Vec<_> = (0..=200)
            .map(|id| {
                cyrus_assets::audio::pcm_wav(
                    rgpre::import::sfx::AudioType::Mono8,
                    11025,
                    &[id as u8; 16],
                )
                .unwrap()
            })
            .collect();
        let effects =
            bank.iter()
                .enumerate()
                .map(|(id, bytes)| {
                    matches!(id, 1 | 200).then(|| Effect {
                        audio: app.world_mut().resource_mut::<Assets<AudioSource>>().add(
                            AudioSource {
                                bytes: bytes.clone().into(),
                            },
                        ),
                        looping: false,
                        base_volume: 255,
                        sample_rate: 11025,
                    })
                })
                .collect();
        app.insert_resource(SoundEffects(effects));
        let actor = app
            .world_mut()
            .spawn((
                cyrus_game::player::Player,
                GlobalTransform::default(),
                SoundSurface(0),
            ))
            .id();
        let mut case: Case = serde_json::from_value(json!({
            "name":"synthetic","initial_surface":0,"collision_attribute":6,"group":0,"grounded":true,
            "steps":[
                {"position_q8":[131072,-16384,131072],"view":[512,512],"heading":0,"ticks":0,
                    "movement":{"walk":true,"frame_scale":256},"later_surface":11},
                {"position_q8":[131072,-16384,131584],"view":[512,512],"heading":0,"ticks":1,
                    "movement":{"walk":false,"frame_scale":256},"later_surface":11}
            ]
        })).unwrap();
        let report = grounded_steps(&case, &scene, &data, (&mut app, actor, &bank)).unwrap();
        assert_eq!(report[0]["position_q8"], json!([131072, -16384, 131584]));
        assert_eq!(report[1]["position_q8"], json!([131072, -16384, 132266]));
        let events = report[0]["events"].as_array().unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0]["stage"], "ordinary_animation");
        assert_eq!(events[0]["captured_surface"], 0);
        assert_eq!(events[0]["started"][0]["target_candidates"], json!([200]));
        assert_eq!(events[1]["stage"], "forward_animation");
        assert_eq!(events[1]["kind"], "forget");
        assert_eq!(events[1]["captured_surface"], 6);
        assert_eq!(events[1]["started"], json!([]));
        assert_eq!(events[1]["remaining"], 1);
        assert_eq!(events[2]["stage"], "forward_animation");
        assert_eq!(events[2]["captured_surface"], 6);
        assert_eq!(events[2]["started"][0]["target_candidates"], json!([1]));
        assert_eq!(events[2]["captured_position_q8"], Value::Null);
        assert_eq!(events[2]["started"][0]["speed"], 1.0);
        case.steps[1].position_q8[2] += 1;
        let error = grounded_steps(&case, &scene, &data, (&mut app, actor, &bank)).unwrap_err();
        assert!(format!("{error:#}").contains("step 1: expected input position"));
    }
}
