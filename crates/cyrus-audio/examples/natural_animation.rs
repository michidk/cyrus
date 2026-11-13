//! Installed draw/sheath commands through production animation and sample owners.
//! The JSON fixture contains the player's exported animation metadata and WAVs;
//! it stays outside the repository. Nominal pacing comes from SYSTEM.INI.
use bevy::{
    app::ScheduleRunnerPlugin,
    audio::{AudioPlugin, AudioSource},
    prelude::*,
};
use cyrus_audio::sound::persistence::SamplePlaybackSnapshot;
use cyrus_audio::sound::{Effect, SampleRepetitions, SoundEffects, SoundPlugin};
use cyrus_script::script::{ScriptActorId, ScriptBinding};
use cyrus_soup::AnimationGroup;
use cyrus_world::{
    animation::{ActorAnimation, AnimationSound, Animations, TransitionDecision},
    sound::{ScriptSound, SoundCue},
};
use std::time::Duration;

#[derive(serde::Deserialize)]
struct Fixture {
    animations: Animations,
    system: String,
    rate: u32,
    effects: Vec<InstalledEffect>,
}
#[derive(serde::Deserialize)]
struct InstalledEffect {
    index: usize,
    wav: Vec<u8>,
    bits_per_sample: u16,
    sample_rate: u32,
    repetitions: SampleRepetitions,
    base_volume: u8,
}
#[derive(Resource)]
struct Probe {
    animation: ActorAnimation,
    actor: Entity,
    next: f64,
    interval: f64,
    frame: u32,
    phase: u8,
    replacement: bool,
}
fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    let text = std::fs::read_to_string(
        std::env::var("CYRUS_AUDIO_FIXTURE").expect("explicit installed fixture required"),
    )
    .unwrap();
    #[cfg(target_arch = "wasm32")]
    let text = fixture_text();
    let fixture: Fixture = serde_json::from_str(&text).unwrap();
    assert!(fixture.rate > 0);
    let animation =
        ActorAnimation::from_asset(&fixture.animations, "CYRUS", AnimationGroup::PANIC).unwrap();
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(5))),
        AssetPlugin::default(),
        bevy::log::LogPlugin::default(),
        AudioPlugin::default(),
        SoundPlugin,
    ))
    .insert_resource(cyrus_audio::AudioReadiness { prepare: false })
    .insert_resource(cyrus_audio::sound::SoundSettings::parse(&fixture.system))
    .insert_resource(cyrus_audio::AudioListener(GlobalTransform::default()))
    .insert_resource(cyrus_audio::ScriptListener(Some(
        cyrus_world::sound::ListenerPose {
            position: [0; 3],
            yaw: 0,
        },
    )));
    let mut effects = (0..=40).map(|_| None).collect::<Vec<_>>();
    for effect in fixture.effects {
        let source = app
            .world_mut()
            .resource_mut::<Assets<AudioSource>>()
            .add(AudioSource {
                bytes: effect.wav.into(),
            });
        effects[effect.index] = Some(Effect {
            audio: source,
            bits_per_sample: effect.bits_per_sample,
            sample_rate: effect.sample_rate,
            repetitions: effect.repetitions,
            base_volume: effect.base_volume,
        });
    }
    let actor = app
        .world_mut()
        .spawn((
            GlobalTransform::default(),
            cyrus_world::sound::PlayerAnimationSource,
            ScriptBinding {
                actor: ScriptActorId {
                    generation: 1,
                    index: 0,
                },
                source: Handle::default(),
            },
        ))
        .id();
    app.insert_resource(SoundEffects(effects))
        .insert_resource(Probe {
            animation,
            actor,
            next: 2.0,
            interval: 1.0 / f64::from(fixture.rate),
            frame: 0,
            phase: 0,
            replacement: false,
        })
        .add_systems(Update, drive.before(cyrus_audio::PlayCues))
        .add_systems(Last, observe);
    app.run();
}
fn drive(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    assert!(now < 30.0, "bounded installed animation fixture expired");
    if now < world.resource::<Probe>().next {
        // Ordinary host updates also advance with zero nominal ticks. They poll
        // completed samples while retaining the installed frame countdown.
        world.resource_scope(|world, mut probe: Mut<Probe>| {
            if probe.phase == 1 || probe.phase == 3 {
                let mut sounds = Vec::new();
                probe
                    .animation
                    .advance_with_decisions(0, &mut |sound| {
                        sounds.push(sound);
                        Ok(TransitionDecision::Continue)
                    })
                    .unwrap();
                for sound in sounds {
                    emit(world, &probe, sound);
                }
            }
        });
        return;
    }
    world.resource_scope(|world, mut probe: Mut<Probe>| {
        if probe.phase == 0 || probe.phase == 2 {
            probe.frame = 0;
            let group = if probe.phase == 0 {
                AnimationGroup::DRAW_SWORD
            } else {
                AnimationGroup::SHEATH_SWORD
            };
            let mut sounds = Vec::new();
            probe.animation.force_request_with_decisions(group, 0, 1, &mut |sound| {
                sounds.push(sound);
                Ok(TransitionDecision::Continue)
            }).unwrap();
            for sound in sounds {
                emit(world, &probe, sound);
            }
            probe.phase += 1;
            report(&serde_json::json!({"natural_animation":true,"phase":probe.phase,"kind":"begin","group":group.index(),"seconds":now}).to_string());
        } else if probe.phase == 1 || probe.phase == 3 {
            probe.frame += 1;
            let mut sounds = Vec::new();
            probe.animation.advance_with_decisions(1, &mut |sound| {
                sounds.push(sound);
                Ok(TransitionDecision::Continue)
            }).unwrap();
            for sound in sounds {
                emit(world, &probe, sound);
            }
            if probe.animation.current_group() == Some(AnimationGroup::PANIC) {
                report(&serde_json::json!({"natural_animation":true,"phase":probe.phase,"kind":"end","frame":probe.frame,"seconds":now}).to_string());
                probe.phase += 1;
                probe.next = now + 2.0;
                return;
            }
        } else if probe.phase == 4 {
            assert!(probe.replacement, "installed sheath never completed before EndAnimation");
            let count = world.query_filtered::<Entity, With<cyrus_audio::sound::PlayingSound>>()
                .iter(world).count();
            assert_eq!(count, 1, "completed sheath stopped the replacement owner");
            report(&serde_json::json!({"natural_animation":true,"kind":"replacement_survived","seconds":now}).to_string());
            world.write_message(cyrus_audio::sound::SoundRequest::StopAll);
            probe.phase = 5;
            probe.next = now + 2.0;
            return;
        } else {
            report(&serde_json::json!({"natural_animation":true,"kind":"complete","seconds":now}).to_string());
            world.write_message(AppExit::Success);
            return;
        }
        probe.next += probe.interval;
    });
}
fn emit(world: &mut World, probe: &Probe, sound: AnimationSound) {
    report(&serde_json::json!({"natural_animation":true,"kind":"command","phase":probe.phase,"frame":probe.frame,"sound":format!("{sound:?}")}).to_string());
    world.write_message(SoundCue::Animation {
        actor: probe.actor,
        sound,
        surface: Some(cyrus_assets::surface::Surface::UNKNOWN),
        position: Some([0; 3]),
    });
}
fn observe(world: &mut World) {
    let Ok(snapshot) = SamplePlaybackSnapshot::capture(world) else {
        return;
    };
    let snapshot = serde_json::to_value(snapshot).unwrap();
    let ownership = &snapshot["ownership"];
    let previous = world.get_resource::<Observation>().map(|p| &p.0);
    if previous != Some(ownership) {
        let p = world.resource::<Probe>();
        report(&serde_json::json!({"natural_animation":true,"kind":"ownership","phase":p.phase,"frame":p.frame,"seconds":world.resource::<Time<Real>>().elapsed_secs_f64(),"ownership":ownership,"players":snapshot["players"]}).to_string());
        world.insert_resource(Observation(ownership.clone()));
    }
    let p = world.resource::<Probe>();
    if p.phase == 3
        && !p.replacement
        && ownership["animation"].as_array().unwrap().is_empty()
        && p.frame >= 7
        && ownership["samples"]["slots"]
            .as_array()
            .unwrap()
            .iter()
            .all(serde_json::Value::is_null)
    {
        world.resource_mut::<Probe>().replacement = true;
        world.write_message(SoundCue::Script {
            generation: 1,
            index: 1,
            sound: ScriptSound::Positional {
                effect: 40,
                volume: 0,
                rate_offset_hz: 0,
                position: [0; 3],
                start_index: Some(0),
            },
        });
        report(&serde_json::json!({"natural_animation":true,"kind":"replacement_requested","seconds":world.resource::<Time<Real>>().elapsed_secs_f64()}).to_string());
    }
}
#[derive(Resource)]
struct Observation(serde_json::Value);
#[cfg(not(target_arch = "wasm32"))]
fn report(text: &str) {
    println!("{text}");
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_namespace=globalThis,js_name=captureFixture)]
    fn fixture_text() -> String;
    #[wasm_bindgen::prelude::wasm_bindgen(js_namespace=console,js_name=log)]
    fn report(text: &str);
}
