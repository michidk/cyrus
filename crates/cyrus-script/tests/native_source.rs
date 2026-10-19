#[cfg(test)]
mod tests {
    use cyrus_script::program::{Call, CallKind, Object, Operation, Program, Step, Value};
    use cyrus_script::{Argument, CallResult, Host, Invocation, Runtime, Target, Thread};
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Recorder(Vec<Vec<Argument>>, Option<std::sync::Arc<Program>>);
    impl Host for Recorder {
        fn establish_native_source(
            &mut self,
            _: cyrus_script::CodeOrigin,
            program: std::sync::Arc<Program>,
        ) {
            self.1 = Some(program);
        }
        fn native_string_parameter(&mut self, index: u32, _: &[String]) -> anyhow::Result<String> {
            let program = self
                .1
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("native source unavailable"))?;
            program
                .strings
                .get(index as usize)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("source string absent"))
        }
        fn call(
            &mut self,
            _: &Target,
            _: CallKind,
            function: &str,
            args: &[Argument],
            _: Invocation,
            _: &mut [i32],
        ) -> anyhow::Result<CallResult> {
            if function == "wait" {
                return Ok(CallResult::Pending(7));
            }
            self.0.push(args.to_vec());
            Ok(CallResult::Complete(0))
        }
        fn poll(
            &mut self,
            token: u64,
            _: &[Argument],
            _: Invocation,
            _: &mut [i32],
        ) -> anyhow::Result<bool> {
            assert_eq!(token, 7);
            Ok(false)
        }
        fn end(&mut self, _: Thread) -> anyhow::Result<()> {
            Ok(())
        }
    }
    fn actor(text: &str) -> Runtime {
        Runtime::new(
            Program {
                version: 4,
                entry: 0,
                variables: vec![],
                strings: vec![text.into()],
                steps: BTreeMap::from([
                    (
                        0,
                        Step {
                            next: 1,
                            operation: Operation::Call {
                                call: Call {
                                    function_id: 0,
                                    function: "record".into(),
                                    kind: CallKind::Function,
                                    target: Object::Me,
                                    arguments: vec![Value::String { index: 0 }],
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
            },
            text.into(),
        )
        .unwrap()
    }
    #[test]
    fn alternate_entry_keeps_previous_normal_actor_string_source() {
        let mut first = actor("first actor");
        let mut second = actor("second actor");
        let mut host = Recorder::default();
        first
            .invoke_main_stage(&mut host, &mut [], 256, 20, false)
            .unwrap();
        second
            .invoke_main_stage(&mut host, &mut [], 256, 20, true)
            .unwrap();
        assert_eq!(
            host.0,
            vec![
                vec![Argument::String("first actor".into())],
                vec![Argument::String("first actor".into())],
            ]
        );
    }
    #[test]
    fn normal_entry_changes_source_even_when_its_task_blocks_main() {
        let mut program = actor("blocked actor").shared_program().as_ref().clone();
        let record = program.steps[&0].operation.clone();
        program.steps = BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: Operation::Call {
                        call: Call {
                            function_id: 0,
                            function: "wait".into(),
                            kind: CallKind::Task,
                            target: Object::Me,
                            arguments: vec![],
                        },
                    },
                },
            ),
            (
                1,
                Step {
                    next: 2,
                    operation: record,
                },
            ),
            (
                2,
                Step {
                    next: 3,
                    operation: Operation::End { target: 0 },
                },
            ),
        ]);
        let mut blocked = Runtime::new(program, "blocked actor".into()).unwrap();
        let mut first = actor("first actor");
        let mut alternate = actor("alternate actor");
        let mut host = Recorder::default();
        blocked
            .invoke_main_stage(&mut host, &mut [], 256, 20, false)
            .unwrap();
        first
            .invoke_main_stage(&mut host, &mut [], 256, 20, false)
            .unwrap();
        blocked
            .invoke_main_stage(&mut host, &mut [], 256, 20, false)
            .unwrap();
        alternate
            .invoke_main_stage(&mut host, &mut [], 256, 20, true)
            .unwrap();
        assert_eq!(
            host.0.last().unwrap(),
            &vec![Argument::String("blocked actor".into())]
        );
    }
    #[test]
    fn unknown_source_on_alternate_entry_is_explicitly_unavailable() {
        let mut alternate = actor("alternate actor");
        let error = alternate
            .invoke_main_stage(&mut Recorder::default(), &mut [], 256, 20, true)
            .unwrap_err();
        assert!(format!("{error:#}").contains("native source unavailable"));
    }
}
