//! Actor frame-source replacement through the shared native/browser GLB loader.
mod persistence;
use bevy::{asset::AssetPath, prelude::*};
use cyrus_game::{player::Player, script::ScriptBinding};
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
        app.add_systems(
            Update,
            (bind, base_visibility)
                .chain()
                .after(cyrus_world::animation::AnimationSystems),
        );
    }
}

type ModelOwners<'w, 's> = Query<
    'w,
    's,
    (Entity, Ref<'static, AnimationPose>, Option<&'static Name>),
    Or<(With<Player>, With<ScriptBinding>)>,
>;

fn bind(
    mut commands: Commands,
    assets: Res<AssetServer>,
    prepared: Option<Res<PreparedWorld>>,
    players: ModelOwners,
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
        if players
            .get(binding.player)
            .map_or(true, |(_, pose, name)| !replaces_base(name, &pose))
        {
            commands.entity(root).despawn();
        }
    }
    for (player, pose, name) in &players {
        if !replaces_base(name, &pose) {
            continue;
        }
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

// A map-local frame keeps its original geometry; a different source needs its own mesh mapping.
fn replaces_base(name: Option<&Name>, pose: &AnimationPose) -> bool {
    name.is_none_or(|name| name.as_str() != pose.model)
}

#[derive(Component)]
struct BaseModelVisibility(Visibility);

type BasePrimitives<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static ChildOf,
        &'static mut Visibility,
        Option<&'static BaseModelVisibility>,
    ),
    With<Mesh3d>,
>;

fn base_visibility(
    mut commands: Commands,
    owners: Query<(Option<&Name>, &AnimationPose), With<ScriptBinding>>,
    mut primitives: BasePrimitives,
) {
    for (entity, parent, mut visibility, saved) in &mut primitives {
        let replaced = owners
            .get(parent.parent())
            .is_ok_and(|(name, pose)| replaces_base(name, pose));
        match (replaced, saved) {
            (true, None) => {
                commands
                    .entity(entity)
                    .insert(BaseModelVisibility(*visibility));
                *visibility = Visibility::Hidden;
            }
            (false, Some(saved)) => {
                *visibility = saved.0;
                commands.entity(entity).remove::<BaseModelVisibility>();
            }
            (true, Some(_)) => *visibility = Visibility::Hidden,
            (false, None) => {}
        }
    }
}
