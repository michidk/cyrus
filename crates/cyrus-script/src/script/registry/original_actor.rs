//! Retained actor values bind before any native receiver is changed.
use super::*;
use crate::script::ScriptControl;
use rgpre::import::savegame::{
    SaveActorBinding, SaveActorController, SaveActorOwners, SaveActorSpatial, SaveObject,
};

impl ScriptWorld {
    /// Rebuild ordinary cells after actor restoration, in serialized SMOB order.
    ///
    /// Bindings must retain file order and include every ordinary cell owner.
    /// Cells use restored effective Q8 positions; insertion prepends each owner.
    /// Persistent and separate installed lists remain independent. Validation is
    /// atomic and runs no callbacks. See preservation SAVEGAME.md, world spatial
    /// and family tables, and commit e799ac8d1107f34217c984d3f7fc6b595a4cbbf4.
    pub fn restore_original_actor_lists(
        &mut self,
        bindings: &[(ScriptActorId, &SaveObject)],
    ) -> Result<()> {
        let mut seen = BTreeSet::new();
        let mut cells = Vec::new();
        for &(id, object) in bindings {
            let index = self.resolve(id)?;
            let context = format!(
                "world {} original actor {} saved owner {} spatial list",
                self.world, index, object.object_id
            );
            anyhow::ensure!(seen.insert(index), "{context}: duplicate receiver");
            let actor = &self.actors[index];
            let expected = match &actor.source {
                ScriptActorSource::Player => -2,
                ScriptActorSource::AlternatePlayer => -3,
                ScriptActorSource::Camera => 2,
                ScriptActorSource::Map(placement)
                    if !placement.object_type.has_separate_record() =>
                {
                    i32::from(
                        actor
                            .runtime_identifier
                            .context("original spatial runtime identity absent")?
                            as u16,
                    )
                }
                _ => anyhow::bail!("{context}: unsupported allocation"),
            };
            anyhow::ensure!(
                object.object_id == expected
                    && actor.runtime_identifier.map(|value| value as u16)
                        == Some(object.runtime_identifier()),
                "{context}: saved identity differs from installed receiver"
            );
            if let ScriptActorSource::Map(placement) = &actor.source {
                let saved = object.actor_state().with_context(|| context.clone())?;
                anyhow::ensure!(
                    (saved.controller.active == 0) == (placement.active == 0),
                    "{context}: changed persistent membership unsupported"
                );
                if placement.active == 0 {
                    let position = if actor.attributes()[Attribute::Slave] == 0 {
                        saved.spatial.position_q8
                    } else {
                        saved.spatial.attached_position_q8
                    };
                    let position = self.spatial(id)?.linked_position().unwrap_or(position);
                    cells.push((index, position));
                }
            }
        }
        let lists = super::super::order::ActorLists::from_original_cells(&self.actors, &cells)
            .with_context(|| format!("world {} original spatial lists", self.world))?;
        self.actor_lists = Some(lists);
        Ok(())
    }

    /// Restore retained actor spatial/controller fields as one component transaction.
    ///
    /// Saved identities must match independently initialized native allocations.
    /// Every reference, represented value and transform is checked before mutation.
    /// This neither runs callbacks nor admits a session: interpreter, animation,
    /// task, global and shared service replacements must join the same off-live
    /// candidate before session publication. Attached families require their own
    /// complete load binding and reject here; detached link caches are retained.
    pub fn restore_original_actor_states(
        &mut self,
        bindings: &[(ScriptActorId, &SaveObject)],
    ) -> Result<()> {
        let mut indices = BTreeSet::new();
        let mut initialized = Vec::with_capacity(bindings.len());
        let mut objects = Vec::with_capacity(bindings.len());
        for &(id, object) in bindings {
            let index = self.resolve(id)?;
            anyhow::ensure!(
                indices.insert(index),
                "original actor: duplicate native receiver {index}"
            );
            let actor = &self.actors[index];
            let object_id = match &actor.source {
                ScriptActorSource::Player => -2,
                ScriptActorSource::AlternatePlayer => -3,
                ScriptActorSource::Camera => 2,
                ScriptActorSource::Map(placement)
                    if !placement.object_type.has_separate_record() =>
                {
                    i32::from(
                        actor
                            .runtime_identifier
                            .context("original actor runtime identity absent")?
                            as u16,
                    )
                }
                _ => anyhow::bail!(
                    "world {} actor {index}: no original actor allocation",
                    self.world
                ),
            };
            initialized.push(SaveActorBinding {
                object_id,
                runtime_identifier: actor
                    .runtime_identifier
                    .context("original actor runtime identity absent")?
                    as u16,
            });
            objects.push(object.clone());
        }
        let owners = SaveActorOwners::bind(&objects, &initialized)
            .with_context(|| format!("world {} original actor ownership", self.world))?;
        let mut replacements = Vec::with_capacity(bindings.len());
        for &(id, object) in bindings {
            let context = format!(
                "world {} actor {} saved owner {}",
                self.world, id.index, object.object_id
            );
            let prepare = || -> Result<_> {
                let receiver = bindings[owners.receiver(object.object_id)?].0;
                anyhow::ensure!(
                    receiver == id,
                    "saved owner names another initialized receiver"
                );
                let state = object.actor_state()?;
                anyhow::ensure!(
                    state.references.parent.is_none() && state.references.linked.is_none(),
                    "original attached-family binding unsupported"
                );
                anyhow::ensure!(
                    object.attachment.is_none(),
                    "original rope attachment binding unsupported"
                );
                let translation = state.model_transform.translation();
                let matrix = state.model_transform.matrix();
                anyhow::ensure!(
                    translation
                        .iter()
                        .chain(matrix.iter().flatten())
                        .all(|v| v.is_finite()),
                    "saved model transform is nonfinite"
                );
                anyhow::ensure!(
                    self.spatial
                        .as_ref()
                        .is_some_and(|spatial| spatial.original_slot(id.index)),
                    "native spatial allocation absent"
                );
                let actor = &self.actors[id.index];
                actor
                    .control
                    .as_ref()
                    .context("native controller allocation absent")?;
                validate_controller(&state.controller)?;
                let attributes = object.attributes.map(Attributes::from).unwrap_or_else(|| {
                    self.shell_templates
                        .get(actor.name())
                        .map(|template| template.attributes.clone())
                        .unwrap_or_default()
                });
                let mut spatial = super::super::ActorSpatial::from_original(
                    &state,
                    &actor.location_offsets,
                    &attributes,
                )?;
                if let Some(link) = object.position_link()? {
                    let owner = link
                        .owner
                        .map(|owner| {
                            owners
                                .receiver(owner)
                                .map(|binding| bindings[binding].0.index)
                        })
                        .transpose()?;
                    spatial.restore_original_position_link(link, owner);
                }
                let support = state
                    .references
                    .support
                    .map(|owner| owners.receiver(owner).map(|binding| bindings[binding].0))
                    .transpose()?;
                Ok((spatial, state, attributes, support, translation, matrix))
            };
            replacements.push((id.index, prepare().with_context(|| context)?));
        }
        // All fallible work ends above this boundary.
        for (index, (spatial, retained, attributes, support, translation, matrix)) in replacements {
            self.spatial
                .as_mut()
                .unwrap()
                .replace_original_actor(index, spatial);
            let actor = &mut self.actors[index];
            actor.attributes = attributes;
            actor.surface = Some(Surface::from_byte(retained.controller.surface));
            actor.message_read = retained.controller.message_index;
            actor.broadcast_index = retained.controller.broadcast_index;
            apply_controller(
                actor.control.as_mut().unwrap(),
                retained.controller,
                retained.spatial,
            );
            actor.model.position = Some(translation);
            actor.model.matrix = Some(matrix);
            actor.model.support = model::ModelSupportOwners {
                actor: support,
                static_placement: None,
            };
            if let Some(current) = actor.model.current.as_mut() {
                current.pose.position = self
                    .spatial
                    .as_ref()
                    .unwrap()
                    .actor(index)
                    .unwrap()
                    .local
                    .position;
                current.pose.angles = self
                    .spatial
                    .as_ref()
                    .unwrap()
                    .actor(index)
                    .unwrap()
                    .local
                    .angles;
                current.pose.matrix = cyrus_world::transform::Matrix(matrix);
                current.position = translation;
            }
        }
        Ok(())
    }
}

fn validate_controller(state: &SaveActorController) -> Result<()> {
    for (field, value) in [
        ("active", state.active),
        ("hidden", state.hidden),
        ("dead", state.dead),
        ("invisible", state.invisible),
        ("weapon_drawn", state.weapon_drawn),
    ] {
        anyhow::ensure!(
            value <= 1,
            "saved controller {field}: nonboolean value {value} is not representable"
        );
    }
    Ok(())
}

fn apply_controller(
    control: &mut ActorControlState,
    state: SaveActorController,
    spatial: SaveActorSpatial,
) {
    control.script_control = ScriptControl::from_bits(state.script_control);
    control.yangle_change = state.heading_change;
    control.rope_wait = state.rope_wait;
    control.slide_flag = state.sliding_state;
    control.falling_state = JumpMotion::from_byte(state.falling_state);
    control.queued_motion = JumpMotion::from_byte(state.queued_motion);
    control.living_state = LivingState::from_byte(state.living_state);
    control.extra_time = state.extra_time;
    control.actions = [state.pending_action, state.accepted_action].into();
    control.jump_window = state.action_time;
    control.action_transition_counter = state.arms_time;
    control.jump_time = state.jump_time;
    control.jump_height = state.jump_height;
    control.fall_origin_y = state.fall_origin_q8;
    control.fall_timer = state.fall_timer;
    control.sliding_distance = state.accumulated_displacement_q8;
    control.motion_classification = MotionDirection::from_byte(state.last_direction);
    control.move_counter = state.movement_counter;
    control.last_collide = super::super::CollisionFlags::from_bits(state.collision_response);
    control.last_collide_count = state.collision_age;
    control.overlap_contact_identifier = state.overlap_identifier;
    control.surface_time = state.surface_time;
    control.ai_type = AiType::from_byte(state.ai_type);
    control.ai_mode = crate::script::AiMode::from_byte(state.ai_mode);
    control.ai_delay = state.ai_delay;
    control.ai = AiRecord {
        var1: state.ai_variables[0],
        var2: state.ai_variables[1],
        length: state.ai_length,
    };
    control.weapon_drawn = state.weapon_drawn != 0;
    control.torch_burning = state.torch_timer;
    control.torch_saved = state.extinguished_torch_timer;
    control.held_item = Some(state.held_item);
    control.motion_vector = state.motion_q8;
    control.ledge_point = state.ledge_position_q8;
    control.hand_point = state.hand_position_q8;
    control.hand_angles = state.hand_angles;
    control.grip_override = state.grip.map(Some);
    control.scabbard_override = state.scabbard.map(Some);
    control.object_update_excluded = state.active != 0;
    control.hidden = state.hidden != 0;
    control.killed = state.dead != 0;
    control.invisible = state.invisible != 0;
    control.anchor = state.anchor;
    control.stable_position_valid = spatial.stable_validity != 0;
    control.stable_local_position = spatial.stored_stable_position_q8;
}
