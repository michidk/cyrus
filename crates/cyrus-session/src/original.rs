//! Complete original logical state joined to independently prepared host owners.
use super::*;
use cyrus_script::script::{OriginalSessionImport, ScriptActorSource};
use cyrus_world::animation::AnimationPose;

/// Off-live runtime admission. The native snapshot supplies only retained process
/// presentation settings; every saved logical owner is replaced before asset staging.
pub struct OriginalRuntimeAdmission {
    pub(crate) snapshot: RuntimeSessionSnapshot,
    pub(crate) aliases: Vec<(ScriptActorId, ScriptActorId)>,
    pub(crate) recipes: OriginalRenderRecipes,
    logical: LogicalSessionCandidate,
    source_identities: BTreeMap<u32, String>,
}

impl RuntimeSessionSnapshot {
    /// Stage the entire original session before requesting any hidden host resources.
    /// Missing external owners reject here; no live logical or presentation state changes.
    /// The caller must supply the acknowledged process continuation, not a fresh baseline.
    pub fn prepare_original(
        mut self,
        prior: LogicalSessionRestoreInputs<'_>,
        original: OriginalSessionImport<'_>,
    ) -> Result<OriginalRuntimeAdmission> {
        ensure!(
            self.current_world() == original.slot.game.metadata.world_id,
            "original runtime admission requires matching retained process world"
        );
        let plan = self.logical.clone().plan(prior)?;
        let old_to_prior = plan.actor_bindings().to_vec();
        let slots = (0..16).map(|slot| (slot, slot)).collect();
        let prior = plan.finish(&slots)?;
        let sources = original.sources;
        let source_identities = sources
            .iter()
            .map(|(&id, source)| Ok((id, source.identity()?)))
            .collect::<Result<_>>()?;
        let (logical, prior_to_saved) = prior.prepare_original(original)?;
        // The original import replaces the logical camera; the host checkpoint's
        // publication and collision history belong to the superseded continuation.
        self.camera_frame = None;
        let aliases = old_to_prior
            .into_iter()
            .filter_map(|(old, prior)| {
                prior_to_saved
                    .iter()
                    .find(|(id, _)| *id == prior)
                    .map(|(_, saved)| (old, *saved))
            })
            .collect::<Vec<_>>();
        let current = logical
            .actors
            .current
            .as_ref()
            .context("original runtime current world absent")?;
        let source = &sources[&current.world];
        let recipes = self.prepare_original_entities(current, source, &aliases)?;
        self.prepare_original_player(current, &aliases)?;
        self.dialogue = self.dialogue.for_saved_world()?;
        self.logical = logical.actors.snapshot_session(
            sources,
            &logical.hud,
            &logical.clock,
            &logical.random,
            logical.toggles,
        )?;
        // Models are rebuilt only after the exact sidecars have loaded independently.
        self.player_model = None;
        self.actor_models.clear();
        Ok(OriginalRuntimeAdmission {
            snapshot: self,
            aliases,
            recipes,
            logical,
            source_identities,
        })
    }
    fn prepare_original_entities(
        &mut self,
        current: &cyrus_script::script::ScriptWorld,
        source: &WorldSnapshotSources<'_>,
        aliases: &[(ScriptActorId, ScriptActorId)],
    ) -> Result<OriginalRenderRecipes> {
        let mut recipes = OriginalRenderRecipes::default();
        let mut represented = std::collections::BTreeSet::new();
        for binding in &mut self.entity_bindings {
            let actor = aliases
                .iter()
                .find(|(old, _)| *old == binding.actor())
                .map(|(_, new)| *new)
                .context("original runtime external actor owner absent")?;
            ensure!(
                actor.generation == current.generation(),
                "original runtime entity belongs to visited world"
            );
            represented.insert(actor.index);
            let owner = &current.actors()[actor.index];
            ensure!(
                owner.name() == binding.name(),
                "original runtime actor name differs"
            );
            let model = current.model_presentation(actor)?;
            let pose = model
                .and_then(|model| model.animation.as_ref())
                .map(|frame| AnimationPose {
                    model: frame.model.clone(),
                    frame: frame.frame,
                });
            let hidden = owner
                .control_state()
                .is_some_and(|state| state.hidden || state.invisible)
                || current.outside_view_window(actor);
            let surface = owner
                .control_state()
                .map(|_| current.actor_surface(actor))
                .transpose()?;
            binding.set_original_display(pose.as_ref(), hidden, surface);
            if let Some(pose) = pose {
                let texture_set = cyrus_assets::animation::actor_texture_set(
                    source.animations.actor(owner.name())?,
                )?;
                if binding.saved_entity() == self.player.entity {
                    recipes.player = Some((pose, texture_set));
                } else if !matches!(owner.source, ScriptActorSource::Map(_))
                    || pose.model != entities::map_node_name(actor.index, owner.name())
                {
                    recipes
                        .actor_models
                        .insert(binding.saved_entity(), (pose, texture_set));
                }
            }
        }
        for (index, actor) in current.actors().iter().enumerate() {
            ensure!(
                // Camera presentation has its own CameraSnapshot/target census;
                // it is not a mesh ScriptBinding. Shells have no presentation owner.
                matches!(
                    actor.source,
                    ScriptActorSource::Shell(_) | ScriptActorSource::Camera
                ) || represented.contains(&index),
                "original runtime actor {} has no external owner",
                actor.name()
            );
        }
        Ok(recipes)
    }

    fn prepare_original_player(
        &mut self,
        current: &cyrus_script::script::ScriptWorld,
        aliases: &[(ScriptActorId, ScriptActorId)],
    ) -> Result<()> {
        let player = current
            .selected_player()
            .context("original runtime player absent")?;
        ensure!(
            aliases.iter().any(
                |(old, new)| (old.generation, old.index) == self.player.actor && *new == player
            ),
            "original runtime selected player owner differs"
        );
        let spatial = current.spatial(player)?;
        let pose = spatial.query_pose()?;
        let projection = match current.model_presentation(player)? {
            Some(model) => {
                cyrus_render::script_transform::scene_transform_at(model.pose, model.position)
            }
            None => cyrus_render::script_transform::scene_transform(pose),
        };
        let marker = self.player.placement.marker_entry();
        self.player.placement = PlayerPlacementSnapshot::capture(
            current.world,
            marker,
            &PlayerPose {
                position: pose.position.map(i64::from),
                heading: cyrus_world::heading::wrap(pose.angles[1]) as u16,
            },
            &PlayerEntrySupport::RestoredContinuation,
            &Transform::from_translation(projection.translation.into()),
        )?;
        self.player.runtime = PlayerRuntimeSnapshot::original_continuation(
            self.player.entity,
            self.player.actor.0,
            marker,
        );
        if let Some(camera) = self.camera.take() {
            let frame = cyrus_script::script::CameraFrameState::from_restored_world(current)?;
            self.camera = Some(camera.with_logical_view(&frame, current.generation())?);
        }
        Ok(())
    }
}

impl OriginalRuntimeAdmission {
    /// Only retained preparation clips need assets; stopped playback labels do not.
    pub fn required_dialogue_labels(&self) -> std::collections::BTreeSet<[u8; 4]> {
        self.snapshot.dialogue.required_labels()
    }

    /// Source and presentation preparation hints; publication still validates every owner.
    pub fn snapshot(&self) -> &RuntimeSessionSnapshot {
        &self.snapshot
    }

    /// Join independently prepared resources through the same complete publication factory.
    pub fn plan<'a>(
        self,
        inputs: LogicalSessionRestoreInputs<'a>,
    ) -> Result<RuntimeSessionPlan<'a>> {
        ensure!(
            self.logical
                .actors
                .current
                .iter()
                .chain(self.logical.actors.visited_worlds().values())
                .all(|world| world.generation() > inputs.last_generation),
            "original runtime actor allocation advanced after preparation"
        );
        let identities = inputs
            .sources
            .iter()
            .map(|(&id, source)| Ok((id, source.identity()?)))
            .collect::<Result<_>>()?;
        ensure!(
            identities == self.source_identities,
            "original runtime prepared sources changed"
        );
        let world = self.snapshot.current_world();
        Ok(self.snapshot.assemble_plan(
            RuntimeLogicalPlan::Original(Box::new(self.logical)),
            identities,
            world,
            Some(self.aliases),
        ))
    }
}
