//! Browser CD output ownership at the playback entity's lifetime boundary.
use bevy::{
    ecs::{lifecycle::HookContext, world::DeferredWorld},
    prelude::{App, Entity, Resource},
};
use std::collections::HashMap;

#[derive(Resource)]
pub(super) struct Players {
    pub(super) ids: HashMap<Entity, u32>,
    stop: fn(u32),
}

impl Drop for Players {
    fn drop(&mut self) {
        for id in self.ids.values() {
            (self.stop)(*id);
        }
    }
}

pub(super) fn register(app: &mut App, stop: fn(u32)) {
    app.insert_resource(Players {
        ids: HashMap::new(),
        stop,
    });
}

pub(super) fn retire(mut world: DeferredWorld, context: HookContext) {
    if let Some(mut players) = world.get_resource_mut::<Players>()
        && let Some(id) = players.ids.remove(&context.entity)
    {
        (players.stop)(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::MusicPlayer;
    use std::sync::Mutex;

    #[test]
    fn removing_a_music_owner_retires_output_before_another_update() {
        static STOPPED: Mutex<Vec<u32>> = Mutex::new(Vec::new());
        let mut app = App::new();
        register(&mut app, |id| STOPPED.lock().unwrap().push(id));
        let old = app.world_mut().spawn(MusicPlayer).id();
        let retained = app.world_mut().spawn(MusicPlayer).id();
        app.world_mut()
            .resource_mut::<Players>()
            .ids
            .extend([(old, 1), (retained, 2)]);
        let stopped = || STOPPED.lock().unwrap().clone();
        let unstaged = app.world_mut().spawn(MusicPlayer).id();
        app.world_mut().despawn(unstaged);
        assert!(stopped().is_empty());
        app.world_mut().despawn(old);
        assert_eq!(stopped(), [1], "publication must not wait for Update");
        assert_eq!(
            app.world().resource::<Players>().ids.get(&retained),
            Some(&2)
        );
        let replacement = app.world_mut().spawn(MusicPlayer).id();
        app.world_mut()
            .resource_mut::<Players>()
            .ids
            .insert(replacement, 3);
        app.world_mut()
            .entity_mut(replacement)
            .remove::<MusicPlayer>();
        assert_eq!(stopped(), [1, 3]);
        drop(app);
        assert_eq!(stopped(), [1, 3, 2]);
    }
}
