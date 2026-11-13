//! Mod-owned transient particles in scene coordinates, independent of gameplay simulation.
use crate::Catalog;
use bevy::prelude::*;
use cyrus_script::script::ScriptActors;
use std::collections::{BTreeMap, HashMap};
use wasvy::prelude::*;

const LIVE_LIMIT: usize = 1024;
const OWNER_LIMIT: usize = 256;

#[derive(Clone, Reflect)]
struct Emission {
    owner: String,
    incarnation: Owner,
    world: (u32, u64),
    position: Vec3,
    velocity: Vec3,
    acceleration: Vec3,
    color: [f32; 4],
    radius: f32,
    lifetime: f32,
}

/// Validated emissions, consumed after the mods' Update systems.
#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Particles {
    pending: Vec<Emission>,
    owners: BTreeMap<String, Owner>,
    world: Option<(u32, u64)>,
}

#[wasvy::methods]
impl Particles {
    #[allow(clippy::too_many_arguments)] // WIT carries primitive lists; the guest SDK supplies a typed Particle.
    pub fn emit(
        &mut self,
        owner: String,
        position: Vec<f32>,
        velocity: Vec<f32>,
        acceleration: Vec<f32>,
        color: Vec<f32>,
        radius: f32,
        lifetime: f32,
    ) -> bool {
        let (Some(incarnation), Some(world)) = (self.owners.get(&owner).copied(), self.world)
        else {
            return false;
        };
        let (Ok(position), Ok(velocity), Ok(acceleration), Ok(color)) = (
            <[f32; 3]>::try_from(position),
            <[f32; 3]>::try_from(velocity),
            <[f32; 3]>::try_from(acceleration),
            <[f32; 4]>::try_from(color),
        ) else {
            return false;
        };
        if !position
            .iter()
            .chain(&velocity)
            .chain(&acceleration)
            .all(|v| v.is_finite() && v.abs() <= 1.0e6)
            || !color
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            || !radius.is_finite()
            || !(0.001..=10.0).contains(&radius)
            || !lifetime.is_finite()
            || !(0.01..=10.0).contains(&lifetime)
            || self.pending.len() >= LIVE_LIMIT
            || self.pending.iter().filter(|p| p.owner == owner).count() >= OWNER_LIMIT
        {
            return false;
        }
        self.pending.push(Emission {
            owner,
            incarnation,
            world,
            position: position.into(),
            velocity: velocity.into(),
            acceleration: acceleration.into(),
            color,
            radius,
            lifetime,
        });
        true
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Reflect)]
struct Owner {
    instance: Entity,
    revision: u64,
}

struct Live {
    emission: Emission,
    entity: Entity,
    material: Handle<StandardMaterial>,
    age: f32,
}

impl Live {
    fn advance(&mut self, seconds: f32) {
        self.age += seconds;
        self.emission.position += self.emission.velocity * seconds
            + self.emission.acceleration * (0.5 * seconds * seconds);
        self.emission.velocity += self.emission.acceleration * seconds;
    }

    fn retained(&self, owners: &BTreeMap<String, Owner>, same_world: bool) -> bool {
        same_world
            && self.age < self.emission.lifetime
            && owners.get(&self.emission.owner) == Some(&self.emission.incarnation)
    }
}

#[derive(Resource, Default)]
pub(crate) struct ParticleState {
    world: Option<(u32, u64)>,
    mesh: Option<Handle<Mesh>>,
    live: Vec<Live>,
}

/// Capture the submitting mod's loaded revision and world before guest Update runs.
/// File reload requests are asynchronous; only a completed asset load changes this lease.
pub(crate) fn publish(
    mut host: Query<&mut Particles>,
    catalog: Res<Catalog>,
    actors: Option<Res<ScriptActors>>,
    mods: Query<&wasvy::prelude::Mod>,
    mut events: MessageReader<AssetEvent<ModAsset>>,
    mut revisions: Local<HashMap<AssetId<ModAsset>, u64>>,
) {
    for event in events.read() {
        if let AssetEvent::LoadedWithDependencies { id } = event {
            *revisions.entry(*id).or_default() += 1;
        }
    }
    let Ok(mut host) = host.single_mut() else {
        return;
    };
    host.world = actors
        .as_deref()
        .and_then(|a| a.current.as_ref())
        .map(|w| (w.world, w.generation()));
    host.owners = catalog
        .entries
        .iter()
        .filter(|e| e.active && !e.gone)
        .filter_map(|entry| {
            let instance = entry.instance?;
            let asset = mods.get(instance).ok()?.asset().id();
            Some((
                entry.id.clone(),
                Owner {
                    instance,
                    revision: revisions.get(&asset).copied().unwrap_or_default(),
                },
            ))
        })
        .collect();
}

/// The renderer owns only generic integration and lifetime, never blood-specific behavior.
#[allow(clippy::too_many_arguments)] // Rendering borrows distinct Bevy resources and queries.
pub(crate) fn update(
    mut commands: Commands,
    mut host: Query<&mut Particles>,
    mut state: ResMut<ParticleState>,
    catalog: Res<Catalog>,
    actors: Option<Res<ScriptActors>>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut transforms: Query<&mut Transform>,
) {
    let world = actors
        .as_deref()
        .and_then(|a| a.current.as_ref())
        .map(|w| (w.world, w.generation()));
    let same_world = world.is_some() && state.world == world;
    let Ok(mut host) = host.single_mut() else {
        return;
    };
    let owners: BTreeMap<_, _> = host
        .owners
        .iter()
        .filter(|(id, owner)| {
            catalog.entries.iter().any(|entry| {
                entry.active
                    && !entry.gone
                    && &entry.id == *id
                    && entry.instance == Some(owner.instance)
            })
        })
        .map(|(id, owner)| (id.clone(), *owner))
        .collect();
    state.live.retain_mut(|particle| {
        particle.advance(time.delta_secs());
        if !particle.retained(&owners, same_world) {
            commands.entity(particle.entity).despawn();
            materials.remove(particle.material.id());
            return false;
        }
        if let Ok(mut transform) = transforms.get_mut(particle.entity) {
            transform.translation = particle.emission.position;
        }
        if let Some(mut material) = materials.get_mut(&particle.material) {
            let [r, g, b, alpha] = particle.emission.color;
            material.base_color = Color::srgba(
                r,
                g,
                b,
                alpha * (1.0 - particle.age / particle.emission.lifetime),
            );
        }
        true
    });
    state.world = world;
    // A batch submitted for the retiring world must never appear in its replacement.
    let pending = std::mem::take(&mut host.pending);
    if !same_world {
        return;
    }
    for emission in pending {
        if owners.get(&emission.owner) != Some(&emission.incarnation)
            || Some(emission.world) != world
        {
            continue;
        }
        if state.live.len() >= LIVE_LIMIT
            || state
                .live
                .iter()
                .filter(|p| p.emission.owner == emission.owner)
                .count()
                >= OWNER_LIMIT
        {
            continue;
        }
        let mesh = state
            .mesh
            .get_or_insert_with(|| meshes.add(Sphere::new(1.0).mesh().uv(6, 4)))
            .clone();
        let [r, g, b, alpha] = emission.color;
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(r, g, b, alpha),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        let entity = commands
            .spawn((
                Name::new(format!("mod particle: {}", emission.owner)),
                Mesh3d(mesh),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(emission.position)
                    .with_scale(Vec3::splat(emission.radius)),
                bevy::light::NotShadowCaster,
            ))
            .id();
        state.live.push(Live {
            emission,
            entity,
            material,
            age: 0.0,
        });
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn world(generation: u64) -> cyrus_script::script::ScriptWorld {
        let mut scripts: cyrus_assets::script::Scripts = serde_json::from_value(serde_json::json!({
            "version": 30, "world": 1, "content_id": "particle-test", "actor_pool_capacity": 10,
            "weapons": {}, "combat_attacks": [], "combat_defenses": [],
            "in_combat_threshold": null, "non_engaged_spacing": null,
            "combat_clank_effects": [null, null], "node_timer": null,
            "player_distance_override": false,
            "clock": {"normal_frame_rate": 12, "min_frame_rate": 6, "max_frame_rate": 300,
                "use_smooth_fps": true, "use_smooth_divisor": true},
            "updates": {"rope": null, "ropes_load": true, "fall_death_ticks": 144,
                "fall_landing": null, "dummy_model_manager": false, "task_system": true,
                "animation_system": true, "disable_slaves": false, "disable_master_slaves": false,
                "final_version": false},
            "dialogue": {"menu_start": [35, 25], "max_menu_items": 300, "max_dialog_lines": 20,
                "max_text_width": 500, "menu_traverse_delay": 4, "print_text": true,
                "use_speech": true, "max_distance": 1600, "rtx_filename": "unused"},
            "definition": {"functions": [], "flags": [], "references": [], "attributes": []},
            "actors": {}, "instances": [], "unsupported": {}
        }))
        .unwrap();
        scripts
            .definition
            .functions
            .push(cyrus_soup::program::Function {
                name: cyrus_soup::Native::NullFunction,
                param_count: 0,
                kind: cyrus_soup::program::CallKind::Function,
            });
        scripts.content_id = scripts.content_identity().unwrap();
        cyrus_script::script::ScriptWorld::prepare(&scripts, generation).unwrap()
    }

    fn system_app() -> (App, Entity) {
        let mut app = App::new();
        let mut actors = ScriptActors::default();
        actors.current = Some(world(1));
        app.add_plugins(MinimalPlugins)
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_millis(100),
            ))
            .insert_resource(actors)
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<ParticleState>()
            .add_systems(PostUpdate, update);
        let mut entry = crate::Entry::read(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods/blood"),
            true,
        )
        .unwrap();
        entry.id = "test".into();
        entry.instance = Some(Entity::PLACEHOLDER);
        app.insert_resource(Catalog {
            entries: vec![entry],
            ..default()
        });
        let host = app.world_mut().spawn(fixture()).id();
        app.update();
        (app, host)
    }

    fn queue(app: &mut App, host: Entity) {
        assert!(emit(
            &mut app.world_mut().get_mut::<Particles>(host).unwrap(),
            vec![0.0; 3],
            0.2
        ));
    }

    fn counts(app: &mut App) -> (usize, usize) {
        (
            app.world_mut().query::<&Mesh3d>().iter(app.world()).count(),
            app.world().resource::<Assets<StandardMaterial>>().len(),
        )
    }

    #[test]
    fn system_removes_entities_and_materials_on_expiry_disable_reload_and_world_replacement() {
        let (mut app, host) = system_app();
        queue(&mut app, host);
        app.update();
        assert_eq!(counts(&mut app), (1, 1));
        app.update();
        app.update();
        assert_eq!(counts(&mut app), (0, 0));

        queue(&mut app, host);
        app.update();
        queue(&mut app, host);
        app.world_mut().resource_mut::<Catalog>().entries[0].active = false;
        app.update();
        assert_eq!(counts(&mut app), (0, 0));
        app.world_mut().resource_mut::<Catalog>().entries[0].active = true;

        queue(&mut app, host);
        app.update();
        queue(&mut app, host);
        // A queued old-revision emission cannot inherit the replacement's lease.
        app.world_mut()
            .get_mut::<Particles>(host)
            .unwrap()
            .owners
            .get_mut("test")
            .unwrap()
            .revision += 1;
        app.update();
        assert_eq!(counts(&mut app), (0, 0));

        queue(&mut app, host);
        app.update();
        queue(&mut app, host);
        app.world_mut().resource_mut::<ScriptActors>().current = Some(world(2));
        app.update();
        assert_eq!(counts(&mut app), (0, 0));
        app.update();
        assert_eq!(counts(&mut app), (0, 0));
    }

    fn fixture() -> Particles {
        Particles {
            owners: BTreeMap::from([(
                "test".into(),
                Owner {
                    instance: Entity::PLACEHOLDER,
                    revision: 1,
                },
            )]),
            world: Some((1, 1)),
            ..default()
        }
    }

    fn emit(particles: &mut Particles, position: Vec<f32>, lifetime: f32) -> bool {
        particles.emit(
            "test".into(),
            position,
            vec![0.0; 3],
            vec![0.0; 3],
            vec![1.0; 4],
            0.1,
            lifetime,
        )
    }

    #[test]
    fn invalid_packets_and_excess_emissions_are_rejected() {
        let mut particles = fixture();
        assert!(!emit(&mut particles, vec![0.0; 2], 1.0));
        assert!(!emit(&mut particles, vec![f32::NAN; 3], 1.0));
        assert!(!emit(&mut particles, vec![0.0; 3], f32::INFINITY));
        assert!(!emit(&mut particles, vec![0.0; 3], 0.0));
        for _ in 0..OWNER_LIMIT {
            assert!(emit(&mut particles, vec![0.0; 3], 1.0));
        }
        assert!(!emit(&mut particles, vec![0.0; 3], 1.0));
    }

    #[test]
    fn particles_integrate_and_end_on_expiry_disable_reload_or_world_change() {
        let mut queue = fixture();
        assert!(queue.emit(
            "test".into(),
            vec![0.0; 3],
            vec![2.0, 0.0, 0.0],
            vec![0.0, -8.0, 0.0],
            vec![1.0; 4],
            0.1,
            1.0
        ));
        let owner = Owner {
            instance: Entity::PLACEHOLDER,
            revision: 1,
        };
        let mut live = Live {
            emission: queue.pending.remove(0),
            entity: Entity::PLACEHOLDER,
            material: Handle::default(),
            age: 0.0,
        };
        let mut owners = BTreeMap::from([("test".into(), owner)]);
        live.advance(0.5);
        assert_eq!(live.emission.position, Vec3::new(1.0, -1.0, 0.0));
        assert!(live.retained(&owners, true));
        assert!(!live.retained(&owners, false));
        owners.get_mut("test").unwrap().revision += 1;
        assert!(!live.retained(&owners, true));
        owners.insert("test".into(), owner);
        live.advance(0.5);
        assert!(!live.retained(&owners, true));
        owners.clear();
        assert!(!live.retained(&owners, true));
    }
}
