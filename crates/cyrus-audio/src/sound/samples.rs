//! Synchronous sample-slot allocation; playback transport cannot own allocation order.
use super::Owner;

/// Identity of one transport start, distinct from the original reusable sample slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Reservation {
    pub slot: u8,
    pub token: u64,
}

struct Sample {
    reservation: Reservation,
    owner: Option<Owner>,
    effect: Option<i32>,
}

/// One allocator is shared by script, animation, ambient, speech and control starts.
/// A remembered engine handle names a reusable slot. Transport completion instead
/// names the individual start, so late backend callbacks cannot retire its replacement.
#[derive(Default)]
pub(super) struct SampleSlots {
    slots: [Option<Sample>; 16],
    next_token: u64,
}

impl SampleSlots {
    /// Reserve the first idle slot at or above the caller's current start index.
    /// No playing sample is stolen. Invalid start indices allocate nothing.
    pub fn start(
        &mut self,
        start_index: usize,
        owner: Option<Owner>,
        effect: Option<i32>,
    ) -> Result<Option<Reservation>, String> {
        let Some(slot) = (start_index..16).find(|&slot| self.slots[slot].is_none()) else {
            return Ok(None);
        };
        let token = self
            .next_token
            .checked_add(1)
            .ok_or("audio transport identity exhausted")?;
        let reservation = Reservation {
            slot: slot as u8,
            token,
        };
        self.next_token = token;
        self.slots[slot] = Some(Sample {
            reservation,
            owner,
            effect,
        });
        Ok(Some(reservation))
    }

    /// Missing and finished handles have status two; reserved starts have status four.
    /// Reservation is synchronous, before the backend processes its start command.
    pub fn status(&self, slot: Option<u8>) -> u32 {
        if slot
            .and_then(|slot| self.slots.get(usize::from(slot)))
            .is_some_and(Option::is_some)
        {
            4
        } else {
            2
        }
    }

    /// Current transport reservation for a remembered engine slot.
    pub fn current(&self, slot: u8) -> Option<Reservation> {
        self.slots
            .get(usize::from(slot))?
            .as_ref()
            .map(|sample| sample.reservation)
    }

    /// Stop the current occupant of an original slot, irrespective of earlier owners.
    /// Ambient and animation handles retain the original slot-number semantics.
    pub fn stop(&mut self, slot: u8) -> Option<Reservation> {
        self.slots
            .get_mut(usize::from(slot))?
            .take()
            .map(|sample| sample.reservation)
    }

    /// Retire only the backend start which actually finished.
    pub fn finish(&mut self, reservation: Reservation) {
        let Some(slot) = self.slots.get_mut(usize::from(reservation.slot)) else {
            return;
        };
        if slot
            .as_ref()
            .is_some_and(|sample| sample.reservation == reservation)
        {
            *slot = None;
        }
    }

    /// Ownership matching retains the authored effect before surface remapping.
    pub fn stop_matching(&mut self, owner: Option<Owner>, effect: Option<i32>) -> Vec<Reservation> {
        self.remove_where(|sample| {
            owner.is_none_or(|owner| sample.owner == Some(owner))
                && effect.is_none_or(|effect| sample.effect == Some(effect))
        })
    }

    pub fn stop_expired(&mut self, current: impl Fn(Owner) -> bool) -> Vec<Reservation> {
        self.remove_where(|sample| sample.owner.is_some_and(|owner| !current(owner)))
    }

    fn remove_where(&mut self, matches: impl Fn(&Sample) -> bool) -> Vec<Reservation> {
        self.slots
            .iter_mut()
            .filter_map(|slot| {
                if slot.as_ref().is_some_and(&matches) {
                    slot.take().map(|sample| sample.reservation)
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleaved_starts_share_capacity_and_never_steal() {
        let mut slots = SampleSlots::default();
        let ambient = slots.start(6, None, Some(10)).unwrap().unwrap();
        assert_eq!(ambient.slot, 6);
        let player = slots.start(0, None, Some(13)).unwrap().unwrap();
        assert_eq!(player.slot, 0);
        for expected in 7..16 {
            assert_eq!(
                slots.start(6, None, Some(10)).unwrap().unwrap().slot,
                expected
            );
        }
        assert_eq!(slots.start(6, None, Some(10)).unwrap(), None);
        assert_eq!(slots.status(Some(6)), 4);
        slots.finish(ambient);
        assert_eq!(slots.status(Some(6)), 2);
        assert_eq!(slots.start(6, None, Some(10)).unwrap().unwrap().slot, 6);
    }

    #[test]
    fn remembered_slot_stops_replacement_but_old_completion_does_not() {
        let mut slots = SampleSlots::default();
        let first = slots.start(6, None, Some(10)).unwrap().unwrap();
        slots.finish(first);
        let replacement = slots.start(6, None, Some(11)).unwrap().unwrap();
        slots.finish(first);
        assert_eq!(slots.status(Some(6)), 4);
        assert_eq!(slots.stop(first.slot), Some(replacement));
        assert_eq!(slots.status(Some(6)), 2);
    }

    #[test]
    fn cleanup_matches_logical_owner_and_authored_effect() {
        let mut slots = SampleSlots::default();
        let owner = Owner::Script {
            generation: 1,
            index: 2,
        };
        let other = Owner::Script {
            generation: 2,
            index: 2,
        };
        let first = slots.start(6, Some(owner), Some(10)).unwrap().unwrap();
        let second = slots.start(6, Some(other), Some(10)).unwrap().unwrap();
        let third = slots.start(6, Some(owner), Some(11)).unwrap().unwrap();
        assert_eq!(slots.stop_matching(Some(owner), Some(10)), vec![first]);
        assert_eq!(slots.stop_matching(None, Some(10)), vec![second]);
        assert_eq!(slots.stop_matching(Some(owner), None), vec![third]);
        assert_eq!(slots.status(None), 2);
        assert_eq!(slots.status(Some(255)), 2);
        assert_eq!(slots.start(16, None, None).unwrap(), None);
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct SampleSlotsSnapshot {
    version: u32,
    next_token: u64,
    slots: Vec<Option<SavedSample>>,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct SavedSample {
    token: u64,
    owner: Option<SavedOwner>,
    effect: Option<i32>,
}
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub(super) enum SavedOwner {
    Entity(u64),
    Script { generation: u64, index: usize },
}
impl From<Owner> for SavedOwner {
    fn from(owner: Owner) -> Self {
        match owner {
            Owner::Entity(entity) => Self::Entity(entity.to_bits()),
            Owner::Script { generation, index } => Self::Script { generation, index },
        }
    }
}
impl SampleSlots {
    pub(super) fn snapshot(&self) -> SampleSlotsSnapshot {
        SampleSlotsSnapshot {
            version: 1,
            next_token: self.next_token,
            slots: self
                .slots
                .iter()
                .map(|sample| {
                    sample.as_ref().map(|sample| SavedSample {
                        token: sample.reservation.token,
                        owner: sample.owner.map(Into::into),
                        effect: sample.effect,
                    })
                })
                .collect(),
        }
    }

    pub(super) fn last_token(&self) -> u64 {
        self.next_token
    }

    /// Preserve engine slot numbers but replace transport identities above both counters.
    /// Old completion callbacks therefore cannot retire the restored playback.
    pub(super) fn from_snapshot(
        snapshot: SampleSlotsSnapshot,
        live_last_token: u64,
        rebind: &impl Fn(SavedOwner) -> anyhow::Result<Owner>,
    ) -> anyhow::Result<(Self, std::collections::BTreeMap<u64, Reservation>)> {
        use anyhow::{Context, ensure};
        ensure!(
            snapshot.version == 1,
            "sample-slot snapshot version unsupported"
        );
        ensure!(
            snapshot.slots.len() == 16,
            "sample-slot snapshot capacity differs"
        );
        let mut candidate = Self {
            slots: Default::default(),
            next_token: snapshot.next_token.max(live_last_token),
        };
        let mut tokens = std::collections::BTreeMap::new();
        let mut owners = std::collections::BTreeMap::new();
        let mut destinations = std::collections::BTreeMap::new();
        for (slot, saved) in snapshot.slots.into_iter().enumerate() {
            let Some(saved) = saved else { continue };
            ensure!(
                saved.token != 0 && saved.token <= snapshot.next_token,
                "sample slot {slot}: saved transport token outside allocation range"
            );
            ensure!(
                !tokens.contains_key(&saved.token),
                "sample slot {slot}: duplicate transport token"
            );
            candidate.next_token = candidate
                .next_token
                .checked_add(1)
                .context("restored audio transport identity exhausted")?;
            let reservation = Reservation {
                slot: slot as u8,
                token: candidate.next_token,
            };
            let owner = saved
                .owner
                .map(rebind)
                .transpose()
                .with_context(|| format!("sample slot {slot}: restored owner"))?;
            if let (Some(old), Some(new)) = (saved.owner, owner) {
                let new = SavedOwner::from(new);
                ensure!(
                    owners
                        .insert(old, new)
                        .is_none_or(|previous| previous == new),
                    "sample slot {slot}: inconsistent restored owner binding"
                );
                ensure!(
                    destinations
                        .insert(new, old)
                        .is_none_or(|previous| previous == old),
                    "sample slot {slot}: noninjective restored owner binding"
                );
            }
            candidate.slots[slot] = Some(Sample {
                reservation,
                owner,
                effect: saved.effect,
            });
            tokens.insert(saved.token, reservation);
        }
        Ok((candidate, tokens))
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;

    #[test]
    fn restored_slots_rebind_owners_and_ignore_old_transport_completion() -> anyhow::Result<()> {
        let mut live = SampleSlots::default();
        let old_owner = Owner::Script {
            generation: 4,
            index: 2,
        };
        let old = live.start(6, Some(old_owner), Some(12)).unwrap().unwrap();
        let new_owner = Owner::Script {
            generation: 9,
            index: 2,
        };
        let (mut restored, tokens) = SampleSlots::from_snapshot(live.snapshot(), 50, &|owner| {
            anyhow::ensure!(
                matches!(
                    owner,
                    SavedOwner::Script {
                        generation: 4,
                        index: 2
                    }
                ),
                "wrong saved owner"
            );
            Ok(new_owner)
        })?;
        let new = tokens[&old.token];
        assert_eq!(new, Reservation { slot: 6, token: 51 });
        restored.finish(old);
        assert_eq!(restored.status(Some(6)), 4);
        assert!(restored.stop_matching(Some(old_owner), None).is_empty());
        assert_eq!(restored.stop_matching(Some(new_owner), Some(12)), [new]);
        let next = restored.start(6, None, None).unwrap().unwrap();
        assert_eq!(next, Reservation { slot: 6, token: 52 });
        assert_eq!(live.current(6), Some(old));
        Ok(())
    }

    #[test]
    fn restored_slots_reject_aliased_and_inconsistent_owner_bindings() {
        let first = Owner::Script {
            generation: 4,
            index: 0,
        };
        let second = Owner::Script {
            generation: 4,
            index: 1,
        };
        let replacement = Owner::Script {
            generation: 9,
            index: 0,
        };
        let mut live = SampleSlots::default();
        let a = live.start(0, Some(first), None).unwrap().unwrap();
        let b = live.start(0, Some(second), None).unwrap().unwrap();
        assert!(SampleSlots::from_snapshot(live.snapshot(), 0, &|_| Ok(replacement)).is_err());
        assert_eq!(live.current(0), Some(a));
        assert_eq!(live.current(1), Some(b));

        live.stop(1);
        live.start(0, Some(first), None).unwrap().unwrap();
        let calls = std::cell::Cell::new(0);
        assert!(
            SampleSlots::from_snapshot(live.snapshot(), 0, &|_| {
                let index = calls.get();
                calls.set(index + 1);
                Ok(Owner::Script {
                    generation: 9,
                    index,
                })
            })
            .is_err()
        );
        assert_eq!(live.status(Some(0)), 4);
        assert_eq!(live.status(Some(1)), 4);
    }
}

/// Logical channel ownership, separate from decoder and backend continuation.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct ChannelOwnershipSnapshot {
    version: u32,
    samples: SampleSlotsSnapshot,
    start: usize,
    animation: Vec<(u64, u8)>,
    checkpoints: Vec<(u64, Option<u8>)>,
}

/// Remains off-live until every reserved start has a prepared replacement transport.
pub(super) struct ChannelOwnershipCandidate {
    samples: SampleSlots,
    pub tokens: std::collections::BTreeMap<u64, Reservation>,
    start: usize,
    animation: std::collections::HashMap<bevy::prelude::Entity, u8>,
    checkpoints: std::collections::HashMap<bevy::prelude::Entity, Option<u8>>,
}

impl super::Channels {
    pub(super) fn snapshot_ownership(&self) -> anyhow::Result<ChannelOwnershipSnapshot> {
        use anyhow::ensure;
        let occupied = self.samples.slots.iter().flatten().count();
        ensure!(
            self.playing.len() == occupied,
            "audio snapshot: transport/slot census differs"
        );
        let mut entities = std::collections::HashSet::new();
        for (&token, channel) in &self.playing {
            ensure!(
                entities.insert(channel.sound),
                "audio snapshot: playback entity is shared by multiple starts"
            );
            ensure!(
                token == channel.reservation.token
                    && self.samples.current(channel.reservation.slot) == Some(channel.reservation),
                "audio snapshot: transport {token} has no matching slot"
            );
        }
        let mut animation: Vec<_> = self
            .animation
            .iter()
            .map(|(entity, slot)| (entity.to_bits(), *slot))
            .collect();
        let mut checkpoints: Vec<_> = self
            .animation_checkpoint
            .iter()
            .map(|(entity, slot)| (entity.to_bits(), *slot))
            .collect();
        animation.sort_unstable();
        checkpoints.sort_unstable();
        Ok(ChannelOwnershipSnapshot {
            version: 1,
            samples: self.samples.snapshot(),
            start: self.start,
            animation,
            checkpoints,
        })
    }
}

impl ChannelOwnershipSnapshot {
    pub(super) fn entity_owners(&self) -> std::collections::BTreeSet<u64> {
        self.samples
            .slots
            .iter()
            .flatten()
            .filter_map(|sample| match sample.owner {
                Some(SavedOwner::Entity(entity)) => Some(entity),
                _ => None,
            })
            .chain(self.animation.iter().map(|(entity, _)| *entity))
            .chain(self.checkpoints.iter().map(|(entity, _)| *entity))
            .collect()
    }

    pub(super) fn restore(
        self,
        live_last_token: u64,
        bindings: &crate::persistence::AudioRestoreBindings,
    ) -> anyhow::Result<ChannelOwnershipCandidate> {
        use anyhow::{Context, ensure};
        ensure!(
            self.version == 1,
            "channel ownership snapshot version unsupported"
        );
        ensure!(
            self.start < 16,
            "channel ownership: start index outside pool"
        );
        let bind = |old: SavedOwner| -> anyhow::Result<Owner> {
            match old {
                SavedOwner::Entity(old) => Ok(Owner::Entity(bindings.entity(old)?)),
                SavedOwner::Script { generation, index } => {
                    let new =
                        bindings.actor(cyrus_game::script::ScriptActorId { generation, index })?;
                    Ok(Owner::Script {
                        generation: new.generation,
                        index: new.index,
                    })
                }
            }
        };
        let (samples, tokens) = SampleSlots::from_snapshot(self.samples, live_last_token, &bind)?;
        let entity = |old| -> anyhow::Result<bevy::prelude::Entity> {
            match bind(SavedOwner::Entity(old))? {
                Owner::Entity(entity) => Ok(entity),
                _ => anyhow::bail!("animation owner must remain an entity"),
            }
        };
        let mut animation = std::collections::HashMap::new();
        for (old, slot) in self.animation {
            ensure!(
                slot < 16,
                "animation owner {old}: remembered sample outside pool"
            );
            ensure!(
                animation
                    .insert(
                        entity(old).with_context(|| format!("animation owner {old}"))?,
                        slot
                    )
                    .is_none(),
                "animation owner {old}: duplicate remembered slot"
            );
        }
        let mut checkpoints = std::collections::HashMap::new();
        for (old, slot) in self.checkpoints {
            ensure!(
                slot.is_none_or(|slot| slot < 16),
                "animation owner {old}: checkpoint sample outside pool"
            );
            ensure!(
                checkpoints
                    .insert(
                        entity(old).with_context(|| format!("animation checkpoint owner {old}"))?,
                        slot
                    )
                    .is_none(),
                "animation owner {old}: duplicate checkpoint"
            );
        }
        Ok(ChannelOwnershipCandidate {
            samples,
            tokens,
            start: self.start,
            animation,
            checkpoints,
        })
    }
}

impl ChannelOwnershipCandidate {
    pub(super) fn finish(
        self,
        playing: std::collections::HashMap<u64, super::Channel>,
    ) -> anyhow::Result<super::Channels> {
        use anyhow::ensure;
        ensure!(
            playing.len() == self.tokens.len(),
            "restored audio transport census differs"
        );
        let mut entities = std::collections::HashSet::new();
        for channel in playing.values() {
            ensure!(
                entities.insert(channel.sound),
                "restored audio: playback entity is shared by multiple starts"
            );
        }
        for reservation in self.tokens.values() {
            ensure!(
                playing
                    .get(&reservation.token)
                    .is_some_and(|channel| channel.reservation == *reservation),
                "restored sample {}: matching transport absent",
                reservation.slot
            );
        }
        Ok(super::Channels {
            samples: self.samples,
            playing,
            start: self.start,
            animation: self.animation,
            animation_checkpoint: self.checkpoints,
        })
    }
}

#[cfg(test)]
mod channel_persistence_tests {
    use super::*;
    use bevy::prelude::World;

    #[test]
    fn ownership_candidate_preserves_empty_checkpoints_and_requires_every_transport()
    -> anyhow::Result<()> {
        let mut world = World::new();
        let owner = world.spawn_empty().id();
        let replacement = world.spawn_empty().id();
        let sound = world.spawn_empty().id();
        let restored_sound = world.spawn_empty().id();
        let mut live = super::super::Channels::default();
        live.start = 6;
        let old = live
            .samples
            .start(6, Some(Owner::Entity(owner)), Some(12))
            .unwrap()
            .unwrap();
        live.playing.insert(
            old.token,
            super::super::Channel {
                sound,
                reservation: old,
                gains: None,
            },
        );
        live.animation.insert(owner, 6);
        live.animation_checkpoint.insert(owner, None);
        let snapshot = live.snapshot_ownership()?;
        let bind =
            crate::persistence::AudioRestoreBindings::new(&[], &[(owner.to_bits(), replacement)])?;
        let incomplete = snapshot.clone().restore(50, &bind)?;
        assert!(incomplete.finish(Default::default()).is_err());
        let candidate = snapshot.restore(50, &bind)?;
        let new = candidate.tokens[&old.token];
        let mut restored = candidate.finish(
            [(
                new.token,
                super::super::Channel {
                    sound: restored_sound,
                    reservation: new,
                    gains: None,
                },
            )]
            .into(),
        )?;
        assert_eq!(restored.start, 6);
        assert_eq!(restored.animation[&replacement], 6);
        assert_eq!(restored.animation_checkpoint[&replacement], None);
        restored.samples.finish(old);
        assert_eq!(restored.samples.status(Some(6)), 4);
        assert_eq!(live.animation[&owner], 6);
        assert_eq!(live.samples.current(6), Some(old));
        Ok(())
    }

    #[test]
    fn ownership_snapshot_rejects_missing_transport_and_malformed_remembered_slot()
    -> anyhow::Result<()> {
        let mut world = World::new();
        let owner = world.spawn_empty().id();
        let mut live = super::super::Channels::default();
        live.animation.insert(owner, 6);
        let mut saved = serde_json::to_value(live.snapshot_ownership()?)?;
        saved["animation"][0][1] = 16.into();
        let invalid: ChannelOwnershipSnapshot = serde_json::from_value(saved)?;
        assert!(
            invalid
                .restore(
                    0,
                    &crate::persistence::AudioRestoreBindings::new(
                        &[],
                        &[(owner.to_bits(), owner)]
                    )?
                )
                .is_err()
        );
        live.samples.start(0, None, None).unwrap();
        assert!(live.snapshot_ownership().is_err());
        Ok(())
    }

    #[test]
    fn ownership_candidate_rejects_two_starts_sharing_one_playback_entity() -> anyhow::Result<()> {
        let mut world = World::new();
        let sound = world.spawn_empty().id();
        let other_sound = world.spawn_empty().id();
        let mut live = super::super::Channels::default();
        for entity in [sound, other_sound] {
            let reservation = live.samples.start(0, None, None).unwrap().unwrap();
            live.playing.insert(
                reservation.token,
                super::super::Channel {
                    sound: entity,
                    reservation,
                    gains: None,
                },
            );
        }
        let candidate = live.snapshot_ownership()?.restore(
            10,
            &crate::persistence::AudioRestoreBindings::new(&[], &[])?,
        )?;
        let aliased = candidate
            .tokens
            .values()
            .map(|&reservation| {
                (
                    reservation.token,
                    super::super::Channel {
                        sound,
                        reservation,
                        gains: None,
                    },
                )
            })
            .collect();
        assert!(candidate.finish(aliased).is_err());
        assert_eq!(live.playing.len(), 2);
        for channel in live.playing.values_mut() {
            channel.sound = sound;
        }
        assert!(live.snapshot_ownership().is_err());
        Ok(())
    }
}
