//! Shell native-call dispatch.
use super::super::{Argument, CallKind, CallResult, GameHost, Native, Result, ScriptActorSource};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_shell(
        &mut self,
        kind: CallKind,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // CountAiShells(name) counts the running shells of that name; FindAiShell(name) reports
        // whether one runs. Both walk the shell list; the receiver is not read.
        if kind == CallKind::Function
            && (function == Native::CountAiShells || function == Native::FindAiShell)
        {
            let [Argument::String(name)] = arguments else {
                anyhow::bail!("{function} requires one script name")
            };
            let first = function == Native::FindAiShell;
            return Ok(Some(CallResult::Complete(
                self.registry.count_shell_matches(name, first)?,
            )));
        }

        // OpenShell(name) starts the named shell script; it runs from the next update.
        if kind == CallKind::Function && function == Native::OpenShell {
            let [Argument::String(name)] = arguments else {
                anyhow::bail!("OpenShell requires one script name")
            };
            self.registry.spawn_shell(name)?;
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }

    pub(in crate::script::registry) fn call_shell_context(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // IsShellScript returns the shell execution-context word, or zero while no shell script
        // exists. The host models the word as a Boolean; whether the shell list keeps killed
        // shells, and any word value other than zero or one, are not established.
        if function == Native::IsShellScript {
            anyhow::ensure!(arguments.is_empty(), "IsShellScript requires no arguments");
            let context = self
                .inputs
                .shell_execution
                .context("IsShellScript: shell context unavailable")?;
            let has_shell = self
                .registry
                .actors
                .iter()
                .any(|actor| matches!(actor.source, ScriptActorSource::Shell(_)));
            return Ok(Some(CallResult::Complete(i32::from(context && has_shell))));
        }
        Ok(None)
    }
}
