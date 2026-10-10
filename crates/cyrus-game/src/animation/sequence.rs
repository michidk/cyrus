//! Forward current-group playback from preservation's engine/animation.md.
use super::Animations;
use anyhow::Context;
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
enum Command {
    Frame(u32),
    End,
    Previous(usize),
    Future(usize),
    Breakpoint,
    Unsupported(u8),
}

/// A forward current group. Callers supply activation, whole nominal ticks, and exit eligibility.
/// Event commands, reverse playback, and other group types remain explicitly unsupported.
pub struct FrameSequence {
    context: String,
    commands: Vec<Command>,
    command: usize,
    speed: u8,
    countdown: u8,
    loops: u8,
    exit_reached: bool,
    ended: bool,
    failure: Option<String>,
    translation_suppressed: bool,
}

/// A resolved RAAN model/frame pair; this is not an attachment vertex selection.
#[derive(Debug, PartialEq, Eq)]
pub struct ModelFrame {
    /// Uppercase model key in the world's validated animation asset.
    pub model: String,
    /// Zero-based model frame index.
    pub frame: usize,
}

impl Animations {
    /// Resolves a file ShowFrame reference across RAAN entries in their original order.
    pub fn resolve_frame(&self, actor: &str, reference: u32) -> anyhow::Result<ModelFrame> {
        let context = format!(
            "world {} actor {actor} frame reference {reference}",
            self.world
        );
        let actor = self.actor(actor).with_context(|| context.clone())?;
        let entries = actor["raan_entries"]
            .as_array()
            .with_context(|| format!("{context}: RAAN entries absent"))?;
        let mut remaining = u64::from(reference);
        for entry in entries {
            let count = entry["frame_count"]
                .as_u64()
                .with_context(|| format!("{context}: RAAN frame count absent"))?;
            if remaining < count {
                let path = entry["file_path"]
                    .as_str()
                    .with_context(|| format!("{context}: RAAN model path absent"))?;
                let filename = path.rsplit(['/', '\\']).next().unwrap_or(path);
                let model = filename
                    .rsplit_once('.')
                    .map_or(filename, |(stem, _)| stem)
                    .to_ascii_uppercase();
                let frame = usize::try_from(remaining)?;
                let data = self
                    .models
                    .get(&model)
                    .with_context(|| format!("{context}: model {model} unavailable"))?;
                anyhow::ensure!(
                    frame < data.positions.len(),
                    "{context}: model {model} has no frame {frame}"
                );
                return Ok(ModelFrame { model, frame });
            }
            remaining -= count;
        }
        anyhow::bail!("{context}: outside RAAN frame table")
    }

    fn actor(&self, name: &str) -> anyhow::Result<&serde_json::Value> {
        self.metadata["actors"]
            .as_array()
            .with_context(|| format!("world {} actor {name}: actor table absent", self.world))?
            .iter()
            .find(|actor| {
                actor["script_name"]
                    .as_str()
                    .is_some_and(|script| script.eq_ignore_ascii_case(name))
            })
            .with_context(|| format!("world {} actor {name}: absent", self.world))
    }

    /// Selects an explicitly requested forward group and its finite loop countdown.
    /// This does not run the actor script or choose a default animation.
    pub fn sequence(&self, actor: &str, group: u16, loops: u8) -> anyhow::Result<FrameSequence> {
        self.validate()?;
        let context = format!("world {} actor {actor} group {group}", self.world);
        let actor = self.actor(actor)?;
        let selected = actor["animation_groups"]
            .as_array()
            .with_context(|| format!("{context}: groups absent"))?
            .iter()
            .find(|candidate| candidate["group_index"].as_u64() == Some(u64::from(group)))
            .with_context(|| format!("{context}: absent"))?;
        anyhow::ensure!(
            selected["anim_type"].as_u64() == Some(0),
            "{context}: group type unsupported"
        );
        let speed = u16::try_from(
            selected["anim_id"]
                .as_u64()
                .with_context(|| format!("{context}: frame countdown absent"))?,
        )
        .with_context(|| format!("{context}: frame countdown outside u16 range"))?
        .to_le_bytes()[0];
        let mut commands = Vec::new();
        for (index, command) in selected["commands"]
            .as_array()
            .with_context(|| format!("{context}: command table absent"))?
            .iter()
            .enumerate()
        {
            let opcode = u8::try_from(
                command["opcode"]
                    .as_u64()
                    .with_context(|| format!("{context} command {index}: opcode absent"))?,
            )?;
            let decoded = match opcode {
                0 => Command::Frame(u32::try_from(command["frame_reference"].as_u64().with_context(|| format!("{context} command {index}: file frame reference absent; re-export the world"))?)?),
                1 => Command::End,
                5 => Command::Breakpoint,
                2 | 3 => {
                    let target = usize::try_from(command["value"].as_u64().with_context(|| format!("{context} command {index}: invalid target"))?)?;
                    if opcode == 2 { Command::Previous(target) } else { Command::Future(target) }
                }
                _ => Command::Unsupported(opcode),
            };
            commands.push(decoded);
        }
        for (index, command) in commands.iter().enumerate() {
            if let Command::Previous(target) | Command::Future(target) = command {
                anyhow::ensure!(
                    *target < commands.len(),
                    "{context} command {index}: target {target} outside group"
                );
            }
        }
        Ok(FrameSequence {
            context,
            commands,
            command: 0,
            speed,
            countdown: speed,
            loops,
            exit_reached: false,
            ended: false,
            failure: None,
            translation_suppressed: false,
        })
    }
}

impl FrameSequence {
    /// The current group's BreakPoint marker, checked by ordinary forward walking.
    /// End does not select a replacement group or establish other movement eligibility.
    pub fn translation_suppressed(&self) -> bool {
        self.translation_suppressed
    }

    /// Advances using the original whole nominal-tick input; returns a file frame reference.
    /// `None` means EndAnimation. Unknown events fail at their command without being skipped.
    pub fn advance(&mut self, ticks: u32, exit_requested: bool) -> anyhow::Result<Option<u32>> {
        if let Some(message) = &self.failure {
            anyhow::bail!("{message}");
        }
        let result = self.advance_current(ticks, exit_requested);
        if let Err(error) = &result {
            self.failure = Some(error.to_string());
        }
        result
    }

    fn advance_current(&mut self, ticks: u32, exit_requested: bool) -> anyhow::Result<Option<u32>> {
        if self.ended {
            return Ok(None);
        }
        let mut skip = 0;
        if ticks != 0 {
            if self.countdown == 0 {
                self.command += 1;
                self.translation_suppressed = false;
                self.countdown = self.speed;
                skip = ticks - 1;
            }
            self.countdown = self.countdown.wrapping_sub(1);
        }
        let mut visited = BTreeSet::new();
        loop {
            anyhow::ensure!(
                visited.insert((self.command, self.loops)),
                "{} command {}: control cycle has no displayed frame",
                self.context,
                self.command
            );
            let command = self.commands.get(self.command).with_context(|| {
                format!("{} command {}: outside group", self.context, self.command)
            })?;
            match *command {
                Command::Frame(reference) => {
                    if skip == 0 {
                        return Ok(Some(reference));
                    }
                    skip -= 1;
                    self.command += 1;
                    self.translation_suppressed = false;
                    visited.clear();
                }
                Command::End => {
                    self.ended = true;
                    return Ok(None);
                }
                Command::Previous(target) => {
                    if self.loops != 0 {
                        self.loops -= 1;
                        if self.loops == 0 {
                            self.command += 1;
                            continue;
                        }
                    }
                    self.command = target;
                    self.translation_suppressed = false;
                }
                Command::Future(target) => {
                    if self.loops == 0 && (exit_requested || self.exit_reached) {
                        self.exit_reached = true;
                        self.command = target;
                        self.translation_suppressed = false;
                    } else {
                        self.command += 1;
                    }
                }
                Command::Breakpoint => {
                    self.translation_suppressed = true;
                    self.command += 1;
                }
                Command::Unsupported(opcode) => anyhow::bail!(
                    "{} command {}: opcode {opcode} playback unsupported",
                    self.context,
                    self.command
                ),
            }
        }
    }
}
