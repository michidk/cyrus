//! Placed static models a provider may tilt about their base ([`crate::hooks::Scenery`]).
//!
//! A placed static model is named `S<index>_<model>` by the scene export. Each gets its rest
//! placement and the bottom of its bounds once its children's bounds are known; a tilt in
//! [`crate::hooks::SceneryTilts`] turns its drawn transform about that point, so its shadow and
//! depth follow, and an untilted piece returns to rest.
use crate::hooks::{Scenery, SceneryPiece, SceneryTilts};
use bevy::{camera::primitives::Aabb, prelude::*};

/// A placed static model whose bounds are not known yet, with its installed model name.
#[derive(Component)]
struct Pending(String);

/// A placed static model at rest, and the point it tilts about in its own space.
#[derive(Component)]
struct Rest {
    model: String,
    rest: Transform,
    pivot: Vec3,
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<Scenery>()
        .init_resource::<SceneryTilts>()
        .add_systems(Update, (find, settle, tilt).chain());
}

/// The installed model name of a placed static model's scene node.
fn placed_model(name: &str) -> Option<&str> {
    let (index, model) = name.split_once('_')?;
    let digits = index.strip_prefix('S')?;
    (!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())).then_some(model)
}

fn find(mut commands: Commands, named: Query<(Entity, &Name), Added<Name>>) {
    for (entity, name) in &named {
        if let Some(model) = placed_model(name.as_str()) {
            commands
                .entity(entity)
                .insert(Pending(model.to_ascii_uppercase()));
        }
    }
}

/// Gives pending pieces their rest placement and pivot, and rebuilds the list when pieces come or go.
fn settle(
    mut commands: Commands,
    pending: Query<(Entity, &Pending, &Transform, &Children), Without<Rest>>,
    bounds: Query<(&Aabb, &Transform), Without<Rest>>,
    pieces: Query<(Entity, &Rest)>,
    mut removed: RemovedComponents<Rest>,
    mut scenery: ResMut<Scenery>,
) {
    for (entity, piece, transform, children) in &pending {
        let Some(pivot) = children
            .iter()
            .filter_map(|child| bounds.get(child).ok())
            .map(|(aabb, local)| {
                let center = Vec3::from(aabb.center);
                local.transform_point(center - Vec3::Y * aabb.half_extents.y)
            })
            .reduce(|a, b| if a.y < b.y { a } else { b })
        else {
            continue;
        };
        commands.entity(entity).remove::<Pending>().insert(Rest {
            model: piece.0.clone(),
            rest: *transform,
            pivot,
        });
    }
    // Inserted pieces show up in the query next frame, which changes the count; a removal in
    // the same frame as an insertion is caught by the removal.
    let gone = removed.read().count() > 0;
    if gone || pieces.iter().count() != scenery.pieces.len() {
        scenery.revision += 1;
        scenery.pieces = pieces
            .iter()
            .map(|(entity, rest)| SceneryPiece {
                entity,
                model: rest.model.clone(),
                base: rest.rest.transform_point(rest.pivot),
            })
            .collect();
    }
}

/// Applies this frame's tilts and returns every other piece to rest.
fn tilt(
    mut scenery: ResMut<Scenery>,
    tilts: Res<SceneryTilts>,
    mut pieces: Query<(&Rest, &mut Transform, &GlobalTransform)>,
) {
    for (index, piece) in scenery.pieces.iter_mut().enumerate() {
        let Ok((rest, mut transform, global)) = pieces.get_mut(piece.entity) else {
            continue;
        };
        piece.base = global.transform_point(rest.pivot);
        let wanted = match u32::try_from(index).ok().and_then(|i| tilts.0.get(&i)) {
            Some(&rotation) => tilted(rest, rotation),
            None => rest.rest,
        };
        transform.set_if_neq(wanted);
    }
}

/// `rest` turned by the world-space `rotation` about its pivot. The pieces' parent is the
/// world-aligned scene root, so its space stands for world space here.
fn tilted(rest: &Rest, rotation: Quat) -> Transform {
    let pivot = rest.rest.transform_point(rest.pivot);
    Transform::from_translation(pivot)
        * Transform::from_rotation(rotation)
        * Transform::from_translation(-pivot)
        * rest.rest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placed_models_are_read_from_the_scene_names() {
        assert_eq!(placed_model("S001_PALMTR01"), Some("PALMTR01"));
        assert_eq!(placed_model("S040_NCTREE02"), Some("NCTREE02"));
        assert_eq!(placed_model("B_015_FIRESND"), None);
        assert_eq!(placed_model("F350_PALM"), None);
        assert_eq!(placed_model("S_PALM"), None);
    }

    #[test]
    fn a_tilt_keeps_the_pivot_in_place_and_untilted_pieces_rest() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        register(&mut app);
        let rest = Transform::from_xyz(10.0, 2.0, -4.0).with_rotation(Quat::from_rotation_y(0.7));
        let root = app
            .world_mut()
            .spawn((Name::new("S001_PALMTR01"), rest))
            .id();
        app.world_mut().spawn((
            ChildOf(root),
            Transform::default(),
            Aabb::from_min_max(Vec3::new(-1.0, -3.0, -1.0), Vec3::new(1.0, 5.0, 1.0)),
        ));
        for _ in 0..3 {
            app.update();
        }
        let scenery = app.world().resource::<Scenery>();
        assert_eq!(scenery.pieces.len(), 1);
        assert_eq!(scenery.pieces[0].model, "PALMTR01");
        let base = rest.transform_point(Vec3::new(0.0, -3.0, 0.0));
        app.world_mut()
            .resource_mut::<SceneryTilts>()
            .0
            .insert(0, Quat::from_rotation_x(0.05));
        app.update();
        let moved = *app.world().get::<Transform>(root).unwrap();
        assert!(!moved.rotation.abs_diff_eq(rest.rotation, 1e-6));
        let local_pivot = rest.rotation.inverse() * (base - rest.translation);
        assert!(moved.transform_point(local_pivot).abs_diff_eq(base, 1e-4));
        app.world_mut().resource_mut::<SceneryTilts>().0.clear();
        app.update();
        assert_eq!(*app.world().get::<Transform>(root).unwrap(), rest);
    }
}
