//! Group synchronization channels: a rendezvous between actors that share an attribute-29 channel.
//! `SyncWithGroup(group)` posts a group for the calling actor and waits until the channel releases that
//! group; the channel is updated once per world-loop iteration after the family update.
use super::ScriptActor;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Actor data byte 29: the group channel an actor belongs to; zero means none.
pub(super) const CHANNEL_ATTRIBUTE: usize = 29;
/// Actor data byte 43: a nonzero value excludes the actor from its channel's rendezvous.
const IGNORED_ATTRIBUTE: usize = 43;
/// Set in a posted value, or in a channel's released value, once every active member has posted.
const POSTED: u8 = 0x80;

/// Retained channel state. Cleared when a world is entered, as the engine reinitializes its tables.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GroupSync {
    /// Channel membership is sampled when an actor record is created.  Later writes to attribute
    /// 29 do not move the actor between rendezvous tables.
    #[serde(default)]
    channels: BTreeMap<usize, u8>,
    /// Released value per channel: the current group in the low seven bits, `POSTED` when every
    /// active member has posted.
    released: BTreeMap<u8, u8>,
    /// Posted value per member actor: the requested group, with `POSTED` while it waits.
    posted: BTreeMap<usize, u8>,
}

impl GroupSync {
    /// Register an actor's channel at creation time.  Re-registration is intentionally ignored so
    /// an attribute write cannot silently move an existing actor to another channel.
    pub(super) fn register(&mut self, actor: usize, channel: u8) {
        self.channels.entry(actor).or_insert(channel);
    }

    pub(super) fn register_actors(&mut self, actors: &[ScriptActor]) {
        for (index, actor) in actors.iter().enumerate() {
            self.register(index, actor.attributes()[CHANNEL_ATTRIBUTE]);
        }
    }

    /// One `SyncWithGroup(group)` invocation by `actor` on `channel` (nonzero). Returns whether the
    /// task completes: the channel's released value, compared as a whole byte, must equal the posted
    /// group with its mark, so a group that is not released, or a channel where not every member
    /// has posted, keeps the task waiting. Completing clears the mark but keeps the group.
    pub(super) fn post(&mut self, actor: usize, channel: u8, group: i32) -> bool {
        let state = (group as u8) | POSTED;
        if self.released.get(&channel).copied().unwrap_or(0) == state {
            self.posted.insert(actor, state & !POSTED);
            return true;
        }
        self.posted.insert(actor, state);
        false
    }

    /// Clears an actor's pending state, as `End` and `ResetGroupSync` do.
    pub(super) fn reset(&mut self, actor: usize) {
        self.posted.insert(actor, 0);
    }

    /// Per-iteration update of every channel that has members.
    pub(super) fn update(&mut self, actors: &[ScriptActor]) {
        self.update_members(actors.iter().enumerate().filter_map(|(index, actor)| {
            let channel = self.channels.get(&index).copied().unwrap_or(0);
            (channel != 0).then(|| (index, channel, actor.attributes()[IGNORED_ATTRIBUTE] != 0))
        }));
    }

    /// `members` are `(actor, channel, ignored)`. Each channel releases the first posted group at or
    /// after its previous one (cyclically over 0 to 127) and carries the mark only when every
    /// non-ignored member has posted. Completed members keep their group, so they still count as
    /// posting it until they post again or end.
    fn update_members(&mut self, members: impl IntoIterator<Item = (usize, u8, bool)>) {
        let mut channels: BTreeMap<u8, Vec<(usize, bool)>> = BTreeMap::new();
        for (actor, channel, ignored) in members {
            channels.entry(channel).or_default().push((actor, ignored));
        }
        for (channel, members) in channels {
            let state = |actor: usize| self.posted.get(&actor).copied().unwrap_or(0);
            let mark = if members
                .iter()
                .all(|&(actor, ignored)| ignored || state(actor) & POSTED != 0)
            {
                POSTED
            } else {
                0
            };
            let mut present = [false; 256];
            for &(actor, ignored) in &members {
                if !ignored {
                    present[usize::from(state(actor) & !POSTED)] = true;
                }
            }
            let mut current = usize::from(self.released.get(&channel).copied().unwrap_or(0));
            let mut released = mark;
            for _ in 0..256 {
                if present[current] {
                    released = current as u8 | mark;
                    break;
                }
                current = (current + 1) & 0x7f;
            }
            self.released.insert(channel, released);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Actors 0 and 1 are active members of channel 1 and actor 2 is an ignored member.
    const MEMBERS: [(usize, u8, bool); 3] = [(0, 1, false), (1, 1, false), (2, 1, true)];

    #[test]
    fn a_channel_releases_groups_cyclically_once_every_active_member_has_posted() {
        let mut sync = GroupSync::default();
        // Actor 1 has not posted. An unposted member counts as group 0, so the channel stays at 0.
        assert!(!sync.post(0, 1, 3));
        sync.update_members(MEMBERS);
        assert_eq!(sync.released[&1], 0);
        // Both active members posted (groups 3 and 5): the first posted group from 0 is released marked.
        assert!(!sync.post(1, 1, 5));
        sync.update_members(MEMBERS);
        assert_eq!(sync.released[&1], 3 | POSTED);
        // Group 3 completes and keeps its group; group 5 keeps waiting.
        assert!(sync.post(0, 1, 3));
        assert_eq!(
            sync.posted[&0], 3,
            "completing clears the mark and keeps the group"
        );
        assert!(!sync.post(1, 1, 5));
        // Actor 0 no longer counts as posted, so the channel advances to group 5, unmarked.
        sync.update_members(MEMBERS);
        assert_eq!(sync.released[&1], 5);
        // Once actor 0 posts group 5 too, group 5 is released marked and both complete.
        assert!(!sync.post(0, 1, 5));
        sync.update_members(MEMBERS);
        assert_eq!(sync.released[&1], 5 | POSTED);
        assert!(sync.post(0, 1, 5) && sync.post(1, 1, 5));
    }

    #[test]
    fn ending_clears_pending_state_and_ignored_members_do_not_block() {
        let mut sync = GroupSync::default();
        sync.post(0, 1, 2);
        sync.post(1, 1, 2);
        // The ignored actor 2 never posted but does not block the mark.
        sync.update_members(MEMBERS);
        assert_eq!(sync.released[&1], 2 | POSTED);
        sync.reset(1);
        assert_eq!(sync.posted[&1], 0);
        // Actor 1 reads as unposted group 0 again: the mark drops and the cycle from 2 wraps to 0.
        sync.update_members(MEMBERS);
        assert_eq!(sync.released[&1], 0);
        // A channel that was never updated releases nothing.
        assert!(!sync.post(0, 9, 2));
        assert!(!sync.released.contains_key(&9));
    }
}
