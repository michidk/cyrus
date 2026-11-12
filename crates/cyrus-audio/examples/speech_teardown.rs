//! Installed speech across public world teardown and an unrelated replacement voice.
#[allow(dead_code)] // The shared caller fixture also supplies other acceptance entry points.
#[path = "../tests/support/sound_callers.rs"]
mod fixture;
use bevy::{app::ScheduleRunnerPlugin, audio::AudioPlugin, prelude::*};
use cyrus_audio::sound::{ScriptSounds, SoundPlugin, dialogue::DialogueClip};
use cyrus_script::script::{
    DialogueEffects, DialoguePlayback, ScriptActorId, ScriptActors, ScriptPlugin,
};
use cyrus_world::{
    animation::{AnimationPlugin, AnimationTick},
    lifecycle::PreparedWorld,
};
use std::{path::PathBuf, time::Duration};

#[derive(Resource)]
struct Probe {
    directory: PathBuf,
    prepared: PreparedWorld,
    clip: DialogueClip,
    phase: u8,
    report: Option<u8>,
    stale_record_control: bool,
}

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(
        matches!(args.len(), 4 | 5),
        "usage: speech_teardown INSTALL FOUR_BYTE_LABEL OUTPUT"
    );
    let stale_record_control = args
        .get(4)
        .is_some_and(|arg| arg == "--stale-record-control");
    anyhow::ensure!(
        args.len() == 4 || stale_record_control,
        "unknown control argument"
    );
    let install = cyrus_assets::install::GameInstall::locate(Some(args[1].as_ref()))?;
    let label: [u8; 4] = args[2].as_bytes().try_into()?;
    let directory = PathBuf::from(&args[3]);
    std::fs::create_dir_all(&directory)?;
    let cache = directory.join("cache");
    cyrus_assets::sound::dialogue::export::Exporter::open(&install, &cache)?.export(label)?;
    let key = label
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let entry: cyrus_assets::sound::DialogueEntry = serde_json::from_slice(&std::fs::read(
        cache.join(format!("dialogue/{key}.dialogue")),
    )?)?;
    let voice = entry
        .voice
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("selected RTX label has no voice"))?;
    anyhow::ensure!(
        voice.lifetime >= 512,
        "select a clip lasting at least two seconds"
    );
    let wav = std::fs::read(cache.join(&voice.audio))?;
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(10))),
        AssetPlugin::default(),
        bevy::log::LogPlugin::default(),
        AudioPlugin::default(),
    ))
    .init_asset::<cyrus_assets::script::Scripts>()
    .init_asset::<cyrus_assets::animation::Animations>()
    .add_message::<AnimationTick>()
    .add_plugins((AnimationPlugin, ScriptPlugin, SoundPlugin))
    .insert_resource(cyrus_audio::AudioReadiness { prepare: false })
    .insert_resource(cyrus_audio::AudioListener(GlobalTransform::default()))
    .insert_resource(cyrus_assets::audio::read_settings(&install));
    let scripts = fixture::separate_sound_scripts(false);
    let animations = cyrus_assets::animation::Animations {
        version: 2,
        world: scripts.world,
        metadata: serde_json::json!({"script_content_id": scripts.content_id,
            "actors": [], "markers": [], "walk_node_maps": []}),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let prepared = PreparedWorld {
        requested_name: "speech-owner-probe".into(),
        world: Some(scripts.world),
        scripts: app.world_mut().resource_mut::<Assets<_>>().add(scripts),
        animations: app.world_mut().resource_mut::<Assets<_>>().add(animations),
        player_path: String::new().into(),
        scene_path: String::new().into(),
        environment_path: String::new().into(),
    };
    let audio = app
        .world_mut()
        .resource_mut::<Assets<AudioSource>>()
        .add(AudioSource { bytes: wav.into() });
    app.insert_resource(prepared.clone())
        .insert_resource(Probe {
            directory,
            prepared,
            clip: DialogueClip {
                entry,
                audio: Some(audio),
            },
            phase: 0,
            report: None,
            stale_record_control,
        })
        .add_systems(Update, step.after(cyrus_audio::PlayCues));
    app.run();
    Ok(())
}

/// The skip stage does not inhibit or reposition speakers. All audio operations
/// still use the shared production sample owner through DialoguePlayback.
struct Skip<'a>(&'a mut dyn DialoguePlayback);
impl DialoguePlayback for Skip<'_> {
    fn sample_playing(&mut self, sample: u8) -> anyhow::Result<bool> {
        self.0.sample_playing(sample)
    }
    fn stop_sample(&mut self, sample: u8) -> anyhow::Result<()> {
        self.0.stop_sample(sample)
    }
    fn refresh_volume(&mut self, speaker: ScriptActorId, sample: u8) -> anyhow::Result<()> {
        self.0.refresh_volume(speaker, sample)
    }
    fn present_line(&mut self, index: usize, text: &str) -> anyhow::Result<()> {
        self.0.present_line(index, text)
    }
}
impl DialogueEffects for Skip<'_> {
    fn inhibit_speaker(&mut self, _: ScriptActorId) -> anyhow::Result<()> {
        anyhow::bail!("skip unexpectedly visited a speaker")
    }
}

fn start(mut sounds: ScriptSounds, probe: Res<Probe>, mut actors: ResMut<ScriptActors>) {
    let started = sounds.start_dialogue(&probe.clip).unwrap();
    assert_eq!(started.sample, Some(0));
    if probe.phase == 0 {
        let world = actors.current.as_mut().unwrap();
        let speaker = world.id(0).unwrap();
        world
            .globals_mut()
            .dialogue_mut()
            .unwrap()
            .begin_line(
                speaker,
                probe.clip.entry.text.clone(),
                started.sample,
                started.lifetime,
            )
            .unwrap();
        sounds.with_dialogue_playback(&mut |_| Ok([0, 0, 16_777_215]), |playback| {
            world.advance_dialogue(0, true, false, playback).unwrap();
        });
    } else if probe.stale_record_control {
        let dialogue = actors
            .current
            .as_mut()
            .unwrap()
            .globals_mut()
            .dialogue_mut()
            .unwrap();
        let mut saved = serde_json::to_value(dialogue.snapshot()).unwrap();
        saved["records"][0]["sample"] = 0.into();
        *dialogue = cyrus_script::script::DialogueState::from_snapshot(
            dialogue.settings(),
            serde_json::from_value(saved).unwrap(),
            &[],
            &std::collections::BTreeMap::from([(0, 0)]),
        )
        .unwrap();
    }
}

fn skip(mut sounds: ScriptSounds, mut actors: ResMut<ScriptActors>) {
    let dialogue = actors
        .current
        .as_mut()
        .unwrap()
        .globals_mut()
        .dialogue_mut()
        .unwrap();
    sounds.with_dialogue_playback(
        &mut |_| anyhow::bail!("skip requested a speaker pose"),
        |playback| {
            dialogue
                .advance(0, true, true, Some(1), &mut Skip(playback))
                .unwrap();
        },
    );
}

fn step(world: &mut World) {
    if let Some(phase) = world.resource_mut::<Probe>().report.take() {
        let players: Vec<_> = world
            .query_filtered::<Entity, With<cyrus_audio::sound::PlayingSound>>()
            .iter(world)
            .map(|entity| entity.to_bits())
            .collect();
        let actors = world.resource::<ScriptActors>();
        let report = serde_json::json!({
            "phase": phase,
            "generation": actors.current.as_ref().map(|world| world.generation()),
            "dialogue": actors.current.as_ref().and_then(|world| world.globals().dialogue()).map(|dialogue| serde_json::to_value(dialogue).unwrap()),
            "playing": players.len(),
            "players": players,
        });
        let directory = &world.resource::<Probe>().directory;
        let temporary = directory.join(format!("phase-{phase}.tmp"));
        std::fs::write(&temporary, serde_json::to_vec(&report).unwrap()).unwrap();
        std::fs::rename(temporary, directory.join(format!("phase-{phase}.json"))).unwrap();
    }
    let probe = world.resource::<Probe>();
    let command = std::fs::read_to_string(probe.directory.join("command")).unwrap_or_default();
    if command.trim().parse::<u8>().ok() != Some(probe.phase) {
        return;
    }
    let phase = probe.phase;
    match phase {
        0 => {
            if world.resource::<ScriptActors>().current.is_none() {
                return;
            }
            world.run_system_cached(start).unwrap();
        }
        1 => {
            world.remove_resource::<PreparedWorld>();
        }
        2 => {
            let prepared = world.resource::<Probe>().prepared.clone();
            world.insert_resource(prepared);
        }
        3 => {
            if world.resource::<ScriptActors>().current.is_none() {
                return;
            }
            world.run_system_cached(start).unwrap();
        }
        4 => {
            world.run_system_cached(skip).unwrap();
        }
        5 => {
            world.write_message(cyrus_audio::sound::SoundRequest::StopAll);
        }
        6 => {
            world.write_message(AppExit::Success);
        }
        _ => unreachable!(),
    }
    let mut probe = world.resource_mut::<Probe>();
    probe.report = Some(phase);
    probe.phase += 1;
}
