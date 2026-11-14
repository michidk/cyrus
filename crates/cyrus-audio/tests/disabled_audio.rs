//! The public music plugin must honor disabled installed audio before loading a track.
use bevy::audio::AudioSource;
use bevy::prelude::*;
use cyrus_assets::{install::GameInstall, sound::SoundSettings};
use cyrus_audio::music::{Music, MusicPlugin, MusicTransport};
use cyrus_world::lifecycle::CurrentScene;

#[test]
fn disabled_sound_does_not_admit_menu_cd_playback() -> anyhow::Result<()> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (disabled audio CD startup)");
        return Ok(());
    };
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::state::app::StatesPlugin,
    ))
    .init_state::<cyrus_game::AppState>()
    .init_resource::<CurrentScene>()
    .init_asset::<AudioSource>()
    .init_asset::<MusicTransport>()
    .insert_resource(install)
    .insert_resource(SoundSettings::parse(
        "[system]\nsound=0\nredbook=on\nredbook_volume=200\n",
    ))
    .add_plugins(MusicPlugin);
    app.finish();
    app.cleanup();
    app.world_mut()
        .resource_mut::<NextState<cyrus_game::AppState>>()
        .set(cyrus_game::AppState::Menu);
    app.update();
    assert_eq!(
        app.world().resource::<Music>().track,
        None,
        "disabled sound admitted CD track2"
    );
    Ok(())
}
