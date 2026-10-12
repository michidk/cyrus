//! Animation host for the shared SOUP core; eligibility and ticks remain explicit.
use anyhow::Context;
use cyrus_assets::{
    animation::Animations,
    script::{CallKind, Program},
};
use cyrus_script::{Argument, CallResult, Host, Runtime, Target, Thread};
use cyrus_world::animation::{ActorAnimation, AnimationSound};

struct BlockingTask {
    group: u16,
    count: u8,
    started: bool,
}

/// Explicit animation-only host. Unknown native services fail instead of succeeding.
pub struct AnimationScript {
    runtime: Runtime,
    task: Option<BlockingTask>,
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
            task: None,
        })
    }
}

impl AnimationScript {
    pub fn program_counter(&self) -> u32 {
        self.runtime.program_counter(Thread::Main).unwrap()
    }
    pub fn waiting_for_group(&self) -> Option<u16> {
        self.task.as_ref().map(|task| task.group)
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
        let mut host = AnimationHost {
            animation,
            ticks,
            sounds,
            task: &mut self.task,
            frame: None,
        };
        // This explicit inspection host has no gameplay globals or calibrated production clock.
        self.runtime
            .invoke(Thread::Main, &mut host, &mut [], 100_000)?;
        Ok(host.frame)
    }
}
struct AnimationHost<'a, 'sound> {
    animation: &'a mut ActorAnimation,
    ticks: u32,
    sounds: Option<&'sound mut dyn FnMut(AnimationSound) -> anyhow::Result<()>>,
    task: &'a mut Option<BlockingTask>,
    frame: Option<u32>,
}
impl AnimationHost<'_, '_> {
    fn request(&mut self) -> anyhow::Result<()> {
        let task = self.task.as_mut().context("animation task absent")?;
        if self.animation.current_group() == Some(task.group) {
            self.animation.set_loop_count(task.count)?;
            task.started = true;
        } else {
            let frame = match &mut self.sounds {
                Some(sounds) => self
                    .animation
                    .request_with_sounds(task.group, 0, self.ticks, *sounds)?,
                None => self.animation.request(task.group, 0, self.ticks)?,
            };
            self.frame = frame.or(self.frame);
        }
        Ok(())
    }
}
impl Host for AnimationHost<'_, '_> {
    fn call(
        &mut self,
        target: &Target,
        kind: CallKind,
        function: &str,
        arguments: &[Argument],
        _: cyrus_script::Invocation,
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
        let [group, count] = arguments else {
            anyhow::bail!("PushAnimation requires two arguments")
        };
        let group = u16::try_from(group.integer()?)
            .context("PushAnimation group outside supported range")?;
        anyhow::ensure!(
            group <= i16::MAX as u16,
            "PushAnimation signed group unsupported"
        );
        *self.task = Some(BlockingTask {
            group,
            count: count.integer()? as u8,
            started: false,
        });
        self.request()?;
        Ok(CallResult::Pending(1))
    }
    fn poll(
        &mut self,
        token: u64,
        arguments: &[Argument],
        _: cyrus_script::Invocation,
        _: &mut [i32],
    ) -> anyhow::Result<bool> {
        anyhow::ensure!(token == 1, "animation task token unavailable");
        let [group, count] = arguments else {
            anyhow::bail!("PushAnimation requires two arguments")
        };
        let group = u16::try_from(group.integer()?)
            .context("PushAnimation group outside supported range")?;
        anyhow::ensure!(
            group <= i16::MAX as u16,
            "PushAnimation signed group unsupported"
        );
        let task = self.task.as_mut().context("animation task absent")?;
        task.group = group;
        task.count = count.integer()? as u8;
        let task = self.task.as_ref().context("animation task absent")?;
        if task.started {
            if self.animation.current_group() == Some(task.group) {
                return Ok(false);
            }
            *self.task = None;
            Ok(true)
        } else {
            self.request()?;
            Ok(false)
        }
    }
    fn cancel(&mut self, token: u64) -> anyhow::Result<()> {
        anyhow::ensure!(token == 1, "animation task token unavailable");
        *self.task = None;
        Ok(())
    }
    fn end(&mut self, _: Thread) -> anyhow::Result<()> {
        Ok(())
    }
}
