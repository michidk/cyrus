//! Load the actual grass component against typed terrain and owned mesh hooks.
use super::*;
use crate::{Catalog, Entry, add_components_to_linker, geometry, terrain};
use bevy::asset::AssetPlugin;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
#[derive(Resource)]
struct GuestPath(String);
fn setup(
    guest: Res<GuestPath>,
    mut mods: Mods,
    mut catalog: ResMut<Catalog>,
    mut host: Query<(&mut Settings, &mut Camera, &mut terrain::Terrain)>,
) {
    let entry = &mut catalog.entries[0];
    let instance = mods.spawn(guest.0.clone(), Some(entry.manifest.name.clone()));
    mods.enable_access(instance, ModAccess::World);
    entry.instance = Some(instance);
    let (mut settings, mut camera, mut terrain) = host.single_mut().unwrap();
    settings.numbers.insert("grass/density".into(), 1.0);
    settings.numbers.insert("grass/wind".into(), 0.18);
    camera.pose = vec![2., 2., 2., 0., 0., 0., 1.];
    *terrain = terrain::Terrain::fixture();
}
fn test_context() -> cyrus_render::fog::geometry::GeometryContext {
    let environment = cyrus_assets::environment::Environment {
        effects: None,
        texture_scale: [1, 1],
        sky: None,
        background: [0; 3],
        clipping: [1., 5000.],
        fog_table: [[0; 4]; 16],
        terrain_heights: None,
        water: None,
        terrain_lighting: None,
        model_lighting: cyrus_assets::environment::ModelLighting {
            ambient: [64.; 3],
            directional: [255.; 3],
            direction: [0., -1., 0.],
        },
        world_palette: vec![],
        prelighting: cyrus_assets::environment::Prelighting {
            instances: default(),
            lights: vec![],
        },
        object_lights: vec![],
        water_textures: vec![],
    };
    cyrus_render::fog::geometry::GeometryContext::from_environment(
        &environment,
        &default(),
        environment.clipping,
    )
}

fn grass_app(wasm: &std::path::Path) -> App {
    let mut actors = ScriptActors::default();
    actors.current = Some(crate::particles::tests::world(1));
    let context = test_context();
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::log::LogPlugin::default(),
        AssetPlugin {
            file_path: wasm.parent().unwrap().to_string_lossy().into(),
            ..default()
        },
    ))
    .add_plugins(ModLoaderPlugin::default().add_functionality(add_components_to_linker))
    .register_type::<Vec<f32>>()
    .insert_resource(GuestPath(
        wasm.file_name().unwrap().to_string_lossy().into(),
    ))
    .insert_resource(actors)
    .insert_resource(Catalog {
        entries: vec![
            Entry::read(
                &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../mods/grass"),
                true,
            )
            .unwrap(),
        ],
        ..default()
    })
    .init_resource::<geometry::GeometryState>()
    .insert_resource(context)
    .init_resource::<Assets<Mesh>>()
    .init_resource::<Assets<cyrus_render::fog::geometry::GeometryMaterial>>()
    .add_systems(Startup, (spawn_host, setup).chain())
    .add_systems(PreUpdate, geometry::publish)
    .add_systems(PostUpdate, geometry::update);
    app.finish();
    app.cleanup();
    app
}

fn wait_for_chunk(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        app.update();
        if app
            .world_mut()
            .query::<&mut geometry::Geometry>()
            .single_mut(app.world_mut())
            .unwrap()
            .contains("grass".into(), "chunk/0/0".into())
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "real grass component did not publish its seeded patch"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires CYRUS_GRASS_WASM pointing to the built grass.wasm"]
fn wasm_grass_geometry() {
    let wasm = PathBuf::from(std::env::var_os("CYRUS_GRASS_WASM").expect("set CYRUS_GRASS_WASM"));
    assert!(wasm.is_file());
    let mut app = grass_app(&wasm);
    wait_for_chunk(&mut app);
    let first_entities: Vec<_> = app
        .world_mut()
        .query_filtered::<Entity, With<Mesh3d>>()
        .iter(app.world())
        .collect();
    assert!(
        !first_entities.is_empty(),
        "guest uploads allocated actual render meshes"
    );
    assert!(!app.world().resource::<Assets<Mesh>>().is_empty());
    app.world_mut().resource_mut::<ScriptActors>().current =
        Some(crate::particles::tests::world(2));
    app.update();
    for entity in first_entities {
        assert!(
            app.world().get_entity(entity).is_err(),
            "world replacement retires old geometry"
        );
    }
    // The actual component can rebuild in the new generation after its old leases disappear.
    for _ in 0..3 {
        app.update();
    }
    app.world_mut().resource_mut::<Catalog>().entries[0].active = false;
    app.update();
    assert!(
        !app.world_mut()
            .query::<&mut geometry::Geometry>()
            .single_mut(app.world_mut())
            .unwrap()
            .contains("grass".into(), "chunk/0/0".into())
    );
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
    assert!(
        app.world()
            .resource::<Assets<cyrus_render::fog::geometry::GeometryMaterial>>()
            .is_empty()
    );
}

#[test]
#[ignore = "requires CYRUS_GRASS_WASM pointing to the built grass.wasm"]
fn wasm_grass_geometry_reloads_cached_empty_chunks() {
    let wasm = PathBuf::from(std::env::var_os("CYRUS_GRASS_WASM").expect("set CYRUS_GRASS_WASM"));
    let mut app = grass_app(&wasm);
    wait_for_chunk(&mut app);
    for _ in 0..3 {
        app.update();
    }
    let old_lease = app
        .world_mut()
        .query::<&mut geometry::Geometry>()
        .single_mut(app.world_mut())
        .unwrap()
        .lease("grass".into());
    // A previous guest version classified this visible region as empty. Its cache
    // survives reload, even though the replacement guest grows blades here.
    app.world_mut()
        .query::<&mut Memory>()
        .single_mut(app.world_mut())
        .unwrap()
        .save("grass/chunks".into(), vec![0., 0., 0., 1.]);
    app.world_mut()
        .query::<&mut geometry::Geometry>()
        .single_mut(app.world_mut())
        .unwrap()
        .remove("grass".into(), "chunk/0/0".into());
    app.update();
    assert!(
        !app.world_mut()
            .query::<&mut geometry::Geometry>()
            .single_mut(app.world_mut())
            .unwrap()
            .contains("grass".into(), "chunk/0/0".into()),
        "the old empty decision is retained until a lease change"
    );
    app.world()
        .resource::<AssetServer>()
        .reload(wasm.file_name().unwrap().to_string_lossy().into_owned());
    wait_for_chunk(&mut app);
    let new_lease = app
        .world_mut()
        .query::<&mut geometry::Geometry>()
        .single_mut(app.world_mut())
        .unwrap()
        .lease("grass".into());
    assert_ne!(old_lease, new_lease, "successful reload changes the lease");
}
