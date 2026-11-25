//! Effects native-call dispatch.
use super::super::{
    Argument, CallKind, CallResult, GameHost, Native, Result, ScriptActorId, flat, light,
};
use anyhow::Context;

impl GameHost<'_, '_, '_, '_> {
    pub(in crate::script::registry) fn call_effects(
        &mut self,
        kind: CallKind,
        function: Native,
        arguments: &[Argument],
        id: ScriptActorId,
    ) -> Result<Option<CallResult>> {
        if kind == CallKind::Function
            && [
                Native::Light,
                Native::LightRadius,
                Native::LightIntensity,
                Native::LightOffset,
                Native::LightFlicker,
                Native::FlickerLight,
                Native::LightFlickerOff,
                Native::LightSize,
                Native::FxLightSize,
                Native::LightSizeOff,
            ]
            .contains(&function)
        {
            let values = arguments
                .iter()
                .map(|argument| argument.integer())
                .collect::<Result<Vec<_>>>()?;
            let expected = match function {
                Native::Light | Native::LightFlicker | Native::LightSize | Native::FxLightSize => 2,
                Native::LightOffset => 3,
                Native::LightFlickerOff | Native::LightSizeOff => 0,
                _ => 1,
            };
            anyhow::ensure!(values.len() == expected, "{function}: wrong argument count");
            let actor = &mut self.registry.actors[self.index];
            let placement = actor.placement();
            let position = placement
                .map(|placement| placement.map_position.map(|value| value as i32))
                .unwrap_or_default();
            let light = actor
                .light
                .get_or_insert_with(|| light::LightState::dynamic(position));
            // Stored values keep their low sixteen bits, as the original's word stores do.
            match function {
                Native::Light => {
                    light.enabled = true;
                    light.starting_radius = values[0] as i16;
                    light.radius = values[0] as i16;
                    light.starting_intensity = values[1] as i16;
                    light.intensity = values[1] as i16;
                }
                Native::LightRadius => {
                    light.starting_radius = values[0] as i16;
                    light.radius = values[0] as i16;
                    light.enabled = true;
                    light.inactive_ticks = 0;
                }
                Native::LightIntensity => {
                    light.starting_intensity = values[0] as i16;
                    light.intensity = values[0] as i16;
                    light.enabled = true;
                    light.inactive_ticks = 0;
                }
                Native::LightOffset => {
                    light.offset = [values[0] as i16, values[1] as i16, values[2] as i16]
                }
                Native::LightFlicker => light.flicker = [values[0] as u16, 0, values[1] as u16],
                Native::LightSize | Native::FxLightSize => {
                    light.size = [values[0] as u16, 0, values[1] as u16]
                }
                Native::LightSizeOff => light.size[0] = 0,
                Native::FlickerLight => light.flicker = [values[0] as u16, 0, 1],
                _ => light.flicker[0] = 0,
            }
            return Ok(Some(CallResult::Complete(0)));
        }

        if kind == CallKind::Function
            && [
                Native::Flat,
                Native::FlatSetTexture,
                Native::FlatOff,
                Native::FlatOffset,
                Native::FlatAnimate,
                Native::FlatStop,
                Native::FlatLikeStatic,
            ]
            .contains(&function)
        {
            let values = arguments
                .iter()
                .map(|argument| argument.integer())
                .collect::<Result<Vec<_>>>()?;
            flat::call(
                &mut self.registry.actors[self.index].flat,
                function,
                &values,
                &self.registry.texture_frames,
            )?;
            return Ok(Some(CallResult::Complete(0)));
        }

        if kind == CallKind::Function && function == Native::LightOff {
            anyhow::ensure!(arguments.is_empty(), "LightOff requires no arguments");
            let actor = &mut self.registry.actors[self.index];
            if let Some(light) = &mut actor.light {
                light.locked = false;
                light.enabled = false;
                light.inactive_ticks = 0;
            }
            return Ok(Some(CallResult::Complete(0)));
        }

        if kind == CallKind::Function
            && (function == Native::LightTorch || function == Native::UnlightTorch)
        {
            anyhow::ensure!(arguments.is_empty(), "{function} requires no arguments");
            let inputs = self.torch_inputs(self.index, function)?;
            let ticks = self
                .inputs
                .nominal_ticks
                .with_context(|| format!("{function} nominal ticks unavailable"))?;
            let sounds = self
                .sounds
                .as_deref_mut()
                .with_context(|| format!("{function} sound handler unavailable"))?;
            if function == Native::LightTorch {
                self.registry.light_torch(id, inputs, ticks, sounds)?;
            } else {
                self.registry.unlight_torch(id, inputs, ticks, sounds)?;
            }
            return Ok(Some(CallResult::Complete(0)));
        }

        // GolemSteam: once every few updates, a puff of steam at one of the receiver's four weapon
        // attachment vertices (a grip or a scabbard point), in the alternating effect variants.
        if kind == CallKind::Function && function == Native::GolemSteam {
            anyhow::ensure!(arguments.is_empty(), "GolemSteam requires no arguments");
            let excluded = self.registry.actors[self.index]
                .control
                .as_ref()
                .is_some_and(|control| control.object_update_excluded);
            if excluded {
                return Ok(Some(CallResult::Complete(0)));
            }
            let random = self
                .random
                .as_deref_mut()
                .context("GolemSteam shared engine random stream unavailable")?;
            if random.rnd(4)? != 0 {
                return Ok(Some(CallResult::Complete(0)));
            }
            let field = ["grip0", "scabbard1", "grip1", "scabbard0"][random.rnd(4)? as usize];
            let id = self.registry.id(self.index)?;
            let rotated = self.attachment_offset(Native::GolemSteam, field)?;
            let position = self.registry.spatial(id)?.local.position;
            let point = std::array::from_fn(|axis| rotated[axis].wrapping_add(position[axis]));
            let descriptor = if self.registry.globals.world_loop_frame & 1 != 0 {
                0x32480
            } else {
                0x22480
            };
            self.registry.spawn_environment_effect(descriptor, point)?;
            return Ok(Some(CallResult::Complete(0)));
        }

        // DragonBreath: the dragon's smoke, fireball and death item swap; `DragonBreathTask` runs the
        // same update as a multitask. The return is one while the dragon keeps breathing.
        if kind == CallKind::Function && function == Native::DragonBreath {
            anyhow::ensure!(arguments.is_empty(), "DragonBreath requires no arguments");
            return Ok(Some(CallResult::Complete(i32::from(self.breathe()?))));
        }

        // AddFlatEffect(x, y, z, spread, descriptor, speed) builds a type-one environmental effect
        // at a Q8 point, scattered by the spread when it is nonzero; the animation speed is retained.
        if kind == CallKind::Function && function == Native::AddFlatEffect {
            let [x, y, z, spread, descriptor, speed] = arguments else {
                anyhow::bail!("AddFlatEffect requires six arguments")
            };
            self.registry.spawn_environment_effect_with_speed(
                descriptor.integer()? as u32,
                [x.integer()?, y.integer()?, z.integer()?],
                speed.integer()?,
                spread.integer()?,
                self.random.as_deref_mut(),
            )?;
            return Ok(Some(CallResult::Complete(0)));
        }

        // AddLightEffect(a, b, c, d, e, f, g, h) allocates a type-two light record; it returns
        // the record (one-based slot) or zero when the pool is disabled or full.
        if kind == CallKind::Function && function == Native::AddLightEffect {
            let values = arguments
                .iter()
                .map(Argument::integer)
                .collect::<Result<Vec<_>>>()?;
            let values: [i32; 8] = values
                .try_into()
                .map_err(|_| anyhow::anyhow!("AddLightEffect requires eight arguments"))?;
            return Ok(Some(CallResult::Complete(
                self.registry.add_light_effect(values)?,
            )));
        }
        Ok(None)
    }
}
