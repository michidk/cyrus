//! Native task continuations validate against saved VM calls without execution.
use super::{NativeTasks, Task};
use anyhow::{Context, Result, ensure};
use cyrus_script::{Native, persistence::Snapshot, program::CallKind};
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
    pub(in crate::script) fn validate_actor_references(
        &self,
        mut validate: impl FnMut(u32) -> Result<()>,
    ) -> Result<()> {
        for (&token, state) in &self.tasks {
            let (name, index) = match state {
                Task::LineUp {
                    reference: Some(index),
                    ..
                } => ("PlayerLineUp", *index),
                Task::ReceiverLineUp {
                    reference: Some(index),
                    ..
                } => ("LineUp", *index),
                _ => continue,
            };
            validate(index)
                .with_context(|| format!("native {name} token {token} reference {index}"))?;
        }
        Ok(())
    }

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
                    call.call.function == name,
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
                    Task::AnimationRequest => {
                        ensure!(
                            bindings.animation.is_some(),
                            "restored PlayAnimation request animation owner unavailable"
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
                            i16::try_from(*group).is_ok(),
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
    fn contract(self) -> (Native, usize, Option<CallKind>) {
        match self {
            Self::ColorFlicker => (Native::FxFlickerDim, 1, Some(CallKind::Multitask)),
            Self::ColorPhase => (Native::FxPhase, 7, Some(CallKind::Multitask)),
            Self::RadiusFlicker => (Native::FxFlickerOnOff, 1, Some(CallKind::Multitask)),
            Self::AnimationRequest => (Native::PlayAnimation, 2, Some(CallKind::Multitask)),
            Self::Dialogue => (Native::WaitOnDialog, 0, Some(CallKind::Task)),
            Self::Menu => (Native::MenuProc, 0, Some(CallKind::Task)),
            Self::AnimationFrame { player: false } => {
                (Native::WaitAnimFrame, 2, Some(CallKind::Task))
            }
            Self::AnimationFrame { player: true } => {
                (Native::WaitPlayerAnimFrame, 2, Some(CallKind::Task))
            }
            Self::PlayerDialogue { .. } => (Native::RTXp, 1, None),
            Self::GroupSync => (Native::SyncWithGroup, 1, Some(CallKind::Task)),
            Self::UserSync => (Native::SyncWithUser, 1, Some(CallKind::Task)),
            Self::SpeakerDialogue(_) => (Native::Rtx, 1, None),
            Self::SpeakerAnimation { player: false, .. } => {
                (Native::RtxAnim, 4, Some(CallKind::Task))
            }
            Self::SpeakerAnimation { player: true, .. } => {
                (Native::RTXpAnim, 4, Some(CallKind::Task))
            }
            Self::ShowPlayer { .. } => (Native::ShowPlayer, 0, Some(CallKind::Task)),
            Self::ShowPlayerPan { .. } => (Native::ShowPlayerPan, 1, Some(CallKind::Task)),
            Self::ObjectPan { command, .. } => (command.native(), 4, None),
            Self::ShowLocation { command, .. } => (command.native(), 3, None),
            Self::Look { .. } => (Native::LookObj, 1, Some(CallKind::Task)),
            Self::LookCyrus { .. } => (Native::LookCyrus, 1, Some(CallKind::Task)),
            Self::ObjectRotation => (Native::ShowObjRot, 4, Some(CallKind::Task)),
            Self::Ambient { .. } => (Native::AmbientSound, 3, Some(CallKind::Multitask)),
            Self::AmbientRtx { .. } => (Native::AmbientRtx, 3, Some(CallKind::Multitask)),
            Self::Movement { .. } => (Native::MoveByAxis, 3, None),
            Self::ObjectMovement { .. } => (Native::MoveObjectAxis, 3, None),
            Self::ScaledMovement { .. } => (Native::SMoveByAxis, 3, None),
            Self::Rotation { .. } => (Native::RotateByAxis, 3, None),
            Self::AbsoluteRotation { .. } => (Native::RotateToAxis, 3, None),
            Self::ScaledRotation { .. } => (Native::SRotateToAxis, 3, None),
            Self::ScaledRelativeRotation { .. } => (Native::SRotateByAxis, 3, None),
            Self::PlayerFace => (Native::PlayerFaceObj, 0, Some(CallKind::Task)),
            Self::PickupLineUp { .. } => (Native::PlayerPickupLineUp, 3, Some(CallKind::Task)),
            Self::ParallelLineUp => (Native::PlayerParallelLineUp, 3, Some(CallKind::Task)),
            Self::Bounce(_) => (Native::BounceObject, 3, Some(CallKind::Task)),
            Self::LineUp { .. } => (Native::PlayerLineUp, 3, Some(CallKind::Task)),
            Self::PlayerFaceTalk => (Native::PlayerFaceTalk, 1, Some(CallKind::Task)),
            Self::ReceiverLineUp { .. } => (Native::LineUp, 3, Some(CallKind::Task)),
            Self::ObjectLineUp => (Native::ObjectLineUp, 3, Some(CallKind::Task)),
            Self::ObjectParallelLineUp => (Native::ObjectParallelLineUp, 3, Some(CallKind::Task)),
            Self::ObjectPickupLineUp { .. } => {
                (Native::ObjectPickupLineUp, 3, Some(CallKind::Task))
            }
            Self::FacePlayer => (Native::FacePlayer, 1, None),
            Self::FaceLocation => (Native::FaceLocation, 2, None),
            Self::FacePlayerInertia => (Native::FacePlayerInertia, 1, None),
            Self::Move => (Native::Move, 4, None),
            Self::Rotate => (Native::Rotate, 4, None),
            Self::ScaledAxisMovement => (Native::SMoveAlongAxis, 3, None),
            Self::ScaledMove => (Native::SMove, 4, None),
            Self::ScaledRotate => (Native::SRotate, 4, None),
            Self::PlayerTilt => (Native::PlayerTilt, 3, Some(CallKind::Multitask)),
            Self::PlayerTiltRevx => (Native::PlayerTiltRevx, 3, Some(CallKind::Multitask)),
            Self::PlayerOffset => (Native::PlayerOffsettask, 3, Some(CallKind::Multitask)),
            Self::DragonBreath => (Native::DragonBreathTask, 0, Some(CallKind::Multitask)),
            Self::Projectile => (Native::ProjectileAI, 0, None),
            Self::MoveMarker => (Native::MoveMarker, 2, None),
            Self::MoveLocation => (Native::MoveLocation, 2, None),
            Self::GotoLocation => (Native::GotoLocation, 1, Some(CallKind::Task)),
            Self::MovePosition => (Native::MovePosition, 4, None),
            Self::MoveNodeMarker => (Native::MoveNodeMarker, 2, None),
            Self::MoveNodeLocation => (Native::MoveNodeLocation, 2, None),
            Self::MoveNodePosition => (Native::MoveNodePosition, 4, None),
            Self::FaceAngle => (Native::FaceAngle, 1, None),
            Self::FacePos => (Native::FacePos, 2, None),
            Self::FaceObject => (Native::FaceObject, 1, None),
            Self::Backward => (Native::MoveBackward, 1, None),
            Self::WalkBackward => (Native::WalkBackward, 1, None),
            Self::Forward => (Native::MoveForward, 1, None),
            Self::WalkForward => (Native::WalkForward, 1, None),
            // Only created synchronously, never stored; it runs `MoveForward`'s step.
            Self::Lunge => (Native::MoveForward, 1, None),
            Self::Strafe(super::super::locomotion::Strafe::Left) => (Native::MoveLeft, 1, None),
            Self::Strafe(super::super::locomotion::Strafe::Right) => (Native::MoveRight, 1, None),
            Self::Location => (Native::MoveToLocation, 2, None),
            Self::Marker => (Native::MoveToMarker, 2, None),
            Self::AxisMovement { .. } => (Native::MoveAlongAxis, 3, None),
            Self::WanderLocation { .. } => (Native::WanderToLocation, 2, None),
            Self::Duration => (Native::Wait, 1, None),
            Self::ViewGxa => (Native::ViewGXA, 3, None),
            Self::AnimateGxa => (Native::AnimateGXA, 4, None),
            Self::WaitLineUp => (Native::WaitLineUp, 1, None),
            Self::Tasks => (Native::WaitOnTasks, 0, None),
            Self::Blocking => (Native::WaitNonMulti, 0, None),
            Self::Animation { .. } => (Native::PushAnimation, 2, None),
            Self::ControlAnimation { .. } => (Native::PushControlAnimation, 2, None),
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
