//! Exact matrix presentation and binding lifetime through the public plugin.
use bevy::prelude::*;
use cyrus_assets::{animation::Animations, script::Scripts};
use cyrus_game::script::{Pose, ScriptActors, ScriptBinding, ScriptWorld};
use cyrus_render::script_transform::{ScriptTransformPlugin, scene_transform};
use cyrus_world::transform::Matrix;
use serde_json::json;

#[test]
fn scene_conversion_preserves_nonorthogonal_matrix_and_integer_position_shift() {
    let pose = Pose {
        position: [511, -1, 256],
        angles: [0; 3],
        matrix: Matrix([[1.0, 0.25, 0.5], [0.0, 0.75, 0.0], [0.125, 0.0, 1.0]]),
    };
    let affine = scene_transform(pose);
    assert_eq!(Vec3::from(affine.translation), Vec3::new(-0.05, 0.05, 0.05));
    assert_eq!(
        affine.matrix3.to_cols_array(),
        [1.0, 0.0, -0.125, 0.25, 0.75, 0.0, -0.5, 0.0, 1.0]
    );
}

#[test]
fn binding_updates_descendants_and_removal_restores_authored_transforms() {
    let mut scripts: Scripts = serde_json::from_value(json!({
        "version":3,"actor_pool_capacity":10,"content_id":"","world":17,
        "definition":{"functions":[{"name":"NullFunction","param_count":0,"kind":"function"}],"flags":[],"references":[],"attributes":[]},
        "actors":{},
        "instances":[{"object_id":0,"actor":"STATIC","object_type":0,"active":0,"map_position":[40,-60,80],"angles":[0,0,0]}],
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
        Vec3::new(8.0, 3.0, 4.0)
    );
    assert_eq!(
        app.world()
            .get::<GlobalTransform>(child)
            .unwrap()
            .translation(),
        Vec3::new(8.0, 4.0, 4.0)
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
