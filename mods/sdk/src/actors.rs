//! Opaque actor identity and committed lifecycle observations in scene coordinates.
use crate::{Pose, Vec3, bindings::cyrus::mods::components as raw};

/// Generation-scoped logical actor identity. Tokens cannot be constructed from actor names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorId(pub(crate) u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorState {
    Alive,
    Dead,
    HiddenAlive,
    HiddenDead,
    Removed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActorSnapshot {
    pub id: ActorId,
    pub state: ActorState,
    pub player: bool,
    /// Source geometry key, when prepared. This is a model identity, not an actor identity.
    pub model: Option<String>,
    pub frame: Option<u32>,
    pub pose: Option<Pose>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeathEvent {
    pub actor: ActorId,
    pub player: bool,
    pub model: Option<String>,
    pub frame: Option<u32>,
    pub pose: Option<Pose>,
    /// Known attacker query position in scene coordinates; unknown sources remain absent.
    pub source: Option<Vec3>,
}

pub struct Actors(pub(crate) raw::Actors);
impl Actors {
    pub fn ids(&self) -> Vec<ActorId> {
        self.0.ids().into_iter().map(ActorId).collect()
    }
    /// Stale generations and missing actors return None. Hidden and removed actors are explicit.
    pub fn snapshot(&self, id: ActorId) -> Option<ActorSnapshot> {
        let state = match self.0.state(id.0) {
            1 => ActorState::Alive,
            2 => ActorState::Dead,
            3 => ActorState::HiddenAlive,
            4 => ActorState::HiddenDead,
            5 => ActorState::Removed,
            _ => return None,
        };
        let model = self.0.model(id.0);
        Some(ActorSnapshot {
            id,
            state,
            player: self.0.player(id.0),
            model: (!model.is_empty()).then_some(model),
            frame: self.0.frame(id.0).first().copied(),
            pose: Pose::from_list(&self.0.pose(id.0)),
        })
    }
    /// Shared read-only events for this frame. Reading twice does not consume the batch.
    /// Raw health edits, loaded corpses and disappearance do not synthesize transitions.
    pub fn deaths(&self) -> Vec<DeathEvent> {
        (0..self.0.count())
            .filter_map(|index| {
                let actor = ActorId(*self.0.death(index).first()?);
                let source = match self.0.source(index).as_slice() {
                    &[x, y, z] => Some(Vec3::new(x, y, z)),
                    _ => None,
                };
                let model = self.0.eventmodel(index);
                Some(DeathEvent {
                    actor,
                    source,
                    player: self.0.eventplayer(index),
                    model: (!model.is_empty()).then_some(model),
                    frame: self.0.eventframe(index).first().copied(),
                    pose: Pose::from_list(&self.0.eventpose(index)),
                })
            })
            .collect()
    }
}

impl crate::Persist for ActorId {
    fn pack(&self) -> Vec<f32> {
        (0..4)
            .map(|i| ((self.0 >> (i * 16)) & 0xffff) as f32)
            .collect()
    }
    fn unpack(values: &[f32]) -> Option<Self> {
        if values.len() != 4
            || values
                .iter()
                .any(|v| !v.is_finite() || v.fract() != 0. || !(0.0..=65535.).contains(v))
        {
            return None;
        }
        let id = values
            .iter()
            .enumerate()
            .fold(0u64, |bits, (i, v)| bits | ((*v as u64) << (i * 16)));
        (id != 0).then_some(Self(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Persist;
    #[test]
    fn identity_persistence_preserves_all_bits_and_rejects_corruption() {
        for raw in [1, (1 << 53) + 3, u64::MAX] {
            let actor = ActorId(raw);
            assert_eq!(ActorId::unpack(&actor.pack()), Some(actor));
        }
        assert!(ActorId::unpack(&[0.; 4]).is_none());
        assert!(ActorId::unpack(&[1.5, 0., 0., 0.]).is_none());
        assert!(ActorId::unpack(&[f32::NAN; 4]).is_none());
    }
}
