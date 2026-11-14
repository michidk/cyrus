//! Native gameplay services requested by Wasm guests; hosts own eligibility and completion.
use crate::Catalog;
use crate::ownership::{Owner, Owners};
use bevy::prelude::*;
use cyrus_script::script::ScriptActors;
use std::collections::HashMap;
use wasvy::prelude::*;

#[derive(Reflect)]
struct Request {
    owner: String,
    incarnation: Owner,
    world: (u32, u64),
}

fn request(
    owner: String,
    owners: &HashMap<String, Owner>,
    world: Option<(u32, u64)>,
) -> Option<Request> {
    Some(Request {
        incarnation: *owners.get(&owner)?,
        owner,
        world: world?,
    })
}

fn owners(catalog: &Catalog, incarnations: &Owners) -> HashMap<String, Owner> {
    catalog
        .entries
        .iter()
        .filter(|entry| entry.active && !entry.gone)
        .filter_map(|entry| {
            let incarnation = *incarnations.active.get(&entry.id)?;
            (entry.instance == Some(incarnation.instance)).then(|| (entry.id.clone(), incarnation))
        })
        .collect()
}

fn current(world: &World) -> Option<(u32, u64)> {
    let actors = world.get_resource::<ScriptActors>()?.current.as_ref()?;
    Some((actors.world, actors.generation()))
}

/// Finish a displayed speech line through the synchronous audio owner.
#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Dialogue {
    available: bool,
    pending: Option<(Request, usize, u64)>,
    world: Option<(u32, u64)>,
    line: Option<(usize, u64)>,
    owners: HashMap<String, Owner>,
}

#[wasvy::methods]
impl Dialogue {
    pub fn available(&mut self) -> bool {
        self.available
    }
    pub fn advance(&mut self, owner: String) -> bool {
        if !self.available || self.pending.is_some() {
            return false;
        }
        let Some(request) = request(owner, &self.owners, self.world) else {
            return false;
        };
        let Some((index, revision)) = self.line else {
            return false;
        };
        self.pending = Some((request, index, revision));
        true
    }
}

#[derive(Reflect, Clone)]
enum SaveState {
    Pending,
    Saved,
    Failed(String),
}
impl SaveState {
    fn name(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Saved => "saved",
            Self::Failed(_) => "failed",
        }
    }
}

/// One native save request at a time, with terminal completion retained per guest.
#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Saves {
    ready: bool,
    pending: Option<Request>,
    world: Option<(u32, u64)>,
    owners: HashMap<String, Owner>,
    running: Option<Request>,
    states: HashMap<String, SaveState>,
}

#[wasvy::methods]
impl Saves {
    pub fn ready(&mut self) -> bool {
        self.ready && self.pending.is_none() && self.running.is_none()
    }
    pub fn status(&mut self, owner: String) -> String {
        self.states
            .get(&owner)
            .map_or("", |state| state.name())
            .into()
    }
    pub fn error(&mut self, owner: String) -> String {
        match self.states.get(&owner) {
            Some(SaveState::Failed(error)) => error.clone(),
            _ => String::new(),
        }
    }
    pub fn save(&mut self, owner: String) -> bool {
        if !self.ready() {
            return false;
        }
        let Some(request) = request(owner.clone(), &self.owners, self.world) else {
            return false;
        };
        self.states.insert(owner.clone(), SaveState::Pending);
        self.pending = Some(request);
        true
    }
}

pub(crate) fn publish(
    mut dialogue: Query<&mut Dialogue>,
    catalog: Res<Catalog>,
    incarnations: Res<Owners>,
    mut actors: Option<ResMut<ScriptActors>>,
    state: Option<Res<State<cyrus_game::AppState>>>,
    console: Option<Res<cyrus_console::Console>>,
) {
    let Ok(mut hook) = dialogue.single_mut() else {
        return;
    };
    let in_game = state.is_some_and(|state| *state.get() == cyrus_game::AppState::InGame)
        && !console.is_some_and(|console| console.open || console.blocked);
    let world = actors
        .as_deref_mut()
        .and_then(|actors| actors.current.as_mut());
    let Some(world) = world else {
        hook.available = false;
        hook.pending = None;
        return;
    };
    let stamp = (world.world, world.generation());
    let active = owners(&catalog, &incarnations);
    let eligible =
        in_game && world.globals().lockout() == Some(1) && world.globals().menu_state() == 0;
    if let Some((request, index, revision)) = hook.pending.take()
        && eligible
        && request.world == stamp
        && active.get(&request.owner) == Some(&request.incarnation)
        && let Some(lines) = world.globals_mut().dialogue_mut()
        && lines.presented_token() == Some((index, revision))
    {
        lines.request_line_advance();
    }
    hook.world = Some(stamp);
    hook.owners = active;
    hook.line = world
        .globals()
        .dialogue()
        .and_then(|lines| lines.presented_token());
    hook.available = eligible
        && world
            .globals()
            .dialogue()
            .is_some_and(|lines| lines.presented_line().is_some());
}

/// Native storage adapter publishes live eligibility before consuming a request.
/// Returns true once for an active guest's request. World/safety changes fail explicitly;
/// expired guest incarnations discard their queued work and completion state.
pub fn take_save_request(world: &mut World, ready: bool) -> bool {
    let mut query = world.query::<(Entity, &Saves)>();
    let Some((entity, _)) = query.iter(world).next() else {
        return false;
    };
    let stamp = current(world);
    let active = world
        .get_resource::<Catalog>()
        .zip(world.get_resource::<Owners>())
        .map(|(catalog, incarnations)| owners(catalog, incarnations))
        .unwrap_or_default();
    let mut hook = world.get_mut::<Saves>(entity).expect("save hook queried");
    let expired: Vec<_> = hook
        .owners
        .iter()
        .filter(|(id, incarnation)| active.get(*id) != Some(*incarnation))
        .map(|(id, _)| id.clone())
        .collect();
    for id in expired {
        hook.states.remove(&id);
    }
    hook.world = stamp;
    hook.owners = active.clone();
    hook.ready = ready;
    let Some(request) = hook.pending.take() else {
        return false;
    };

    let valid = ready
        && stamp == Some(request.world)
        && active.get(&request.owner) == Some(&request.incarnation);
    hook.running = Some(request);
    if !valid {
        finish_save(world, Err("save request no longer eligible".into()));
    }
    valid
}

/// Called only after the existing native save adapter reports a terminal storage result.
pub fn finish_save(world: &mut World, result: Result<(), String>) {
    let mut query = world.query::<&mut Saves>();
    let Some(mut hook) = query.iter_mut(world).next() else {
        return;
    };
    let Some(request) = hook.running.take() else {
        return;
    };
    if hook.owners.get(&request.owner) != Some(&request.incarnation) {
        return;
    }
    let owner = request.owner;
    match result {
        Ok(()) => {
            hook.states.insert(owner, SaveState::Saved);
        }
        Err(error) => {
            hook.states.insert(owner, SaveState::Failed(error));
        }
    }
}

#[cfg(test)]
pub(crate) fn dialogue_fixture(world: &mut World) -> bool {
    let owners = world.resource::<Owners>();
    let active: HashMap<_, _> = owners
        .active
        .iter()
        .map(|(id, owner)| (id.clone(), *owner))
        .collect();
    let stamp = owners.world;
    let mut query = world.query::<&mut Dialogue>();
    let mut hook = query.single_mut(world).unwrap();
    let requested = hook.pending.take().is_some();
    hook.available = true;
    hook.world = stamp;
    hook.owners = active;
    hook.line = Some((1, 1));
    requested
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (World, Entity) {
        let mut world = World::new();
        let mut entry = crate::Entry::read(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods/autosave"),
            true,
        )
        .unwrap();
        entry.instance = Some(Entity::PLACEHOLDER);
        world.insert_resource(Catalog {
            entries: vec![entry],
            ..Default::default()
        });
        let mut actors = ScriptActors::default();
        actors.current = Some(crate::particles::tests::world(1));
        world.insert_resource(actors);
        world.insert_resource(Owners {
            active: std::collections::BTreeMap::from([(
                "autosave".into(),
                Owner {
                    instance: Entity::PLACEHOLDER,
                    revision: 1,
                },
            )]),
            world: Some((1, 1)),
        });
        let host = world.spawn(Saves::default()).id();
        assert!(!take_save_request(&mut world, true));
        (world, host)
    }
    #[test]
    fn saves_serialize_and_report_only_terminal_storage_results() {
        let (mut world, host) = fixture();
        let mut saves = world.get_mut::<Saves>(host).unwrap();
        assert!(!saves.save("unknown".into()));
        assert!(saves.save("autosave".into()));
        assert!(!saves.save("autosave".into()));
        assert_eq!(saves.status("autosave".into()), "pending");
        assert!(take_save_request(&mut world, true));
        assert!(!take_save_request(&mut world, true));
        assert!(!world.get_mut::<Saves>(host).unwrap().ready());
        finish_save(&mut world, Err("disk write failed".into()));
        let mut saves = world.get_mut::<Saves>(host).unwrap();
        assert_eq!(saves.status("autosave".into()), "failed");
        assert_eq!(saves.error("autosave".into()), "disk write failed");
        assert!(saves.save("autosave".into()));
        assert!(saves.error("autosave".into()).is_empty());
        assert!(take_save_request(&mut world, true));
        finish_save(&mut world, Ok(()));
        assert_eq!(
            world
                .get_mut::<Saves>(host)
                .unwrap()
                .status("autosave".into()),
            "saved"
        );
    }
    #[test]
    fn saves_reject_disabled_world_replaced_and_newly_blocked_requests() {
        enum Invalidation {
            Disabled,
            World,
            Blocked,
            Reloaded,
        }
        for case in [
            Invalidation::Disabled,
            Invalidation::World,
            Invalidation::Blocked,
            Invalidation::Reloaded,
        ] {
            let (mut world, host) = fixture();
            assert!(
                world
                    .get_mut::<Saves>(host)
                    .unwrap()
                    .save("autosave".into())
            );
            match case {
                Invalidation::Disabled => world.resource_mut::<Catalog>().entries[0].active = false,
                Invalidation::World => {
                    world.resource_mut::<ScriptActors>().current =
                        Some(crate::particles::tests::world(2))
                }
                Invalidation::Reloaded => {
                    world
                        .resource_mut::<Owners>()
                        .active
                        .get_mut("autosave")
                        .unwrap()
                        .revision += 1
                }
                Invalidation::Blocked => {}
            }
            assert!(!take_save_request(
                &mut world,
                !matches!(case, Invalidation::Blocked)
            ));
            assert_eq!(
                world
                    .get_mut::<Saves>(host)
                    .unwrap()
                    .status("autosave".into()),
                if matches!(case, Invalidation::Disabled | Invalidation::Reloaded) {
                    ""
                } else {
                    "failed"
                }
            );
        }
    }
    #[test]
    fn dialogue_requires_an_active_owner_and_coalesces_requests() {
        let mut dialogue = Dialogue {
            available: true,
            world: Some((1, 1)),
            line: Some((1, 1)),
            owners: HashMap::from([(
                "dialogue-skip".into(),
                Owner {
                    instance: Entity::PLACEHOLDER,
                    revision: 1,
                },
            )]),
            ..Default::default()
        };
        assert!(!dialogue.advance("unknown".into()));
        assert!(dialogue.advance("dialogue-skip".into()));
        assert!(!dialogue.advance("dialogue-skip".into()));
    }
}
