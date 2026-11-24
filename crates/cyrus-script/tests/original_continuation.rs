//! Public semantic actor import does not execute a restored native call.
#![cfg(feature = "original-save-import")]
use anyhow::Result;
use cyrus_script::script::original_save::{OriginalActorBinding, OriginalOwnerContinuation};
use cyrus_soup::{persistence::ContinuationTask, program::*, *};
use rgpre::import::savegame::{parse_save_objects, parse_save_script_locals};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Default)]
struct Service {
    calls: usize,
    polls: Vec<(u64, Option<i32>, Vec<Argument>)>,
    ready: bool,
}
impl Host for Service {
    fn call(
        &mut self,
        _: &Target,
        _: CallKind,
        _: Native,
        _: &[Argument],
        _: Invocation,
        _: &mut [i32],
    ) -> Result<CallResult> {
        self.calls += 1;
        anyhow::bail!("import must not restart the saved call")
    }
    fn poll(
        &mut self,
        token: u64,
        args: &[Argument],
        invocation: Invocation,
        _: &mut [i32],
    ) -> Result<bool> {
        self.polls.push((token, invocation.elapsed, args.to_vec()));
        Ok(self.ready)
    }
    fn end(&mut self, _: Thread) -> Result<()> {
        Ok(())
    }
}

#[test]
fn public_actor_import_restores_continuation_and_words_before_first_explicit_update() -> Result<()>
{
    let program = Arc::new(Program {
        version: 4,
        entry: 0,
        variables: vec![0],
        strings: vec![],
        steps: BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: Operation::Call {
                        call: Call {
                            function_id: 60,
                            kind: CallKind::Task,
                            target: Object::Me,
                            function: Native::Wait,
                            arguments: vec![Value::Read {
                                place: Place::Local { index: 0 },
                            }],
                        },
                    },
                },
            ),
            (
                1,
                Step {
                    next: 2,
                    operation: Operation::Return,
                },
            ),
            (
                2,
                Step {
                    next: 3,
                    operation: Operation::Increment {
                        destination: Place::Flag { index: 0 },
                        delta: 1,
                    },
                },
            ),
            (
                3,
                Step {
                    next: 4,
                    operation: Operation::End { target: 0 },
                },
            ),
        ]),
    });
    let sources = BTreeMap::from([(CodeOrigin(100), program)]);
    // Synthetic version-1.31 SMOB record, written as input to the public parser.
    let mut bytes = vec![0u8; 4 + 9 + 783 + 48];
    let size = (bytes.len() - 4) as u32;
    bytes[..4].copy_from_slice(&size.to_le_bytes());
    bytes[4..8].copy_from_slice(&73i32.to_le_bytes());
    let body = &mut bytes[13..13 + 783];
    body[20..22].copy_from_slice(&44u16.to_le_bytes());
    for (offset, word) in [
        (0x23b, u32::MAX),
        (0x260, 100),
        (0x268, 101),
        (0x26c, 102),
        (0x27f, 101),
    ] {
        body[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    body[0x25b] = 11;
    body[0x270] = 7;
    body[0x271] = 9;
    body[0x2af] = 1;
    body[0x2bb..0x2bd].copy_from_slice(&12u16.to_le_bytes());
    for (array, base) in [0x297, 0x2c7, 0x2df, 0x2f7].into_iter().enumerate() {
        for slot in 0..6 {
            let value = 0xf000_0000u32 | (array as u32) << 8 | slot as u32;
            body[base + slot * 4..base + slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    let mut objects = parse_save_objects(&bytes)?;
    let region = parse_save_script_locals(&41i32.to_le_bytes())?;
    let binding = || OriginalActorBinding {
        object_id: 73,
        runtime_identifier: 44,
        origin: CodeOrigin(100),
        tasks: [
            Some(ContinuationTask {
                token: 7,
                source: TaskSource {
                    origin: CodeOrigin(100),
                    instruction: 0,
                    object_call: false,
                    thread: Thread::Main,
                },
            }),
            None,
            None,
            None,
            None,
            None,
        ],
    };
    let imported = OriginalOwnerContinuation::decode(&objects[0], &region, binding(), &sources)?;
    for (slot, words) in imported.invocation_words.iter().enumerate() {
        for (array, word) in words.iter().enumerate() {
            assert_eq!(*word, 0xf000_0000 | (array as u32) << 8 | slot as u32);
        }
    }
    let mut runtime = Runtime::from_snapshot(
        CodeOrigin(100),
        imported.vm,
        &sources,
        &BTreeMap::from([(7, 70)]),
        "synthetic original actor".into(),
    )?;
    let mut host = Service::default();
    let mut flags = [0];
    assert_eq!(runtime.variables(), &[41]);
    assert_eq!(runtime.program_counter(Thread::Main), Some(1));
    assert_eq!(runtime.script_return(), 7);
    assert_eq!(runtime.anchor(), 11);
    runtime.invoke_tasks_timed(&mut host, &mut flags, 128)?;
    assert_eq!(host.calls, 0);
    assert_eq!(host.polls, [(70, Some(6), vec![Argument::Integer(41)])]);
    host.ready = true;
    runtime.invoke_tasks_timed(&mut host, &mut flags, 256)?;
    runtime.invoke_body(Thread::Main, &mut host, &mut flags, 100)?;
    assert_eq!(flags, [1]);
    assert_eq!(runtime.script_return(), 9);
    assert_eq!(host.calls, 0);
    let mut wrong = binding();
    wrong.runtime_identifier = 45;
    assert!(OriginalOwnerContinuation::decode(&objects[0], &region, wrong, &sources).is_err());
    objects[0].private_variables = 1i32.to_le_bytes().to_vec();
    assert!(OriginalOwnerContinuation::decode(&objects[0], &region, binding(), &sources).is_err());
    objects[0].private_variables.clear();
    objects[0].body[0x27f..0x283].copy_from_slice(&102u32.to_le_bytes());
    assert!(OriginalOwnerContinuation::decode(&objects[0], &region, binding(), &sources).is_err());
    assert_eq!(host.calls, 0);
    Ok(())
}

fn compact_program() -> Arc<Program> {
    Arc::new(Program {
        version: 4,
        entry: 0,
        variables: vec![0],
        strings: vec![],
        steps: BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: Operation::Call {
                        call: Call {
                            function_id: 60,
                            kind: CallKind::Task,
                            target: Object::Me,
                            function: Native::Wait,
                            arguments: vec![Value::Read {
                                place: Place::Local { index: 0 },
                            }],
                        },
                    },
                },
            ),
            (
                1,
                Step {
                    next: 2,
                    operation: Operation::End { target: 0 },
                },
            ),
        ]),
    })
}

fn compact_tasks() -> [Option<ContinuationTask>; 6] {
    [
        Some(ContinuationTask {
            token: 7,
            source: TaskSource {
                origin: CodeOrigin(100),
                instruction: 0,
                object_call: false,
                thread: Thread::Main,
            },
        }),
        None,
        None,
        None,
        None,
        None,
    ]
}

fn compact_owners() -> (
    rgpre::import::savegame::SaveCodeOwner,
    rgpre::import::savegame::SaveShell,
) {
    use rgpre::import::savegame::{SaveCodeOwner, SaveShell};
    let mut code = SaveCodeOwner {
        owner_id: 73,
        body: [0; 204],
    };
    let mut shell = SaveShell {
        body: [0; 186],
        variables: 41i32.to_le_bytes().to_vec(),
    };
    shell.body[..8].copy_from_slice(b"TEST0001");
    shell.body[0x28..0x2c].copy_from_slice(&1u32.to_le_bytes());
    for (body, references, callback, status, timer, words) in [
        (
            &mut code.body[..],
            0x23,
            0x42,
            0x72,
            0x78,
            [0x5a, 0x84, 0x9c, 0xb4],
        ),
        (
            &mut shell.body[..],
            0x12,
            0x30,
            0x60,
            0x66,
            [0x48, 0x72, 0x8a, 0xa2],
        ),
    ] {
        body[references..references + 4].copy_from_slice(&100u32.to_le_bytes());
        body[references + 8..references + 12].copy_from_slice(&101u32.to_le_bytes());
        body[callback..callback + 4].copy_from_slice(&101u32.to_le_bytes());
        body[status] = 1;
        body[timer..timer + 2].copy_from_slice(&12u16.to_le_bytes());
        for (array, base) in words.into_iter().enumerate() {
            for slot in 0..6 {
                body[base + slot * 4..base + slot * 4 + 4].copy_from_slice(
                    &(0xf000_0000u32 | (array as u32) << 8 | slot as u32).to_le_bytes(),
                );
            }
        }
    }
    (code, shell)
}

#[test]
fn public_compact_owner_import_preserves_locals_and_resumes_without_reissuing() -> Result<()> {
    use cyrus_script::script::original_save::{OriginalCodeBinding, OriginalShellBinding};
    let sources = BTreeMap::from([(CodeOrigin(100), compact_program())]);
    let (mut code, mut shell) = compact_owners();
    let region = parse_save_script_locals(&41i32.to_le_bytes())?;
    let code_binding = || OriginalCodeBinding {
        owner_id: 73,
        origin: CodeOrigin(100),
        tasks: compact_tasks(),
    };
    let shell_binding = || OriginalShellBinding {
        name: *b"TEST0001",
        origin: CodeOrigin(100),
        tasks: compact_tasks(),
    };
    let imported_code =
        OriginalOwnerContinuation::decode_code(&code, &region, code_binding(), &sources)?;
    let imported_shell =
        OriginalOwnerContinuation::decode_shell(&shell, shell_binding(), &sources)?;
    for imported in [imported_code, imported_shell] {
        for (slot, words) in imported.invocation_words.iter().enumerate() {
            for (array, word) in words.iter().enumerate() {
                assert_eq!(*word, 0xf000_0000 | (array as u32) << 8 | slot as u32);
            }
        }
        let mut runtime = Runtime::from_snapshot(
            CodeOrigin(100),
            imported.vm,
            &sources,
            &BTreeMap::from([(7, 70)]),
            "synthetic compact owner".into(),
        )?;
        assert_eq!(runtime.variables(), &[41]);
        assert_eq!(runtime.anchor(), 0);
        assert_eq!(runtime.program_counter(Thread::Main), Some(1));
        let mut host = Service::default();
        runtime.invoke_tasks_timed(&mut host, &mut [], 128)?;
        assert_eq!(host.calls, 0);
        assert_eq!(host.polls, [(70, Some(6), vec![Argument::Integer(41)])]);
    }
    let mut wrong = code_binding();
    wrong.owner_id += 1;
    assert!(OriginalOwnerContinuation::decode_code(&code, &region, wrong, &sources).is_err());
    let mut wrong = shell_binding();
    wrong.name[0] = b'X';
    assert!(OriginalOwnerContinuation::decode_shell(&shell, wrong, &sources).is_err());
    shell.variables.clear();
    assert!(OriginalOwnerContinuation::decode_shell(&shell, shell_binding(), &sources).is_err());
    code.body[0x42..0x46].copy_from_slice(&102u32.to_le_bytes());
    assert!(
        OriginalOwnerContinuation::decode_code(&code, &region, code_binding(), &sources).is_err()
    );
    Ok(())
}
