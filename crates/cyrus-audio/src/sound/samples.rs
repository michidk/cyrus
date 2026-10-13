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
