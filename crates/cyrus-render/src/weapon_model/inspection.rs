//! Read-only correlation of logical attachment poses with rendered owners.
use super::{WeaponRoot, wanted};
use anyhow::{Context, Result, ensure};
use bevy::prelude::{Entity, GlobalTransform, Transform, Visibility, World};
use cyrus_script::script::ScriptActorId;
use cyrus_script::script::{ScriptActors, ScriptBinding};
use cyrus_world::{animation::AnimationPose, lifecycle::PreparedWorld};
use serde_json::{Value, json};

/// Inspect one current actor after presentation updates. Positions and matrices
/// use scene axes, exactly as the production attachment renderer does. No
/// animation advances, assets are requested or missing roots reconstructed.
pub fn inspect_actor_model(world: &World, actor: ScriptActorId) -> Result<Value> {
    let actors = world
        .get_resource::<ScriptActors>()
        .context("rendered actor: script worlds absent")?;
    let current = actors
        .current
        .as_ref()
        .context("rendered actor: current world absent")?;
    let logical = current.model_presentation(actor)?;
    let name = current
        .actors()
        .get(actor.index)
        .context("rendered actor: actor absent")?
        .name();
    let owners: Vec<_> = world
        .try_query::<(Entity, &ScriptBinding)>()
        .map(|mut query| {
            query
                .iter(world)
                .filter(|(_, binding)| binding.actor == actor)
                .map(|(entity, _)| entity)
                .collect()
        })
        .unwrap_or_default();
    ensure!(
        owners.len() == 1,
        "world {} actor {} {name}: expected one rendered owner, found {}",
        current.world,
        actor.index,
        owners.len()
    );
    let owner = owners[0];
    let binding = world
        .get::<ScriptBinding>(owner)
        .context("rendered actor binding absent")?;
    let prepared = world
        .get_resource::<PreparedWorld>()
        .context("rendered actor: prepared world absent")?;
    let expected: Vec<_> = wanted(prepared, actors, binding).into_iter().map(|want| json!({
        "source": want.source.to_string(), "transform": want.transform, "global_transform": want.transform.to_matrix(), "hidden": want.hidden,
    })).collect();
    let actual: Vec<_> = world.try_query::<(Entity, &WeaponRoot, &Transform, &Visibility)>()
        .map(|mut query| query.iter(world).filter(|(_, root, _, _)| root.owner == owner)
             .map(|(entity, root, transform, visibility)| json!({
                 "entity": entity.to_bits(), "owner": root.owner.to_bits(),
                 "source": root.source.to_string(), "transform": transform,
                 "global_transform": world.get::<GlobalTransform>(entity).map(GlobalTransform::to_matrix),
                 "hidden": *visibility == Visibility::Hidden,
             })).collect()).unwrap_or_default();
    let pose = world
        .get::<AnimationPose>(owner)
        .map(|pose| json!({"model": pose.model, "frame": pose.frame}));
    Ok(json!({
        "world": current.world, "actor": actor, "name": name, "owner": owner.to_bits(),
        "logical_pose": logical.map(|model| json!({"animation": model.animation.as_ref().map(|frame| json!({"model": frame.model, "frame": frame.frame})), "position": model.position})),
        "displayed_pose": pose,
        "model": crate::player_model::PlayerModelSnapshot::capture(world, owner)?,
        "expected_attachments": expected, "attachments": actual,
    }))
}
