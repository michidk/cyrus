//! Installed restored CD ownership across public sound preference updates.
use bevy::{
    app::ScheduleRunnerPlugin,
    audio::{AudioPlayer, AudioPlugin, AudioSink, AudioSinkPlayback},
    prelude::*,
};
use cyrus_assets::{install::GameInstall, sound::SoundSettings};
use cyrus_audio::music::{Music, MusicPlayer, MusicPlugin, MusicTransport};
use cyrus_world::lifecycle::CurrentScene;
use std::{path::PathBuf, time::Duration};

#[derive(Resource)]
struct Probe {
    directory: PathBuf,
    phase: Option<u8>,
    report: Option<u8>,
}

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 4,
        "usage: music_settings INSTALL TRACK OUTPUT"
    );
    let install = GameInstall::locate(Some(args[1].as_ref()))?;
    let track = args[2].parse()?;
    let cue = cyrus_assets::music::cue_path(&install)
        .ok_or_else(|| anyhow::anyhow!("installed CD cue missing"))?;
    let transport = MusicTransport::new(bevy::audio::AudioSource {
        bytes: cyrus_assets::music::read_track(&cue, track)
            .map_err(anyhow::Error::msg)?
            .into(),
    })?;
    let mut owner = Music::default();
    owner.track = Some(track);
    let saved = owner.snapshot(&[(&transport, false)])?;
    let candidate = Music::from_snapshot(saved, Some(transport.source().clone()))?;
    let directory = PathBuf::from(&args[3]);
    std::fs::create_dir_all(&directory)?;
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(10))),
        AssetPlugin::default(),
        bevy::state::app::StatesPlugin,
        AudioPlugin::default(),
    ))
    .insert_state(cyrus_game::AppState::InGame)
    .insert_resource(install)
    .insert_resource(SoundSettings::parse(""))
    .insert_resource(CurrentScene {
        name: "0".into(),
        world: Some(0),
    })
    .add_plugins(MusicPlugin)
    .insert_resource(candidate.music)
    .insert_resource(Probe {
        directory,
        phase: None,
        report: None,
    })
    .add_systems(PreUpdate, advance)
    .add_systems(Last, report);
    let (transport, settings) = candidate.playback.unwrap();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<MusicTransport>>()
        .add(transport);
    app.world_mut()
        .spawn((MusicPlayer, AudioPlayer(handle), settings));
    app.run();
    Ok(())
}

fn advance(
    mut probe: ResMut<Probe>,
    mut settings: ResMut<SoundSettings>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(command) = std::fs::read_to_string(probe.directory.join("command")) else {
        return;
    };
    let Ok(phase) = command.trim().parse::<u8>() else {
        return;
    };
    if probe.phase == Some(phase) {
        return;
    }
    match phase {
        0 => {}
        1 => settings.speech = false,
        2 => settings.redbook_volume = 0,
        3 => settings.redbook_volume = 64,
        4 => settings.master_volume = 0,
        _ => {
            exit.write(AppExit::Success);
            return;
        }
    }
    probe.phase = Some(phase);
    probe.report = Some(phase);
}

fn report(
    mut probe: ResMut<Probe>,
    music: Res<Music>,
    players: Query<(Entity, &AudioSink), With<MusicPlayer>>,
) {
    let Some(phase) = probe.report else {
        return;
    };
    let Ok((entity, sink)) = players.single() else {
        return;
    };
    let row = serde_json::json!({"phase": phase, "track": music.track, "gain":music.gain,
        "entity":entity.to_bits(), "paused":sink.is_paused()});
    std::fs::write(
        probe.directory.join(format!("phase-{phase}.json")),
        row.to_string(),
    )
    .unwrap();
    probe.report = None;
}
