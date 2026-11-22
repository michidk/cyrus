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
#[ignore = "requires CYRUS_FLYCAM_WASM built for wasm32-wasip2"]
fn wasm_flycam_exit_respects_carry_player_without_mutability_panics() {
    for carry in [false, true] {
        let mut app = guest_app("flycam", "CYRUS_FLYCAM_WASM");
        {
            let world = app.world_mut();
            let mut host = world.query::<(&mut Memory, &mut Input, &mut Settings, &mut Player)>();
            let (mut memory, mut input, mut settings, mut player) = host.single_mut(world).unwrap();
            memory.values.insert(
                "flycam/flight".into(),
                vec![-2000.0, 50.0, -1200.0, 0.0, 0.0],
            );
            input.just_pressed = vec!["Insert".into()];
            settings.flags.insert("flycam/carry_player".into(), carry);
            player.heading = 513;
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            app.update();
            let mut memory = app.world_mut().query::<&Memory>();
            if !memory
                .single(app.world())
                .unwrap()
                .values
                .contains_key("flycam/flight")
            {
                break;
            }
            assert!(Instant::now() < deadline, "flycam did not finish flight");
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut player = app.world_mut().query::<&Player>();
        let placement = player.single(app.world()).unwrap().placement;
        assert_eq!(
            placement,
            carry.then_some(([40000.0, -1000.0, 41535.996], 513))
        );
    }
}

#[test]
#[ignore = "requires CYRUS_FLYCAM_WASM built for wasm32-wasip2"]
fn wasm_flycam_follow_places_the_player_under_the_camera_each_frame() {
    let mut app = guest_app("flycam", "CYRUS_FLYCAM_WASM");
    {
        let world = app.world_mut();
        let mut host = world.query::<(&mut Memory, &mut Settings, &mut Player)>();
        let (mut memory, mut settings, mut player) = host.single_mut(world).unwrap();
        memory.values.insert(
            "flycam/flight".into(),
            vec![-2000.0, 50.0, -1200.0, 0.0, 0.0],
        );
        settings.flags.insert("flycam/follow".into(), true);
        player.position = vec![1.0, -744.0, 3.0];
        player.heading = 513;
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        app.update();
        let mut player = app.world_mut().query::<&Player>();
        if let Some(placement) = player.single(app.world()).unwrap().placement {
            assert_eq!(placement, ([40000.0, -744.0, 41535.996], 513));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "flycam did not place a following player"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
