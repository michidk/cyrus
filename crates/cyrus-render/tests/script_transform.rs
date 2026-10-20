//! Exact matrix presentation and binding lifetime through the public plugin.
use bevy::prelude::*;
use cyrus_assets::{animation::Animations, script::Scripts};
use cyrus_game::script::{Pose, ScriptActors, ScriptBinding, ScriptWorld};
use cyrus_render::script_transform::{ScriptTransformPlugin, scene_transform};
use cyrus_world::transform::Matrix;
use serde_json::json;

#[test]
fn script_visibility_and_stored_position_follow_binding_lifetime() {
    use cyrus_game::script::{ScriptInputs, ScriptStage};
    for (function, expected) in [
        ("HideMe", Visibility::Hidden),
        ("ShowMe", Visibility::Inherited),
    ] {
        let mut scripts: Scripts = serde_json::from_value(json!({
            "version":26,"weapons":{},"combat_clank_effects":[null,null],"combat_defenses":[],"combat_attacks":[],"dialogue":{"menu_start":[35,25],"max_menu_items":300,"max_dialog_lines":20,"max_text_width":500,"menu_traverse_delay":4,"print_text":true,"use_speech":true,"max_distance":1600,"rtx_filename":"ENGLISH.RTX"},"player_distance_override":false,"updates":{"fall_death_ticks":144,"task_system":true,"animation_system":true,"disable_slaves":false,"disable_master_slaves":false,"dummy_model_manager":false},
            "clock":{"normal_frame_rate":12,"min_frame_rate":6,"max_frame_rate":300,"use_smooth_fps":true,"use_smooth_divisor":true},
            "actor_pool_capacity":10,"content_id":"","world":17,
            "definition":{"functions":[{"name":"NullFunction","param_count":0,"kind":"function"},{"name":function,"param_count":0,"kind":"function"}],"flags":[],"references":[],"attributes":[]},
            "actors":{"STATIC":{"header_index":0,"code_origin":0,"instances":1,"instance_variables":[[]],"location_offsets":[],"attributes":{},
                "program":{"version":4,"entry":0,"steps":{
                    "0":{"next":1,"operation":{"type":"call","call":{"function_id":1,"kind":"function","target":{"type":"me"},"function":function,"arguments":[]}}},
                    "1":{"next":2,"operation":{"type":"end","target":0}}
                }}}},
            "instances":[{"object_id":0,"actor":"STATIC","object_type":1,"active":0,"model_name":"test","model_mode":0,"light_intensity":0,"light_radius":0,"light_rgb":[0,0,0],"map_position":[40,-60,80],"angles":[0,0,0]}],"unsupported":{}
        })).unwrap();
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let data = Animations {
            version: 2,
            world: scripts.world,
            metadata: json!({
                "script_content_id": scripts.content_id,
                "actors": [{"script_name":"STATIC", "model_load_path":"static"}]
            }),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        let collision: cyrus_assets::collision::ActorCollisionModels =
            serde_json::from_value(json!({
                "object_count":1,
                "models":{"test":{
                    "version":[118,53,46,48], "radius":42.0,
                    "vertices":[[-20.0,0.0,-20.0],[20.0,0.0,-20.0],[0.0,0.0,20.0]],
                    "normals":[[0.0,-1.0,0.0]], "faces":[[0,1,2]],
                    "textures":[null], "subobjects":[]
                }},
                "instances":{"0":"test"}, "collision_flags":{"0":0}, "unsupported":{}
            }))
            .unwrap();
        world
            .prepare_model_resources(
                &data,
                &cyrus_collision::actor_models::ActorModelCollision::new(&collision).unwrap(),
            )
            .unwrap();
        let id = world.id(0).unwrap();
        world
            .advance(
                id,
                ScriptStage::OrdinaryMain,
                ScriptInputs {
                    frame_scale: Some(256),
                    nominal_ticks: Some(1),
                    ..Default::default()
                },
                None,
                None,
                100,
            )
            .unwrap();
        if function == "ShowMe" {
            let collision = Default::default();
            let sources = cyrus_game::script::WorldSnapshotSources {
                effects: None,
                player: None,
                scripts: &scripts,
                animations: &data,
                collision: &collision,
            };
            let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
            saved["actors"][0]["model"]["current"]["position"] =
                json!([40.0_f32.to_bits(), 900.0_f32.to_bits(), 80.0_f32.to_bits()]);
            saved["actors"][0]["model"]["position"] =
                saved["actors"][0]["model"]["current"]["position"].clone();
            world = ScriptWorld::from_snapshot(
                serde_json::from_value(saved).unwrap(),
                &sources,
                cyrus_game::script::WorldRestoreContext {
                    generation: 2,
                    globals: Default::default(),
                    samples: &Default::default(),
                },
            )
            .unwrap();
            assert_eq!(
                world.spatial(world.id(0).unwrap()).unwrap().local.position,
                [10240, -15360, 20480]
            );
        }
        let id = world.id(0).unwrap();
        let mut actors = ScriptActors::default();
        actors.current = Some(world);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, ScriptTransformPlugin));
        app.insert_resource(actors);
        let authored = if expected == Visibility::Hidden {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let node = app
            .world_mut()
            .spawn((
                Transform::default(),
                if expected == Visibility::Hidden {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
                ScriptBinding {
                    actor: id,
                    source: Handle::<Animations>::default(),
                },
            ))
            .id();
        app.update();
        assert_eq!(*app.world().get::<Visibility>(node).unwrap(), expected);
        assert_eq!(
            app.world()
                .get::<GlobalTransform>(node)
                .unwrap()
                .translation(),
            Vec3::new(
                -2.0,
                if function == "ShowMe" { -45.0 } else { 3.0 },
                -3272.7998
            )
        );
        app.world_mut().entity_mut(node).remove::<ScriptBinding>();
        app.update();
        assert_eq!(*app.world().get::<Visibility>(node).unwrap(), authored);
    }
}

#[test]
fn scene_conversion_preserves_nonorthogonal_matrix_and_integer_position_shift() {
    let pose = Pose {
        position: [511, -1, 256],
        angles: [0; 3],
        matrix: Matrix([[1.0, 0.25, 0.5], [0.0, 0.75, 0.0], [0.125, 0.0, 1.0]]),
    };
    let affine = scene_transform(pose);
    assert_eq!(
        Vec3::from(affine.translation),
        Vec3::new(-0.05, 0.05, -3276.7498)
    );
    assert_eq!(
        affine.matrix3.to_cols_array(),
        [1.0, 0.0, -0.125, 0.25, 0.75, 0.0, -0.5, 0.0, 1.0]
    );
}

#[test]
fn binding_updates_descendants_and_removal_restores_authored_transforms() {
    let mut scripts: Scripts = serde_json::from_value(json!({
        "version":26,"weapons":{},"combat_clank_effects":[null,null],"combat_defenses":[],"combat_attacks":[],"dialogue":{"menu_start":[35,25],"max_menu_items":300,"max_dialog_lines":20,"max_text_width":500,"menu_traverse_delay":4,"print_text":true,"use_speech":true,"max_distance":1600,"rtx_filename":"ENGLISH.RTX"},"player_distance_override":false,"updates":{"fall_death_ticks":144,"task_system":true,"animation_system":true,"disable_slaves":false,"disable_master_slaves":false,"dummy_model_manager":false},"clock":{"normal_frame_rate":12,"min_frame_rate":6,"max_frame_rate":300,"use_smooth_fps":true,"use_smooth_divisor":true},"actor_pool_capacity":10,"content_id":"","world":17,
        "definition":{"functions":[{"name":"NullFunction","param_count":0,"kind":"function"}],"flags":[],"references":[],"attributes":[]},
        "actors":{},
        "instances":[{"object_id":0,"actor":"STATIC","object_type":0,"active":0,"model_name":"","model_mode":0,"light_intensity":0,"light_radius":0,"light_rgb":[0,0,0],"map_position":[40,-60,80],"angles":[0,0,0]}],
        "unsupported":{}
    })).unwrap();
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let id = world.id(0).unwrap();
    let mut actors = ScriptActors::default();
    actors.current = Some(world);
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin, ScriptTransformPlugin));
    app.insert_resource(actors);
    let parent = app
        .world_mut()
        .spawn(Transform::from_xyz(10.0, 0.0, 0.0))
        .id();
    let node = app
        .world_mut()
        .spawn((
            Transform::from_xyz(1.0, 2.0, 3.0),
            ChildOf(parent),
            ScriptBinding {
                actor: id,
                source: Handle::<Animations>::default(),
            },
        ))
        .id();
    let child = app
        .world_mut()
        .spawn((Transform::from_xyz(0.0, 1.0, 0.0), ChildOf(node)))
        .id();
    app.update();
    assert_eq!(
        app.world()
            .get::<GlobalTransform>(node)
            .unwrap()
            .translation(),
        Vec3::new(8.0, 3.0, -3272.7998)
    );
    assert_eq!(
        app.world()
            .get::<GlobalTransform>(child)
            .unwrap()
            .translation(),
        Vec3::new(8.0, 4.0, -3272.7998)
    );
    app.world_mut().entity_mut(node).remove::<ScriptBinding>();
    app.update();
    assert_eq!(
        app.world()
            .get::<GlobalTransform>(node)
            .unwrap()
            .translation(),
        Vec3::new(11.0, 2.0, 3.0)
    );
    assert_eq!(
        app.world()
            .get::<GlobalTransform>(child)
            .unwrap()
            .translation(),
        Vec3::new(11.0, 3.0, 3.0)
    );
}

#[test]
fn script_pose_stays_at_the_exported_world_marker() {
    use rgpre::import::rgm::{RgmFile, RgmSection, RgmSectionHeader, export_rgm_metadata_json};
    let positions = [[20i32, -40, 65_536], [100, 60, 30_000], [0, 0, 0]];
    let mut data = (positions.len() as u32).to_le_bytes().to_vec();
    for position in positions {
        for coordinate in position {
            data.extend_from_slice(&coordinate.to_le_bytes());
        }
    }
    data.extend_from_slice(&[1; 3]);
    let map = RgmFile {
        sections: vec![RgmSection::Mpm(
            RgmSectionHeader {
                name: *b"MPMK",
                data_length: data.len() as u32,
                record_count: None,
            },
            data,
        )],
    };
    let metadata = export_rgm_metadata_json(&map, None);
    for (index, position) in positions.into_iter().enumerate() {
        let exported: [f32; 3] =
            serde_json::from_value(metadata["markers"][index]["position"].clone()).unwrap();
        let pose = Pose {
            position: position.map(|value| value * 256),
            angles: [0; 3],
            matrix: Matrix::IDENTITY,
        };
        let rendered = Vec3::from(scene_transform(pose).translation);
        assert!(
            rendered.distance(Vec3::from_array(exported)) < 0.00001,
            "marker {index}: exported {exported:?}, script rendered {rendered:?}"
        );
    }
}
