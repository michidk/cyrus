//! Authored task scheduling through public APIs, with synthetic program descriptors.
use cyrus_assets::animation::Animations;

fn asset() -> Animations {
    serde_json::from_value(serde_json::json!({
        "version":2, "world":17, "sections":{}, "unsupported":{},
        "models":{"TEST":{
            "base_positions":[[0,0,0],[1,0,0],[0,1,0]],
            "base_normals":[[0,0,1]], "faces":[[0,1,2]],"corner_normals":[[null,null,null]], "encoding":"animated_i32",
            "positions":[[[0,0,0],[1,0,0],[0,1,0]],[[0,0,2],[1,0,2],[0,1,2]]],
            "normals":[[],[[0,0,1]]]
        }},
        "metadata":{
            "actors":[{"script_name":"ACTOR",
                "raan_entries":[{"file_path":"test.3dc","frame_count":2}],
                "animation_groups":[
                    {"group_index":1,"anim_type":0,"anim_id":1,"commands":[
                        {"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]},
                    {"group_index":7,"anim_type":0,"anim_id":1,"commands":[
                        {"opcode":0,"frame_reference":1},{"opcode":2,"value":0},{"opcode":1}]},
                    {"group_index":10,"anim_type":0,"anim_id":1,"commands":[
                        {"opcode":0,"frame_reference":0},{"opcode":2,"value":0},{"opcode":1}]}
                ]}],
            "animation_scripts":{"ACTOR":{"version":4,"entry":0,"steps":{
                "0":{"next":14,"operation":{"type":"call","call":{"function_id":0,"kind":"task","target":{"type":"me"},"function":"PushAnimation","arguments":[{"type":"integer","value":7},{"type":"integer","value":1}]}}},
                "14":{"next":28,"operation":{"type":"call","call":{"function_id":0,"kind":"task","target":{"type":"me"},"function":"PushAnimation","arguments":[{"type":"integer","value":10},{"type":"integer","value":257}]}}},
                "28":{"next":33,"operation":{"type":"end","target":0}}
            }}}
        }
    })).unwrap()
}

#[test]
fn scripted_playback_deforms_only_its_mesh_and_world_change_restores_it() {
    use bevy::{
        asset::RenderAssetUsages,
        gltf::GltfExtras,
        mesh::{PrimitiveTopology, VertexAttributeValues},
        prelude::*,
    };
    use cyrus_game::script::ScriptedPlayback;
    use cyrus_world::animation::AnimationPlugin;
    use cyrus_world::animation::AnimationPose;
    use cyrus_world::animation::AnimationRequest;
    use cyrus_world::animation::AnimationTick;
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        AnimationPlugin,
        cyrus_game::script::ScriptPlugin,
        cyrus_render::animation::AnimationRenderPlugin,
    ))
    .init_asset::<Mesh>();
    let data = asset();
    let source = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    let playback = cyrus_game::script::ScriptedPlayback::new(
        source.clone(),
        app.world()
            .resource::<Assets<Animations>>()
            .get(&source)
            .unwrap(),
        "ACTOR",
        1,
    )
    .unwrap();
    app.world_mut().resource_mut::<AnimationRequest>().0 = source;
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
        .spawn((Name::new("B_001_ACTOR"), playback))
        .id();
    let primitive = app
        .world_mut()
        .spawn((
            Mesh3d(original.clone()),
            ChildOf(node),
            GltfExtras {
                value: r#"{"rgpre_geometry":{"version":1,"source_vertices":[[0,0],[1,0],[2,0]]}}"#
                    .into(),
            },
        ))
        .id();
    let other = app.world_mut().spawn(Mesh3d(original.clone())).id();
    app.update();
    assert!(
        app.world().get::<AnimationPose>(node).is_none(),
        "render updates supply no nominal ticks"
    );
    app.world_mut().write_message(AnimationTick { ticks: 0 });
    app.update();
    assert_eq!(app.world().get::<AnimationPose>(node).unwrap().frame, 1);
    let private = app.world().get::<Mesh3d>(primitive).unwrap().0.clone();
    assert_ne!(private, original);
    assert_eq!(app.world().get::<Mesh3d>(other).unwrap().0, original);
    let meshes = app.world().resource::<Assets<Mesh>>();
    let VertexAttributeValues::Float32x3(positions) = meshes
        .get(&private)
        .unwrap()
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .unwrap()
    else {
        panic!("positions");
    };
    assert_eq!(
        positions,
        &[[0.0, 0.0, 0.1], [-0.05, 0.0, 0.1], [0.0, -0.05, 0.1]]
    );
    app.world_mut().resource_mut::<AnimationRequest>().0 = Handle::default();
    app.update();
    assert!(app.world().get::<ScriptedPlayback>(node).is_none());
    assert!(app.world().get::<AnimationPose>(node).is_none());
    assert_eq!(app.world().get::<Mesh3d>(primitive).unwrap().0, original);
}
