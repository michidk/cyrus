//! Equipped weapon models, placed from the retained logical attachment pose.
use crate::{player_model::model_source, script_transform::scene_transform_at};
use bevy::{asset::AssetPath, prelude::*};
use cyrus_game::script::{Pose, ScriptActors, ScriptBinding};
use cyrus_world::{animation::AnimationPose, lifecycle::PreparedWorld};

/// Draws the hand or hilt model the script world retained for each bound actor.
pub struct WeaponModelPlugin;
impl Plugin for WeaponModelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            bind.after(cyrus_game::script::OrdinaryUpdates)
                .after(cyrus_world::animation::AnimationSystems),
        );
    }
}

/// A weapon model owned by one bound actor; the transform is in scene axes, outside the actor.
#[derive(Component)]
struct WeaponRoot {
    owner: Entity,
    source: AssetPath<'static>,
}

struct Wanted {
    source: AssetPath<'static>,
    transform: Transform,
    hidden: bool,
}

fn wanted(
    prepared: &PreparedWorld,
    actors: &ScriptActors,
    owner: &ScriptBinding,
) -> Option<Wanted> {
    let world = actors.current.as_ref()?;
    if owner.actor.generation != world.generation() {
        return None;
    }
    let weapon = world.equipped_weapon(owner.actor).ok()??;
    let hidden = world
        .actors()
        .get(owner.actor.index)?
        .control_state()
        .is_some_and(|control| control.hidden);
    let pose = Pose {
        position: [0; 3],
        angles: [0; 3],
        matrix: weapon.geometry.matrix,
    };
    Some(Wanted {
        source: model_source(prepared, &weapon.model, None),
        transform: Transform::from_matrix(Mat4::from(scene_transform_at(
            pose,
            weapon.geometry.position,
        ))),
        hidden,
    })
}

fn bind(
    mut commands: Commands,
    assets: Res<AssetServer>,
    prepared: Option<Res<PreparedWorld>>,
    actors: Option<Res<ScriptActors>>,
    owners: Query<(Entity, &ScriptBinding), With<AnimationPose>>,
    mut roots: Query<(Entity, &WeaponRoot, &mut Transform, &mut Visibility)>,
) {
    let wanted: Vec<_> = prepared
        .as_deref()
        .zip(actors.as_deref())
        .map(|(prepared, actors)| {
            owners
                .iter()
                .filter_map(|(entity, binding)| Some((entity, wanted(prepared, actors, binding)?)))
                .collect()
        })
        .unwrap_or_default();
    for (root, weapon, mut transform, mut visibility) in &mut roots {
        match wanted
            .iter()
            .find(|(owner, want)| *owner == weapon.owner && want.source == weapon.source)
        {
            Some((_, want)) => {
                *transform = want.transform;
                *visibility = if want.hidden {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                };
            }
            None => commands.entity(root).despawn(),
        }
    }
    for (owner, want) in wanted {
        if roots
            .iter()
            .any(|(_, root, _, _)| root.owner == owner && root.source == want.source)
        {
            continue;
        }
        commands.spawn((
            WeaponRoot {
                owner,
                source: want.source.clone(),
            },
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(want.source))),
            want.transform,
            Visibility::Hidden,
        ));
    }
}
