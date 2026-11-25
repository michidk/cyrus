//! Synchronization and task-wait completion.
use super::super::*;
impl Task {
    pub(in super::super) fn complete_waits(
        &mut self,
        arguments: &[Argument],
        invocation: Invocation,
        services: NativeServices<'_>,
    ) -> Result<bool> {
        Ok(match self {
            Self::GroupSync => {
                let [group] = arguments else {
                    anyhow::bail!("SyncWithGroup requires one argument");
                };
                let input = services
                    .group_sync
                    .context("SyncWithGroup channel state unavailable")?;
                // An actor without a member entry in its current channel never waits.
                !input
                    .state
                    .contains_member(input.actor_index, input.channel)
                    || input
                        .state
                        .post(input.actor_index, input.channel, group.integer()?)
            }
            Self::UserSync => {
                let [value] = arguments else {
                    anyhow::bail!("SyncWithUser requires one argument");
                };
                let input = services
                    .group_sync
                    .context("SyncWithUser channel state unavailable")?;
                input.state.post_user(
                    input.actor_index,
                    input.user_channel,
                    value.integer()?,
                    input.user_has_active,
                )
            }
            Self::Duration => {
                let [duration] = arguments else {
                    anyhow::bail!("Wait requires one argument")
                };
                invocation
                    .elapsed
                    .context("Wait requires explicit elapsed input")?
                    >= duration.integer()?.wrapping_sub(1)
            }
            Self::WaitLineUp => {
                let [Argument::Actor(_)] = arguments else {
                    anyhow::bail!("WaitLineUp requires one actor reference")
                };
                // The handler returns one (still active) while the flag is set.
                !services
                    .alignment_flag
                    .context("WaitLineUp alignment flag unavailable")?
            }
            Self::Tasks | Self::Blocking => {
                anyhow::ensure!(
                    arguments.is_empty(),
                    "task-count wait requires no arguments"
                );
                match self {
                    Self::Tasks => invocation.unpaused_tasks <= 1,
                    _ => invocation.unpaused_blocking_tasks <= 1,
                }
            }
            _ => unreachable!("task dispatched to another completion family"),
        })
    }
}
