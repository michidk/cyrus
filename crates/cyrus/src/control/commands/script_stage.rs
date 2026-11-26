//! Script-stage preparation, sound routing and selected-player query projection.
use super::*;
use cyrus_script::script::{ScriptActorId, ScriptSound, ScriptStage, Thread};

pub(super) fn prepare_stage(
    world: &mut cyrus_script::script::ScriptWorld,
    id: ScriptActorId,
    stage: cyrus_control::ScriptStage,
) -> anyhow::Result<Option<ScriptStage>> {
    match stage {
        cyrus_control::ScriptStage::PrepareSpatial => world.prepare_spatial()?,
        cyrus_control::ScriptStage::Snapshot => world.snapshot_spatial(id)?,
        cyrus_control::ScriptStage::AxisLocks => world.restore_axis_locks(id)?,
        cyrus_control::ScriptStage::Transform => world.refresh_transform(id)?,
        cyrus_control::ScriptStage::Families => world.refresh_families()?,
        _ => {}
    }
    let stage = match stage {
        cyrus_control::ScriptStage::PrepareSpatial
        | cyrus_control::ScriptStage::Snapshot
        | cyrus_control::ScriptStage::AxisLocks
        | cyrus_control::ScriptStage::Transform
        | cyrus_control::ScriptStage::Families => return Ok(None),
        cyrus_control::ScriptStage::Animation => ScriptStage::Animation,
        cyrus_control::ScriptStage::OrdinaryMain => ScriptStage::OrdinaryMain,
        cyrus_control::ScriptStage::OrdinaryHook => ScriptStage::OrdinaryHook,
        cyrus_control::ScriptStage::NativeHook => ScriptStage::NativeHook,
        cyrus_control::ScriptStage::Entry => ScriptStage::Entry,
        cyrus_control::ScriptStage::Tasks => ScriptStage::Tasks,
        cyrus_control::ScriptStage::Main => ScriptStage::Body(Thread::Main),
        cyrus_control::ScriptStage::Interrupt => ScriptStage::Body(Thread::Interrupt),
    };
    Ok(Some(stage))
}
pub(super) fn emit_script_sound(
    actor: ScriptActorId,
    sound: ScriptSound,
    script_bindings: &std::collections::BTreeMap<Entity, ScriptActorId>,
    script_sounds: &mut Option<&mut Messages<cyrus_world::sound::SoundCue>>,
    captured_sounds: &mut Vec<serde_json::Value>,
) -> anyhow::Result<()> {
    use cyrus_script::script::ScriptSound;
    let (sound, surface, position) = match sound {
        ScriptSound::Animation {
            sound,
            surface,
            position,
        } => (sound, surface, position),
        sound => {
            let event = script_sound_event(&sound);
            captured_sounds.push(json!({"actor":actor, "sound":event}));
            if let Some(sounds) = script_sounds.as_deref_mut() {
                sounds.write(cyrus_world::sound::SoundCue::Script {
                    generation: actor.generation,
                    index: actor.index,
                    sound,
                });
            }
            return Ok(());
        }
    };
    anyhow::ensure!(
        !matches!(
            sound,
            cyrus_world::animation::AnimationSound::Event { .. }
                | cyrus_world::animation::AnimationSound::Transition { .. }
        ),
        "animation actor events and transitions are unsupported in the control script stage"
    );
    let mut nodes = script_bindings.iter().filter(|(_, bound)| **bound == actor);
    let Some((entity, _)) = nodes.next() else {
        use cyrus_world::animation::AnimationSound;
        let event = match sound {
            AnimationSound::Play {
                effect,
                rate_offset_hz,
            } => json!({"type":"play", "effect":effect, "rate_offset_hz":rate_offset_hz}),
            AnimationSound::CheckSample => json!({"type":"check_sample"}),
            AnimationSound::Stop => json!({"type":"stop"}),
            AnimationSound::Forget => json!({"type":"forget"}),
            AnimationSound::Event { .. } | AnimationSound::Transition { .. } => {
                unreachable!()
            }
        };
        captured_sounds.push(json!({"actor":actor, "sound":event}));
        return Ok(());
    };
    let entity = *entity;
    anyhow::ensure!(
        nodes.next().is_none(),
        "script sound receiver has multiple scene bindings"
    );
    script_sounds
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("script sound consumer unavailable"))?
        .write(cyrus_world::sound::SoundCue::Animation {
            actor: entity,
            sound,
            surface,
            position,
        });
    Ok(())
}
fn script_sound_event(sound: &ScriptSound) -> serde_json::Value {
    match sound {
        ScriptSound::Flat {
            effect,
            volume,
            rate_offset_hz,
        } => {
            json!({"type":"flat", "effect":effect, "volume":volume, "rate_offset_hz":rate_offset_hz})
        }
        ScriptSound::Positional {
            effect,
            volume,
            rate_offset_hz,
            position,
            start_index,
        } => {
            json!({"type":"positional", "effect":effect, "volume":volume, "rate_offset_hz":rate_offset_hz, "position":position, "start_index":start_index})
        }
        ScriptSound::StopEffect { effect, owned } => {
            json!({"type":"stop_effect", "effect":effect, "owned":owned})
        }
        ScriptSound::StopOwned => json!({"type":"stop_owned"}),
        ScriptSound::StopAll => json!({"type":"stop_all"}),
        ScriptSound::AnimationState(state) => {
            json!({"type":"animation_state", "state":format!("{state:?}")})
        }
        ScriptSound::Animation { .. } => unreachable!(),
    }
}
pub(super) fn player_query(
    player_poses: &Query<&cyrus_script::player::placement::PlayerPose, With<Player>>,
) -> anyhow::Result<Option<cyrus_script::script::PlayerQuery>> {
    let mut poses = player_poses.iter();
    let selected = poses.next();
    anyhow::ensure!(
        poses.next().is_none(),
        "selected-player query has multiple player poses"
    );
    let player = selected
        .map(|pose| -> anyhow::Result<_> {
            let mut position = [0; 3];
            for (axis, coordinate) in pose.position.into_iter().enumerate() {
                position[axis] = i32::try_from(coordinate).map_err(|_| {
                    anyhow::anyhow!("player query coordinate axis {axis} outside Q8 i32 range")
                })?;
            }
            Ok(cyrus_script::script::PlayerQuery {
                position,
                heading: pose.heading,
            })
        })
        .transpose()?;
    Ok(player)
}

pub(super) fn binding_entity(
    script_nodes: &Query<(Entity, &Name)>,
    script_bindings: &std::collections::BTreeMap<Entity, ScriptActorId>,
    generation: u64,
    node: Option<String>,
) -> anyhow::Result<Option<Entity>> {
    let entity = if let Some(name) = node {
        let matches: Vec<_> = script_nodes
            .iter()
            .filter(|(_, candidate)| candidate.as_str() == name)
            .collect();
        anyhow::ensure!(
            matches.len() == 1,
            "scene node {name:?}: expected one match, found {}",
            matches.len()
        );
        anyhow::ensure!(
            script_bindings
                .get(&matches[0].0)
                .is_none_or(|bound| bound.generation != generation),
            "scene node {name:?} already has a script binding"
        );
        Some(matches[0].0)
    } else {
        None
    };
    Ok(entity)
}

pub(super) fn script_inputs<'a>(
    hud: Option<&'a cyrus_script::script::ScriptHud>,
    activation: Option<&'a cyrus_script::script::ActivationInputs>,
    cheats: &cyrus_game::cheats::Cheats,
    query: cyrus_script::script::QueryInputs,
    frame_scale: Option<u32>,
    nominal_ticks: Option<u32>,
) -> cyrus_script::script::ScriptInputs<'a> {
    cyrus_script::script::ScriptInputs {
        hud,
        activation,
        death_suppressed: cheats.death_suppressed().ok(),
        torch_retained: cheats.torch_retained().ok(),
        actor_update_skipped: cheats.actor_update_skipped().ok(),
        ground_bypass: cheats.ground_bypass().ok(),
        magic_carpet: cheats.magic_carpet().ok(),
        player_collision_skipped: cheats.player_collision_skipped().ok(),
        nominal_ticks,
        frame_scale,
        player_query: Some(cyrus_script::script::QueryInputs {
            distance_override: query
                .distance_override
                .or_else(|| cheats.distance_override().ok()),
            ..query
        }),
        ..Default::default()
    }
}

pub(super) fn execute_stage(
    world: &mut cyrus_script::script::ScriptWorld,
    id: ScriptActorId,
    stage: ScriptStage,
    inputs: cyrus_script::script::ScriptInputs,
    random: &mut cyrus_game::random::EngineRandom,
    mut sounds: Option<&mut Messages<cyrus_world::sound::SoundCue>>,
    script_bindings: &std::collections::BTreeMap<Entity, ScriptActorId>,
) -> anyhow::Result<serde_json::Value> {
    let mut captured_sounds = Vec::new();
    let mut emit = |actor, sound| {
        emit_script_sound(
            actor,
            sound,
            script_bindings,
            &mut sounds,
            &mut captured_sounds,
        )
    };
    world.advance(id, stage, inputs, Some(&mut emit), Some(random), 100_000)?;
    let mut result = world.inspect(id.index, 1);
    result["captured_sounds"] = json!(captured_sounds);
    Ok(result)
}
