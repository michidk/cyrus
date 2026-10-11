//! Player state, input and deterministic controller rules.
pub mod contact;
pub mod data;
pub mod definition;
pub mod grounded;
pub mod input;
pub mod motion;
pub mod placement;
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
            .init_resource::<input::InputSuppressed>()
            .init_resource::<placement::MarkerEntry>()
            .add_systems(Update, input::sample.in_set(input::InputSample))
            .add_systems(OnEnter(AppState::InGame), spawn)
            .add_systems(
                Update,
                placement::enter_marker
                    .after(cyrus_world::lifecycle::WorldChange)
                    .run_if(in_state(AppState::InGame)),
            );
    }
}
fn spawn(mut commands: Commands) {
    commands.spawn((Player, Transform::default()));
}
