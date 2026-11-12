//! Script patches: a mod's declared edits to an actor's script in one world, applied when the
//! world's scripts are prepared. A patch is a few words of data (`mod.toml`'s `[[script-patch]]`),
//! never game bytes; the install is not written, and the scripts are prepared again at every
//! world load, so removing the mod restores the original script.
use anyhow::{Context, Result, ensure};
use cyrus_soup::Native;
use cyrus_soup::program::{Operation, Program, Step};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Edits one actor's script: skips calls to some natives and lets `End` statements fall through.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptPatch {
    /// The map the actor is in, by file stem (`ISLAND`); case does not matter.
    pub world: String,
    /// The actor's script name; case does not matter.
    pub actor: String,
    /// Natives by their `SOUP386.DEF` names. Each call statement to one of them is skipped, as if
    /// the script did not make it; its arguments are not evaluated. A native used as a value in an
    /// expression stays.
    #[serde(default)]
    pub remove: Vec<Native>,
    /// Offsets of `End` statements (the `#offset` labels of a decompiled listing, in decimal).
    /// Such a statement no longer ends the script's turn: execution continues with the statement
    /// after it, including code the original script never reaches because the `End` loops on
    /// itself.
    #[serde(default, rename = "fall-through")]
    pub fall_through: Vec<u32>,
}

impl ScriptPatch {
    /// Whether the patch is for `actor` of the map `world` (file stem).
    pub fn applies_to(&self, world: &str, actor: &str) -> bool {
        self.world.eq_ignore_ascii_case(world) && self.actor.eq_ignore_ascii_case(actor)
    }

    /// Applies the patch to `program` and returns how many statements it changed. A skipped call
    /// becomes a jump to the statement after it, which neither waits nor changes any state, so every
    /// offset in the program stays valid. A fall-through `End` becomes the same jump; `decode`
    /// decodes the script's statements from an offset, for code that was not reachable before.
    pub fn apply(
        &self,
        program: &mut Program,
        decode: &dyn Fn(u32) -> Result<BTreeMap<u32, Step>>,
    ) -> Result<usize> {
        let mut changed = 0;
        for step in program.steps.values_mut() {
            let Operation::Call { call } = &step.operation else {
                continue;
            };
            if self.remove.contains(&call.function) {
                step.operation = Operation::Goto { target: step.next };
                changed += 1;
            }
        }
        for &offset in &self.fall_through {
            let step = program
                .steps
                .get(&offset)
                .with_context(|| format!("fall-through {offset}: no statement at that offset"))?;
            ensure!(
                matches!(step.operation, Operation::End { .. }),
                "fall-through {offset}: the statement there is not an End"
            );
            let next = step.next;
            if !program.steps.contains_key(&next) {
                for (at, step) in decode(next)
                    .with_context(|| format!("fall-through {offset}: statements from {next}"))?
                {
                    program.steps.entry(at).or_insert(step);
                }
            }
            program
                .steps
                .get_mut(&offset)
                .expect("checked above")
                .operation = Operation::Goto { target: next };
            changed += 1;
        }
        Ok(changed)
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
            fall_through: vec![],
        }
    }

    fn never_decodes(_: u32) -> Result<BTreeMap<u32, Step>> {
        anyhow::bail!("nothing to decode")
    }

    #[test]
    fn a_removed_call_becomes_a_jump_to_the_next_statement() {
        let mut program = program();
        assert_eq!(patch().apply(&mut program, &never_decodes).unwrap(), 2);
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
        assert_eq!(unrelated.apply(&mut program, &never_decodes).unwrap(), 0);
        assert_eq!(program, before);
    }

    #[test]
    fn a_patch_matches_world_and_actor_ignoring_case() {
        assert!(patch().applies_to("island", "Serpent"));
        assert!(!patch().applies_to("ISLAND", "SERPDEAD"));
        assert!(!patch().applies_to("CATACOMB", "SERPENT"));
    }

    /// A script that loops on an `End` has statements after it that nothing reaches.
    fn looping() -> Program {
        Program {
            version: 4,
            entry: 0,
            variables: vec![],
            strings: vec![],
            steps: BTreeMap::from([
                (0, call(Native::HideMe, 4)),
                (
                    4,
                    Step {
                        next: 9,
                        operation: Operation::End { target: 4 },
                    },
                ),
            ]),
        }
    }

    fn unreachable_code(from: u32) -> Result<BTreeMap<u32, Step>> {
        assert_eq!(from, 9);
        Ok(BTreeMap::from([
            (9, call(Native::ShowMe, 14)),
            (
                14,
                Step {
                    next: 19,
                    operation: Operation::End { target: 14 },
                },
            ),
        ]))
    }

    #[test]
    fn a_fall_through_end_continues_into_code_the_script_never_reached() {
        let mut program = looping();
        let patch = ScriptPatch {
            remove: vec![],
            fall_through: vec![4],
            ..patch()
        };
        assert_eq!(patch.apply(&mut program, &unreachable_code).unwrap(), 1);
        assert_eq!(program.steps[&4].operation, Operation::Goto { target: 9 });
        assert!(matches!(
            program.steps[&9].operation,
            Operation::Call { .. }
        ));
        assert!(program.steps.contains_key(&14));
    }

    #[test]
    fn a_fall_through_needs_an_end_at_that_offset() {
        let patch = ScriptPatch {
            remove: vec![],
            fall_through: vec![0],
            ..patch()
        };
        let error = patch.apply(&mut looping(), &unreachable_code).unwrap_err();
        assert!(error.to_string().contains("not an End"), "{error}");
        let missing = ScriptPatch {
            fall_through: vec![77],
            ..patch
        };
        assert!(missing.apply(&mut looping(), &unreachable_code).is_err());
    }
}
