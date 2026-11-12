//! Installed attachment-family probe through public SOUP calls and real audio output.
use bevy::{
    app::ScheduleRunnerPlugin,
    audio::{AudioPlugin, AudioSource},
    prelude::*,
};
use cyrus_audio::{
    AudioListener,
    sound::{Effect, SampleRepetitions, SoundEffects, SoundPlugin},
};
use cyrus_script::script::{ScriptInputs, ScriptStage, ScriptWorld};
#[cfg(not(target_arch = "wasm32"))]
use cyrus_soup::Native;
#[cfg(not(target_arch = "wasm32"))]
use cyrus_soup::program::{Call, CallKind, Object, Operation, Program, Step, Value};
use cyrus_soup::{Attribute, Thread};
use cyrus_world::sound::{ScriptSound, SoundCue};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Serialize, Deserialize)]
struct Fixture {
    scripts: cyrus_assets::script::Scripts,
    system: String,
    wav: Vec<u8>,
    sample_rate: u32,
    bits: u16,
    loop_flag: i8,
    source_rms: f64,
    effect: usize,
}
#[cfg(not(target_arch = "wasm32"))]
fn program(operations: Vec<Operation>) -> Program {
    Program {
        version: 4,
        entry: 0,
        variables: vec![],
        strings: vec![],
        steps: operations
            .into_iter()
            .enumerate()
            .map(|(i, operation)| {
                (
                    i as u32,
                    Step {
                        next: i as u32 + 1,
                        operation,
                    },
                )
            })
            .collect(),
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn call(function: Native, function_id: u16, arguments: &[i32]) -> Operation {
    Operation::Call {
        call: Call {
            function_id,
            function,
            kind: CallKind::Function,
            target: Object::Me,
            arguments: arguments
                .iter()
                .map(|&value| Value::Integer { value })
                .collect(),
        },
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn export(root: &std::path::Path, out: &std::path::Path) -> anyhow::Result<()> {
    use anyhow::ensure;
    let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(root.join("maps/ISLAND.RGM"))?)?;
    let mut scripts = cyrus_assets::script_export::prepare(&map, root, 0)?;
    let child = scripts
        .instances
        .iter()
        .find(|i| i.actor == "DR_SPIN6")
        .ok_or_else(|| anyhow::anyhow!("ISLAND DR_SPIN6 absent"))?
        .clone();
    let actor = &scripts.actors[&child.actor];
    let group = actor.attributes[Attribute::Slave];
    ensure!(group != 0, "DR_SPIN6 has no installed family");
    let effect = actor
        .program
        .steps
        .values()
        .find_map(|step| match &step.operation {
            Operation::Call { call } if call.function == Native::AmbientSound => {
                match call.arguments.first() {
                    Some(Value::Integer { value }) => usize::try_from(*value).ok(),
                    _ => None,
                }
            }
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("DR_SPIN6 authored AmbientSound effect absent"))?;
    let parent = scripts
        .instances
        .iter()
        .find(|i| {
            scripts.actors.get(&i.actor).is_some_and(|a| {
                a.attributes[Attribute::Master] == group && a.attributes[Attribute::Slave] == 0
            })
        })
        .ok_or_else(|| anyhow::anyhow!("DR_SPIN6 root master absent"))?
        .clone();
    scripts.instances = vec![child.clone(), parent.clone()];
    scripts.shells.clear();
    scripts.unsupported.clear();
    scripts
        .actors
        .retain(|name, _| name == &child.actor || name == &parent.actor);
    scripts.definition.functions = [
        (Native::NullFunction, 0),
        (Native::Sound, 3),
        (Native::SetPosition, 3),
    ]
    .into_iter()
    .map(|(name, param_count)| cyrus_soup::program::Function {
        name,
        param_count,
        kind: CallKind::Function,
    })
    .collect();
    let mut moved = parent.map_position.map(|x| (x * 256) as i32);
    moved[0] += 512 * 256;
    for (name, operations) in [
        (
            &child.actor,
            vec![
                call(Native::Sound, 1, &[effect as i32, 0, 0]),
                Operation::End { target: 0 },
            ],
        ),
        (
            &parent.actor,
            vec![
                call(Native::SetPosition, 2, &moved),
                Operation::End { target: 0 },
            ],
        ),
    ] {
        let actor = scripts.actors.get_mut(name).unwrap();
        actor.instances = 1;
        actor.instance_variables = vec![vec![]];
        actor.native_hook = None;
        actor.program = program(operations);
    }
    scripts.content_id = scripts.content_identity()?;
    let sfx = rgpre::import::sfx::parse_sfx_file(&std::fs::read(root.join("sound/MAIN.SFX"))?)?;
    let sound = &sfx.effects[effect];
    ensure!(
        sound.audio_type.channels() == 1,
        "fixture requires mono installed effect"
    );
    ensure!(
        SampleRepetitions::from_sfx_flag(sound.loop_flag).is_infinite(),
        "fixture requires installed unbounded loop"
    );
    let samples: Vec<f64> = if sound.audio_type.bits_per_sample() == 8 {
        sound
            .pcm_data
            .iter()
            .map(|&x| (f64::from(x) - 128.0) / 128.0)
            .collect()
    } else {
        sound
            .pcm_data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|x| f64::from(i16::from_le_bytes([x[0], x[1]])) / 32768.0)
            .collect()
    };
    let source_rms = (samples.iter().map(|x| x * x).sum::<f64>() / samples.len() as f64).sqrt();
    let fixture = Fixture {
        scripts,
        system: std::fs::read_to_string(root.join("SYSTEM.INI"))?,
        wav: cyrus_assets::audio::pcm_wav(sound.audio_type, sound.sample_rate, &sound.pcm_data)
            .map_err(anyhow::Error::msg)?,
        sample_rate: sound.sample_rate,
        bits: sound.audio_type.bits_per_sample(),
        loop_flag: sound.loop_flag,
        source_rms,
        effect,
    };
    std::fs::write(out, serde_json::to_vec(&fixture)?)?;
    println!(
        "{}",
        serde_json::json!({"fixture":"ISLAND DR_SPIN6", "master":parent.actor,"effect":effect,"source_rms":source_rms,"loop_flag":sound.loop_flag})
    );
    Ok(())
}

#[derive(Resource)]
struct Probe {
    world: ScriptWorld,
    phase: usize,
    next: f64,
    origin: [i32; 3],
    slave: u8,
    fixture: Fixture,
}

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    let fixture: Fixture = {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if args.first().is_some_and(|a| a == "--export") {
            export(
                std::path::Path::new(&args[1]),
                std::path::Path::new(&args[2]),
            )
            .unwrap();
            return;
        }
        serde_json::from_slice(&std::fs::read(&args[0]).unwrap()).unwrap()
    };
    #[cfg(target_arch = "wasm32")]
    let fixture: Fixture = {
        #[wasm_bindgen::prelude::wasm_bindgen]
        extern "C" {
            #[wasm_bindgen::prelude::wasm_bindgen(js_namespace=globalThis)]
            fn positionalFixture() -> String;
        }
        serde_json::from_str(&positionalFixture()).unwrap()
    };
    let mut world = ScriptWorld::prepare(&fixture.scripts, 19).unwrap();
    world.prepare_spatial().unwrap();
    world.refresh_families().unwrap();
    let id = world.id(0).unwrap();
    let origin = world.spatial(id).unwrap().effective.unwrap().position;
    let slave = world.actors()[0].attributes()[Attribute::Slave];
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(10))),
        AssetPlugin::default(),
        bevy::log::LogPlugin::default(),
        AudioPlugin::default(),
        SoundPlugin,
    ))
    .insert_resource(cyrus_audio::AudioReadiness { prepare: false })
    .insert_resource(cyrus_assets::sound::SoundSettings::parse(&fixture.system));
    let audio = app
        .world_mut()
        .resource_mut::<Assets<AudioSource>>()
        .add(AudioSource {
            bytes: fixture.wav.clone().into(),
        });
    let mut effects = (0..=fixture.effect).map(|_| None).collect::<Vec<_>>();
    effects[fixture.effect] = Some(Effect {
        audio,
        bits_per_sample: fixture.bits,
        base_volume: 64,
        sample_rate: fixture.sample_rate,
        repetitions: SampleRepetitions::from_sfx_flag(fixture.loop_flag),
    });
    app.insert_resource(SoundEffects(effects))
        .insert_resource(AudioListener(GlobalTransform::IDENTITY))
        .insert_resource(Probe {
            world,
            phase: 0,
            next: 2.0,
            origin,
            slave,
            fixture,
        })
        .add_systems(Update, step.before(cyrus_audio::PlayCues));
    app.run();
}
fn step(
    time: Res<Time<Real>>,
    mut probe: ResMut<Probe>,
    mut listener: ResMut<AudioListener>,
    mut cues: MessageWriter<SoundCue>,
    mut exit: MessageWriter<AppExit>,
) {
    if time.elapsed_secs_f64() < probe.next {
        return;
    }
    let phase = probe.phase;
    if phase == 13 {
        exit.write(AppExit::Success);
        return;
    }
    let id = probe.world.id(0).unwrap();
    let timing = ScriptInputs {
        nominal_ticks: Some(0),
        frame_scale: Some(256),
        ..Default::default()
    };
    if phase == 1 {
        let parent = probe.world.id(1).unwrap();
        probe
            .world
            .advance(
                parent,
                ScriptStage::Body(Thread::Main),
                timing,
                None,
                None,
                20,
            )
            .unwrap();
        probe.world.refresh_families().unwrap();
    }
    let stop = [2, 4, 6, 8, 10, 12].contains(&phase);
    let slave = if phase == 5 || phase == 11 {
        0
    } else {
        probe.slave
    };
    probe
        .world
        .set_actor_attribute(id.index, Attribute::Slave as usize, slave)
        .unwrap();
    let position = if slave == 0 {
        probe.world.spatial(id).unwrap().local.position
    } else {
        probe.world.spatial(id).unwrap().effective.unwrap().position
    };
    let origin = if phase == 11 { position } else { probe.origin };
    let camera = [origin[0] - 512 * 256, origin[1], origin[2] - 512 * 256];
    let yaw = if phase >= 7 { 512 } else { 0 };
    let translation = Vec3::new(
        -camera[0] as f32 / 5120.0,
        -camera[1] as f32 / 5120.0,
        (camera[2] as f32 - 16_777_215.0) / 5120.0,
    );
    let direction = if yaw == 0 { Vec3::Z } else { -Vec3::X };
    let mut transform = Transform::from_translation(translation).looking_to(direction, Vec3::Y);
    if phase == 9 {
        transform.rotate_local_x(0.4);
        transform.rotate_local_z(0.3);
    }
    listener.0 = GlobalTransform::from(transform);
    let mut emitted = Vec::new();
    if stop {
        cues.write(SoundCue::Script {
            generation: id.generation,
            index: id.index,
            sound: ScriptSound::StopAll,
        });
    } else if phase != 1 {
        probe
            .world
            .advance(
                id,
                ScriptStage::Body(Thread::Main),
                timing,
                Some(&mut |actor, sound| {
                    emitted.push((actor, sound));
                    Ok(())
                }),
                None,
                20,
            )
            .unwrap();
        for (actor, sound) in emitted {
            match &sound {
                ScriptSound::Positional {
                    position: actual, ..
                } => assert_eq!(*actual, position, "fresh positional emission phase {phase}"),
                _ => panic!("unexpected cue"),
            };
            cues.write(SoundCue::Script {
                generation: actor.generation,
                index: actor.index,
                sound,
            });
        }
    }
    let heard = if phase == 1 { probe.origin } else { position };
    report(&serde_json::json!({"positional_emitters":true,"phase":phase,"stop":stop,"position":heard,"listener":camera,"yaw":yaw,"bits":probe.fixture.bits,"source_rms":probe.fixture.source_rms,"master":cyrus_assets::sound::SoundSettings::parse(&probe.fixture.system).master_volume,"sound_distance":cyrus_assets::sound::SoundSettings::parse(&probe.fixture.system).sound_distance,"slave":slave}).to_string());
    probe.phase += 1;
    probe.next = time.elapsed_secs_f64() + 3.0;
}
#[cfg(not(target_arch = "wasm32"))]
fn report(s: &str) {
    println!("{s}");
}
#[cfg(target_arch = "wasm32")]
fn report(s: &str) {
    #[wasm_bindgen::prelude::wasm_bindgen]
    extern "C" {
        #[wasm_bindgen::prelude::wasm_bindgen(js_namespace=console,js_name=log)]
        fn log(s: &str);
    }
    log(s);
}
