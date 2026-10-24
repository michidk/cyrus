//! Public animation contracts. The exported-model check reports a missing install.
use cyrus_assets::animation::Animations;

fn asset() -> Animations {
    serde_json::from_value(serde_json::json!({
        "version": 2, "world": 17,
        "sections": {"RAHD": [1, 2, 3]},
        "metadata": {"actors": [{"script_name": "ACTOR", "raan_entries": [{"file_path": "fxart/test.3dc", "frame_count": 2}], "animation_groups": [{"group_index": 7, "commands": [{"opcode": 4, "sound_param": 9}]}]}]},
        "models": {"TEST": {
            "base_positions": [[0, 0, 0], [1, 0, 0], [0, 1, 0]],
            "base_normals": [[0, 0, 1]], "faces": [[0, 1, 2]], "corner_normals": [[null, null, null]],
            "encoding": "animated_i16",
            "positions": [[[0, 0, 0], [1, 0, 0], [0, 1, 0]], [[0, 0, 2], [1, 0, 2], [0, 1, 2]]],
            "normals": [[], [[0, 0, 1]]]
        }},
        "unsupported": {"MISSING": "model absent"}
    })).unwrap()
}

#[test]
fn frame_projection_keeps_coincident_source_vertices_distinct() {
    use cyrus_render::animation::MeshFrameBinding;
    let mut input = asset();
    let model = input.models.get_mut("TEST").unwrap();
    model.encoding = "animated_i32".into();
    model.base_positions[2] = model.base_positions[0];
    model.positions[0][2] = model.positions[0][0];
    model.positions[1][2] = [20.0, 40.0, 60.0];
    model.normals[1][0] = [0.0, 1.0, 0.0];
    let binding = MeshFrameBinding {
        version: 1,
        source_vertices: vec![[2, 0], [0, 0], [1, 0], [2, 0]],
    };
    let frame = cyrus_render::animation::FrameGeometry::from_model(model, 1, &binding).unwrap();
    assert_eq!(
        frame.positions,
        [
            [-1.0, -2.0, 3.0],
            [0.0, 0.0, 0.1],
            [-0.05, 0.0, 0.1],
            [-1.0, -2.0, 3.0]
        ]
    );
    assert_eq!(frame.normals, [[0.0, -1.0, 0.0]; 4]);
}

#[test]
fn frame_projection_keeps_authored_corner_normals_with_face_fallback() {
    use cyrus_render::animation::MeshFrameBinding;
    let mut input = asset();
    let model = input.models.get_mut("TEST").unwrap();
    model.encoding = "animated_i32".into();
    model.normals[1][0] = [0.0, 1.0, 0.0];
    model.corner_normals[0] = vec![Some([1.0, 0.0, 0.0]), None, Some([0.0, 0.0, 2.0])];
    let binding = MeshFrameBinding {
        version: 1,
        source_vertices: vec![[0, 0], [1, 0], [2, 0]],
    };
    let frame = cyrus_render::animation::FrameGeometry::from_model(model, 1, &binding).unwrap();
    // Magnitude is retained; only the missing corner uses this frame's face normal.
    assert_eq!(
        frame.normals,
        [[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 2.0]]
    );
    model.corner_normals.clear();
    assert!(cyrus_render::animation::FrameGeometry::from_model(model, 1, &binding).is_err());
}

#[test]
fn frame_projection_rejects_unknown_mapping_and_unproven_encoding() {
    use cyrus_render::animation::MeshFrameBinding;
    let input = asset();
    let model = &input.models["TEST"];
    let mut binding = MeshFrameBinding {
        version: 2,
        source_vertices: vec![[0, 0]],
    };
    assert_eq!(
        cyrus_render::animation::FrameGeometry::from_model(model, 0, &binding)
            .err()
            .unwrap()
            .to_string(),
        "mesh frame mapping version 2 unsupported"
    );
    binding.version = 1;
    assert_eq!(
        cyrus_render::animation::FrameGeometry::from_model(model, 1, &binding)
            .err()
            .unwrap()
            .to_string(),
        "frame 1: playback for animated_i16 is unsupported"
    );
    binding.source_vertices[0][0] = 3;
    assert_eq!(
        cyrus_render::animation::FrameGeometry::from_model(model, 0, &binding)
            .err()
            .unwrap()
            .to_string(),
        "frame 0: vertex 3 does not belong to face 0"
    );
}

#[test]
fn explicit_pose_deforms_a_private_mesh_and_scene_change_restores_it() {
    use bevy::{
        asset::RenderAssetUsages,
        gltf::GltfExtras,
        mesh::{PrimitiveTopology, VertexAttributeValues},
        prelude::*,
    };
    use cyrus_world::animation::AnimationPlugin;
    use cyrus_world::animation::AnimationPose;
    use cyrus_world::animation::AnimationRequest;
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        AnimationPlugin,
        cyrus_render::animation::AnimationRenderPlugin,
    ))
    .init_asset::<Mesh>();
    let mut data = asset();
    data.models.get_mut("TEST").unwrap().encoding = "animated_i32".into();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    app.world_mut().resource_mut::<AnimationRequest>().0 = handle;
    let original = app.world_mut().resource_mut::<Assets<Mesh>>().add(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0.0, 0.0, 0.0], [-0.05, 0.0, 0.0], [0.0, -0.05, 0.0]],
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 3]),
    );
    let node = app
        .world_mut()
        .spawn((
            Name::new("B_000_ACTOR"),
            AnimationPose {
                model: "TEST".into(),
                frame: 1,
            },
        ))
        .id();
    let primitive = app
        .world_mut()
        .spawn((
            ChildOf(node),
            Mesh3d(original.clone()),
            GltfExtras {
                value: r#"{"rgpre_geometry":{"version":1,"source_vertices":[[0,0],[1,0],[2,0]]}}"#
                    .into(),
            },
        ))
        .id();
    let incompatible = app.world_mut().resource_mut::<Assets<Mesh>>().add(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[9.0, 0.0, 0.0]; 3]),
    );
    let rejected = app
        .world_mut()
        .spawn((
            ChildOf(node),
            Mesh3d(incompatible.clone()),
            GltfExtras {
                value: r#"{"rgpre_geometry":{"version":1,"source_vertices":[[0,0],[1,0],[2,0]]}}"#
                    .into(),
            },
        ))
        .id();
    app.update();
    assert_eq!(
        app.world().get::<Mesh3d>(rejected).unwrap().0,
        incompatible,
        "a stale or different model must not be deformed using this mapping"
    );
    let private = &app.world().get::<Mesh3d>(primitive).unwrap().0;
    assert_ne!(private, &original);
    let meshes = app.world().resource::<Assets<Mesh>>();
    let VertexAttributeValues::Float32x3(positions) = meshes
        .get(private)
        .unwrap()
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .unwrap()
    else {
        panic!("missing positions");
    };
    assert_eq!(
        positions,
        &[[0.0, 0.0, 0.1], [-0.05, 0.0, 0.1], [0.0, -0.05, 0.1]]
    );
    app.world_mut().resource_mut::<AnimationRequest>().0 = Handle::default();
    app.update();
    assert_eq!(app.world().get::<Mesh3d>(primitive).unwrap().0, original);
}

#[test]
fn exported_island_flag_mapping_binds_a_real_frame_and_restores_the_mesh() {
    use bevy::{
        asset::RenderAssetUsages,
        gltf::{GltfExtras, gltf::Gltf},
        mesh::{PrimitiveTopology, VertexAttributeValues},
        prelude::*,
    };
    use cyrus_assets::install::GameInstall;
    use cyrus_world::animation::AnimationPlugin;
    use cyrus_world::animation::AnimationPose;
    use cyrus_world::animation::AnimationRequest;
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let install = match GameInstall::open(&root) {
        Ok(install) => install,
        Err(error) => {
            eprintln!("skipped: no install (exported animation contract: no install ({error}))");
            return;
        }
    };
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../game-config")
        .join(format!("animation-export-test-{}", std::process::id()));
    let path = directory.join("ISLAND.glb");
    cyrus_assets::scene::export_scene(&install, "ISLAND", &path).unwrap();
    let data: Animations =
        serde_json::from_slice(&std::fs::read(path.with_extension("animations")).unwrap()).unwrap();
    data.validate().unwrap();
    assert!(data.sections.contains_key("RASC"));
    let mut script = cyrus_game::script::AnimationScript::from_asset(&data, "FLAG2").unwrap();
    let (mut actor, _) =
        cyrus_world::animation::ActorAnimation::initialize(&data, "FLAG2", 1, &mut |_| Ok(()))
            .unwrap();
    let scripts: cyrus_assets::script::Scripts =
        serde_json::from_slice(&std::fs::read(path.with_extension("scripts")).unwrap()).unwrap();
    let mut world = cyrus_game::script::ScriptWorld::prepare(&scripts, 1).unwrap();
    let index = world
        .actors()
        .iter()
        .position(|actor| actor.name() == "FLAG2")
        .unwrap();
    let id = world.id(index).unwrap();
    world
        .initialize_map_animation(id, &data, 1, &mut |_, _| Ok(()))
        .unwrap();
    let advance = |world: &mut cyrus_game::script::ScriptWorld, ticks| {
        use cyrus_game::script::{ScriptStage, Thread};
        for stage in [
            ScriptStage::Animation,
            ScriptStage::Entry,
            ScriptStage::Tasks,
            ScriptStage::Body(Thread::Main),
        ] {
            world
                .advance(
                    id,
                    stage,
                    cyrus_game::script::ScriptInputs {
                        nominal_ticks: Some(ticks),
                        frame_scale: Some(256),
                        ..Default::default()
                    },
                    None,
                    None,
                    100_000,
                )
                .unwrap();
        }
    };
    let mut transitions = Vec::new();
    let mut ended = false;
    for _ in 0..10_000 {
        actor.advance(1).unwrap();
        actor.request(0, 0, 1).unwrap();
        let before = script.program_counter();
        script.advance(&mut actor, 1).unwrap();
        advance(&mut world, 1);
        assert_eq!(
            world.actors()[index]
                .runtime()
                .unwrap()
                .program_counter(cyrus_game::script::Thread::Main),
            Some(script.program_counter())
        );
        if script.program_counter() != before {
            if script.waiting_for_group().is_none() {
                ended = true;
                break;
            }
            transitions.push(script.waiting_for_group().unwrap());
        }
    }
    assert!(ended, "the installed finite tasks must reach End");
    assert_eq!(transitions, [1, 10, 1, 10]);
    assert_eq!(script.program_counter(), 0);
    // Recorded original FLAG2 updates after its authored End, with supplied nominal ticks.
    for (ticks, expected_frame) in [
        (1, 2),
        (1, 3),
        (0, 3),
        (1, 4),
        (1, 5),
        (1, 6),
        (0, 6),
        (1, 7),
        (1, 8),
        (0, 8),
        (1, 9),
        (1, 10),
        (0, 10),
        (1, 11),
        (1, 2),
        (1, 3),
        (0, 3),
        (1, 4),
        (1, 5),
        (0, 5),
    ] {
        let frame = actor.advance(ticks).unwrap();
        let frame = actor.request(0, 0, ticks).unwrap().or(frame);
        let frame = script
            .advance(&mut actor, ticks)
            .unwrap()
            .or(frame)
            .unwrap();
        advance(&mut world, ticks);
        let shared_frame = data
            .resolve_frame("FLAG2", world.animation_frame(id).unwrap().unwrap())
            .unwrap();
        assert_eq!(
            (&*shared_frame.model, shared_frame.frame),
            ("FLAGA01", expected_frame)
        );
        let frame = data.resolve_frame("FLAG2", frame).unwrap();
        assert_eq!((&*frame.model, frame.frame), ("FLAGA01", expected_frame));
        assert_eq!(actor.current_group(), Some(1));
        assert_eq!(script.waiting_for_group(), Some(1));
        assert_eq!(script.program_counter(), 14);
    }
    let resolved = data.resolve_frame("FLAG2", 2).unwrap();
    assert_eq!((&*resolved.model, resolved.frame), ("FLAGA01", 2));
    let bytes = std::fs::read(&path).unwrap();
    let glb = Gltf::from_slice(&bytes).unwrap();
    let node = glb
        .nodes()
        .find(|node| node.name() == Some("B_346_FLAG2"))
        .unwrap();
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        AnimationPlugin,
        cyrus_render::animation::AnimationRenderPlugin,
    ))
    .init_asset::<Mesh>();
    let source = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    app.world_mut().resource_mut::<AnimationRequest>().0 = source;
    let actor = app
        .world_mut()
        .spawn((
            Name::new("B_346_FLAG2"),
            AnimationPose {
                model: resolved.model.clone(),
                frame: resolved.frame,
            },
        ))
        .id();
    let mut primitives = Vec::new();
    for primitive in node.mesh().unwrap().primitives() {
        let reader = primitive.reader(|_| glb.blob.as_deref());
        let base: Vec<_> = reader.read_positions().unwrap().collect();
        let extras = primitive.extras().as_ref().unwrap().get();
        let original = app.world_mut().resource_mut::<Assets<Mesh>>().add(
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, base.clone()),
        );
        let entity = app
            .world_mut()
            .spawn((
                ChildOf(actor),
                Mesh3d(original.clone()),
                GltfExtras {
                    value: extras.into(),
                },
            ))
            .id();
        primitives.push((entity, original, base));
    }
    assert!(!primitives.is_empty());
    app.update();
    let mut moved = false;
    for (entity, original, base) in &primitives {
        let private = &app.world().get::<Mesh3d>(*entity).unwrap().0;
        assert_ne!(
            private, original,
            "the real export must bind to a private mesh"
        );
        let meshes = app.world().resource::<Assets<Mesh>>();
        let VertexAttributeValues::Float32x3(selected) = meshes
            .get(private)
            .unwrap()
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
        else {
            panic!("positions absent")
        };
        moved |= selected != base;
    }
    assert!(
        moved,
        "the selected authored flag frame must change visible geometry"
    );
    app.world_mut().entity_mut(actor).remove::<AnimationPose>();
    app.update();
    for (entity, original, _) in &primitives {
        assert_eq!(&app.world().get::<Mesh3d>(*entity).unwrap().0, original);
    }
    drop(app);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn reconstructed_i16_frames_project_absolute_positions_and_old_caches_stay_rejected() {
    use cyrus_render::animation::{FrameGeometry, MeshFrameBinding};
    let mut input = asset();
    let model = input.models.get_mut("TEST").unwrap();
    let binding = MeshFrameBinding {
        version: 1,
        source_vertices: vec![[0, 0]],
    };
    assert!(FrameGeometry::from_model(model, 1, &binding).is_err());
    model.encoding = "animated_i16_absolute".into();
    model.positions[1][0] = [9.0, -18.0, 29.5];
    let frame = FrameGeometry::from_model(model, 1, &binding).unwrap();
    assert_eq!(frame.positions, [[-0.45, 0.9, 1.475]]);
}
