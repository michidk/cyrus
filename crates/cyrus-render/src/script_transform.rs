//! Bound script actors retain their original affine matrices through scene presentation.
use bevy::{camera::visibility::VisibilitySystems, math::Affine3A, prelude::*};
use cyrus_game::script::{Pose, ScriptActors, ScriptBinding};

/// Applies explicit script poses after ordinary scene transform propagation.
pub struct ScriptTransformPlugin;
impl Plugin for ScriptTransformPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            apply
                .after(TransformSystems::Propagate)
                .before(VisibilitySystems::CheckVisibility),
        );
    }
}

#[derive(Component)]
struct AppliedScriptTransform;

/// Converts original axes to rgpre scene axes without decomposing the matrix.
/// Position truncation precedes conversion, per preservation's family-render rules.
pub fn scene_transform(pose: Pose) -> Affine3A {
    let sign = [-1.0, -1.0, 1.0];
    let matrix = Mat3::from_cols_array(&std::array::from_fn::<_, 9, _>(|index| {
        let (row, column) = (index % 3, index / 3);
        sign[row] * pose.matrix.0[row][column] * sign[column]
    }));
    let position = Vec3::from_array(std::array::from_fn(|axis| {
        sign[axis] * (pose.position[axis] >> 8) as f32 / 20.0
    }));
    Affine3A::from_mat3_translation(matrix, position)
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
fn apply(
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
) {
    let mut roots: Vec<_> = nodes.iter().collect();
    roots.sort_by_key(|(entity, _, _)| {
        let mut depth = 0;
        let mut current = *entity;
        while let Ok((Some(parent), _, _)) = hierarchy.get(current) {
            depth += 1;
            current = parent.parent();
        }
        depth
    });
    for (entity, binding, applied) in roots {
        let pose = binding.and_then(|binding| {
            let world = actors.as_ref()?.current.as_ref()?;
            world.spatial(binding.actor).ok()?.query_pose().ok()
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
            commands.entity(entity).insert(AppliedScriptTransform);
            parent * GlobalTransform::from(Mat4::from(scene_transform(pose)))
        } else {
            commands.entity(entity).remove::<AppliedScriptTransform>();
            parent * *local
        };
        let mut pending = vec![(entity, global)];
        while let Some((entity, global)) = pending.pop() {
            if let Ok(mut target) = globals.get_mut(entity) {
                *target = global;
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
