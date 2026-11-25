//! Actor property descriptors and their read/write dispatch.
use super::{
    ActorControlState, AnimationGroup, GameHost, JumpMotion, LivingState, MotionDirection,
    Property, Result, Surface, Target, raex_word,
};
use anyhow::Context;
use cyrus_soup::Host;

/// The actor-record triples a position or angle property names: the local pose, the previous
/// position, the saved start position (location zero) and the angle history.
enum PoseProperty {
    Position(usize),
    Angle(usize),
    LastPosition(usize),
    StartPosition(usize),
    LastAngle(usize),
    StartAngle(usize),
    SlavePosition(usize),
    LastSlavePosition(usize),
    SlaveAngle(usize),
    LastSlaveAngle(usize),
}

/// Script properties naming actor motion bytes, following preservation engine/SOUPPROPS.md.
/// The independent ordinary countdown and retained movement classification keep their owners.
fn motion_property(control: &mut ActorControlState, name: Property) -> Option<&mut u8> {
    Some(match name {
        Property::RopeWait => &mut control.jump_selector,
        Property::ExtraTime => &mut control.extra_time,
        Property::SurfaceTime => &mut control.surface_time,
        Property::JumpTime => &mut control.jump_time,
        Property::JumpHeight => &mut control.jump_height,
        Property::Anchor => &mut control.anchor,
        Property::SlideFlag => &mut control.sliding_state,
        Property::MoveCounter => &mut control.move_counter,
        Property::OriginTime => &mut control.fall_timer,
        Property::ActionTime => &mut control.jump_window,
        Property::ArmsTime => &mut control.action_transition_counter,
        Property::LastCollideCount => &mut control.last_collide_count,
        Property::DisableScript => &mut control.script_control,
        Property::AiMode => &mut control.ai_mode,
        Property::AiDelay => &mut control.ai_delay,
        _ => return None,
    })
}

/// A control-record field a script property names: the word fields read sign-extended and keep the
/// low sixteen bits of a write, the dword fields keep all of it, and the flag byte reads
/// zero-extended (cyrus keeps it as a boolean, so a read gives 0 or 1 and any nonzero write sets it).
enum ControlField<'a> {
    Word(&'a mut i16),
    Dword(&'a mut i32),
    Flag(&'a mut bool),
}

fn control_record_property(
    control: &mut ActorControlState,
    name: Property,
) -> Option<ControlField<'_>> {
    Some(match name {
        Property::AiVar1 => ControlField::Word(&mut control.ai.var1),
        Property::AiVar2 => ControlField::Word(&mut control.ai.var2),
        Property::AiLength => ControlField::Dword(&mut control.ai.length),
        Property::YangleChange => ControlField::Word(&mut control.yangle_change),
        // The node fields are the walk-map owner's held nodes: current, next and final destination.
        Property::Node => ControlField::Dword(&mut control.navigation.current),
        Property::Dnode => ControlField::Dword(&mut control.navigation.next),
        Property::Fdnode => ControlField::Dword(&mut control.navigation.destination),
        // The stable-position record the fall and recovery code restores from.
        // KillMe's finished marker and the weapon-drawn byte of the equipped-weapon routines.
        Property::Dead => ControlField::Flag(&mut control.killed),
        Property::WeaponDrawn => ControlField::Flag(&mut control.weapon_drawn),
        Property::Invisible => ControlField::Flag(&mut control.invisible),
        Property::MoveVx => ControlField::Dword(&mut control.motion_vector[0]),
        Property::MoveVy => ControlField::Dword(&mut control.motion_vector[1]),
        Property::MoveVz => ControlField::Dword(&mut control.motion_vector[2]),
        Property::OvVx => ControlField::Dword(&mut control.ledge_point[0]),
        Property::OvVy => ControlField::Dword(&mut control.ledge_point[1]),
        Property::OvVz => ControlField::Dword(&mut control.ledge_point[2]),
        Property::HandPosVx => ControlField::Dword(&mut control.hand_point[0]),
        Property::HandPosVy => ControlField::Dword(&mut control.hand_point[1]),
        Property::HandPosVz => ControlField::Dword(&mut control.hand_point[2]),
        Property::HandAngleVx => ControlField::Dword(&mut control.hand_angles[0]),
        Property::HandAngleVy => ControlField::Dword(&mut control.hand_angles[1]),
        Property::HandAngleVz => ControlField::Dword(&mut control.hand_angles[2]),
        Property::LastValidFlag => ControlField::Flag(&mut control.stable_position_valid),
        Property::LastValidPosVx => ControlField::Dword(&mut control.stable_local_position[0]),
        Property::LastValidPosVy => ControlField::Dword(&mut control.stable_local_position[1]),
        Property::LastValidPosVz => ControlField::Dword(&mut control.stable_local_position[2]),
        _ => return None,
    })
}

/// The bounds-record triple a `size.psize`, `size.msize` or `size.pos` property names, and the axis.
enum CollisionField {
    Positive(usize),
    Negative(usize),
    Center(usize),
    Total(usize),
}

fn collision_extent_property(name: Property) -> Option<CollisionField> {
    use Property::{
        SizeMsizeVx, SizeMsizeVy, SizeMsizeVz, SizePosVx, SizePosVy, SizePosVz, SizePsizeVx,
        SizePsizeVy, SizePsizeVz, SizeSizeVx, SizeSizeVy, SizeSizeVz,
    };
    let find = |names: [Property; 3]| names.iter().position(|&n| n == name);
    find([SizePsizeVx, SizePsizeVy, SizePsizeVz])
        .map(CollisionField::Positive)
        .or_else(|| find([SizeMsizeVx, SizeMsizeVy, SizeMsizeVz]).map(CollisionField::Negative))
        .or_else(|| find([SizePosVx, SizePosVy, SizePosVz]).map(CollisionField::Center))
        .or_else(|| find([SizeSizeVx, SizeSizeVy, SizeSizeVz]).map(CollisionField::Total))
}

fn pose_property(name: Property) -> Option<PoseProperty> {
    use Property::{
        AngleVx, AngleVy, AngleVz, LastAngleVx, LastAngleVy, LastAngleVz, LastPosVx, LastPosVy,
        LastPosVz, LastSlaveAngleVx, LastSlaveAngleVy, LastSlaveAngleVz, LastSlavePosVx,
        LastSlavePosVy, LastSlavePosVz, PosVx, PosVy, PosVz, SlaveAngleVx, SlaveAngleVy,
        SlaveAngleVz, SlavePosVx, SlavePosVy, SlavePosVz, StartAngleVx, StartAngleVy, StartAngleVz,
        StartPosVx, StartPosVy, StartPosVz,
    };
    let find = |names: [Property; 3]| names.iter().position(|&n| n == name);
    find([PosVx, PosVy, PosVz])
        .map(PoseProperty::Position)
        .or_else(|| find([AngleVx, AngleVy, AngleVz]).map(PoseProperty::Angle))
        .or_else(|| find([LastPosVx, LastPosVy, LastPosVz]).map(PoseProperty::LastPosition))
        .or_else(|| find([StartPosVx, StartPosVy, StartPosVz]).map(PoseProperty::StartPosition))
        .or_else(|| find([LastAngleVx, LastAngleVy, LastAngleVz]).map(PoseProperty::LastAngle))
        .or_else(|| find([StartAngleVx, StartAngleVy, StartAngleVz]).map(PoseProperty::StartAngle))
        .or_else(|| find([SlavePosVx, SlavePosVy, SlavePosVz]).map(PoseProperty::SlavePosition))
        .or_else(|| {
            find([LastSlavePosVx, LastSlavePosVy, LastSlavePosVz])
                .map(PoseProperty::LastSlavePosition)
        })
        .or_else(|| find([SlaveAngleVx, SlaveAngleVy, SlaveAngleVz]).map(PoseProperty::SlaveAngle))
        .or_else(|| {
            find([LastSlaveAngleVx, LastSlaveAngleVy, LastSlaveAngleVz])
                .map(PoseProperty::LastSlaveAngle)
        })
}

/// The animation record field an `anim.*` property names. The other record fields have no
/// recovered owner or contract and stay unsupported.
fn playback_field(name: Property) -> Option<cyrus_world::animation::PlaybackField> {
    use cyrus_world::animation::PlaybackField;
    Some(match name {
        Property::AnimNext => PlaybackField::Next,
        Property::AnimNextSpeed => PlaybackField::NextSpeed,
        Property::AnimFrame => PlaybackField::Frame,
        Property::AnimSpeed => PlaybackField::Speed,
        Property::AnimSpeedCount => PlaybackField::SpeedCount,
        Property::AnimStopFlag => PlaybackField::StopFlag,
        Property::AnimHaltFlag => PlaybackField::HaltFlag,
        Property::AnimHoldFlag => PlaybackField::HoldFlag,
        Property::AnimLoopCount => PlaybackField::LoopCount,
        Property::AnimRepCount => PlaybackField::RepCount,
        Property::AnimPosVx => PlaybackField::Position(0),
        Property::AnimPosVy => PlaybackField::Position(1),
        Property::AnimPosVz => PlaybackField::Position(2),
        Property::AnimAngleVx => PlaybackField::Rotation(0),
        Property::AnimAngleVy => PlaybackField::Rotation(1),
        Property::AnimAngleVz => PlaybackField::Rotation(2),
        Property::AnimRewindFlag => PlaybackField::RewindFlag,
        Property::AnimRealFrame => PlaybackField::RealFrame,
        Property::AnimMaxGroup => PlaybackField::MaxGroup,
        Property::AnimLastAngleVx => PlaybackField::LastRotation(0),
        Property::AnimLastAngleVy => PlaybackField::LastRotation(1),
        Property::AnimLastAngleVz => PlaybackField::LastRotation(2),
        _ => return None,
    })
}

/// The task slot and retained word a `task_variable*` property names.
fn task_word(name: Property) -> Option<(usize, crate::script::tasks::TaskWord)> {
    use crate::script::tasks::TaskWord;
    use Property::*;
    let families = [
        (
            TaskWord::Continuation,
            [
                TaskVariable0,
                TaskVariable1,
                TaskVariable2,
                TaskVariable3,
                TaskVariable4,
                TaskVariable5,
            ],
        ),
        (
            TaskWord::PauseTicks,
            [
                TaskVariable2_0,
                TaskVariable2_1,
                TaskVariable2_2,
                TaskVariable2_3,
                TaskVariable2_4,
                TaskVariable2_5,
            ],
        ),
        (
            TaskWord::ContinuationDirection,
            [
                TaskVariable3_0,
                TaskVariable3_1,
                TaskVariable3_2,
                TaskVariable3_3,
                TaskVariable3_4,
                TaskVariable3_5,
            ],
        ),
        (
            TaskWord::PointCommand,
            [
                TaskVariable4_0,
                TaskVariable4_1,
                TaskVariable4_2,
                TaskVariable4_3,
                TaskVariable4_4,
                TaskVariable4_5,
            ],
        ),
    ];
    families.into_iter().find_map(|(word, names)| {
        names
            .iter()
            .position(|&candidate| candidate == name)
            .map(|slot| (slot, word))
    })
}

/// The task slot field a `task_status`, `task_pause` or `task_timer` property names.
fn task_slot_name(name: Property) -> Option<(usize, cyrus_soup::TaskSlotField)> {
    use Property::*;
    use cyrus_soup::TaskSlotField;
    [
        (
            TaskSlotField::Status,
            [
                TaskStatus0,
                TaskStatus1,
                TaskStatus2,
                TaskStatus3,
                TaskStatus4,
                TaskStatus5,
            ],
        ),
        (
            TaskSlotField::Pause,
            [
                TaskPause0, TaskPause1, TaskPause2, TaskPause3, TaskPause4, TaskPause5,
            ],
        ),
        (
            TaskSlotField::Timer,
            [
                TaskTimer0, TaskTimer1, TaskTimer2, TaskTimer3, TaskTimer4, TaskTimer5,
            ],
        ),
    ]
    .into_iter()
    .find_map(|(field, names)| {
        names
            .iter()
            .position(|&candidate| candidate == name)
            .map(|slot| (slot, field))
    })
}

impl GameHost<'_, '_, '_, '_> {
    pub(super) fn actor_task_slot_property(
        &mut self,
        target: &Target,
        reference: u16,
    ) -> Result<Option<cyrus_soup::TaskSlotProperty>> {
        let Some((slot, field)) = task_slot_name(self.property(target, reference)?) else {
            return Ok(None);
        };
        Ok(Some(cyrus_soup::TaskSlotProperty {
            slot,
            field,
            own: self.property_receiver(target)? == self.index,
        }))
    }

    pub(super) fn read_script_value(
        &mut self,
        target: &Target,
        reference: u16,
    ) -> Result<cyrus_soup::ScriptValue> {
        if self.property(target, reference)? == Property::Address {
            let actor = if matches!(target, Target::Player) {
                self.registry.id(self.property_receiver(target)?)?
            } else {
                self.target_reference(target)?
            };
            anyhow::ensure!(
                self.registry.actors[actor.index]
                    .runtime_identifier
                    .is_some(),
                "property address: world {} actor {} temporary or unallocated reference identity unsupported",
                self.world,
                actor.index
            );
            return Ok(cyrus_soup::ScriptValue::Actor(actor));
        }
        self.read_property(target, reference)
            .map(cyrus_soup::ScriptValue::Integer)
    }

    pub(super) fn read_actor_property(&mut self, target: &Target, reference: u16) -> Result<i32> {
        let name = self.property(target, reference)?;
        if name == Property::LastCollide {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_ref()
                .context("last_collide control owner unavailable")?;
            return Ok(i32::from(control.last_collide.bits() as u8));
        }
        if name == Property::Sflags {
            let index = self.property_receiver(target)?;
            return self.registry.actors[index]
                .model
                .collision_flags
                .map(i32::from)
                .with_context(|| format!("property {name}: collision metadata unavailable"));
        }
        if name == Property::PanicGroup {
            let index = self.property_receiver(target)?;
            let actor = &self.registry.actors[index];
            return Ok(i32::from(
                actor
                    .animation
                    .as_ref()
                    .map_or(actor.unprepared_animation_default, |animation| {
                        animation.playback.default_group().index() as i16
                    }),
            ));
        }
        if let Some(field) = playback_field(name) {
            let index = self.property_receiver(target)?;
            // An actor without an animation association reads zero, as `anim.current` does.
            return self.registry.actors[index]
                .animation
                .as_ref()
                .map_or(Ok(0), |animation| animation.playback.record_field(field))
                .with_context(|| format!("property {name}"));
        }
        if name == Property::Frame || name == Property::LastFrame {
            // The word is the frame index inside the model the selected ShowFrame names, -1 when
            // no frame was selected.
            let index = self.property_receiver(target)?;
            let animation = self.registry.actors[index]
                .animation
                .as_ref()
                .with_context(|| format!("property {name}: actor {index} has no animation"))?;
            let Some(reference) = (if name == Property::Frame {
                animation.frame
            } else {
                animation.playback.last_frame()
            }) else {
                return Ok(-1);
            };
            let data = self
                .inputs
                .animations
                .with_context(|| format!("property {name}: animation data unavailable"))?
                .0;
            let actor = &self.registry.actors[index];
            let resolved = if actor.placement().is_some() {
                data.resolve_map_frame(index, reference)
            } else {
                data.resolve_frame(actor.name(), reference)
            }
            .with_context(|| format!("property {name}"))?;
            return i32::try_from(resolved.frame).with_context(|| format!("property {name}"));
        }
        if name == Property::AnimCurrent {
            let index = self.property_receiver(target)?;
            return Ok(self.registry.actors[index]
                .animation
                .as_ref()
                .map_or(0, |animation| {
                    animation
                        .playback
                        .current_group()
                        .map_or(-1, |group| i32::from(group.index() as i16))
                }));
        }
        if let Some(word) = [Property::Grip0, Property::Grip1]
            .iter()
            .position(|&word| word == name)
        {
            let index = self.property_receiver(target)?;
            let actor = &self.registry.actors[index];
            let control = actor
                .control
                .as_ref()
                .with_context(|| format!("{name} control owner unavailable"))?;
            let value = if let Some(value) = control.grip_override[word] {
                value
            } else {
                let data = self
                    .inputs
                    .animations
                    .with_context(|| format!("{name} animation data unavailable"))?
                    .0;
                let field = ["grip0", "grip1"][word];
                raex_word(Some(control), data.actor(actor.name())?, field)
                    .with_context(|| format!("actor {} RAEX {field} unavailable", actor.name()))?
            };
            return Ok(i32::from(value));
        }
        if let Some(word) = [Property::Scabbard0, Property::Scabbard1]
            .iter()
            .position(|&word| word == name)
        {
            let index = self.property_receiver(target)?;
            let actor = &self.registry.actors[index];
            let control = actor
                .control
                .as_ref()
                .with_context(|| format!("{name} control owner unavailable"))?;
            let value = if let Some(value) = control.scabbard_override[word] {
                value
            } else {
                let data = self
                    .inputs
                    .animations
                    .with_context(|| format!("{name} animation data unavailable"))?
                    .0;
                let field = ["scabbard0", "scabbard1"][word];
                raex_word(Some(control), data.actor(actor.name())?, field)
                    .with_context(|| format!("actor {} RAEX {field} unavailable", actor.name()))?
            };
            return Ok(i32::from(value));
        }
        if name == Property::UnderSurface {
            let index = self.property_receiver(target)?;
            let id = self.registry.id(index)?;
            return self
                .registry
                .actor_surface(id)
                .map(|surface| i32::from(surface.byte()));
        }
        if matches!(
            name,
            Property::Origin | Property::OriginAccum | Property::ActionFlag | Property::Action
        ) {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_ref()
                .with_context(|| format!("{name} control owner unavailable"))?;
            return Ok(match name {
                Property::Origin => control.fall_origin_y,
                Property::OriginAccum => control.sliding_distance,
                Property::ActionFlag => control.actions.pending as i32,
                _ => control.actions.accepted as i32,
            });
        }
        if name == Property::TorchTimer {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_ref()
                .context("torch_timer control owner unavailable")?;
            return Ok(i32::from(control.torch_burning));
        }
        // `rope` holds a record pointer in the original; installed scripts only test it against zero.
        if name == Property::Rope || name == Property::Hide {
            let rope = name == Property::Rope;
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_ref()
                .with_context(|| format!("{name} control owner unavailable"))?;
            return Ok(i32::from(if rope {
                control.rope_attached
            } else {
                control.hidden
            }));
        }
        if matches!(
            name,
            Property::AiType
                | Property::ExtraFlag
                | Property::FallFlag
                | Property::JumpFlag
                | Property::LastDirection
                | Property::HandItem
        ) {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_ref()
                .with_context(|| format!("property {name} control owner unavailable"))?;
            return Ok(i32::from(match name {
                Property::AiType => control.ai_type.byte(),
                // An unestablished held item reads as the allocator's zero.
                Property::HandItem => control.held_item.unwrap_or(0),
                Property::ExtraFlag => control.living_state.byte(),
                Property::FallFlag => control.falling_state.byte(),
                Property::LastDirection => control.motion_classification.byte(),
                _ => control.queued_motion.byte(),
            }));
        }
        if motion_property(&mut ActorControlState::default(), name).is_some() {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .with_context(|| format!("property {name} control owner unavailable"))?;
            return Ok(i32::from(
                *motion_property(control, name).expect("named motion byte"),
            ));
        }
        // The message and broadcast selections are the actor-record bytes the message natives read.
        if name == Property::MessageIndex || name == Property::BroadcastIndex {
            let index = self.property_receiver(target)?;
            let actor = &self.registry.actors[index];
            return Ok(i32::from(if name == Property::MessageIndex {
                actor.message_read
            } else {
                actor.broadcast_index
            }));
        }
        if name == Property::Id {
            let index = self.property_receiver(target)?;
            return self.registry.actors[index]
                .runtime_identifier
                .map(i32::from)
                .with_context(|| format!("property {name}: runtime identifier unavailable"));
        }
        if let Some((slot, field)) = task_slot_name(name) {
            let index = self.property_receiver(target)?;
            return Ok(self.registry.actors[index]
                .runtime
                .as_ref()
                .with_context(|| {
                    format!("property {name}: task slots of actor {index} unavailable")
                })?
                .task_slot_field(slot, field));
        }
        if let Some((slot, word)) = task_word(name) {
            let index = self.property_receiver(target)?;
            return Ok(self.registry.actors[index].native_commands[slot].word(word) as i32);
        }
        if control_record_property(&mut ActorControlState::default(), name).is_some() {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .with_context(|| format!("property {name} control owner unavailable"))?;
            return Ok(
                match control_record_property(control, name).expect("named control field") {
                    ControlField::Word(word) => i32::from(*word),
                    ControlField::Dword(dword) => *dword,
                    ControlField::Flag(flag) => i32::from(*flag),
                },
            );
        }
        if let Some(field) = collision_extent_property(name) {
            let index = self.property_receiver(target)?;
            let shape = self.registry.actors[index]
                .collision
                .as_ref()
                .with_context(|| {
                    format!("property {target:?}.{name}: collision shape not prepared")
                })?;
            return Ok(match field {
                CollisionField::Positive(axis) => shape.positive_extent[axis],
                CollisionField::Negative(axis) => shape.negative_extent[axis],
                CollisionField::Center(axis) => shape.center_offset[axis],
                CollisionField::Total(axis) => shape.total_extent[axis],
            });
        }
        let pose = pose_property(name)
            .with_context(|| format!("property {target:?}.{name} unsupported"))?;
        let index = self.property_receiver(target)?;
        let spatial = self
            .registry
            .spatial
            .as_ref()
            .with_context(|| format!("property {target:?}.{name}: spatial owner unavailable"))?
            .actor(index)?;
        Ok(match pose {
            PoseProperty::Position(axis) => spatial.local.position[axis],
            PoseProperty::Angle(axis) => spatial.local.angles[axis],
            PoseProperty::LastPosition(axis) => spatial.previous_local_position[axis],
            PoseProperty::StartPosition(axis) => spatial
                .locations
                .first()
                .with_context(|| format!("property {name}: saved start position unavailable"))?
                [axis],
            PoseProperty::LastAngle(axis) => spatial.angle_history[axis],
            PoseProperty::StartAngle(axis) => spatial.initial_angles[axis],
            PoseProperty::SlavePosition(axis) => spatial.attached_pose(name)?.position[axis],
            PoseProperty::SlaveAngle(axis) => spatial.attached_pose(name)?.angles[axis],
            PoseProperty::LastSlavePosition(axis) => spatial
                .previous_effective_position
                .with_context(|| format!("property {name}: no family refresh has run"))?[axis],
            PoseProperty::LastSlaveAngle(axis) => spatial
                .previous_effective_angles
                .with_context(|| format!("property {name}: no family refresh has run"))?[axis],
        })
    }

    pub(super) fn write_actor_property(
        &mut self,
        target: &Target,
        reference: u16,
        value: i32,
    ) -> Result<()> {
        let name = self.property(target, reference)?;
        if name == Property::LastCollide {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .context("last_collide control owner unavailable")?;
            control.last_collide = crate::script::CollisionFlags::from_bits(
                (control.last_collide.bits() & !255) | u32::from(value as u8),
            );
            return Ok(());
        }
        if name == Property::Sflags {
            let index = self.property_receiver(target)?;
            let flags = self.registry.actors[index]
                .model
                .collision_flags
                .as_mut()
                .with_context(|| format!("property {name}: collision metadata unavailable"))?;
            *flags = value as u8;
            return Ok(());
        }
        if let Some(field) = playback_field(name) {
            let index = self.property_receiver(target)?;
            return self.registry.actors[index]
                .animation
                .as_mut()
                .with_context(|| format!("property {name}: actor {index} has no animation"))?
                .playback
                .set_record_field(field, value)
                .with_context(|| format!("property {name}"));
        }
        if name == Property::AnimCurrent {
            let index = self.property_receiver(target)?;
            return self.registry.actors[index]
                .animation
                .as_mut()
                .with_context(|| format!("property {name}: actor {index} has no animation"))?
                .playback
                .set_current_group(value)
                .with_context(|| format!("property {name}"));
        }
        if name == Property::PanicGroup {
            let index = self.property_receiver(target)?;
            let actor = &mut self.registry.actors[index];
            if let Some(animation) = actor.animation.as_mut() {
                animation
                    .playback
                    .set_stored_default(AnimationGroup::from_index(value as u16));
            } else {
                actor.unprepared_animation_default = value as i16;
            }
            return Ok(());
        }
        if name == Property::UnderSurface {
            let index = self.property_receiver(target)?;
            let id = self.registry.id(index)?;
            self.registry.actor_surface(id)?;
            return self
                .registry
                .set_actor_surface(id, Surface::from_byte(value as u8));
        }
        if matches!(
            name,
            Property::Origin | Property::OriginAccum | Property::ActionFlag | Property::Action
        ) {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .with_context(|| format!("{name} control owner unavailable"))?;
            match name {
                Property::Origin => control.fall_origin_y = value,
                Property::OriginAccum => control.sliding_distance = value,
                Property::ActionFlag => control.actions.pending = value as u32,
                _ => control.actions.accepted = value as u32,
            }
            return Ok(());
        }
        if name == Property::TorchTimer {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .context("torch_timer control owner unavailable")?;
            control.torch_burning = value as u8;
            return Ok(());
        }
        if matches!(
            name,
            Property::AiType
                | Property::ExtraFlag
                | Property::FallFlag
                | Property::JumpFlag
                | Property::LastDirection
                | Property::HandItem
        ) {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .with_context(|| format!("property {name} control owner unavailable"))?;
            let byte = value as u8;
            match name {
                Property::HandItem => control.held_item = Some(byte),
                Property::AiType => self
                    .registry
                    .set_actor_ai_type(index, crate::script::AiType::from_byte(byte)),
                Property::ExtraFlag => control.living_state = LivingState::from_byte(byte),
                Property::FallFlag => control.falling_state = JumpMotion::from_byte(byte),
                Property::LastDirection => {
                    control.motion_classification = MotionDirection::from_byte(byte)
                }
                _ => control.queued_motion = JumpMotion::from_byte(byte),
            }
            return Ok(());
        }
        if motion_property(&mut ActorControlState::default(), name).is_some() {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .with_context(|| format!("property {name} control owner unavailable"))?;
            *motion_property(control, name).expect("named motion byte") = value as u8;
            return Ok(());
        }
        if name == Property::MessageIndex || name == Property::BroadcastIndex {
            let index = self.property_receiver(target)?;
            let actor = &mut self.registry.actors[index];
            if name == Property::MessageIndex {
                actor.message_read = value as u8;
            } else {
                actor.broadcast_index = value as u8;
            }
            return Ok(());
        }
        anyhow::ensure!(
            name != Property::Id,
            "property {name} write unsupported: the runtime identifier is installed identity that name lookup, route ownership and restore validation key on"
        );
        if let Some((slot, field)) = task_slot_name(name) {
            let index = self.property_receiver(target)?;
            self.registry.actors[index]
                .runtime
                .as_mut()
                .with_context(|| {
                    format!("property {name}: task slots of actor {index} unavailable")
                })?
                .set_task_slot_field(slot, field, value);
            return Ok(());
        }
        if let Some((slot, word)) = task_word(name) {
            let index = self.property_receiver(target)?;
            self.registry.actors[index].native_commands[slot].set_word(word, value as u32);
            return Ok(());
        }
        if control_record_property(&mut ActorControlState::default(), name).is_some() {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .with_context(|| format!("property {name} control owner unavailable"))?;
            match control_record_property(control, name).expect("named control field") {
                ControlField::Word(word) => *word = value as i16,
                ControlField::Dword(dword) => *dword = value,
                ControlField::Flag(flag) => *flag = value as u8 != 0,
            }
            return Ok(());
        }
        let scabbard = [Property::Scabbard0, Property::Scabbard1]
            .iter()
            .position(|&word| word == name);
        let grip = [Property::Grip0, Property::Grip1]
            .iter()
            .position(|&word| word == name);
        if name == Property::Hide || scabbard.is_some() || grip.is_some() {
            let index = self.property_receiver(target)?;
            let control = self.registry.actors[index]
                .control
                .as_mut()
                .with_context(|| format!("property {name} control owner unavailable"))?;
            match (scabbard, grip) {
                (Some(word), _) => control.scabbard_override[word] = Some(value as i16),
                (_, Some(word)) => control.grip_override[word] = Some(value as i16),
                _ => control.hidden = value as u8 != 0,
            }
            return Ok(());
        }
        if let Some(field) = collision_extent_property(name) {
            let index = self.property_receiver(target)?;
            let shape = self.registry.actors[index]
                .collision
                .as_mut()
                .with_context(|| {
                    format!("property {target:?}.{name}: collision shape not prepared")
                })?;
            match field {
                CollisionField::Positive(axis) => shape.positive_extent[axis] = value,
                CollisionField::Negative(axis) => shape.negative_extent[axis] = value,
                CollisionField::Center(axis) => shape.center_offset[axis] = value,
                CollisionField::Total(axis) => shape.total_extent[axis] = value,
            }
            return Ok(());
        }
        let pose = pose_property(name)
            .with_context(|| format!("property {target:?}.{name} write unsupported"))?;
        let index = self.property_receiver(target)?;
        let spatial = self
            .registry
            .spatial
            .as_mut()
            .with_context(|| format!("property {target:?}.{reference}: spatial owner unavailable"))?
            .actor_mut(index)?;
        match pose {
            PoseProperty::Position(axis) => spatial.local.position[axis] = value,
            PoseProperty::Angle(axis) => spatial.local.angles[axis] = value,
            PoseProperty::LastPosition(axis) => spatial.previous_local_position[axis] = value,
            PoseProperty::StartPosition(axis) => {
                spatial.locations.first_mut().with_context(|| {
                    format!("property {name}: saved start position unavailable")
                })?[axis] = value
            }
            PoseProperty::LastAngle(axis) => spatial.angle_history[axis] = value,
            PoseProperty::StartAngle(axis) => spatial.initial_angles[axis] = value,
            PoseProperty::SlavePosition(axis) => {
                spatial.attached_pose_mut(name)?.position[axis] = value
            }
            PoseProperty::SlaveAngle(axis) => spatial.attached_pose_mut(name)?.angles[axis] = value,
            PoseProperty::LastSlavePosition(axis) => {
                spatial.previous_effective_position.get_or_insert([0; 3])[axis] = value
            }
            PoseProperty::LastSlaveAngle(axis) => {
                spatial.previous_effective_angles.get_or_insert([0; 3])[axis] = value
            }
        }
        Ok(())
    }
}
