//! Presentation observations at committed actor lifecycle writes, independent of health loss.
use super::{ScriptActorId, ScriptWorld};
use crate::script::{AiType, Pose};
use anyhow::Result;
use std::collections::VecDeque;

const LIMIT: usize = 4096;

/// Current logical lifetime. Visibility and removal do not change the dead selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorPresence {
    Visible,
    Hidden,
    Removed,
}

/// Read-only state of one generation-checked actor. Restored corpses appear here without events.
#[derive(Clone, Debug)]
pub struct ActorSnapshot {
    pub actor: ScriptActorId,
    pub name: String,
    /// Presentation incarnation. Explicit resurrection expires earlier pose ownership.
    pub revision: u64,
    /// Committed lifecycle selector, independent of the health byte and IsDead's player veto.
    pub dead: bool,
    pub presence: ActorPresence,
    pub player: bool,
    /// Current geometry key and frame, absent before geometry preparation or after removal.
    pub model: Option<(String, usize)>,
    /// Captured animation reference for presentation adapters when a model is not yet published.
    pub frame_reference: Option<u32>,
    /// Captured model orientation and displaced map-unit position, independent of later movement.
    pub presentation: Option<(Pose, [f32; 3])>,
    /// Query coordinates in original Q8 units and axes, when available.
    pub pose: Option<Pose>,
}

/// One transition into the dead lifecycle. Retained even when a later service fails.
#[derive(Clone, Debug)]
pub struct DeathEvent {
    pub snapshot: ActorSnapshot,
    /// Known attacker query position in original map units. Never derived from the view camera.
    pub source_position: Option<[f32; 3]>,
}

#[derive(Default)]
pub(super) struct LifecycleLog {
    events: VecDeque<(u64, DeathEvent)>,
    sequence: u64,
    revisions: std::collections::BTreeMap<usize, u64>,
}

impl LifecycleLog {
    pub(super) fn revive(&mut self, index: usize) {
        *self.revisions.entry(index).or_default() += 1;
    }
}

impl ScriptWorld {
    /// Snapshot validates the full logical identity. Hidden/removed actors remain distinguishable
    /// from stale generations, which return an error. Reading never synthesizes a death event.
    pub fn actor_snapshot(&self, id: ScriptActorId) -> Result<ActorSnapshot> {
        let index = self.resolve(id)?;
        let actor = &self.actors[index];
        let control = actor.control.as_ref();
        let presence = if actor.created_retired || control.is_some_and(|c| c.killed) {
            ActorPresence::Removed
        } else if control.is_some_and(|c| c.hidden || c.invisible) {
            ActorPresence::Hidden
        } else {
            ActorPresence::Visible
        };
        let model = actor.model.current.as_ref().and_then(|current| {
            current
                .animation
                .as_ref()
                .map(|frame| (frame.model.clone(), frame.frame))
                .or_else(|| {
                    actor
                        .model
                        .static_model
                        .as_ref()
                        .map(|model| (model.clone(), 0))
                })
        });
        Ok(ActorSnapshot {
            actor: id,
            name: actor.name().to_owned(),
            revision: self
                .lifecycle
                .revisions
                .get(&index)
                .copied()
                .unwrap_or_default(),
            dead: control.is_some_and(|c| c.ai_type == AiType::DEAD),
            presence,
            player: self.selected_player() == Some(id),
            model,
            frame_reference: actor
                .animation
                .as_ref()
                .and_then(|animation| animation.frame),
            presentation: actor
                .model
                .current
                .as_ref()
                .map(|model| (model.pose, model.position)),
            pose: self.spatial(id).ok().and_then(|spatial| spatial.effective),
        })
    }

    /// Consume up to 4096 recent committed transitions, oldest first. Not saved or replayed.
    pub fn take_death_events(&mut self) -> Vec<DeathEvent> {
        self.lifecycle
            .events
            .drain(..)
            .map(|(_, event)| event)
            .collect()
    }

    /// Call only after the caller has validated its control owner. Recording uses no fallible
    /// gameplay services, so observation cannot prevent an already-admitted selector write.
    pub(super) fn set_actor_ai_type(&mut self, index: usize, value: AiType) {
        let control = self.actors[index]
            .control
            .as_mut()
            .expect("validated lifecycle control");
        let transition = control.ai_type != AiType::DEAD && value == AiType::DEAD;
        let revived = control.ai_type == AiType::DEAD && value != AiType::DEAD;
        control.ai_type = value;
        if revived {
            self.lifecycle.revive(index);
        }
        if transition {
            let id = ScriptActorId {
                generation: self.generation,
                index,
            };
            let snapshot = self.actor_snapshot(id).expect("validated lifecycle actor");
            if self.lifecycle.events.len() == LIMIT {
                self.lifecycle.events.pop_front();
            }
            self.lifecycle.sequence += 1;
            self.lifecycle.events.push_back((
                self.lifecycle.sequence,
                DeathEvent {
                    snapshot,
                    source_position: None,
                },
            ));
        }
    }

    /// Attribute only events appended by the current combat service, never older deaths.
    pub(super) fn attribute_deaths(
        &mut self,
        first: u64,
        target: ScriptActorId,
        source: Option<[i32; 3]>,
    ) {
        for (sequence, event) in &mut self.lifecycle.events {
            if *sequence <= first {
                continue;
            }
            if event.snapshot.actor == target {
                event.source_position = source.map(|position| position.map(|q8| q8 as f32 / 256.0));
            }
        }
    }

    pub(super) fn death_observation_count(&self) -> u64 {
        self.lifecycle.sequence
    }
}
