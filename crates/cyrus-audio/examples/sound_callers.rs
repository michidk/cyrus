//! Isolated public SOUP caller acceptance with real native/browser audio output.
#[path = "../tests/support/sound_callers.rs"]
mod fixture;
use bevy::{
    app::ScheduleRunnerPlugin,
    audio::{AudioPlugin, AudioSource},
    prelude::*,
};
use cyrus_audio::sound::{Effect, SoundEffects, SoundPlugin};
use cyrus_script::script::{ScriptInputs, ScriptWorld};
use cyrus_world::sound::SoundCue;
use std::time::Duration;

#[derive(Resource)]
struct Probe {
    scripts: ScriptWorld,
    random: cyrus_game::random::EngineRandom,
    phase: usize,
    shells: bool,
    next: f64,
}
const COUNTS: [usize; 9] = [4, 3, 2, 2, 2, 0, 1, 0, 0];

fn main() {
    std::panic::set_hook(Box::new(|info| {
        report_line(&format!("sound caller panic: {info}"))
    }));
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(10))),
        AssetPlugin::default(),
        bevy::log::LogPlugin::default(),
        AudioPlugin::default(),
        SoundPlugin,
    ))
    .insert_resource(cyrus_audio::AudioReadiness { prepare: false })
    .insert_resource(cyrus_assets::sound::SoundSettings::parse(
        "[system]\nsound_distance=64",
    ));
    let effects = [440.0, 660.0]
        .into_iter()
        .map(|frequency| {
            // Synthetic periodic PCM makes cancellation and continuation observable without game data.
            let pcm: Vec<_> = (0..22050)
                .map(|i| {
                    (128.0 + 48.0 * (i as f32 * frequency * std::f32::consts::TAU / 22050.0).sin())
                        as u8
                })
                .collect();
            let mut wav = std::io::Cursor::new(Vec::new());
            let mut writer = hound::WavWriter::new(
                &mut wav,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 22050,
                    bits_per_sample: 8,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for sample in pcm {
                writer
                    .write_sample((i16::from(sample) - 128) as i8)
                    .unwrap();
            }
            writer.finalize().unwrap();
            let bytes = wav.into_inner();
            let audio = app
                .world_mut()
                .resource_mut::<Assets<AudioSource>>()
                .add(AudioSource {
                    bytes: bytes.into(),
                });
            Some(Effect {
                audio,
                bits_per_sample: 8,
                base_volume: 64,
                sample_rate: 22050,
                repetitions: cyrus_audio::sound::SampleRepetitions::FOREVER,
            })
        })
        .collect();
    app.insert_resource(SoundEffects(effects))
        .insert_resource(Probe {
            scripts: fixture::separate_sound_world(true),
            random: Default::default(),
            phase: 0,
            shells: true,
            next: 2.0,
        })
        .add_systems(Update, step.before(cyrus_audio::PlayCues));
    app.run();
}

fn step(
    time: Res<Time<Real>>,
    mut probe: ResMut<Probe>,
    mut cues: MessageWriter<SoundCue>,
    mut exit: MessageWriter<AppExit>,
) {
    if time.elapsed_secs_f64() < probe.next {
        return;
    }
    if probe.phase == COUNTS.len() {
        if probe.shells {
            probe.shells = false;
            probe.scripts = fixture::separate_sound_world(false);
            probe.phase = 0;
        } else {
            exit.write(AppExit::Success);
            return;
        }
    }
    let Probe {
        scripts,
        random,
        shells,
        phase,
        ..
    } = &mut *probe;
    let mut emitted = Vec::new();
    let mut emit = |actor: cyrus_script::script::ScriptActorId, sound| {
        emitted.push(SoundCue::Script {
            generation: actor.generation,
            index: actor.index,
            sound,
        });
        Ok(())
    };
    let inputs = ScriptInputs {
        nominal_ticks: Some(1),
        frame_scale: Some(256),
        ..Default::default()
    };
    if *shells {
        scripts
            .update_shells(inputs, random, &mut emit, 100)
            .unwrap();
    } else {
        scripts
            .update_separate([600, 600], inputs, random, &mut emit, 100)
            .unwrap();
    }
    for actor in scripts.actors() {
        if *phase >= 7 && actor.name() == "KEEPER" {
            assert!(
                actor
                    .failure()
                    .unwrap()
                    .contains("has no supported numeric result")
            );
        } else {
            assert!(actor.failure().is_none(), "{:?}", actor.failure());
        }
    }
    if *phase >= 5 {
        assert_eq!(scripts.flags(), [7]);
    }
    assert_eq!(emitted.len(), [4, 1, 1, 2, 0, 1, 1, 1, 0][*phase]);
    let report = serde_json::json!({"sound_callers":true,"shells":*shells,"phase":*phase,
        "nominal_updates":*phase+1,"expected_channels":COUNTS[*phase],"emitted":emitted.len(),"flags":scripts.flags(),
        "numeric_rejected":*phase>=7}).to_string();
    for cue in emitted {
        cues.write(cue);
    }
    report_line(&report);
    probe.phase += 1;
    probe.next = time.elapsed_secs_f64() + 3.0;
}
#[cfg(not(target_arch = "wasm32"))]
fn report_line(value: &str) {
    println!("{value}");
}
#[cfg(target_arch = "wasm32")]
fn report_line(value: &str) {
    #[wasm_bindgen::prelude::wasm_bindgen]
    extern "C" {
        #[wasm_bindgen::prelude::wasm_bindgen(js_namespace=console, js_name=log)]
        fn report(value: &str);
    }
    report(value);
}
