//! The original's changed-actor list: actors whose map-cell membership must be refreshed before the
//! next regional selection, registered by the natives and movers that write a position.
use anyhow::Result;

/// The lower bound of the installed capacity: `max_remap_objects` below it reads as it.
const CAPACITY_FLOOR: usize = 32;

/// Registered actor indices in first-registration order, with the installed capacity once learned.
///
/// Registration is the only way to add an actor, so every writer raises the original's runtime
/// error 196 inside its own call, after its position stores, when another distinct actor does not
/// fit. The installed configuration disables recovery, so the error is final.
#[derive(Clone, Debug, Default)]
pub(super) struct MapUpdates {
    entries: Vec<usize>,
    capacity: Option<usize>,
}

impl MapUpdates {
    /// Record the installed capacity, when the caller has it. Absence leaves a learned value.
    pub(super) fn learn_capacity(&mut self, capacity: Option<usize>) {
        if capacity.is_some() {
            self.capacity = capacity;
        }
    }

    /// Append the actor unless the list is empty (which appends without a capacity test), the
    /// actor is already in it, or the list is below the capacity. A full list is error 196. A
    /// list at the capacity floor without a learned capacity cannot be judged: an explicit error.
    pub(super) fn register(&mut self, index: usize) -> Result<()> {
        if self.entries.is_empty() {
            self.entries.push(index);
            return Ok(());
        }
        if self.entries.contains(&index) {
            return Ok(());
        }
        let limit = if self.entries.len() < CAPACITY_FLOOR {
            usize::MAX
        } else {
            self.capacity.ok_or_else(|| {
                anyhow::anyhow!(
                    "changed-actor list holds {} actors and its installed capacity is unavailable; re-export",
                    self.entries.len()
                )
            })?
        };
        anyhow::ensure!(
            self.entries.len() < limit,
            "changed-actor list is full ({limit} actors): original runtime error 196"
        );
        self.entries.push(index);
        Ok(())
    }

    /// Replace the registrations with saved ones; the installed capacity is not saved.
    pub(super) fn restore(&mut self, entries: Vec<usize>) {
        self.entries = entries;
    }

    pub(super) fn as_slice(&self) -> &[usize] {
        &self.entries
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Consume the registrations in first-registration order, leaving the list empty.
    pub(super) fn take(&mut self) -> Vec<usize> {
        std::mem::take(&mut self.entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_dedups_and_fails_at_the_capacity_after_the_floor() {
        let mut list = MapUpdates::default();
        for index in 0..CAPACITY_FLOOR {
            list.register(index).unwrap();
            list.register(index).unwrap();
        }
        assert_eq!(list.as_slice().len(), CAPACITY_FLOOR);
        // At the floor the capacity is needed.
        let error = list.register(99).unwrap_err().to_string();
        assert!(error.contains("capacity is unavailable"), "{error}");
        list.learn_capacity(Some(CAPACITY_FLOOR + 1));
        list.register(99).unwrap();
        let error = list.register(100).unwrap_err().to_string();
        assert!(error.contains("error 196"), "{error}");
        // An already registered actor still passes with the list full.
        list.register(0).unwrap();
        assert_eq!(list.take().len(), CAPACITY_FLOOR + 1);
        assert!(list.is_empty());
    }
}
