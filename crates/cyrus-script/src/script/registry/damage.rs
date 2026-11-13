//! Bounded observations of actual health loss, separate from damage requests and gameplay state.
use super::{ScriptActorId, ScriptWorld};
use cyrus_soup::Attribute;
use std::collections::VecDeque;

const LIMIT: usize = 4096;

/// One accepted health reduction, captured at the health service's publication point.
/// This is an observation, not an instruction to apply damage or dispatch death.
#[derive(Clone, Debug, PartialEq)]
pub struct DamageEvent {
    pub actor: ScriptActorId,
    pub name: String,
    /// Health actually lost, after the service's clamping and inhibition rules.
    pub loss: u8,
    /// Query position in map units and original axes, if spatial state was available.
    pub position: Option<[f32; 3]>,
    /// Attacker position in map units when a combat source is known. None for
    /// environmental damage and health services without an attributable source.
    pub source_position: Option<[f32; 3]>,
}

#[derive(Default)]
pub(super) struct DamageLog(VecDeque<DamageEvent>);

impl DamageLog {
    fn push(&mut self, event: DamageEvent) {
        if self.0.len() == LIMIT {
            self.0.pop_front();
        }
        self.0.push_back(event);
    }
}

impl ScriptWorld {
    /// Consume accepted health-loss observations, oldest first. At most 4096 recent
    /// events are retained. Transient observations are neither saved nor restored.
    /// The presentation adapter distributes this batch to all interested mods.
    pub fn take_damage_events(&mut self) -> Vec<DamageEvent> {
        self.damage.0.drain(..).collect()
    }

    /// Attach a known combat source only to the health reduction just observed.
    pub(super) fn attribute_damage(&mut self, target: ScriptActorId, before: u8, source: [i32; 3]) {
        if self.actors[target.index].attributes[Attribute::Health] >= before {
            return;
        }
        if let Some(event) = self
            .damage
            .0
            .back_mut()
            .filter(|event| event.actor == target)
        {
            event.source_position = Some(source.map(|q8| q8 as f32 / 256.0));
        }
    }

    pub(super) fn observe_health_loss(&mut self, index: usize, before: u8) {
        let after = self.actors[index].attributes[Attribute::Health];
        if after >= before {
            return;
        }
        let actor = ScriptActorId {
            generation: self.generation,
            index,
        };
        let position = self
            .query_position(actor)
            .ok()
            .map(|position| position.map(|q8| q8 as f32 / 256.0));
        self.damage.push(DamageEvent {
            actor,
            name: self.actors[index].name().to_owned(),
            loss: before - after,
            position,
            source_position: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_history_is_bounded_and_keeps_newest_events_in_order() {
        let mut log = DamageLog::default();
        for index in 0..LIMIT + 2 {
            log.push(DamageEvent {
                actor: ScriptActorId {
                    generation: 1,
                    index,
                },
                name: String::new(),
                loss: 1,
                position: None,
                source_position: None,
            });
        }
        assert_eq!(log.0.len(), LIMIT);
        assert_eq!(log.0.front().unwrap().actor.index, 2);
        assert_eq!(log.0.back().unwrap().actor.index, LIMIT + 1);
    }
}
