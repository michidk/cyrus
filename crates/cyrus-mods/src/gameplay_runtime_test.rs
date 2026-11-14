//! Actual Wasm guests exercise typed gameplay hooks without a renderer or installed assets.
use super::*;
use crate::{Catalog, Entry, add_components_to_linker, gameplay, ownership};
use bevy::asset::AssetPlugin;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Resource)]
struct GuestFile(String);

fn setup(guest: Res<GuestFile>, mut mods: Mods, mut catalog: ResMut<Catalog>) {
    let entity = mods.spawn(guest.0.clone(), None);
    mods.enable_access(entity, ModAccess::World);
    catalog.entries[0].instance = Some(entity);
}

fn guest_app(id: &str, env: &str) -> App {
    let wasm = PathBuf::from(std::env::var_os(env).expect("set explicit guest Wasm artifact"));
    assert!(wasm.is_file(), "{}", wasm.display());
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: wasm.parent().unwrap().to_string_lossy().into(),
            ..default()
        },
    ))
    .add_plugins(ModLoaderPlugin::default().add_functionality(add_components_to_linker))
    .register_type::<Vec<f32>>()
    .init_resource::<ownership::Owners>()
    .add_systems(Startup, (spawn_host, setup).chain())
    .add_systems(PreUpdate, ownership::publish);
    let mut actors = ScriptActors::default();
    actors.current = Some(crate::particles::tests::world(1));
    app.insert_resource(actors);
    let entry = Entry::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../mods")
            .join(id),
        true,
    )
    .unwrap();
    app.insert_resource(Catalog {
        entries: vec![entry],
        ..default()
    });
    app.insert_resource(GuestFile(
        wasm.file_name().unwrap().to_string_lossy().into_owned(),
    ));
    app.finish();
    app.cleanup();
    app.update();
    app
}

#[test]
#[ignore = "requires CYRUS_AUTOSAVE_WASM built for wasm32-wasip2"]
fn wasm_autosave_waits_for_terminal_host_completion() {
    let mut app = guest_app("autosave", "CYRUS_AUTOSAVE_WASM");
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut requested = false;
    while !requested {
        let mut input = app.world_mut().query::<&mut Input>();
        input.single_mut(app.world_mut()).unwrap().delta_seconds = 1.0;
        requested = gameplay::take_save_request(app.world_mut(), true);
        app.update();
        assert!(
            Instant::now() < deadline,
            "Wasm autosave submitted no request"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    for _ in 0..5 {
        app.update();
        assert!(
            !gameplay::take_save_request(app.world_mut(), true),
            "pending save duplicated"
        );
    }
    gameplay::finish_save(app.world_mut(), Ok(()));
    app.update();
    let mut memory = app.world_mut().query::<&Memory>();
    let timer = memory
        .single(app.world())
        .unwrap()
        .values
        .get("autosave/timer")
        .unwrap();
    assert_eq!(timer[0], 0.0, "timer resets only after completion");
    assert_eq!(timer[1], 0.0, "guest acknowledged completion");
}

#[test]
#[ignore = "requires CYRUS_DIALOGUE_SKIP_WASM built for wasm32-wasip2"]
fn wasm_dialogue_skip_uses_press_edges_and_refuses_game_bindings() {
    let mut app = guest_app("dialogue-skip", "CYRUS_DIALOGUE_SKIP_WASM");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let mut input = app.world_mut().query::<&mut Input>();
        let mut input = input.single_mut(app.world_mut()).unwrap();
        input.just_pressed = vec!["Backspace".into()];
        input.pressed = vec!["Backspace".into()];
        let requested = gameplay::dialogue_fixture(app.world_mut());
        app.update();
        if requested {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Wasm dialogue guest submitted no advance"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    // Clear the one queued press used to detect load, then keep only the held key.
    let mut input = app.world_mut().query::<&mut Input>();
    input
        .single_mut(app.world_mut())
        .unwrap()
        .just_pressed
        .clear();
    gameplay::dialogue_fixture(app.world_mut());
    for _ in 0..5 {
        app.update();
        assert!(
            !gameplay::dialogue_fixture(app.world_mut()),
            "held key advanced another line"
        );
    }
    let mut input = app.world_mut().query::<&mut Input>();
    let mut input = input.single_mut(app.world_mut()).unwrap();
    input.just_pressed = vec!["Backspace".into()];
    input.bound_keys = vec!["Backspace".into()];
    for _ in 0..5 {
        app.update();
        assert!(
            !gameplay::dialogue_fixture(app.world_mut()),
            "game binding also advanced dialogue"
        );
    }
}
