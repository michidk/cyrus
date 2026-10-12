//! Animation host for the shared SOUP core; eligibility and ticks remain explicit.
use super::tasks::{AnimationCall, NativeTasks};
use anyhow::Context;
use cyrus_assets::{
    animation::Animations,
    script::{CallKind, Program},
};
use cyrus_script::{Argument, CallResult, Host, Runtime, Target, Thread};
use cyrus_world::animation::{ActorAnimation, AnimationSound};

/// Explicit animation-only host. Unknown native services fail instead of succeeding.
pub struct AnimationScript {
    runtime: Runtime,
    tasks: NativeTasks,
}
impl AnimationScript {
    pub fn from_asset(data: &Animations, actor: &str) -> anyhow::Result<AnimationScript> {
        data.require_version()?;
        let context = format!("world {} actor {actor} script", data.world);
        let metadata = data.actor(actor)?;
        if let Some(attributes) = metadata.get("attributes") {
            let attributes = attributes
                .as_object()
                .with_context(|| format!("{context}: invalid attributes"))?;
            for attribute in [29, 30] {
                if let Some(value) = attributes.get(&format!("attr_{attribute}")) {
                    let value = value
                        .as_u64()
                        .with_context(|| format!("{context}: invalid attribute {attribute}"))?;
                    anyhow::ensure!(
                        value == 0,
                        "{context}: synchronization channels unsupported"
                    );
                }
            }
        }
        let descriptor = data.metadata["animation_scripts"]
            .as_object()
            .and_then(|scripts| {
                scripts
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(actor))
            })
            .map(|(_, descriptor)| descriptor)
            .with_context(|| format!("{context}: program absent; re-export the world"))?;
        let program: Program = serde_json::from_value(descriptor.clone())
            .with_context(|| format!("{context}: invalid program descriptor"))?;
        anyhow::ensure!(program.entry == 0, "{context}: interrupt entry unsupported");
        Ok(AnimationScript {
            runtime: cyrus_script::Runtime::new(program, context)?,
            tasks: NativeTasks::default(),
        })
    }
}

impl AnimationScript {
    pub fn program_counter(&self) -> u32 {
        self.runtime.program_counter(Thread::Main).unwrap()
    }
    pub fn waiting_for_group(&self) -> Option<u16> {
        self.tasks.animation_group()
    }
    pub fn advance(
        &mut self,
        animation: &mut ActorAnimation,
        ticks: u32,
    ) -> anyhow::Result<Option<u32>> {
        self.dispatch(animation, ticks, None)
    }
    pub fn advance_with_sounds(
        &mut self,
        animation: &mut ActorAnimation,
        ticks: u32,
        sounds: &mut dyn FnMut(AnimationSound) -> anyhow::Result<()>,
    ) -> anyhow::Result<Option<u32>> {
        self.dispatch(animation, ticks, Some(sounds))
    }
    fn dispatch(
        &mut self,
        animation: &mut ActorAnimation,
        ticks: u32,
        sounds: Option<&mut dyn FnMut(AnimationSound) -> anyhow::Result<()>>,
    ) -> anyhow::Result<Option<u32>> {
        let mut frame = None;
        let mut host = AnimationHost {
            input: Some(AnimationCall {
                animation,
                ticks,
                sounds,
                frame: &mut frame,
            }),
            tasks: &mut self.tasks,
        };
        // This explicit inspection host has no gameplay globals or calibrated production clock.
        self.runtime
            .invoke(Thread::Main, &mut host, &mut [], 100_000)?;
        Ok(frame)
    }
}
struct AnimationHost<'a, 'sound> {
    input: Option<AnimationCall<'a, 'sound>>,
    tasks: &'a mut NativeTasks,
}
impl Host for AnimationHost<'_, '_> {
    fn call(
        &mut self,
        target: &Target,
        kind: CallKind,
        function: &str,
        arguments: &[Argument],
        invocation: cyrus_script::Invocation,
        _: &mut [i32],
    ) -> anyhow::Result<CallResult> {
        anyhow::ensure!(
            target == &Target::Me,
            "target {target:?} unsupported by animation host"
        );
        anyhow::ensure!(
            kind == CallKind::Task && function.eq_ignore_ascii_case("PushAnimation"),
            "task {function} unsupported"
        );
        self.tasks
            .call(function, kind, arguments, invocation, &mut self.input)?
            .context("animation task absent")
    }
    fn poll(
        &mut self,
        token: u64,
        arguments: &[Argument],
        invocation: cyrus_script::Invocation,
        _: &mut [i32],
    ) -> anyhow::Result<bool> {
        self.tasks
            .poll(token, arguments, invocation, &mut self.input)
    }
    fn cancel(&mut self, token: u64) -> anyhow::Result<()> {
        self.tasks.cancel(token)
    }
    fn end(&mut self, _: Thread) -> anyhow::Result<()> {
        Ok(())
    }
}
