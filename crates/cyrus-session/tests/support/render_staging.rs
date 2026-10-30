//! Real world-instance staging exercised without a renderer or live session mutation.
use anyhow::Result;
use bevy::{
    asset::io::{
        AssetSourceBuilder, AssetSourceId,
        memory::{Dir, MemoryAssetReader},
    },
    gltf::GltfPlugin,
    mesh::MeshPlugin,
    prelude::*,
    world_serialization::WorldSerializationPlugin,
};
use cyrus_session::{RuntimeSessionSnapshot, SessionSceneInstances};
use cyrus_world::lifecycle::PreparedWorld;

pub fn check_loaded_staging(
    snapshot: &RuntimeSessionSnapshot,
    prepared: &PreparedWorld,
) -> Result<()> {
    // Only the rendering hints are needed by instance loading; full aggregate validation
    // remains covered separately and must reject this incomplete rendered fixture.
    let mut hints = serde_json::to_value(snapshot)?;
    hints["sky"] = serde_json::json!({
        "version":1,"environment":"fixture","velocity":[0.0,0.0],"speed":0.0,"tick":0,
        "plane":[0.0,0.0,0.0,0.0],"fog":[0.0,0.0,0.0,0.0],"scroll":[0.0,0.0,0.0,0.0],
        "sun_position":[0.0,0.0,0.0,0.0],"sun_color":[0.0,0.0,0.0,0.0],"pending_sync":null
    });
    if hints["camera"].is_null() {
        hints["camera"] = serde_json::json!({
            "version":1,"active":true,"pose":{"translation":[0.0,0.0,0.0],
                "rotation":[0.0,0.0,0.0,1.0],"scale":[1.0,1.0,1.0]},
            "projection":[1.0,1.0,0.1,100.0],"near_clip_plane":[0.0,0.0,1.0,0.0],
            "flycam":false,"speed":150.0,"frame_scene":false,"initial":null
        });
    }
    let hints: RuntimeSessionSnapshot = serde_json::from_value(hints)?;
    let directory = Dir::default();
    directory.insert_asset_text(
        std::path::Path::new("test.glb"),
        &serde_json::json!({
            "asset":{"version":"2.0"},
            "buffers":[{"byteLength":36,"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}],
            "bufferViews":[{"buffer":0,"byteLength":36}],
            "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[0,0,0]}],
            "materials":[{}],
            "meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0,"extras":{
                "rgpre_render":{"version":1,"rob_attributes":null},
                "rgpre_lighting":{"version":5,"radius_fixed":0,"solid_rgb":null,"vertices":[[0,0],[64,0],[255,0]]}
            }}]}],
            "nodes":[{"name":"B_000_ACTOR","mesh":0}],
            "scenes":[{"nodes":[0]}], "scene":0
        })
        .to_string(),
    );
    let environment: cyrus_assets::environment::Environment = serde_json::from_value(
        serde_json::json!({
            "texture_scale":[1,1],"sky":null,"background":[0,0,0],"clipping":[7.0,3800.0],
            "fog_table":vec![[0,0,0,0];16],"water":null,"terrain_lighting":null,
            "model_lighting":{"ambient":[64.0,64.0,64.0],"directional":[255.0,255.0,255.0],"direction":[0.0,-1.0,0.0]},
            "prelighting":{"instances":{},"lights":[]},"object_lights":[]
        }),
    )?;
    directory.insert_asset_text(
        std::path::Path::new("test.environment"),
        &serde_json::to_string(&environment)?,
    );
    let mut app = App::new();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || {
            Box::new(MemoryAssetReader {
                root: directory.clone(),
            })
        }),
    )
    .add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        WorldSerializationPlugin,
        MeshPlugin,
        GltfPlugin::default(),
        cyrus_render::persistence::SceneIdentityPlugin,
        cyrus_render::fog::SceneMaterialAssetsPlugin,
    ))
    .init_asset::<Image>()
    .init_asset::<StandardMaterial>()
    .init_asset::<cyrus_assets::environment::Environment>()
    .init_asset_loader::<cyrus_assets::environment::EnvironmentLoader>();
    app.init_resource::<cyrus_render::fog::RenderDistance>();
    app.finish();
    app.cleanup();
    let untouched = app
        .world_mut()
        .spawn((
            Name::new("live sentinel"),
            Transform::from_xyz(1.0, 2.0, 3.0),
        ))
        .id();
    // Keep the parent asset alive: without the PBR extension, no material component
    // retains its material dependency until the explicit conversion below.
    let material_owner: Handle<Gltf> = app.world().resource::<AssetServer>().load("test.glb");
    let installed_environment: Handle<cyrus_assets::environment::Environment> = app
        .world()
        .resource::<AssetServer>()
        .load(prepared.environment_path.clone());
    let mut staged = SessionSceneInstances::request(&hints, prepared, app.world_mut())?;
    assert!(
        !app.world()
            .get::<Camera>(staged.camera())
            .unwrap()
            .is_active
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app
        .world()
        .resource::<Assets<cyrus_assets::environment::Environment>>()
        .get(&installed_environment)
        .is_none()
        || !staged.ready(app.world())?
        || app
            .world()
            .resource::<Assets<Gltf>>()
            .get(&material_owner)
            .is_none()
    {
        app.update();
        assert!(
            std::time::Instant::now() < deadline,
            "staged GLTF did not finish loading"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(
        app.world().get::<Visibility>(staged.root()),
        Some(&Visibility::Hidden)
    );
    cyrus_render::persistence::SceneSourceIdentity::capture(app.world(), staged.root())?;
    // CPU-only GLTF loading omits PBR's private extension handler. Use Bevy's
    // public conversion on this fixture's sole loaded material.
    let material = bevy::pbr::gltf::standard_material_from_gltf_material(
        app.world()
            .resource::<Assets<bevy::gltf::GltfMaterial>>()
            .iter()
            .next()
            .unwrap()
            .1,
    );
    let material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(material);
    let meshes: Vec<_> = app
        .world_mut()
        .query_filtered::<Entity, With<Mesh3d>>()
        .iter(app.world())
        .collect();
    assert_eq!(meshes.len(), 1);
    for entity in meshes {
        app.world_mut()
            .entity_mut(entity)
            .insert(MeshMaterial3d(material.clone()));
    }
    let environment = app
        .world_mut()
        .resource_mut::<Assets<cyrus_assets::environment::Environment>>()
        .add(environment);
    let textures = staged.prepare_textures(&environment, app.world_mut())?;
    let terrain = staged.prepare_terrain(&environment, &[], app.world_mut())?;
    let mut playback_hints = serde_json::to_value(&hints)?;
    playback_hints["render_playback"] = serde_json::json!({
        "scene_source":cyrus_render::persistence::SceneSourceIdentity::capture(app.world(), staged.root())?,
        "textures":textures.snapshot_playback()?,
        "texture_sources":textures.snapshot_sources(app.world(), &[staged.root()])?,
        "terrain":terrain.snapshot_playback(app.world().resource::<Assets<cyrus_assets::environment::Environment>>().get(&environment).unwrap(), app.world())?,
        "entities":[],"terrain_paths":[],"distance":{"original":[7.0,3800.0],"override_far":8000.0}
    });
    let playback_hints: RuntimeSessionSnapshot = serde_json::from_value(playback_hints)?;
    let saved_edits: cyrus_render::persistence::EnvironmentEditsSnapshot =
        serde_json::from_value(serde_json::json!({"version":1,"edits":{}}))?;
    let edits = saved_edits.restore(&std::collections::BTreeMap::from([(
        snapshot.current_world(),
        app.world()
            .resource::<Assets<cyrus_assets::environment::Environment>>()
            .get(&installed_environment)
            .unwrap(),
    )]))?;
    let (scene, playback) = staged.prepare_rendered(
        &playback_hints,
        prepared,
        &environment,
        &edits,
        app.world_mut(),
    )?;
    scene.validate(app.world())?;
    assert_eq!(scene.environment(), &environment);
    assert!(staged.sky_images().is_some());
    assert!(playback.terrain_bindings.is_empty());
    assert!(playback.terrain.is_bound_to(environment.id()));
    assert!(playback.textures.is_bound_to(environment.id()));
    assert!(playback.textures.snapshot().is_empty());
    cyrus_render::fog::validate_scene_materials(app.world(), &[staged.root()])?;
    assert!(
        !app.world()
            .contains_resource::<cyrus_render::fog::TextureAnimations>()
    );
    assert!(!app.world().contains_resource::<PreparedWorld>());
    let root = staged.root();
    let camera = staged.camera();
    staged.discard(app.world_mut());
    assert!(app.world().get_entity(root).is_err());
    assert!(app.world().get_entity(camera).is_err());
    assert_eq!(
        app.world().get::<Transform>(untouched).unwrap().translation,
        Vec3::new(1.0, 2.0, 3.0)
    );
    // A real asset-loader rejection must leave the live camera and remove the
    // private roots. This stage is deliberately not a publishable session.
    app.world_mut()
        .entity_mut(untouched)
        .insert((Camera3d::default(), cyrus_render::camera::ViewCamera));
    let mut missing = prepared.clone();
    missing.scene_path = "absent.glb".into();
    let rejected = SessionSceneInstances::request(&hints, &missing, app.world_mut())?;
    let mut inspection =
        bevy::ecs::system::SystemState::<cyrus_session::SessionBoundaryInspection>::new(
            app.world_mut(),
        );
    assert!(
        inspection
            .get(app.world())
            .unwrap()
            .snapshot(false)
            .is_none()
    );
    let positive = inspection.get(app.world()).unwrap().snapshot(true).unwrap();
    assert!(positive.staged_entities >= 2);
    assert!(positive.staged_render_roots >= 1);
    assert_eq!(positive.staged_cameras, 1);
    assert_eq!(positive.active_live_cameras, 1);
    assert!(positive.assets.iter().any(|asset| {
        asset
            .path
            .as_deref()
            .is_some_and(|path| path.starts_with("absent.glb"))
    }));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let failure = loop {
        app.update();
        if let Err(error) = rejected.ready(app.world()) {
            break error;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "missing GLTF did not reject"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert!(failure.to_string().contains("session scene staging"));
    rejected.discard(app.world_mut());
    let cleaned = inspection.get(app.world()).unwrap().snapshot(true).unwrap();
    assert_eq!(cleaned.staged_entities, 0);
    assert_eq!(cleaned.staged_render_roots, 0);
    assert_eq!(cleaned.staged_cameras, 0);
    assert!(cleaned.assets.is_empty());
    assert_eq!(cleaned.active_live_cameras, 1);
    assert_eq!(
        app.world().get::<Transform>(untouched).unwrap().translation,
        Vec3::new(1.0, 2.0, 3.0)
    );
    Ok(())
}
