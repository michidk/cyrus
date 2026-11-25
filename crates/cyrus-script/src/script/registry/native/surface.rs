//! Surface native-call dispatch.
use super::super::{Argument, CallResult, GameHost, Native, Result, Surface};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_surface(
        &mut self,
        function: Native,
        arguments: &[Argument],
    ) -> Result<Option<CallResult>> {
        let surface_test: Option<fn(Surface) -> bool> = match function {
            Native::IsInDeepWater => Some(|surface| surface.is_deep_water()),
            Native::IsInWater => Some(|surface| surface.is_water()),
            Native::IsInLava => Some(|surface| surface == Surface::LAVA),
            _ => None,
        };

        if let Some(test) = surface_test {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let surface = self.registry.actors[self.index]
                .surface
                .with_context(|| format!("{function} actor surface state unavailable"))?;
            return Ok(Some(CallResult::Complete(i32::from(test(surface)))));
        }
        Ok(None)
    }
}
