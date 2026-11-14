//! Generation-scoped opaque actor readback and committed death observations for native guests.
use bevy::prelude::*;
use cyrus_assets::animation::{Animations, ModelFrame};
use cyrus_script::script::{ActorPresence, ActorSnapshot, ScriptActors, ScriptWorld};
use std::collections::BTreeMap;
use wasvy::prelude::*;

#[derive(Clone, Copy, Reflect)]
enum State {
    Alive,
    Dead,
    HiddenAlive,
    HiddenDead,
    Removed,
}
impl State {
    fn code(self) -> u32 {
        match self {
            Self::Alive => 1,
            Self::Dead => 2,
            Self::HiddenAlive => 3,
            Self::HiddenDead => 4,
            Self::Removed => 5,
        }
    }
}

#[derive(Clone, Reflect)]
struct Snapshot {
    id: u64,
    generation: u64,
    index: usize,
    state: State,
    player: bool,
    model: String,
    frame: Vec<u32>,
    pose: Vec<f32>,
    source: Vec<f32>,
}

/// Read-only actor table. Tokens are never reused, including after a save load.
#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Actors {
    snapshots: Vec<Snapshot>,
    deaths: Vec<Snapshot>,
}

impl Actors {
    pub(crate) fn visible_dead(&self, id: u64) -> bool {
        self.snapshots
            .iter()
            .any(|s| s.id == id && matches!(s.state, State::Dead))
    }
    pub(crate) fn logical(&self, id: u64) -> Option<cyrus_script::script::ScriptActorId> {
        let entry = self.snapshots.iter().find(|s| s.id == id)?;
        Some(cyrus_script::script::ScriptActorId {
            generation: entry.generation,
            index: entry.index,
        })
    }
}

#[wasvy::methods]
impl Actors {
    pub fn ids(&self) -> Vec<u64> {
        self.snapshots.iter().map(|snapshot| snapshot.id).collect()
    }
    /// 0 missing/stale, 1 alive, 2 dead, 3 hidden alive, 4 hidden dead, 5 removed.
    pub fn state(&self, id: u64) -> u32 {
        self.snapshots
            .iter()
            .find(|s| s.id == id)
            .map_or(0, |s| s.state.code())
    }
    pub fn player(&self, id: u64) -> bool {
        self.snapshots.iter().any(|s| s.id == id && s.player)
    }
    pub fn model(&self, id: u64) -> String {
        self.snapshots
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.model.clone())
            .unwrap_or_default()
    }
    pub fn frame(&self, id: u64) -> Vec<u32> {
        self.snapshots
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.frame.clone())
            .unwrap_or_default()
    }
    pub fn pose(&self, id: u64) -> Vec<f32> {
        self.snapshots
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.pose.clone())
            .unwrap_or_default()
    }
    pub fn count(&self) -> u32 {
        self.deaths.len() as u32
    }
    pub fn death(&self, index: u32) -> Vec<u64> {
        self.deaths
            .get(index as usize)
            .map(|s| vec![s.id])
            .unwrap_or_default()
    }
    pub fn eventmodel(&self, index: u32) -> String {
        self.deaths
            .get(index as usize)
            .map(|s| s.model.clone())
            .unwrap_or_default()
    }
    pub fn eventpose(&self, index: u32) -> Vec<f32> {
        self.deaths
            .get(index as usize)
            .map(|s| s.pose.clone())
            .unwrap_or_default()
    }
    pub fn eventframe(&self, index: u32) -> Vec<u32> {
        self.deaths
            .get(index as usize)
            .map(|s| s.frame.clone())
            .unwrap_or_default()
    }
    pub fn eventplayer(&self, index: u32) -> bool {
        self.deaths.get(index as usize).is_some_and(|s| s.player)
    }

    pub fn source(&self, index: u32) -> Vec<f32> {
        self.deaths
            .get(index as usize)
            .map(|s| s.source.clone())
            .unwrap_or_default()
    }
}

#[derive(Default)]
pub(crate) struct Identities {
    next: u64,
    world: Option<(u32, u64)>,
    ids: BTreeMap<(usize, u64), u64>,
}

impl Identities {
    fn reset(&mut self, world: Option<(u32, u64)>) {
        if self.world != world {
            self.ids.clear();
            self.world = world;
        }
    }
    fn token(&mut self, index: usize, revision: u64) -> u64 {
        *self.ids.entry((index, revision)).or_insert_with(|| {
            self.next = self
                .next
                .checked_add(1)
                .expect("actor token space exhausted");
            self.next
        })
    }
}

fn snapshot(
    raw: ActorSnapshot,
    id: u64,
    world: &ScriptWorld,
    data: Option<&Animations>,
) -> Snapshot {
    let state = match raw.presence {
        ActorPresence::Removed => State::Removed,
        ActorPresence::Hidden => {
            if raw.dead {
                State::HiddenDead
            } else {
                State::HiddenAlive
            }
        }
        ActorPresence::Visible => {
            if raw.dead {
                State::Dead
            } else {
                State::Alive
            }
        }
    };
    let model = raw.model.or_else(|| {
        let frame = raw.frame_reference?;
        let data = data?;
        let actor = world.actors().get(raw.actor.index)?;
        let ModelFrame { model, frame } = if actor.placement().is_some() {
            data.resolve_map_frame(raw.actor.index, frame).ok()?
        } else {
            data.resolve_frame(actor.name(), frame).ok()?
        };
        Some((model, frame))
    });
    let pose = raw
        .pose
        .map(|pose| {
            // Conjugate the original row-major orientation by the export-axis basis.
            let affine = cyrus_render::script_transform::scene_transform(pose);
            let (_, rotation, position) = affine.to_scale_rotation_translation();
            vec![
                position.x, position.y, position.z, rotation.x, rotation.y, rotation.z, rotation.w,
            ]
        })
        .unwrap_or_default();
    Snapshot {
        id,
        generation: raw.actor.generation,
        index: raw.actor.index,
        state,
        player: raw.player,
        model: model.as_ref().map(|m| m.0.clone()).unwrap_or_default(),
        frame: model
            .and_then(|m| u32::try_from(m.1).ok())
            .into_iter()
            .collect(),
        pose,
        source: vec![],
    }
}

pub(crate) fn publish(
    mut host: Query<&mut Actors>,
    actors: Option<ResMut<ScriptActors>>,
    animations: Option<Res<Assets<Animations>>>,
    request: Option<Res<cyrus_world::animation::AnimationRequest>>,
    mut identities: Local<Identities>,
) {
    let Ok(mut host) = host.single_mut() else {
        return;
    };
    host.snapshots.clear();
    host.deaths.clear();
    let Some(mut actors) = actors else {
        identities.reset(None);
        return;
    };
    let Some(world) = actors.current.as_mut() else {
        identities.reset(None);
        return;
    };
    identities.reset(Some((world.world, world.generation())));
    let data = request
        .as_deref()
        .and_then(|r| animations.as_deref()?.get(&r.0));
    for index in 0..world.actors().len() {
        let id = world.id(index).expect("current actor index");
        if let Ok(raw) = world.actor_snapshot(id) {
            let token = identities.token(index, raw.revision);
            host.snapshots.push(snapshot(raw, token, world, data));
        }
    }
    for event in world.take_death_events() {
        let mut entry = snapshot(
            event.snapshot.clone(),
            identities.token(event.snapshot.actor.index, event.snapshot.revision),
            world,
            data,
        );
        entry.source = event
            .source_position
            .map(|[x, y, z]| vec![-x / 20., -y / 20., (z - 65_535.996) / 20.])
            .unwrap_or_default();
        host.deaths.push(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opaque_tokens_distinguish_duplicate_names_and_replaced_worlds() {
        let mut ids = Identities::default();
        ids.reset(Some((1, 1)));
        let first = ids.token(0, 0);
        let second = ids.token(1, 0);
        assert_ne!(first, second);
        assert_eq!(ids.token(0, 0), first);
        ids.reset(Some((1, 2)));
        assert_ne!(ids.token(0, 0), first);
        ids.reset(None);
        ids.reset(Some((1, 3)));
        assert_ne!(ids.token(0, 0), second);
    }
}

#[cfg(test)]
impl Actors {
    pub(crate) fn fixture_death(id: u64) -> Self {
        let entry = Snapshot {
            id,
            generation: 1,
            index: 0,
            state: State::Dead,
            player: true,
            model: "CYRSA001".into(),
            frame: vec![0],
            pose: vec![0., 0., 0., 0., 0., 0., 1.],
            source: vec![],
        };
        Self {
            snapshots: vec![entry.clone()],
            deaths: vec![entry],
        }
    }
}
