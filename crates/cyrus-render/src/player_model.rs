//! Player frame-source replacement through the shared native/browser GLB loader.
mod persistence;
use bevy::{asset::AssetPath, prelude::*};
use cyrus_game::player::Player;
use cyrus_world::{animation::AnimationPose, lifecycle::PreparedWorld};
pub use persistence::{PlayerModelCandidate, PlayerModelSnapshot};

#[derive(Component)]
struct ModelRoot {
    player: Entity,
    source: AssetPath<'static>,
    model: String,
}

/// Loads only a caller-selected pose; supplies no animation state or clock.
pub struct PlayerModelPlugin;
impl Plugin for PlayerModelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, bind.after(cyrus_world::animation::AnimationSystems));
    }
}

fn bind(
    mut commands: Commands,
    assets: Res<AssetServer>,
    prepared: Option<Res<PreparedWorld>>,
    players: Query<(Entity, Ref<AnimationPose>), With<Player>>,
    roots: Query<(Entity, &ModelRoot)>,
    children: Query<&Children>,
    nodes: Query<(&Name, Option<&AnimationPose>)>,
) {
    let Some(prepared) = prepared else {
        for (root, _) in &roots {
            commands.entity(root).despawn();
        }
        return;
    };
    for (root, binding) in &roots {
        if players.get(binding.player).is_err() {
            commands.entity(root).despawn();
        }
    }
    for (player, pose) in &players {
        // A pose is an external component input; never turn a model key into a path traversal.
        if pose.model.is_empty()
            || !pose
                .model
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            if pose.is_changed() {
                error!("player model: invalid frame-source key {:?}", pose.model);
            }
            if let Some((root, _)) = roots.iter().find(|(_, binding)| binding.player == player) {
                commands.entity(root).despawn();
            }
            continue;
        }
        let source = model_source(&prepared, &pose.model);
        if let Some((root, binding)) = roots.iter().find(|(_, binding)| binding.player == player) {
            if binding.source == source && binding.model == pose.model {
                for child in children.iter_descendants(root) {
                    if let Ok((name, current)) = nodes.get(child)
                        && name.as_str() == pose.model
                        && current != Some(&*pose)
                    {
                        commands.entity(child).insert((*pose).clone());
                        commands.entity(root).insert(Visibility::Inherited);
                    }
                }
                continue;
            }
            commands.entity(root).despawn();
        }
        commands.entity(player).insert_if_new(Visibility::Inherited);
        commands.spawn((
            ModelRoot {
                player,
                source: source.clone(),
                model: pose.model.clone(),
            },
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(source))),
            Transform::default(),
            Visibility::Hidden,
            ChildOf(player),
        ));
    }
}

fn model_source(prepared: &PreparedWorld, model: &str) -> AssetPath<'static> {
    AssetPath::from(
        prepared
            .scene_path
            .path()
            .with_extension("player-models")
            .join(format!("{model}.glb")),
    )
    .with_source(prepared.scene_path.source().clone())
    .into_owned()
}
