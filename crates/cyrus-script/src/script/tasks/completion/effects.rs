//! Light-effect task completion.
use super::super::*;
impl Task {
    pub(in super::super) fn complete_effects(
        &mut self,
        arguments: &[Argument],
        mut services: NativeServices<'_>,
    ) -> Result<bool> {
        Ok(match self {
            Self::RadiusFlicker | Self::ColorPhase | Self::ColorFlicker => {
                let function = match self {
                    Self::RadiusFlicker => Native::FxFlickerOnOff,
                    Self::ColorPhase => Native::FxPhase,
                    Self::ColorFlicker => Native::FxFlickerDim,
                    _ => unreachable!(),
                };
                services
                    .light_effect
                    .as_mut()
                    .context("light effect service unavailable")?(
                    function, arguments
                )?
            }
            _ => unreachable!("task dispatched to another completion family"),
        })
    }
}
