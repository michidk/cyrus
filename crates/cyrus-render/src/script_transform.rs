//! Bound script actors retain their original affine matrices through scene presentation.
use bevy::{
    camera::visibility::VisibilitySystems, math::Affine3A, platform::collections::HashSet,
    prelude::*,
};
use cyrus_script::script::{Pose, ScriptActors, ScriptBinding};

/// Applies explicit script poses after ordinary scene transform propagation.
pub struct ScriptTransformPlugin;
impl Plugin for ScriptTransformPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            visibility.after(cyrus_script::script::OrdinaryUpdates),
        );
        app.add_systems(
            PostUpdate,
            apply
                .after(TransformSystems::Propagate)
                .before(VisibilitySystems::CheckVisibility)
                .before(bevy::asset::AssetEventSystems),
        );
    }
}

#[derive(Component)]
struct AppliedScriptVisibility(Visibility);

// Visibility ownership must expire with the logical actor binding.
#[allow(clippy::type_complexity)]
fn visibility(
    mut commands: Commands,
    actors: Option<Res<ScriptActors>>,
    cheats: Option<Res<cyrus_game::cheats::Cheats>>,
    mut nodes: Query<
        (
            Entity,
            Option<&ScriptBinding>,
            Option<&AppliedScriptVisibility>,
            &mut Visibility,
        ),
        Or<(With<ScriptBinding>, With<AppliedScriptVisibility>)>,
    >,
) {
    for (entity, binding, applied, mut visibility) in &mut nodes {
        let mut outside_window = false;
        let control = binding.and_then(|binding| {
            let world = actors.as_ref()?.current.as_ref()?;
            if binding.actor.generation != world.generation() {
                return None;
            }
            outside_window = world.outside_view_window(binding.actor);
            world.actors().get(binding.actor.index)?.control_state()
        });
        // With the animation cheat at zero the selected player's animated body is not drawn.
        let player_hidden = binding
            .and_then(|binding| {
                let world = actors.as_ref()?.current.as_ref()?;
                (binding.actor.generation == world.generation()
                    && world.selected_player() == Some(binding.actor))
                .then_some(())
            })
            .is_some()
            && cheats
                .as_ref()
                .is_some_and(|cheats| cheats.animated_player_hidden().unwrap_or(false));
        let Some(control) = control else {
            if let Some(applied) = applied {
                *visibility = applied.0;
                commands.entity(entity).remove::<AppliedScriptVisibility>();
            }
            continue;
        };
        if applied.is_none() {
            commands
                .entity(entity)
                .insert(AppliedScriptVisibility(*visibility));
        }
        let next = if control.hidden || control.invisible || player_hidden || outside_window {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *visibility != next {
            *visibility = next;
        }
    }
}

#[derive(Component)]
pub(crate) struct AppliedScriptTransform;

/// Converts original axes to rgpre scene axes without decomposing the matrix.
/// Position truncation precedes conversion, per preservation's family-render rules.
pub fn scene_transform(pose: Pose) -> Affine3A {
    scene_transform_at(pose, pose.position.map(|value| (value >> 8) as f32))
}

pub(crate) fn scene_transform_at(pose: Pose, model_position: [f32; 3]) -> Affine3A {
    let sign = [-1.0, -1.0, 1.0];
    let matrix = Mat3::from_cols_array(&std::array::from_fn::<_, 9, _>(|index| {
        let (row, column) = (index % 3, index / 3);
        sign[row] * pose.matrix.0[row][column] * sign[column]
    }));
    let mut position = Vec3::from_array(std::array::from_fn(|axis| {
        sign[axis] * model_position[axis] / 20.0
    }));
    // rgpre offsets scene Z to align authored placements with the exported terrain.
    position.z = -((16_777_215.0 - f64::from(model_position[2]) * 256.0) as f32) * (1.0 / 5120.0);
    Affine3A::from_mat3_translation(matrix, position)
}

/// The number of ancestors of `entity`, or `None` when the parent chain loops back on itself.
fn hierarchy_depth(hierarchy: &Hierarchy, entity: Entity) -> Option<usize> {
    let mut seen = HashSet::new();
    let mut current = entity;
    while let Ok((Some(parent), _, _)) = hierarchy.get(current) {
        if !seen.insert(current) {
            return None;
        }
        current = parent.parent();
    }
    Some(seen.len())
}

type Hierarchy<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static ChildOf>,
        Option<&'static Children>,
        Option<&'static Transform>,
    ),
>;

// Bound roots and ordinary descendants have different component requirements.
#[allow(clippy::type_complexity)]
pub(crate) fn apply(
    mut commands: Commands,
    actors: Option<Res<ScriptActors>>,
    nodes: Query<
        (
            Entity,
            Option<&ScriptBinding>,
            Option<&AppliedScriptTransform>,
        ),
        Or<(With<ScriptBinding>, With<AppliedScriptTransform>)>,
    >,
    hierarchy: Hierarchy,
    mut globals: Query<&mut GlobalTransform>,
    names: Query<&Name>,
    mut reported: Local<HashSet<Entity>>,
) {
    // Parents come before children. A node whose parent chain returns to itself has no root, so
    // transform propagation never reaches it either; it is skipped and reported once instead of
    // walking the chain forever.
    let mut roots: Vec<_> = nodes
        .iter()
        .filter_map(|node| match hierarchy_depth(&hierarchy, node.0) {
            Some(depth) => Some((depth, node)),
            None => {
                if reported.insert(node.0) {
                    error!(
                        "script transform: {:?} {:?} is its own ancestor; its pose is not applied",
                        node.0,
                        names.get(node.0).ok()
                    );
                }
                None
            }
        })
        .collect();
    roots.sort_by_key(|(depth, _)| *depth);
    let roots = roots.into_iter().map(|(_, node)| node);
    for (entity, binding, applied) in roots {
        let pose = binding.and_then(|binding| {
            let world = actors.as_ref()?.current.as_ref()?;
            match world.model_presentation(binding.actor).ok()? {
                Some(model) => Some(scene_transform_at(model.pose, model.position)),
                None => world
                    .spatial(binding.actor)
                    .ok()?
                    .query_pose()
                    .ok()
                    .map(scene_transform),
            }
        });
        if pose.is_none() && applied.is_none() {
            continue;
        }
        let Ok((parent, _, local)) = hierarchy.get(entity) else {
            continue;
        };
        let Some(local) = local else {
            continue;
        };
        let parent = parent
            .and_then(|parent| globals.get(parent.parent()).ok())
            .copied()
            .unwrap_or_default();
        let global = if let Some(pose) = pose {
            if applied.is_none() {
                commands.entity(entity).insert(AppliedScriptTransform);
            }
            parent * GlobalTransform::from(Mat4::from(pose))
        } else {
            commands.entity(entity).remove::<AppliedScriptTransform>();
            parent * *local
        };
        let mut pending = vec![(entity, global)];
        while let Some((entity, global)) = pending.pop() {
            // An unchanged pose must not mark its subtree changed: visibility, bounds,
            // extraction and camera clipping redo their work for changed transforms.
            if let Ok(mut target) = globals.get_mut(entity) {
                target.set_if_neq(global);
            }
            let Ok((_, Some(children), _)) = hierarchy.get(entity) else {
                continue;
            };
            for &child in children {
                if let Ok((_, _, Some(local))) = hierarchy.get(child) {
                    pending.push((child, global * *local));
                }
            }
        }
    }
}
