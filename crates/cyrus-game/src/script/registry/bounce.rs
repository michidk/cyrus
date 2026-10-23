//! The receiver's side of `BounceObject`: live actor state behind [`BouncePort`]. The rules are
//! `script::bounce`'s; a missing input fails the native instead of continuing on a guess.
use super::{GameHost, ScriptActorId};
use crate::script::bounce::BouncePort;
use anyhow::{Context, Result};

struct BounceActor<'h, 'a, 's, 'i, 'r> {
    host: &'h mut GameHost<'a, 's, 'i, 'r>,
    id: ScriptActorId,
    frame_scale: u32,
    ticks: u32,
    item_system_ready: bool,
}

impl<'h, 'a, 's, 'i, 'r> BounceActor<'h, 'a, 's, 'i, 'r> {
    /// Validates every input the state machine reads, so the port methods cannot meet an absent one.
    fn new(host: &'h mut GameHost<'a, 's, 'i, 'r>) -> Result<Self> {
        let id = ScriptActorId {
            generation: host.generation,
            index: host.index,
        };
        let actor = &host.registry.actors[host.index];
        anyhow::ensure!(
            actor.control.is_some() && actor.animation.is_some(),
            "BounceObject receiver control and animation unavailable"
        );
        host.registry
            .spatial(id)
            .context("BounceObject receiver pose unavailable")?;
        anyhow::ensure!(
            host.sounds.is_some(),
            "BounceObject requires synchronous sounds"
        );
        let frame_scale = host
            .inputs
            .frame_scale
            .context("BounceObject requires frame_scale")?;
        let ticks = host
            .inputs
            .nominal_ticks
            .context("BounceObject requires nominal_ticks")?;
        let item_system_ready = host
            .inputs
            .hud
            .context("BounceObject inventory owner unavailable")?
            .inventory_ready()?;
        Ok(Self {
            host,
            id,
            frame_scale,
            ticks,
            item_system_ready,
        })
    }

    fn control(&self) -> &super::ActorControlState {
        self.host.registry.actors[self.id.index]
            .control
            .as_ref()
            .expect("validated receiver control")
    }

    fn control_mut(&mut self) -> &mut super::ActorControlState {
        self.host.registry.actors[self.id.index]
            .control
            .as_mut()
            .expect("validated receiver control")
    }

    fn pose(&self) -> ([i32; 3], [u16; 3]) {
        let local = self
            .host
            .registry
            .spatial(self.id)
            .expect("validated receiver pose")
            .local;
        (local.position, local.angles)
    }
}

impl BouncePort for BounceActor<'_, '_, '_, '_, '_> {
    fn item_system_ready(&self) -> Result<bool> {
        Ok(self.item_system_ready)
    }
    fn frame_scale(&self) -> u32 {
        self.frame_scale
    }
    fn living_state(&self) -> u8 {
        self.control().living_state
    }
    fn set_living_state(&mut self, state: u8) {
        self.control_mut().living_state = state;
    }
    fn surface(&self) -> u8 {
        self.host.registry.actors[self.id.index]
            .surface
            .unwrap_or(0)
    }
    fn holding_weapon(&mut self) -> Result<bool> {
        Ok(self
            .host
            .actor_weapon_predicates(self.id.index, "IsHoldingWeapon")?
            .0)
    }
    fn combat_mode(&self) -> bool {
        self.control()
            .combat
            .as_ref()
            .is_some_and(|combat| combat.flags & 4 != 0)
    }
    fn torch_burning(&self) -> bool {
        self.control().torch_burning != 0
    }
    fn attribute_32(&self) -> u8 {
        self.host.registry.actors[self.id.index].attributes[32]
    }
    fn clear_motion(&mut self) {
        let control = self.control_mut();
        control.falling_state = 0;
        control.queued_motion = 0;
        control.sliding_state = 0;
        control.motion_vector[0] = 0;
        control.motion_vector[1] = 0;
        self.host.registry.actors[self.id.index].sync_falling_motion();
    }
    fn falling_state(&self) -> u8 {
        self.control().falling_state
    }
    fn begin_fall(&mut self) {
        let (position, _) = self.pose();
        let attribute = self.attribute_32();
        let (origin, timer) = super::falling::ordinary_fall_origin(position[1], attribute);
        let control = self.control_mut();
        control.fall_origin_y = origin;
        control.fall_timer = timer;
        control.falling_state = 1;
        self.host.registry.actors[self.id.index].sync_falling_motion();
    }
    fn lift(&self) -> i32 {
        self.control().fall_origin_y
    }
    fn set_lift(&mut self, lift: i32) {
        self.control_mut().fall_origin_y = lift;
    }
    fn position(&self) -> [i32; 3] {
        self.pose().0
    }
    fn set_position(&mut self, position: [i32; 3]) -> Result<()> {
        let index = self.id.index;
        let spatial = self
            .host
            .registry
            .spatial
            .as_mut()
            .context("BounceObject spatial state not prepared")?
            .actor_mut(index)?;
        spatial.local.position = position;
        if !self.host.registry.map_updates.contains(&index) {
            self.host.registry.map_updates.push(index);
        }
        Ok(())
    }
    fn heading(&self) -> u16 {
        self.pose().1[1]
    }
    fn set_heading(&mut self, heading: u16) {
        let index = self.id.index;
        if let Some(spatial) = self
            .host
            .registry
            .spatial
            .as_mut()
            .and_then(|spatial| spatial.actor_mut(index).ok())
            && spatial.local.angles[1] != heading
        {
            spatial.local.angles[1] = heading;
            // A failed transform refresh surfaces on the next pose read of the same actor.
            let _ = spatial.refresh_transform();
        }
    }
    fn is_selected_player(&self) -> bool {
        self.host.selected_player() == Some(self.id)
    }
    fn turn_delta(&self, scale: u32) -> i64 {
        self.host.registry.globals.turn.delta(scale)
    }
    fn current_group(&self) -> Option<u16> {
        self.host.registry.actors[self.id.index]
            .animation
            .as_ref()
            .and_then(|animation| animation.playback.current_group())
    }
    fn request_group(&mut self, group: u16, force: bool) -> Result<()> {
        let sounds = self
            .host
            .sounds
            .as_mut()
            .context("BounceObject requires synchronous sounds")?;
        self.host
            .registry
            .request_ordinary_group(self.id, group, force, self.ticks, &mut **sounds)
    }
}

impl GameHost<'_, '_, '_, '_> {
    /// First invocation of `BounceObject`. The receiver's native tasks leave the actor for the
    /// call so the state machine can use the world.
    pub(super) fn bounce_call(
        &mut self,
        function: &str,
        kind: super::CallKind,
        arguments: &[super::Argument],
        invocation: cyrus_script::Invocation,
    ) -> Result<Option<super::CallResult>> {
        let index = self.index;
        let mut tasks = std::mem::take(&mut self.registry.actors[index].tasks);
        let result = BounceActor::new(self).and_then(|mut port| {
            tasks.call(
                (function, kind),
                arguments,
                invocation,
                &mut None,
                &mut None,
                crate::script::tasks::NativeServices {
                    bounce: Some(&mut port),
                    ..Default::default()
                },
            )
        });
        self.registry.actors[index].tasks = tasks;
        result
    }

    /// Later invocation of a `BounceObject` continuation.
    pub(super) fn bounce_poll(
        &mut self,
        token: u64,
        arguments: &[super::Argument],
        invocation: cyrus_script::Invocation,
    ) -> Result<bool> {
        let index = self.index;
        let mut tasks = std::mem::take(&mut self.registry.actors[index].tasks);
        let result = BounceActor::new(self).and_then(|mut port| {
            tasks.poll(
                token,
                arguments,
                invocation,
                &mut None,
                &mut None,
                crate::script::tasks::NativeServices {
                    bounce: Some(&mut port),
                    ..Default::default()
                },
            )
        });
        self.registry.actors[index].tasks = tasks;
        result
    }
}
