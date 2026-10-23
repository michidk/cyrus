//! Actor frame-source replacement through the shared native/browser GLB loader.
mod persistence;
use bevy::{asset::AssetPath, prelude::*};
use cyrus_assets::animation::Animations;
use cyrus_game::{
    player::Player,
    script::{ScriptActors, ScriptBinding},
};
use cyrus_world::{animation::AnimationPose, lifecycle::PreparedWorld};
pub use persistence::{PlayerModelCandidate, PlayerModelSnapshot};

#[derive(Component)]
struct ModelRoot {
    player: Entity,
    source: AssetPath<'static>,
    model: String,
    texture_set: Option<u16>,
}

/// A replaced model kept on screen until its successor has loaded, so a frame-source switch
/// never leaves the player invisible while the next scene loads.
#[derive(Component)]
struct RetiredRoot {
    player: Entity,
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
    (
        Entity,
        Ref<'static, AnimationPose>,
        Option<&'static Name>,
        Option<&'static ScriptBinding>,
    ),
    Or<(With<Player>, With<ScriptBinding>)>,
>;

// Bevy supplies independent scene, actor and asset inputs.
#[allow(clippy::too_many_arguments)]
fn bind(
    mut commands: Commands,
    assets: Res<AssetServer>,
    prepared: Option<Res<PreparedWorld>>,
    actors: Option<Res<ScriptActors>>,
    animations: Option<Res<Assets<Animations>>>,
    players: ModelOwners,
    roots: Query<(Entity, &ModelRoot)>,
    retired: Query<(Entity, &RetiredRoot)>,
    visibilities: Query<&Visibility>,
    children: Query<&Children>,
    nodes: Query<(&Name, Option<&AnimationPose>)>,
) {
    let Some(prepared) = prepared else {
        for root in roots
            .iter()
            .map(|(root, _)| root)
            .chain(retired.iter().map(|(root, _)| root))
        {
            commands.entity(root).despawn();
        }
        return;
    };
    for (old, owner) in &retired {
        let successor_shown = roots
            .iter()
            .find(|(_, binding)| binding.player == owner.player)
            .is_none_or(|(root, _)| {
                visibilities
                    .get(root)
                    .is_ok_and(|shown| *shown != Visibility::Hidden)
            });
        let old_hidden = visibilities
            .get(old)
            .is_ok_and(|shown| *shown == Visibility::Hidden);
        if successor_shown || old_hidden || players.get(owner.player).is_err() {
            commands.entity(old).despawn();
        }
    }
    for (root, binding) in &roots {
        if players
            .get(binding.player)
            .map_or(true, |(_, pose, name, _)| !replaces_base(name, &pose))
        {
            commands.entity(root).despawn();
        }
    }
    for (player, pose, name, binding) in &players {
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
        let texture_set = match binding
            .map(|binding| texture_set(binding, actors.as_deref(), animations.as_deref()))
        {
            None => None,
            Some(Ok(Some(set))) => set,
            // The actor's skin is not known yet; loading the default skin would flash.
            Some(Ok(None)) => continue,
            Some(Err(error)) => {
                if pose.is_changed() {
                    error!("player model {}: {error:#}", pose.model);
                }
                continue;
            }
        };
        let source = model_source(&prepared, &pose.model, texture_set);
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
            retire(&mut commands, root, player);
        }
        commands.entity(player).insert_if_new(Visibility::Inherited);
        commands.spawn((
            ModelRoot {
                player,
                source: source.clone(),
                model: pose.model.clone(),
                texture_set,
            },
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(source))),
            Transform::default(),
            Visibility::Hidden,
            ChildOf(player),
        ));
    }
}

fn retire(commands: &mut Commands, root: Entity, player: Entity) {
    commands
        .entity(root)
        .remove::<ModelRoot>()
        .insert(RetiredRoot { player });
}

/// The bound actor's texture set, or `None` while its world or animation data is unavailable
/// (the outer `Option`); the inner one is the set itself.
fn texture_set(
    binding: &ScriptBinding,
    actors: Option<&ScriptActors>,
    animations: Option<&Assets<Animations>>,
) -> anyhow::Result<Option<Option<u16>>> {
    // Without a script registry nothing names a skin; the model's own textures apply.
    let Some(actors) = actors else {
        return Ok(Some(None));
    };
    let (Some(world), Some(data)) = (
        actors.current.as_ref(),
        animations.and_then(|animations| animations.get(&binding.source)),
    ) else {
        return Ok(None);
    };
    if world.generation() != binding.actor.generation {
        return Ok(None);
    }
    let Some(actor) = world.actors().get(binding.actor.index) else {
        return Ok(None);
    };
    cyrus_assets::animation::actor_texture_set(data.actor(actor.name())?).map(Some)
}

pub(crate) fn model_source(
    prepared: &PreparedWorld,
    model: &str,
    texture_set: Option<u16>,
) -> AssetPath<'static> {
    AssetPath::from(
        prepared
            .scene_path
            .path()
            .with_extension("player-models")
            .join(cyrus_assets::animation::frame_source_file(
                model,
                texture_set,
            )),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> ScriptBinding {
        ScriptBinding {
            actor: cyrus_game::script::ScriptActorId {
                generation: 1,
                index: 0,
            },
            source: Handle::default(),
        }
    }

    #[test]
    fn bound_actor_waits_for_its_world_before_choosing_a_skin() {
        let assets = Assets::<Animations>::default();
        // No script registry: nothing names a skin, so the model's own textures apply.
        assert_eq!(
            texture_set(&binding(), None, Some(&assets)).unwrap(),
            Some(None)
        );
        // A registry without a world or animation data must not load the default skin yet.
        let actors = ScriptActors::default();
        assert_eq!(
            texture_set(&binding(), Some(&actors), Some(&assets)).unwrap(),
            None
        );
    }
}
