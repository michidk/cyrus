//! Validate recorded clock evidence before arming a native replay frame.
use super::*;
use cyrus_script::script::replay_clock::RecordedClockTimeline;

pub(super) fn prepare(
    timeline: &cyrus_control::ReplayClockTimeline,
    held: bool,
    scheduler: &input_schedule::InputScheduler,
    scene: &Scene,
    gameplay: &Gameplay,
    elapsed: Option<u32>,
) -> Result<RecordedClockTimeline, String> {
    let world = gameplay
        .script_actors
        .as_deref()
        .and_then(|actors| actors.current.as_ref())
        .ok_or("recorded clock timeline requires a current world")?;
    if !held
        || !scheduler.has_origin(world.generation())
        || !scene.is_ready_world(world.world)
        || elapsed.is_none()
    {
        return Err("recorded clock timeline requires an origin-bound ready held replay and explicit effective interval".into());
    }
    validate(
        timeline,
        world,
        scene
            .clock
            .as_deref()
            .ok_or("recorded clock requires a clock")?,
    )
}

pub(super) fn validate(
    timeline: &cyrus_control::ReplayClockTimeline,
    world: &cyrus_script::script::ScriptWorld,
    clock: &cyrus_script::clock::SimulationClock,
) -> Result<RecordedClockTimeline, String> {
    if timeline.generation != world.generation()
        || timeline.expected_update != world.ordinary_updates()
        || timeline.world != world.world
        || timeline.expected_bios_sample != clock.bios_sample()
    {
        return Err(format!(
            "world {} recorded clock stale identity/update/sample: expected {}/{}/{}, current {}/{}/{}",
            world.world,
            timeline.generation,
            timeline.expected_update,
            timeline.expected_bios_sample,
            world.generation(),
            world.ordinary_updates(),
            clock.bios_sample()
        ));
    }
    if timeline.decisions.is_empty() {
        return Err("recorded clock requires decision entries".into());
    }
    let mut occurrences = std::collections::HashMap::<usize, u32>::new();
    let mut decisions = Vec::with_capacity(timeline.decisions.len());
    for entry in &timeline.decisions {
        let index = resolve_entry(world, entry)?;
        let occurrence = occurrences.entry(entry.index).or_default();
        *occurrence += 1;
        if entry.occurrence != *occurrence {
            return Err(format!(
                "world {} recorded clock actor {} expected {} occurrence{}, current {} occurrence{}",
                world.world,
                entry.index,
                entry.name,
                entry.occurrence,
                world.actors()[index].name(),
                occurrence
            ));
        }
        decisions.push(world.id(index).map_err(|error| error.to_string())?);
    }
    Ok(RecordedClockTimeline {
        stored_sample: timeline.stored_sample,
        decisions,
        before_decisions: timeline.before_decisions.clone(),
        after_decisions: timeline.after_decisions.clone(),
    })
}

fn resolve_entry(
    world: &cyrus_script::script::ScriptWorld,
    entry: &cyrus_control::ReplayDecisionEntry,
) -> Result<usize, String> {
    let identifier = i16::try_from(entry.index).map_err(|_| {
        format!(
            "world {} recorded clock runtime identifier {} outside signed word",
            world.world, entry.index
        )
    })?;
    let mut matches = world.actors().iter().enumerate().filter(|(_, actor)| {
        actor.runtime_identifier == Some(identifier) && actor.name() == entry.name
    });
    let (index, _) = matches.next().ok_or_else(|| {
        format!(
            "world {} recorded clock runtime identifier {} actor {} absent",
            world.world, entry.index, entry.name
        )
    })?;
    if matches.next().is_some() {
        return Err(format!(
            "world {} recorded clock runtime identifier {} actor {} ambiguous",
            world.world, entry.index, entry.name
        ));
    }
    Ok(index)
}
