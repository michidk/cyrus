//! Synchronous receiver dispatch preserves foreign task identity and native string context.
use super::{Host, Route, Runtime, TaskSource};
use anyhow::{Context, Result};
use cyrus_assets::script::Call;
use std::sync::Arc;

impl Runtime {
    pub(super) fn dispatch(
        &mut self,
        call: &Call,
        source: Option<TaskSource>,
        host: &mut dyn Host,
        flags: &mut [i32],
    ) -> Result<(i32, bool)> {
        let target = self.target(&call.target)?;
        let program = Arc::clone(&self.program);
        match host.route(&target, call, &program.strings, source, flags)? {
            Route::Here => self.receive_inner(call, &program.strings, source, host, flags),
            Route::Complete(value) => Ok((value, false)),
        }
    }
    /// Invokes a resolved receiving actor without running either of its script bodies.
    /// Initial string arguments retain the calling actor's table; locals use this instance.
    /// The host must restore this instance and its own receiver context even on failure.
    pub fn receive(
        &mut self,
        call: &Call,
        strings: &[String],
        source: Option<TaskSource>,
        host: &mut dyn Host,
        flags: &mut [i32],
    ) -> Result<i32> {
        if let Some(error) = &self.state.failure {
            anyhow::bail!("{error}");
        }
        match self
            .receive_inner(call, strings, source, host, flags)
            .context("routed native call")
        {
            Ok((value, _)) => Ok(value),
            Err(error) => {
                let error = format!("{} native {}: {error:#}", self.context, call.function);
                self.state.failure = Some(error.clone());
                anyhow::bail!("{error}")
            }
        }
    }
}
