//! Saved pose allocations bind to installed actor frame tables before replacement.
use super::*;
use rgpre::import::savegame::{SaveModelAllocation, SaveObject};

impl ScriptWorld {
    /// Restore a set of animation owners together without executing any commands.
    /// The host must reconstruct allocations from the matching installed world
    /// with preservation's `SaveModelAllocations::from_world`. Both pose pairs
    /// must belong to each receiver's installed frame table and texture set.
    /// Every binding is validated before any receiver changes. This component
    /// transaction does not admit a session or replace its other service owners.
    pub fn restore_original_bound_animations(
        &mut self,
        bindings: &[(ScriptActorId, &SaveObject)],
        allocations: &[SaveModelAllocation],
        data: &Animations,
    ) -> Result<()> {
        anyhow::ensure!(
            data.world == self.world
                && data.metadata["script_content_id"].as_str() == Some(&self.content_id),
            "world {} original pose: installed animation generation differs",
            self.world
        );
        let mut receivers = BTreeSet::new();
        let mut owners = BTreeSet::new();
        let mut replacements = Vec::with_capacity(bindings.len());
        for &(id, object) in bindings {
            let index = self.resolve(id)?;
            let actor = &self.actors[index];
            let context = format!(
                "world {} actor {} saved owner {} pose",
                self.world,
                actor.name(),
                object.object_id
            );
            let mut prepare = || -> Result<_> {
                anyhow::ensure!(receivers.insert(index), "duplicate native receiver");
                anyhow::ensure!(owners.insert(object.object_id), "duplicate saved owner");
                let owner_matches = match actor.source {
                    ScriptActorSource::Player => object.object_id == -2,
                    ScriptActorSource::AlternatePlayer => object.object_id == -3,
                    ScriptActorSource::Map(_) => {
                        object.object_id >= 0
                            && actor.runtime_identifier.is_some_and(|identity| {
                                identity as u16 == object.runtime_identifier()
                            })
                    }
                    _ => false,
                };
                anyhow::ensure!(
                    owner_matches,
                    "saved identity differs from native allocation"
                );
                let header = object
                    .animation_header
                    .as_ref()
                    .context("saved playback allocation absent")?;
                let attributes = object
                    .attributes
                    .as_ref()
                    .context("saved attribute allocation absent")?;
                let pose = object.pose();
                let default = AnimationGroup::from_index(
                    u16::try_from(pose.default_group).context("invalid stored default group")?,
                );
                let bind = |pose: rgpre::import::savegame::SaveModelFrame| -> Result<Option<u32>> {
                    let Some(bound) = pose.bind(allocations)? else {
                        return Ok(None);
                    };
                    owner_reference(
                        actor,
                        index,
                        data,
                        &allocations[bound.allocation],
                        bound.frame,
                    )
                    .map(Some)
                };
                let selected = bind(pose.current).context("selected model/frame")?;
                let previous = bind(pose.previous).context("previous model/frame")?;
                let mut playback = cyrus_world::animation::ActorAnimation::from_asset(
                    data,
                    actor.name(),
                    default,
                )?;
                playback.restore_original_bound(&header.into(), selected, previous, default)?;
                Ok((
                    index,
                    ScriptAnimation {
                        playback,
                        frame: selected,
                    },
                    attributes[Attribute::PanicGroup.index()],
                    pose.default_group,
                ))
            };
            replacements.push(prepare().with_context(|| context)?);
        }
        for (index, animation, panic_group, default) in replacements {
            let actor = &mut self.actors[index];
            actor.animation = Some(animation);
            actor.attributes[Attribute::PanicGroup] = panic_group;
            actor.unprepared_animation_default = default;
        }
        Ok(())
    }
}

// Cache model keys identify the installed resolver's geometry. Original RAAN
// names may use a 3D suffix while the installed resource is compressed 3DC.
fn model_key(source: &str) -> String {
    let filename = source.rsplit(['/', '\\']).next().unwrap_or(source);
    filename
        .rsplit_once('.')
        .map_or(filename, |(stem, _)| stem)
        .to_ascii_uppercase()
}

fn owner_reference(
    actor: &ScriptActor,
    index: usize,
    data: &Animations,
    allocation: &SaveModelAllocation,
    frame: usize,
) -> Result<u32> {
    let key = model_key(&allocation.source);
    let model = data
        .models
        .get(&key)
        .context("registered geometry absent from installed cache")?;
    anyhow::ensure!(
        allocation.frame_count == model.positions.len(),
        "registered frame count differs from installed geometry"
    );
    let metadata = data.actor(actor.name())?;
    let entries = metadata["raan_entries"]
        .as_array()
        .filter(|entries| !entries.is_empty());
    let expected_texture = if entries.is_some() {
        cyrus_assets::animation::actor_texture_set(metadata)?.unwrap_or(0)
    } else {
        0
    };
    anyhow::ensure!(
        allocation.texture == expected_texture,
        "registered texture variant does not belong to receiver"
    );
    if let Some(entries) = entries {
        let mut offset = 0u32;
        let mut found = None;
        for entry in entries {
            let count = u32::try_from(
                entry["frame_count"]
                    .as_u64()
                    .context("installed RAAN frame count absent")?,
            )?;
            let source = entry["file_path"]
                .as_str()
                .context("installed RAAN source absent")?;
            if model_key(source) == key && frame < count as usize {
                let reference = offset
                    .checked_add(u32::try_from(frame)?)
                    .context("installed frame reference overflow")?;
                let resolved = data.resolve_frame(actor.name(), reference)?;
                anyhow::ensure!(
                    resolved.model == key && resolved.frame == frame,
                    "installed frame table resolves a different source"
                );
                anyhow::ensure!(
                    found.replace(reference).is_none(),
                    "registered frame has ambiguous owner-table references"
                );
            }
            offset = offset
                .checked_add(count)
                .context("installed frame table overflow")?;
        }
        return found.context("registered model/frame does not belong to receiver frame table");
    }
    anyhow::ensure!(
        matches!(actor.source, ScriptActorSource::Map(_)),
        "standalone receiver has no installed frame table"
    );
    let reference = u32::try_from(frame)?;
    let resolved = data.resolve_map_frame(index, reference)?;
    anyhow::ensure!(
        resolved.model == key && resolved.frame == frame,
        "registered model/frame differs from placed receiver allocation"
    );
    Ok(reference)
}
