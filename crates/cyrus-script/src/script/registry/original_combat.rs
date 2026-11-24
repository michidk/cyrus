//! Original combat allocations bind installed moves before retained values replace defaults.
use super::*;
use rgpre::import::savegame::SaveObject;

impl ScriptWorld {
    /// Allocate installed combat associations in original construction order,
    /// without animation initialization, script callbacks or combat updates.
    /// Call before restoring saved actor attributes.
    pub fn prepare_original_combat_allocations(
        &mut self,
        data: &Animations,
        construction_order: &[ScriptActorId],
    ) -> Result<()> {
        let mut seen = BTreeSet::new();
        let mut next_index = self.next_combat_index;
        let mut replacements = Vec::new();
        for &id in construction_order {
            let index = self.resolve(id)?;
            anyhow::ensure!(
                seen.insert(index),
                "original combat: duplicate allocation receiver {index}"
            );
            let actor = &self.actors[index];
            let control = actor
                .control
                .as_ref()
                .context("original combat: actor controller unavailable")?;
            anyhow::ensure!(
                control.combat.is_none(),
                "original combat: receiver {index} already allocated"
            );
            let contact = if actor.attributes[Attribute::Combat] != 0 {
                data.combat_contact_profile(actor.name())?
            } else {
                Default::default()
            };
            let combat = super::super::CombatControlState::initialize(
                &actor.attributes,
                None,
                &mut next_index,
                &self.combat_attacks,
                contact,
            )?;
            replacements.push((index, combat));
        }
        for (index, combat) in replacements {
            let control = self.actors[index]
                .control
                .as_mut()
                .expect("validated controller");
            control.combat_death_default = combat.as_ref().map(|_| 0);
            control.combat = combat;
        }
        self.next_combat_index = next_index;
        Ok(())
    }

    /// Restore every retained combat value onto matching installed allocations.
    /// No combat service runs. Actor and combat-list references validate before
    /// mutation; transient RTX presentation is cleared as in the original load.
    pub fn restore_original_combat(
        &mut self,
        bindings: &[(ScriptActorId, &SaveObject)],
    ) -> Result<()> {
        let mut receivers = BTreeMap::new();
        let mut seen = BTreeSet::new();
        for &(id, object) in bindings {
            let index = self.resolve(id)?;
            anyhow::ensure!(
                seen.insert(index),
                "original combat: duplicate receiver {index}"
            );
            anyhow::ensure!(
                receivers.insert(object.object_id, id).is_none(),
                "original combat: duplicate saved actor {}",
                object.object_id
            );
        }
        let mut replacements = Vec::new();
        for &(id, object) in bindings {
            let control = self.actors[id.index]
                .control
                .as_ref()
                .context("original combat controller absent")?;
            let Some(saved) = object.combat() else {
                anyhow::ensure!(
                    control.combat.is_none(),
                    "actor {}: saved combat allocation absent",
                    object.object_id
                );
                continue;
            };
            let initialized = control.combat.as_ref().with_context(|| {
                format!(
                    "actor {}: initialized combat allocation absent",
                    object.object_id
                )
            })?;
            anyhow::ensure!(
                saved.owner_id == object.object_id,
                "actor {}: saved combat owner {} disagrees",
                object.object_id,
                saved.owner_id
            );
            let target = match saved.target_id {
                -1 | 0 => None,
                target => Some(*receivers.get(&target).with_context(|| {
                    format!("actor {} combat target {target} absent", object.object_id)
                })?),
            };
            if saved.next_combat_id != u32::MAX {
                anyhow::ensure!(
                    self.actors.iter().any(|actor| actor
                        .control
                        .as_ref()
                        .and_then(|control| control.combat.as_ref())
                        .is_some_and(
                            |combat| combat.allocation_index as u32 == saved.next_combat_id
                        )),
                    "actor {}: combat-list identity {} absent",
                    object.object_id,
                    saved.next_combat_id
                );
            }
            let taunt_dialogue = if saved.taunt_dialogue == -1 {
                None
            } else {
                Some(
                    usize::try_from(saved.taunt_dialogue)
                        .context("original combat: negative dialogue identity")?,
                )
            };
            let replacement = super::super::CombatControlState {
                attack_indices: initialized.attack_indices,
                defense_indices: initialized.defense_indices,
                contact_profile: cyrus_assets::animation::CombatContactProfile {
                    vertex: saved.contact_vertex,
                    radius: saved.contact_radius_q8,
                },
                target,
                retained_target_position: saved.retained_target_position_q8,
                close_distance_passes: saved.close_distance_passes,
                lost_sight_ticks: saved.lost_sight_ticks,
                lost_sight_routing: saved.lost_sight_routing,
                flags: CombatFlags::from_bits(saved.restored_flags()),
                action: saved.action,
                cooldown: saved.cooldown,
                allocation_index: saved.allocation_index,
                attacker_selection_state: saved.attacker_selection_state,
                retained_action_indices: saved.retained_action_indices,
                taunt_cache: None,
                taunt_dialogue,
                taunt_active: saved.taunt_active,
                taunt_period: saved.taunt_period,
                spacing_ranges: saved.spacing_ranges_q8,
                spacing_period: saved.spacing_period,
                strength: saved.strength,
                armor: saved.armor,
                decision: super::super::combat::CombatDecision {
                    attack_state: super::super::combat::ExecutorState::from_word(
                        saved.executor_state,
                    ),
                    move_selector: saved.move_selector,
                    aggression: saved.aggression,
                    attack_pacing: saved.attack_pacing,
                    defence_pacing: saved.defence_pacing,
                    defence_counter: saved.defence_counter,
                },
                avoid_side: saved.avoid_side,
                avoid_mask: saved.avoid_mask,
                avoid_hold: saved.avoid_hold,
            };
            replacements.push((id.index, saved.death_default_group, replacement));
        }
        for (index, death_default, combat) in replacements {
            let control = self.actors[index]
                .control
                .as_mut()
                .expect("validated controller");
            control.combat_death_default = Some(death_default);
            control.combat = Some(combat);
        }
        Ok(())
    }
}
