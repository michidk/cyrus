//! Native task continuations validate against saved VM calls without execution.
use super::{NativeTasks, Task};
use anyhow::{Context, Result, ensure};
use cyrus_script::{persistence::Snapshot, program::CallKind};
use cyrus_world::animation::ActorAnimation;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Actor-owned native continuations, separate from the VM's timing and slots.
/// Save only with the matching actor/world and VM snapshot outside the install.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeTasksSnapshot {
    version: u32,
    next: u64,
    tasks: Vec<SavedTask>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SavedTask {
    token: u64,
    state: Task,
}

/// Restored service owners used by native continuations.
///
/// The host must validate actor/world generation and restore matching animation,
/// dialogue and audio state before supplying these references. Channel mappings
/// bind saved sample slots to slots already owned by that restored audio owner.
/// This constructor checks mapping shape, not playback ownership or asset readiness.
#[derive(Default)]
pub struct NativeTaskBindings<'a> {
    pub animation: Option<&'a ActorAnimation>,
    pub dialogue: Option<&'a crate::script::DialogueState>,
    pub ambient_channels: Option<&'a BTreeMap<u8, u8>>,
}

impl NativeTasks {
    /// Capture exact native variants and continuation fields without polling.
    pub fn snapshot(&self) -> NativeTasksSnapshot {
        NativeTasksSnapshot {
            version: 1,
            next: self.next,
            tasks: self
                .tasks
                .iter()
                .map(|(&token, &state)| SavedTask { token, state })
                .collect(),
        }
    }

    /// Validate and return replacement native state without calls, cues or polling.
    ///
    /// `tokens` completely and injectively binds saved tokens to new actor-owned
    /// tokens. The VM must come from the same actor/world capture. Its structure
    /// and native-call contracts are checked here; exact installed-source matching
    /// remains `Runtime::from_snapshot`'s responsibility. Commit the replacement
    /// only with the complete validated actor/VM/world candidate and service owners.
    pub fn from_snapshot(
        snapshot: NativeTasksSnapshot,
        vm: &Snapshot,
        tokens: &BTreeMap<u64, u64>,
        bindings: NativeTaskBindings<'_>,
    ) -> Result<Self> {
        ensure!(
            snapshot.version == 1,
            "native task snapshot version {} is unsupported",
            snapshot.version
        );
        let calls = vm.task_calls().context("native task snapshot VM calls")?;
        ensure!(
            snapshot.tasks.len() == calls.len() && tokens.len() == calls.len(),
            "native task snapshot/binding count differs from VM slot count {}",
            calls.len()
        );
        let calls: BTreeMap<_, _> = calls.into_iter().map(|call| (call.token, call)).collect();
        if let Some(channels) = bindings.ambient_channels {
            let mut destinations = BTreeSet::new();
            for (&saved, &restored) in channels {
                ensure!(
                    saved < 16 && restored < 16,
                    "native task ambient channel binding {saved}->{restored} exceeds sample slots"
                );
                ensure!(
                    destinations.insert(restored),
                    "native task ambient channel binding duplicates slot {restored}"
                );
            }
        }
        let mut saved_tokens = BTreeSet::new();
        let mut tasks = BTreeMap::new();
        let mut next = snapshot.next;
        for saved in snapshot.tasks {
            let mut prepare = || -> Result<(u64, Task)> {
                ensure!(
                    saved.token != 0 && saved.token <= snapshot.next,
                    "token {} exceeds saved allocator {} or is zero",
                    saved.token,
                    snapshot.next
                );
                ensure!(
                    saved_tokens.insert(saved.token),
                    "duplicate saved token {}",
                    saved.token
                );
                let call = calls.get(&saved.token).context("VM task token absent")?;
                let (name, arguments, required_kind) = saved.state.contract();
                ensure!(
                    call.call.function.eq_ignore_ascii_case(name),
                    "variant {name} differs from VM call {}",
                    call.call.function
                );
                ensure!(
                    call.call.arguments.len() == arguments,
                    "{name} VM call argument count differs from {arguments}"
                );
                ensure!(
                    call.call.kind != CallKind::Function
                        && required_kind.is_none_or(|kind| kind == call.call.kind),
                    "{name} VM call kind is unsupported"
                );
                let token = *tokens.get(&saved.token).context("token rebinding absent")?;
                ensure!(
                    token != 0 && !tasks.contains_key(&token),
                    "restored token {token} is zero or duplicated"
                );
                let mut state = saved.state;
                match &mut state {
                    Task::Ambient {
                        channel: Some(channel),
                    } => {
                        ensure!(
                            *channel < 16,
                            "ambient saved channel {channel} exceeds sample slots"
                        );
                        *channel = *bindings
                            .ambient_channels
                            .context("ambient channel bindings unavailable")?
                            .get(channel)
                            .context("ambient saved channel has no binding")?;
                    }
                    Task::AmbientRtx {
                        slot: Some(slot), ..
                    } => {
                        ensure!(
                            *slot < 16,
                            "AmbientRtx saved sample slot {slot} exceeds sample slots"
                        );
                        *slot = *bindings
                            .ambient_channels
                            .context("ambient channel bindings unavailable")?
                            .get(slot)
                            .context("AmbientRtx saved sample slot has no binding")?;
                    }
                    Task::Dialogue | Task::PlayerDialogue { .. } | Task::SpeakerDialogue(_) => {
                        ensure!(
                            bindings.dialogue.is_some(),
                            "restored dialogue owner unavailable"
                        );
                    }
                    Task::SpeakerAnimation { index, player, .. } => {
                        ensure!(
                            *index < 3,
                            "rtxAnim saved group index {index} exceeds three"
                        );
                        ensure!(
                            bindings.dialogue.is_some(),
                            "restored dialogue owner unavailable"
                        );
                        // RTXpAnim animates the selected player, not the receiver this binds.
                        ensure!(
                            *player || bindings.animation.is_some(),
                            "restored rtxAnim animation owner unavailable"
                        );
                    }
                    Task::AnimationFrame { player: false } => {
                        ensure!(
                            bindings.animation.is_some(),
                            "restored frame-wait animation owner unavailable"
                        );
                    }
                    Task::Animation { group, .. } | Task::ControlAnimation { group, .. } => {
                        ensure!(
                            *group <= i16::MAX as u16,
                            "animation group {group} exceeds supported signed range"
                        );
                        ensure!(
                            bindings.animation.is_some(),
                            "restored animation owner unavailable"
                        );
                    }
                    _ => {}
                }
                Ok((token, state))
            };
            let (token, state) =
                prepare().with_context(|| format!("native task snapshot token {}", saved.token))?;
            next = next.max(token);
            tasks.insert(token, state);
        }
        Ok(Self { next, tasks })
    }
}

impl Task {
    fn contract(self) -> (&'static str, usize, Option<CallKind>) {
        match self {
            Self::Dialogue => ("WaitOnDialog", 0, Some(CallKind::Task)),
            Self::AnimationFrame { player: false } => ("WaitAnimFrame", 2, Some(CallKind::Task)),
            Self::AnimationFrame { player: true } => {
                ("WaitPlayerAnimFrame", 2, Some(CallKind::Task))
            }
            Self::PlayerDialogue { .. } => ("RTXp", 1, Some(CallKind::Task)),
            Self::GroupSync => ("SyncWithGroup", 1, Some(CallKind::Task)),
            Self::SpeakerDialogue(_) => ("RTX", 1, Some(CallKind::Task)),
            Self::SpeakerAnimation { player: false, .. } => ("rtxAnim", 4, Some(CallKind::Task)),
            Self::SpeakerAnimation { player: true, .. } => ("RTXpAnim", 4, Some(CallKind::Task)),
            Self::ShowPlayer { .. } => ("showPlayer", 0, Some(CallKind::Task)),
            Self::ShowPlayerPan { .. } => ("showPlayerPan", 1, Some(CallKind::Task)),
            Self::ObjectPan { command, .. } => (command.name(), 4, Some(CallKind::Task)),
            Self::ShowLocation { command, .. } => (command.name(), 3, Some(CallKind::Task)),
            Self::Look { .. } => ("lookObj", 1, Some(CallKind::Task)),
            Self::ObjectRotation => ("showObjRot", 4, Some(CallKind::Task)),
            Self::Ambient { .. } => ("AmbientSound", 3, Some(CallKind::Multitask)),
            Self::AmbientRtx { .. } => ("AmbientRtx", 3, Some(CallKind::Multitask)),
            Self::Movement { .. } => ("MoveByAxis", 3, None),
            Self::ObjectMovement { .. } => ("MoveObjectAxis", 3, None),
            Self::ScaledMovement { .. } => ("sMoveByAxis", 3, None),
            Self::Rotation { .. } => ("RotateByAxis", 3, None),
            Self::AbsoluteRotation { .. } => ("RotateToAxis", 3, None),
            Self::ScaledRotation { .. } => ("sRotateToAxis", 3, None),
            Self::ScaledRelativeRotation { .. } => ("sRotateByAxis", 3, None),
            Self::PlayerFace => ("PlayerFaceObj", 0, Some(CallKind::Task)),
            Self::PickupLineUp { .. } => ("PlayerPickupLineUp", 3, Some(CallKind::Task)),
            Self::ParallelLineUp => ("PlayerParallelLineUp", 3, Some(CallKind::Task)),
            Self::LineUp { .. } => ("PlayerLineUp", 3, Some(CallKind::Task)),
            Self::FacePlayer => ("FacePlayer", 1, None),
            Self::FacePlayerInertia => ("FacePlayerInertia", 1, None),
            Self::Backward => ("MoveBackward", 1, None),
            Self::WalkBackward => ("WalkBackward", 1, None),
            Self::Forward => ("MoveForward", 1, None),
            Self::WalkForward => ("WalkForward", 1, None),
            Self::Strafe(super::super::locomotion::Strafe::Left) => ("MoveLeft", 1, None),
            Self::Strafe(super::super::locomotion::Strafe::Right) => ("MoveRight", 1, None),
            Self::Location => ("MoveToLocation", 2, None),
            Self::AxisMovement { .. } => ("MoveAlongAxis", 3, None),
            Self::WanderLocation { .. } => ("WanderToLocation", 2, None),
            Self::Duration => ("Wait", 1, None),
            Self::Tasks => ("WaitOnTasks", 0, None),
            Self::Blocking => ("WaitNonMulti", 0, None),
            Self::Animation { .. } => ("PushAnimation", 2, None),
            Self::ControlAnimation { .. } => ("PushControlAnimation", 2, None),
        }
    }
}

#[cfg(test)]
mod tests;

impl NativeTasks {
    /// Actors without an executable VM cannot retain native continuations.
    pub(in crate::script) fn from_snapshot_without_vm(
        snapshot: NativeTasksSnapshot,
    ) -> Result<Self> {
        ensure!(
            snapshot.version == 1,
            "native task snapshot version unsupported"
        );
        ensure!(
            snapshot.tasks.is_empty(),
            "saved native tasks have no owning VM"
        );
        Ok(Self {
            next: snapshot.next,
            tasks: BTreeMap::new(),
        })
    }
}
