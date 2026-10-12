//! Prepared world changes commit references without graphics or an audio consumer.
use bevy::prelude::*;
use cyrus_world::{
    animation::{AnimationPlugin, AnimationRequest},
    lifecycle::{CurrentScene, PreparedWorld, WorldPlugin},
};

#[test]
fn world_changes_retain_new_animation_input_without_waiting_for_graphics() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        AnimationPlugin,
        WorldPlugin,
    ));
    let prepared = PreparedWorld::from_cache("FIRST".into(), app.world().resource::<AssetServer>());
    let first = prepared.animations.clone();
    app.insert_resource(prepared).update();
    assert_eq!(app.world().resource::<CurrentScene>().name, "FIRST");
    assert_eq!(app.world().resource::<AnimationRequest>().0, first);
    let prepared =
        PreparedWorld::from_cache("SECOND".into(), app.world().resource::<AssetServer>());
    let second = prepared.animations.clone();
    app.insert_resource(prepared).update();
    assert_eq!(app.world().resource::<CurrentScene>().name, "SECOND");
    assert_eq!(app.world().resource::<AnimationRequest>().0, second);
    assert!(
        !app.world()
            .contains_resource::<bevy::ecs::message::Messages<cyrus_world::sound::SoundCue>>()
    );
}

#[test]
fn superseded_metadata_cannot_replace_the_new_world_identity() {
    use cyrus_assets::animation::Animations;
    use cyrus_world::lifecycle::WorldMetadataStatus;
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        AnimationPlugin,
        WorldPlugin,
    ));
    let mut first =
        PreparedWorld::from_cache("ALIAS".into(), app.world().resource::<AssetServer>());
    let data = Animations {
        version: 2,
        world: 17,
        metadata: default(),
        sections: default(),
        models: default(),
        unsupported: default(),
    };
    first.animations = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(data);
    let old = first.animations.clone();
    app.insert_resource(first).update();
    assert_eq!(app.world().resource::<CurrentScene>().name, "ALIAS");
    assert_eq!(app.world().resource::<CurrentScene>().world, Some(17));
    assert!(matches!(
        app.world().resource::<WorldMetadataStatus>(),
        WorldMetadataStatus::Loaded
    ));
    let second = PreparedWorld::from_cache("27".into(), app.world().resource::<AssetServer>());
    app.insert_resource(second).update();
    assert_eq!(app.world().resource::<CurrentScene>().world, None);
    assert_eq!(app.world().resource::<CurrentScene>().name, "27");
    // The first sidecar remains available, but it no longer owns the current identity.
    assert!(
        app.world()
            .resource::<Assets<Animations>>()
            .get(&old)
            .is_some()
    );
    app.update();
    assert_eq!(app.world().resource::<CurrentScene>().world, None);
}

#[test]
fn loaded_metadata_does_not_retrigger_world_change_consumers() {
    use cyrus_assets::animation::Animations;
    use cyrus_world::lifecycle::WorldChange;
    #[derive(Resource, Default)]
    struct Changes(usize);
    fn observe(current: Res<CurrentScene>, mut changes: ResMut<Changes>) {
        if current.is_changed() {
            changes.0 += 1;
        }
    }
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        AnimationPlugin,
        WorldPlugin,
    ))
    .init_resource::<Changes>()
    .add_systems(Update, observe.after(WorldChange));
    let mut prepared =
        PreparedWorld::from_cache("ALIAS".into(), app.world().resource::<AssetServer>());
    prepared.animations = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(Animations {
            version: 2,
            world: 17,
            metadata: default(),
            sections: default(),
            models: default(),
            unsupported: default(),
        });
    app.insert_resource(prepared).update();
    assert_eq!(app.world().resource::<Changes>().0, 1);
    app.update();
    assert_eq!(app.world().resource::<Changes>().0, 1);
}
