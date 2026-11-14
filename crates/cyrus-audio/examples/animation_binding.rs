//! Installed PCM through the production pre-binding animation sound router.
use bevy::{
    app::ScheduleRunnerPlugin,
    audio::{AudioPlugin, AudioSource},
    prelude::*,
};
use cyrus_audio::{
    AudioListener,
    sound::{Effect, PlayingSound, SampleRepetitions, SoundEffects, SoundPlugin},
};
use cyrus_script::script::{ScriptActorId, ScriptSound, sound_dispatch::AnimationSoundQueue};
use cyrus_world::{animation::AnimationSound, sound::SoundCue};
use std::time::Duration;

#[derive(serde::Deserialize)]
struct Fixture {
    system: String,
    wav: Vec<u8>,
    bits: u16,
    sample_rate: u32,
    loop_flag: i8,
    effect: usize,
}
#[derive(Resource)]
struct Probe {
    queue: AnimationSoundQueue,
    nodes: [Entity; 2],
    phase: usize,
    next: f64,
    effect: i16,
}
const COUNTS: [usize; 7] = [1, 0, 1, 0, 0, 1, 0];
fn actor(index: usize) -> ScriptActorId {
    ScriptActorId {
        generation: 1,
        index,
    }
}
fn play(effect: i16) -> ScriptSound {
    ScriptSound::Animation {
        sound: AnimationSound::Play {
            effect,
            rate_offset_hz: 0,
        },
        surface: Some(cyrus_assets::surface::Surface::UNKNOWN),
        position: Some([0; 3]),
    }
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
    assert!(
        SampleRepetitions::from_sfx_flag(fixture.loop_flag).is_infinite(),
        "installed fixture must loop"
    );
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(10))),
        AssetPlugin::default(),
        AudioPlugin::default(),
        SoundPlugin,
    ))
    .insert_resource(cyrus_audio::AudioReadiness { prepare: false })
    .insert_resource(cyrus_assets::sound::SoundSettings::parse(&fixture.system))
    .insert_resource(AudioListener(GlobalTransform::default()))
    .insert_resource(cyrus_audio::ScriptListener(Some(
        cyrus_world::sound::ListenerPose {
            position: [0; 3],
            yaw: 0,
        },
    )));
    let audio = app
        .world_mut()
        .resource_mut::<Assets<AudioSource>>()
        .add(AudioSource {
            bytes: fixture.wav.into(),
        });
    let mut effects = (0..=fixture.effect).map(|_| None).collect::<Vec<_>>();
    effects[fixture.effect] = Some(Effect {
        audio,
        bits_per_sample: fixture.bits,
        repetitions: SampleRepetitions::from_sfx_flag(fixture.loop_flag),
        base_volume: 64,
        sample_rate: fixture.sample_rate,
    });
    let nodes = std::array::from_fn(|index| {
        app.world_mut()
            .spawn((
                GlobalTransform::default(),
                cyrus_script::script::ScriptBinding {
                    actor: actor(index),
                    source: Handle::default(),
                },
            ))
            .id()
    });
    app.insert_resource(SoundEffects(effects))
        .insert_resource(Probe {
            queue: Default::default(),
            nodes,
            phase: 0,
            next: 2.0,
            effect: fixture.effect.try_into().unwrap(),
        })
        .add_systems(Update, step.before(cyrus_audio::PlayCues));
    app.run();
}
fn step(
    time: Res<Time<Real>>,
    mut probe: ResMut<Probe>,
    mut cues: MessageWriter<SoundCue>,
    playing: Query<(), With<PlayingSound>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    if time.elapsed_secs_f64() < probe.next {
        return;
    }
    if probe.phase > 0 {
        assert_eq!(
            playing.iter().count(),
            COUNTS[probe.phase - 1],
            "stopped pre-binding start leaked into output"
        );
    }
    if probe.phase == COUNTS.len() {
        exit.write(AppExit::Success);
        return;
    }
    let phase = probe.phase;
    let effect = probe.effect;
    if phase == 5 {
        commands.entity(probe.nodes[0]).despawn();
        probe.nodes[0] = commands
            .spawn((
                GlobalTransform::default(),
                cyrus_script::script::ScriptBinding {
                    actor: actor(0),
                    source: Handle::default(),
                },
            ))
            .id();
    }
    let nodes = probe.nodes;
    let mut emit = |cue| {
        cues.write(cue);
        Ok(())
    };
    let queue = &mut probe.queue;
    match phase {
        0 | 2 | 4 => {
            queue.send(actor(0), play(effect), None, &mut emit).unwrap();
            queue
                .send(
                    actor(0),
                    ScriptSound::Animation {
                        sound: AnimationSound::Forget,
                        surface: None,
                        position: None,
                    },
                    None,
                    &mut emit,
                )
                .unwrap();
            queue.send(actor(1), play(effect), None, &mut emit).unwrap();
            let stop = match phase {
                0 => ScriptSound::StopOwned,
                2 => ScriptSound::StopEffect {
                    effect: i32::from(effect),
                    owned: true,
                },
                _ => ScriptSound::StopAll,
            };
            queue.send(actor(0), stop, None, &mut emit).unwrap();
            queue
                .release(1, &mut |id| Ok(Some(nodes[id.index])), &mut emit)
                .unwrap();
            assert!(queue.is_empty());
        }
        1 | 3 | 6 => queue
            .send(actor(0), ScriptSound::StopAll, None, &mut emit)
            .unwrap(),
        5 => {
            queue.send(actor(0), play(effect), None, &mut emit).unwrap();
            queue
                .release(1, &mut |id| Ok(Some(nodes[id.index])), &mut emit)
                .unwrap();
        }
        _ => unreachable!(),
    }
    report(&serde_json::json!({"animation_binding":true,"phase":phase,"expected_channels":COUNTS[phase],"nominal_updates":phase+1}).to_string());
    probe.phase += 1;
    probe.next = time.elapsed_secs_f64() + 3.0;
}
#[cfg(not(target_arch = "wasm32"))]
fn report(text: &str) {
    println!("{text}");
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_namespace=globalThis, js_name=captureFixture)]
    fn fixture_text() -> String;
    #[wasm_bindgen::prelude::wasm_bindgen(js_namespace=console, js_name=log)]
    fn report(text: &str);
}
