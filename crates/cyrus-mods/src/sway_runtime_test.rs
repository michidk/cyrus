//! Load the actual sway component against the scenery hook.
use super::*;
use crate::{Catalog, Entry, add_components_to_linker, scenery};
use bevy::asset::AssetPlugin;
use cyrus_render::hooks::{Scenery as Pieces, SceneryPiece, SceneryTilts};
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
    mut host: Query<(&mut Settings, &mut Camera)>,
) {
    let entry = &mut catalog.entries[0];
    let instance = mods.spawn(guest.0.clone(), Some(entry.manifest.name.clone()));
    mods.enable_access(instance, ModAccess::World);
    entry.instance = Some(instance);
    let (mut settings, mut camera) = host.single_mut().unwrap();
    settings.numbers.insert("sway/strength".into(), 1.0);
    camera.pose = vec![0., 2., 0., 0., 0., 0., 1.];
}

#[test]
#[ignore = "requires CYRUS_SWAY_WASM pointing to the built sway.wasm"]
fn wasm_sway_tilts_near_plants_only() {
    let wasm = PathBuf::from(std::env::var_os("CYRUS_SWAY_WASM").expect("set CYRUS_SWAY_WASM"));
    assert!(wasm.is_file());
    let piece = |model: &str, x: f32| SceneryPiece {
        entity: Entity::PLACEHOLDER,
        model: model.into(),
        base: Vec3::new(x, 0., 0.),
    };
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
    .insert_resource(GuestPath(
        wasm.file_name().unwrap().to_string_lossy().into(),
    ))
    .insert_resource(Catalog {
        entries: vec![
            Entry::read(
                &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../mods/sway"),
                true,
            )
            .unwrap(),
        ],
        ..default()
    })
    .insert_resource(Pieces {
        revision: 1,
        pieces: vec![
            piece("PALMTR01", 10.),
            piece("BARREL", 12.),
            piece("PLANT002", 5000.),
        ],
    })
    .init_resource::<SceneryTilts>()
    .add_systems(Startup, (spawn_host, setup).chain())
    .add_systems(PreUpdate, scenery::publish);
    app.finish();
    app.cleanup();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        app.update();
        if !app.world().resource::<SceneryTilts>().0.is_empty() {
            break;
        }
        assert!(Instant::now() < deadline, "sway never tilted the palm");
        std::thread::sleep(Duration::from_millis(10));
    }
    let tilts = &app.world().resource::<SceneryTilts>().0;
    assert_eq!(
        tilts.keys().copied().collect::<Vec<_>>(),
        [0],
        "only the near palm sways"
    );
    assert!(tilts[&0].to_axis_angle().1.abs() <= 0.021);
}
