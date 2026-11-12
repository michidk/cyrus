//! Script patches: a mod's declared edits to an actor's script in one world, applied when the
//! world's scripts are prepared. A patch is a few words of data (`mod.toml`'s `[[script-patch]]`),
//! never game bytes; the install is not written, and the scripts are prepared again at every
//! world load, so removing the mod restores the original script.
use cyrus_soup::Native;
use cyrus_soup::program::{Operation, Program};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Skips every call statement to some natives in one actor's script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptPatch {
    /// The map the actor is in, by file stem (`ISLAND`); case does not matter.
    pub world: String,
    /// The actor's script name; case does not matter.
    pub actor: String,
    /// Natives by their `SOUP386.DEF` names. Each call statement to one of them is skipped, as if
    /// the script did not make it; its arguments are not evaluated. A native used as a value in an
    /// expression stays.
    pub remove: Vec<Native>,
}

impl ScriptPatch {
    /// Whether the patch is for `actor` of the map `world` (file stem).
    pub fn applies_to(&self, world: &str, actor: &str) -> bool {
        self.world.eq_ignore_ascii_case(world) && self.actor.eq_ignore_ascii_case(actor)
    }

    /// Skips the calls in `program` and returns how many it skipped. A skipped call becomes a jump
    /// to the statement after it, which neither waits nor changes any state, so every offset in
    /// the program stays valid.
    pub fn apply(&self, program: &mut Program) -> usize {
        let mut skipped = 0;
        for step in program.steps.values_mut() {
            let Operation::Call { call } = &step.operation else {
                continue;
            };
            if self.remove.contains(&call.function) {
                step.operation = Operation::Goto { target: step.next };
                skipped += 1;
            }
        }
        skipped
    }
}

static PATCHES: OnceLock<Vec<ScriptPatch>> = OnceLock::new();

/// Sets the patches for the rest of the process, before the first world is prepared. Returns
/// whether they were taken: they are fixed by the first call.
pub fn set_script_patches(patches: Vec<ScriptPatch>) -> bool {
    PATCHES.set(patches).is_ok()
}

/// The patches set for this process; none by default.
pub fn script_patches() -> &'static [ScriptPatch] {
    PATCHES.get().map_or(&[][..], Vec::as_slice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrus_soup::program::{Call, CallKind, Object, Step};
    use std::collections::BTreeMap;

    fn call(function: Native, next: u32) -> Step {
        Step {
            next,
            operation: Operation::Call {
                call: Call {
                    function_id: 0,
                    kind: CallKind::Function,
                    target: Object::Me,
                    function,
                    arguments: vec![],
                },
            },
        }
    }

    fn program() -> Program {
        Program {
            version: 4,
            entry: 0,
            variables: vec![],
            strings: vec![],
            steps: BTreeMap::from([
                (0, call(Native::HideMe, 4)),
                (4, call(Native::ShowMe, 9)),
                (
                    9,
                    Step {
                        next: 14,
                        operation: Operation::End { target: 9 },
                    },
                ),
                (14, call(Native::HideMe, 19)),
            ]),
        }
    }

    fn patch() -> ScriptPatch {
        ScriptPatch {
            world: "ISLAND".into(),
            actor: "SERPENT".into(),
            remove: vec![Native::HideMe],
        }
    }

    #[test]
    fn a_removed_call_becomes_a_jump_to_the_next_statement() {
        let mut program = program();
        assert_eq!(patch().apply(&mut program), 2);
        assert_eq!(
            program.steps[&0].operation,
            Operation::Goto { target: 4 },
            "the entry keeps its offset"
        );
        assert_eq!(program.steps[&14].operation, Operation::Goto { target: 19 });
        assert!(matches!(
            program.steps[&4].operation,
            Operation::Call { .. }
        ));
        assert_eq!(
            program.steps.keys().copied().collect::<Vec<_>>(),
            [0, 4, 9, 14]
        );
    }

    #[test]
    fn a_patch_naming_no_call_changes_nothing() {
        let mut program = program();
        let before = program.clone();
        let unrelated = ScriptPatch {
            remove: vec![Native::PlayAnimation],
            ..patch()
        };
        assert_eq!(unrelated.apply(&mut program), 0);
        assert_eq!(program, before);
    }

    #[test]
    fn a_patch_matches_world_and_actor_ignoring_case() {
        assert!(patch().applies_to("island", "Serpent"));
        assert!(!patch().applies_to("ISLAND", "SERPDEAD"));
        assert!(!patch().applies_to("CATACOMB", "SERPENT"));
    }
}
