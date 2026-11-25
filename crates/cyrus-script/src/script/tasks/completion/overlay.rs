//! GXA overlay task completion.
use super::super::*;
impl Task {
    pub(in super::super) fn complete_overlay(
        &mut self,
        arguments: &[Argument],
        invocation: Invocation,
        services: NativeServices<'_>,
    ) -> Result<bool> {
        Ok(match self {
            Self::ViewGxa => {
                let [Argument::String(name), frame, count] = arguments else {
                    anyhow::bail!("ViewGXA requires a file name, a frame and a count")
                };
                let overlay = services
                    .gxa_overlay
                    .context("ViewGXA overlay owner unavailable")?;
                let frame = frame.integer()?;
                let done = invocation
                    .elapsed
                    .context("ViewGXA requires explicit elapsed input")?
                    >= count.integer()?.wrapping_sub(1);
                *overlay = (!done).then(|| GxaOverlay {
                    name: name.clone(),
                    frame,
                    second_offset: None,
                });
                done
            }
            Self::AnimateGxa => {
                let [Argument::String(name), first, cycle, count] = arguments else {
                    anyhow::bail!(
                        "AnimateGXA requires a file name, a first frame, a cycle and a count"
                    )
                };
                let overlay = services
                    .gxa_overlay
                    .context("AnimateGXA overlay owner unavailable")?;
                let animation = services
                    .gxa_animation
                    .context("AnimateGXA animation owner unavailable")?;
                let elapsed = invocation
                    .elapsed
                    .context("AnimateGXA requires explicit elapsed input")?;
                let (first, cycle, count) = (first.integer()?, cycle.integer()?, count.integer()?);
                // The first invocation loads the image and restarts the progress.
                if elapsed == 0 && *animation.resource == 0 {
                    *animation.resource = 1;
                    *animation.progress = 0;
                }
                anyhow::ensure!(
                    *animation.resource != 0,
                    "AnimateGXA {name:?} used before its image was loaded"
                );
                if elapsed >= count.wrapping_sub(1) {
                    // Releasing the image clears both image slots and completes the task.
                    *animation.resource = 0;
                    *overlay = None;
                    return Ok(true);
                }
                *overlay = Some(GxaOverlay {
                    name: name.clone(),
                    frame: first,
                    second_offset: Some(animation.progress.wrapping_add(1)),
                });
                // The original loops until the progress is at most the cycle, which never ends
                // for a cycle of zero or less; that is rejected instead of hanging.
                anyhow::ensure!(
                    cycle > 0,
                    "AnimateGXA cycle {cycle} must be positive: the original never leaves its loop"
                );
                *animation.progress = animation.progress.wrapping_add(animation.ticks as i32);
                while *animation.progress > cycle {
                    *animation.progress = animation.progress.wrapping_sub(cycle);
                }
                false
            }
            _ => unreachable!("task dispatched to another completion family"),
        })
    }
}
