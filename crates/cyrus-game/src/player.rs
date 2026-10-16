//! Player state, input and deterministic controller rules.
pub mod contact;
pub mod controller;
pub mod data;
pub mod definition;
pub mod grounded;
pub mod input;
pub mod motion;
pub mod placement;
pub mod runtime;
pub mod support;
pub mod turn;
pub mod walk;
use crate::AppState;
use bevy::prelude::*;
#[derive(Component)]
pub struct Player;
pub struct PlayerPlugin;
impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        if app.world().contains_resource::<AssetServer>() {
            app.init_asset::<data::PlayerScene>()
                .init_asset_loader::<data::PlayerSceneLoader>()
                .add_systems(
                    Update,
                    data::request
                        .in_set(data::PlayerSceneLoading)
                        .after(cyrus_world::lifecycle::WorldChange),
                );
        }
        app.init_resource::<input::PlayerBindings>()
            .init_resource::<input::PlayerInput>()
            .init_resource::<input::DialogueSkip>()
            .init_resource::<input::InputSuppressed>()
            .init_resource::<placement::MarkerEntry>()
            .init_resource::<runtime::PlayerRuntime>()
            .add_message::<runtime::GroundedTick>()
            .add_systems(
                Update,
                runtime::step
                    .after(placement::enter_marker)
                    .before(cyrus_world::animation::AnimationSystems),
            )
            .add_systems(Update, input::sample.in_set(input::InputSample))
            .add_systems(OnEnter(AppState::InGame), spawn)
            .add_systems(OnExit(AppState::InGame), despawn)
            .add_systems(
                Update,
                placement::enter_marker
                    .in_set(placement::PlayerPlacement)
                    .after(cyrus_world::lifecycle::WorldChange)
                    .after(data::PlayerSceneLoading)
                    .run_if(in_state(AppState::InGame)),
            );
    }
}
fn spawn(mut commands: Commands, players: Query<(), With<Player>>) {
    if players.is_empty() {
        commands.spawn((Player, Transform::default()));
    }
}

fn despawn(mut commands: Commands, players: Query<Entity, With<Player>>) {
    for player in &players {
        commands.entity(player).despawn();
    }
}

#[cfg(test)]
mod entry_tests {
    use super::*;

    #[test]
    fn entry_preserves_a_published_player_and_pose() {
        let mut app = App::new();
        app.add_systems(Update, spawn);
        let player = app
            .world_mut()
            .spawn((Player, Transform::from_xyz(12.0, 34.0, 56.0)))
            .id();
        app.update();
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<Player>>()
                .iter(app.world())
                .collect::<Vec<_>>(),
            vec![player]
        );
        assert_eq!(
            app.world().get::<Transform>(player).unwrap().translation,
            Vec3::new(12.0, 34.0, 56.0)
        );
        app.world_mut().despawn(player);
        app.update();
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<Player>>()
                .iter(app.world())
                .count(),
            1
        );
    }
}
