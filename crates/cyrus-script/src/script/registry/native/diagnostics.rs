//! Diagnostics native-call dispatch.
use super::super::{Argument, CallResult, GameHost, Native, Result};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_diagnostics(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        // The Spare0/Spare1 natives ignore every argument, change no actor state, add a diagnostic
        // line and return zero. The original's line carries the receiver's host pointer, which
        // has no portable value, so the receiver's name stands in for it (as PrintSingleParm's
        // line does); the argument count is the call record's, whatever the definition declares.
        if matches!(
            function,
            Native::Spare0Parm0
                | Native::Spare0Parm1
                | Native::Spare0Parm2
                | Native::Spare0Parm3
                | Native::Spare0Parm4
                | Native::Spare0Parm5
                | Native::Spare0Parm6
                | Native::Spare0Parm7
                | Native::Spare1Parm0
                | Native::Spare1Parm1
                | Native::Spare1Parm2
                | Native::Spare1Parm3
                | Native::Spare1Parm4
                | Native::Spare1Parm5
                | Native::Spare1Parm6
                | Native::Spare1Parm7
        ) {
            let name = self.registry.actors[self.index].name().to_owned();
            self.registry
                .globals
                .print_line(format!("{name} Spare function"));
            return Ok(Some(CallResult::Complete(0)));
        }

        if function == Native::PrintSingleParm {
            let [value] = arguments else {
                anyhow::bail!("{function} requires one argument");
            };
            let value = value.integer()?;
            let name = self.registry.actors[self.index].name().to_owned();
            self.registry.globals.print_line(format!("{name} {value}"));
            return Ok(Some(CallResult::Complete(0)));
        }

        // HeaderHook writes three console lines about the receiver and returns one: a banner, its
        // identifier, object name (the namespace named objects resolve through) and script, and its
        // node and AI record. It changes nothing and
        // does not run the receiver's RAHK hook. `%p` of the original has no portable value, so
        // the pool slot stands in for the record address.
        if function == Native::HeaderHook {
            anyhow::ensure!(arguments.is_empty(), "HeaderHook requires no arguments");
            let actor = &self.registry.actors[self.index];
            let control = actor
                .control
                .as_ref()
                .context("HeaderHook: receiver AI record unavailable")?;
            let address = format!("0x{:08x}", self.index);
            let identifier = actor.runtime_identifier.map_or(0, i32::from);
            let script = actor.name();
            let described = match (actor.namespace.as_deref(), actor.unavailable.is_none()) {
                (None, false) => {
                    format!("Unnamed object (id {identifier}) at {address} no script attached")
                }
                (None, true) => {
                    format!("Unnamed object (id {identifier}) at {address} using \"{script}\"")
                }
                (Some(name), false) => {
                    format!("Object \"{name}\"(id {identifier}) at {address} no script attached")
                }
                (Some(name), true) => {
                    format!("Object \"{name}\"(id {identifier}) at {address} using \"{script}\"")
                }
            };
            let ai = control.ai;
            let record = format!(
                "Node {}, dnode {}, ai_type {}, ai_mode {}, ai_delay {}, ai_length {}, ai_var1 {}, ai_var2 {}",
                control.navigation.current,
                control.navigation.next,
                control.ai_type.byte(),
                control.ai_mode,
                control.ai_delay,
                ai.length,
                ai.var1,
                ai.var2
            );
            let globals = &mut self.registry.globals;
            globals.print_line("\nHeader Hook:".to_owned());
            globals.print_line(described);
            globals.print_line(record);
            return Ok(Some(CallResult::Complete(1)));
        }

        // PrintParms and LogParms format the receiver's script name and all eight arguments as signed
        // decimals; PrintParms takes the first free console line, LogParms goes to the diagnostic log.
        if function == Native::PrintParms || function == Native::LogParms {
            anyhow::ensure!(
                arguments.len() == 8,
                "{function} requires eight integer arguments"
            );
            let values = arguments
                .iter()
                .map(|argument| argument.integer().map(|value| value.to_string()))
                .collect::<Result<Vec<_>>>()
                .with_context(|| format!("{function} arguments"))?
                .join(", ");
            let name = self.registry.actors[self.index].name().to_owned();
            if function == Native::PrintParms {
                self.registry.globals.print_line(format!("{name} {values}"));
            } else {
                bevy::log::debug!("  {name}|3 {values}");
            }
            return Ok(Some(CallResult::Complete(0)));
        }

        // PrintStringParm is PrintSingleParm's line with the already evaluated string as its text.
        if function == Native::PrintStringParm {
            let [Argument::String(text)] = arguments else {
                anyhow::bail!("{function} requires one string argument");
            };
            let name = self.registry.actors[self.index].name().to_owned();
            self.registry.globals.print_line(format!("{name} {text}"));
            return Ok(Some(CallResult::Complete(0)));
        }
        Ok(None)
    }
}
