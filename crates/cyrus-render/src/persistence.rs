//! Presentation time continuation rebased independently of the host's real-time epoch.
pub(crate) mod scene;
pub use crate::console_environment::{EnvironmentEditsCandidate, EnvironmentEditsSnapshot};
pub use crate::presentation_clock::{PresentationClock, PresentationClockSnapshot};
pub use crate::sky::persistence::{SkyCandidate, SkyImages, SkySnapshot};
pub use scene::{SceneCandidate, SceneReplacement};

mod source;
pub use source::{SceneIdentityPlugin, SceneSourceIdentity};

/// Hidden replacement root excluded from live environment-dependent preparation.
/// Remove only when the prepared session is published with its own environment.
#[derive(bevy::prelude::Component)]
pub struct StagedRenderRoot;

/// A live model instance or its frame mesh is still being published.
/// Capture hosts may keep their request pending until a later rendered frame.
#[derive(Debug)]
pub struct PresentationPending(pub String);

impl std::fmt::Display for PresentationPending {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PresentationPending {}

/// Retire a rejected preliminary world before returning to the startup menu.
/// Unrelated UI and asset caches remain owned by their existing systems.
pub fn clear_scene(world: &mut bevy::prelude::World) {
    use bevy::prelude::*;
    let entities: Vec<_> = world
        .query_filtered::<Entity, Or<(
            With<crate::SceneEntity>,
            With<crate::sky::Sky>,
            With<crate::camera::ViewCamera>,
        )>>()
        .iter(world)
        .collect();
    for entity in entities {
        if world.get_entity(entity).is_ok() {
            world.despawn(entity);
        }
    }
    world.insert_resource(crate::environment::ActiveEnvironment::default());
    world.insert_resource(crate::environment::EnvironmentRequest::default());
    world.insert_resource(cyrus_world::animation::AnimationRequest::default());
    world.remove_resource::<crate::ReadyScene>();
    world.remove_resource::<scene::RestoredScene>();
}

#[cfg(test)]
mod clear_tests {
    use super::*;
    use bevy::prelude::*;

    #[test]
    fn rejected_scene_retirement_preserves_unrelated_menu_entities() {
        let mut world = World::new();
        let menu = world.spawn_empty().id();
        let scene = world.spawn(crate::SceneEntity).id();
        let child = world.spawn(ChildOf(scene)).id();
        clear_scene(&mut world);
        assert!(world.get_entity(menu).is_ok());
        assert!(world.get_entity(scene).is_err());
        assert!(world.get_entity(child).is_err());
        assert!(
            world
                .resource::<crate::environment::ActiveEnvironment>()
                .source
                .is_none()
        );
        assert!(!world.contains_resource::<crate::ReadyScene>());
    }
}

/// Environment presentation refreshes that must observe a complete published session.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnvironmentPresentation;
