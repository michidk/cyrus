//! Player model replacement without a second animation interpreter.
use bevy::prelude::*;
use cyrus_game::player::Player;
use cyrus_render::player_model::PlayerModelPlugin;
use cyrus_world::{
    animation::{AnimationPlugin, AnimationPose},
    lifecycle::PreparedWorld,
};

fn roots_of(app: &mut App, path: &str) -> Vec<Entity> {
    let expected = bevy::asset::AssetPath::parse(path);
    let handles: Vec<_> = app
        .world_mut()
        .query::<(Entity, &WorldAssetRoot)>()
        .iter(app.world())
        .map(|(entity, handle)| (entity, handle.0.id()))
        .collect();
    handles
        .into_iter()
        .filter(|(_, id)| {
            app.world().resource::<AssetServer>().get_path(*id).as_ref() == Some(&expected)
        })
        .map(|(entity, _)| entity)
        .collect()
}

#[test]
fn player_pose_binds_updates_replaces_and_removes_its_model() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        AnimationPlugin,
        cyrus_world::lifecycle::WorldPlugin,
        PlayerModelPlugin,
    ))
    .init_asset::<WorldAsset>();
    let prepared =
        PreparedWorld::from_cache("ISLAND".into(), app.world().resource::<AssetServer>());
    app.insert_resource(prepared);
    let player = app
        .world_mut()
        .spawn((
            Player,
            Transform::default(),
            AnimationPose {
                model: "CYRSA001".into(),
                frame: 1,
            },
        ))
        .id();
    app.update();
    let (root, handle) = app
        .world_mut()
        .query::<(Entity, &WorldAssetRoot)>()
        .single(app.world())
        .unwrap();
    let path = app
        .world()
        .resource::<AssetServer>()
        .get_path(handle.0.id())
        .unwrap();
    assert_eq!(
        path,
        bevy::asset::AssetPath::parse("cache://ISLAND.player-models/CYRSA001.glb#Scene0")
    );
    assert_eq!(
        *app.world().get::<Visibility>(root).unwrap(),
        Visibility::Hidden
    );
    let node = app
        .world_mut()
        .spawn((Name::new("CYRSA001"), ChildOf(root)))
        .id();
    app.update();
    assert_eq!(app.world().get::<AnimationPose>(node).unwrap().frame, 1);
    assert_eq!(
        *app.world().get::<Visibility>(root).unwrap(),
        Visibility::Inherited
    );
    app.world_mut()
        .get_mut::<AnimationPose>(player)
        .unwrap()
        .frame = 2;
    app.update();
    assert_eq!(app.world().get::<AnimationPose>(node).unwrap().frame, 2);
    app.world_mut()
        .get_mut::<AnimationPose>(player)
        .unwrap()
        .model = "CYRSA012".into();
    app.update();
    // The shown model stays until its successor has loaded.
    assert!(app.world().get_entity(root).is_ok());
    assert_eq!(
        roots_of(&mut app, "cache://ISLAND.player-models/CYRSA012.glb#Scene0").len(),
        1
    );
    let next = roots_of(&mut app, "cache://ISLAND.player-models/CYRSA012.glb#Scene0")[0];
    assert_eq!(
        *app.world().get::<Visibility>(next).unwrap(),
        Visibility::Hidden
    );
    app.world_mut()
        .spawn((Name::new("CYRSA012"), ChildOf(next)));
    app.update();
    app.update();
    assert!(app.world().get_entity(root).is_err());
    assert!(app.world().get_entity(node).is_err());
    app.world_mut().resource_mut::<PreparedWorld>().scene_path = "cache://PALACE.glb".into();
    app.update();
    let next = roots_of(&mut app, "cache://PALACE.player-models/CYRSA012.glb#Scene0")[0];
    app.world_mut()
        .spawn((Name::new("CYRSA012"), ChildOf(next)));
    app.update();
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&WorldAssetRoot>()
            .iter(app.world())
            .count(),
        1
    );
    app.world_mut().entity_mut(player).remove::<AnimationPose>();
    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&WorldAssetRoot>()
            .iter(app.world())
            .count(),
        0
    );
}

#[test]
fn map_actor_replaces_foreign_geometry_and_restores_original_visibility() {
    use cyrus_game::script::{ScriptActorId, ScriptBinding};
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        AnimationPlugin,
        cyrus_world::lifecycle::WorldPlugin,
        PlayerModelPlugin,
    ))
    .init_asset::<WorldAsset>();
    let prepared =
        PreparedWorld::from_cache("ISLAND".into(), app.world().resource::<AssetServer>());
    app.insert_resource(prepared);
    let actor = app
        .world_mut()
        .spawn((
            Name::new("B_387_KOTARO"),
            ScriptBinding {
                actor: ScriptActorId {
                    generation: 1,
                    index: 387,
                },
                source: Handle::default(),
            },
            AnimationPose {
                model: "THUGA002".into(),
                frame: 1,
            },
            Transform::default(),
        ))
        .id();
    let original = app
        .world_mut()
        .spawn((Mesh3d::default(), Visibility::Inherited, ChildOf(actor)))
        .id();
    app.update();
    assert_eq!(
        app.world().get::<Visibility>(original),
        Some(&Visibility::Hidden)
    );
    let root = app
        .world_mut()
        .query_filtered::<Entity, With<WorldAssetRoot>>()
        .single(app.world())
        .unwrap();
    assert_eq!(app.world().get::<ChildOf>(root).unwrap().parent(), actor);
    let model = app
        .world_mut()
        .spawn((Name::new("THUGA002"), ChildOf(root)))
        .id();
    app.update();
    assert_eq!(app.world().get::<AnimationPose>(model).unwrap().frame, 1);
    app.world_mut().entity_mut(actor).insert(AnimationPose {
        model: "B_387_KOTARO".into(),
        frame: 0,
    });
    app.update();
    assert!(app.world().get_entity(root).is_err());
    assert_eq!(
        app.world().get::<Visibility>(original),
        Some(&Visibility::Inherited)
    );
    assert!(app.world().get::<Mesh3d>(original).is_some());
    app.world_mut().entity_mut(actor).insert(AnimationPose {
        model: "THUGA002".into(),
        frame: 2,
    });
    app.update();
    assert_eq!(
        app.world().get::<Visibility>(original),
        Some(&Visibility::Hidden)
    );
    app.world_mut().entity_mut(actor).remove::<AnimationPose>();
    app.update();
    assert_eq!(
        app.world().get::<Visibility>(original),
        Some(&Visibility::Inherited)
    );
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<WorldAssetRoot>>()
            .iter(app.world())
            .count(),
        0
    );
}
