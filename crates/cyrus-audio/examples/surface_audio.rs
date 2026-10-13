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
    grounded::{StaticGroundQuery, SurfaceUpdate},
    motion::upright_rotation,
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
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 4,
        "usage: surface_audio INSTALL PLAYER_CACHE RGM FIXTURE_JSON"
    );
    let install = GameInstall::resolve(Path::new(&args[0]))?;
    let read = |path: &std::ffi::OsStr| {
        std::fs::read(path).with_context(|| format!("{}: read", Path::new(path).display()))
    };
    let scene = PlayerScene::decode(&read(&args[1])?)
        .with_context(|| format!("{}: player cache", Path::new(&args[1]).display()))?;
    let map = rgpre::import::rgm::parse_rgm_file(&read(&args[2])?)
        .with_context(|| format!("{}: RGM", Path::new(&args[2]).display()))?;
    let data = Animations {
        version: 2,
        world: scene.world,
        metadata: rgpre::import::rgm::export_rgm_metadata_json(&map, None),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
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
        let mut animation = ActorAnimation::from_asset(&data, "CYRUS", 0)?;
        let mut history = SupportHistory::new(0, 0);
        let mut surface = case.initial_surface;
        let mut veto = 0;
        let mut steps = Vec::new();
        for (index, step) in case.steps.into_iter().enumerate() {
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
