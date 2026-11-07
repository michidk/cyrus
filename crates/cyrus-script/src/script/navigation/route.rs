//! Ordered walk-route search and reverse next-hop selection from preservation's SOUP contract.
use super::{ActorNavigation, WalkNode, WalkNodeReservations};
use anyhow::{Context, Result, ensure};

/// Shared route results retain their prior values until a predecessor scan changes them.
/// Bits describe the first link's cost flags; their movement interpretation is separate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WalkRouteState {
    /// Shared point-target exclusion switch, initialized off; active admission is unsupported.
    #[serde(default)]
    pub target_exclusion: bool,
    pub backtracking_inhibition: i32,
    pub link_flags: [u8; 2],
}

#[derive(Debug)]
pub(super) struct WalkLink {
    pub target: usize,
    pub cost: u16,
}

impl WalkNodeReservations {
    pub(in crate::script) fn next_hop(
        &mut self,
        navigation: &ActorNavigation,
        destination: i32,
        identifier: u8,
        state: &mut WalkRouteState,
        mut admit: impl FnMut([i32; 3]) -> Result<bool>,
    ) -> Result<i32> {
        if navigation.current == -1 {
            return Ok(-1);
        }
        if navigation.current == destination {
            return Ok(destination);
        }
        let Some(map) = navigation.map else {
            return Ok(-1);
        };
        let nodes = self.maps.get(map).context("route map unavailable")?;
        if self.disabled[map] {
            return Ok(-1);
        }
        let current = usize::try_from(navigation.current)
            .ok()
            .filter(|&node| node < nodes.len())
            .context("route current node unavailable")?;
        ensure!(
            destination >= 0 && (destination as usize) < nodes.len(),
            "route destination {destination} unavailable"
        );
        let capacity = self.maps.iter().map(Vec::len).max().unwrap_or(0) + 8;
        let mut queue = Vec::with_capacity(capacity);
        let mut visited = vec![false; nodes.len()];
        queue.push((current, 1u16));
        visited[current] = true;
        let mut cursor = 0;
        while queue.len() < capacity {
            let Some(&(node, depth)) = queue.get(cursor) else {
                return Ok(-1);
            };
            cursor += 1;
            for link in &nodes[node].links {
                if link.cost > 2048
                    || !admit(nodes[link.target].position)?
                    || visited[link.target]
                    || (nodes[node].owner != 0 && nodes[node].owner != identifier)
                {
                    continue;
                }
                visited[link.target] = true;
                queue.push((link.target, depth.wrapping_add(1)));
                if link.target == destination as usize {
                    return Ok(backtrack(nodes, &queue, state));
                }
            }
        }
        self.selected_map = None;
        Ok(-1)
    }
}

fn backtrack(nodes: &[WalkNode], queue: &[(usize, u16)], state: &mut WalkRouteState) -> i32 {
    if state.backtracking_inhibition == 1 {
        state.link_flags = [0; 2];
        return -1;
    }
    let mut index = queue.len() - 1;
    let mut next = -1;
    while index != 0 {
        let (node, depth) = queue[index];
        next = node as i32;
        state.link_flags = [0; 2];
        let mut best = u32::MAX;
        let mut predecessor = None;
        for candidate in (0..index).rev() {
            let (parent, parent_depth) = queue[candidate];
            if parent_depth != depth.wrapping_sub(1) {
                continue;
            }
            let cost = nodes[parent]
                .links
                .iter()
                .find(|link| link.target == node)
                .map_or(0, |link| link.cost);
            let distance = u32::from(cost & 0x3fff);
            if cost == 0 || distance >= best {
                continue;
            }
            best = distance;
            predecessor = Some(candidate);
            if cost & 0x4000 != 0 {
                state.link_flags[0] = 1;
            }
            if cost & 0x8000 != 0 {
                state.link_flags[1] = 1;
            }
        }
        let Some(previous) = predecessor else {
            state.link_flags = [0; 2];
            return -1;
        };
        index = previous;
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::GameplayGlobals;
    #[test]
    fn route_flags_and_exact_inhibition_survive_session_restore() {
        let settings = cyrus_assets::script::DialogueSettings {
            menu_start: [35, 25],
            max_menu_items: 300,
            max_dialog_lines: 20,
            max_text_width: 500,
            menu_traverse_delay: 4,
            print_text: true,
            use_speech: true,
            max_distance: 1600,
            rtx_filename: "ENGLISH.RTX".into(),
        };
        let mut globals = GameplayGlobals::default();
        globals.walk_route = WalkRouteState {
            target_exclusion: true,
            backtracking_inhibition: -7,
            link_flags: [1, 1],
        };
        let serialized = serde_json::to_value(globals.snapshot()).unwrap();
        let saved = serde_json::from_value(serialized).unwrap();
        let restored =
            GameplayGlobals::from_snapshot(saved, &settings, &[], &Default::default()).unwrap();
        assert_eq!(
            *restored.walk_route(),
            WalkRouteState {
                target_exclusion: true,
                backtracking_inhibition: -7,
                link_flags: [1, 1]
            }
        );
        globals.initialize_player_controller();
        assert!(!globals.walk_route.target_exclusion);
        let nodes = [
            WalkNode {
                position: [0; 3],
                owner: 0,
                links: vec![WalkLink { target: 1, cost: 1 }],
            },
            WalkNode {
                position: [0; 3],
                owner: 0,
                links: vec![],
            },
        ];
        let queue = [(0, 1), (1, 2)];
        let mut state = *restored.walk_route();
        assert_eq!(backtrack(&nodes, &queue, &mut state), 1);
        state.backtracking_inhibition = 1;
        state.link_flags = [1, 1];
        assert_eq!(backtrack(&nodes, &queue, &mut state), -1);
        assert_eq!(state.link_flags, [0, 0]);
    }
}
