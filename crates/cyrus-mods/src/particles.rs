//! Mod-owned Hanabi GPU bursts in scene coordinates, independent of gameplay rules.
use crate::Catalog;
use crate::ownership::{Owner, Owners};
use bevy::prelude::*;
use bevy_hanabi::prelude::*;
use cyrus_script::script::ScriptActors;
use std::collections::BTreeMap;
use wasvy::prelude::*;

const LIVE_LIMIT: usize = 1024;
const OWNER_LIMIT: usize = 256;
const BURST_LIMIT: u32 = 64;

#[derive(Clone, Reflect)]
struct Emission {
    owner: String,
    incarnation: Owner,
    world: (u32, u64),
    position: Vec3,
    direction: Vec3,
    acceleration: Vec3,
    color: [f32; 4],
    speed: [f32; 2],
    radius: [f32; 2],
    lifetime: [f32; 2],
    spread: f32,
    stretch: f32,
    count: u32,
}

/// Validated bursts, consumed after the mods' Update systems.
#[derive(Component, Reflect, Default, WasvyComponent)]
#[reflect(Component)]
pub struct Particles {
    pending: Vec<Emission>,
    owners: BTreeMap<String, Owner>,
    world: Option<(u32, u64)>,
}

fn range(values: Vec<f32>, minimum: f32, maximum: f32) -> Option<[f32; 2]> {
    let values: [f32; 2] = values.try_into().ok()?;
    (values
        .iter()
        .all(|v| v.is_finite() && (minimum..=maximum).contains(v))
        && values[0] <= values[1])
        .then_some(values)
}

#[wasvy::methods]
impl Particles {
    #[allow(clippy::too_many_arguments)] // The WIT boundary uses primitive lists; the SDK supplies a typed ParticleBurst.
    pub fn burst(
        &mut self,
        owner: String,
        position: Vec<f32>,
        direction: Vec<f32>,
        acceleration: Vec<f32>,
        color: Vec<f32>,
        speed: Vec<f32>,
        radius: Vec<f32>,
        lifetime: Vec<f32>,
        spread: f32,
        stretch: f32,
        count: u32,
    ) -> bool {
        let (Some(incarnation), Some(world)) = (self.owners.get(&owner).copied(), self.world)
        else {
            return false;
        };
        let (Ok(position), Ok(direction), Ok(acceleration), Ok(color)) = (
            <[f32; 3]>::try_from(position),
            <[f32; 3]>::try_from(direction),
            <[f32; 3]>::try_from(acceleration),
            <[f32; 4]>::try_from(color),
        ) else {
            return false;
        };
        let (Some(speed), Some(radius), Some(lifetime)) = (
            range(speed, 0.0, 1000.0),
            range(radius, 0.001, 10.0),
            range(lifetime, 0.01, 10.0),
        ) else {
            return false;
        };
        if !position
            .iter()
            .chain(&direction)
            .chain(&acceleration)
            .all(|v| v.is_finite() && v.abs() <= 1.0e6)
            || !color
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            || !spread.is_finite()
            || !(0.0..=1.0).contains(&spread)
            || !stretch.is_finite()
            || !(1.0..=8.0).contains(&stretch)
            || !(1..=BURST_LIMIT).contains(&count)
        {
            return false;
        }
        let Some(direction) = Vec3::from(direction).try_normalize() else {
            return false;
        };
        let count_usize = count as usize;
        if self.pending.iter().map(|p| p.count as usize).sum::<usize>() + count_usize > LIVE_LIMIT
            || self
                .pending
                .iter()
                .filter(|p| p.owner == owner)
                .map(|p| p.count as usize)
                .sum::<usize>()
                + count_usize
                > OWNER_LIMIT
        {
            return false;
        }
        self.pending.push(Emission {
            owner,
            incarnation,
            world,
            position: position.into(),
            direction,
            acceleration: acceleration.into(),
            color,
            speed,
            radius,
            lifetime,
            spread,
            stretch,
            count,
        });
        true
    }
}

struct Live {
    emission: Emission,
    entity: Entity,
    age: f32,
    started: bool,
}
impl Live {
    fn advance(&mut self, delta: f32, ready: bool) {
        if ready {
            if self.started {
                self.age += delta;
            } else {
                self.started = true;
            }
        }
    }

    fn retained(&self, owners: &BTreeMap<String, Owner>, same_world: bool) -> bool {
        same_world
            && self.age < self.emission.lifetime[1]
            && owners.get(&self.emission.owner) == Some(&self.emission.incarnation)
    }
}

#[derive(Resource, Default)]
pub(crate) struct ParticleState {
    world: Option<(u32, u64)>,
    effect: Option<Handle<EffectAsset>>,
    live: Vec<Live>,
    sequence: u32,
}

/// One shared GPU program; per-burst properties avoid recompiling shaders for each hit.
fn effect_asset() -> EffectAsset {
    let writer = ExprWriter::new();
    let speed_min = writer.add_property("speed_min", 1.0.into());
    let speed_max = writer.add_property("speed_max", 1.0.into());
    let radius_min = writer.add_property("radius_min", 0.05.into());
    let radius_max = writer.add_property("radius_max", 0.05.into());
    let lifetime_min = writer.add_property("lifetime_min", 0.5.into());
    let lifetime_max = writer.add_property("lifetime_max", 0.5.into());
    let spread = writer.add_property("spread", 0.25.into());
    let stretch = writer.add_property("stretch", 1.0.into());
    let color = writer.add_property("color", Vec4::ONE.into());
    let acceleration = writer.add_property("acceleration", Vec3::ZERO.into());
    let speed = writer.prop(speed_min)
        + writer.rand(ScalarType::Float) * (writer.prop(speed_max) - writer.prop(speed_min));
    let x = writer.lit(-1.0).uniform(writer.lit(1.0)) * writer.prop(spread);
    let z = writer.lit(-1.0).uniform(writer.lit(1.0)) * writer.prop(spread);
    let velocity = x.vec3(writer.lit(1.0), z).normalized() * speed;
    let lifetime = (writer.prop(lifetime_min)
        + writer.rand(ScalarType::Float) * (writer.prop(lifetime_max) - writer.prop(lifetime_min)))
    .expr();
    let diameter = (writer.prop(radius_min)
        + writer.rand(ScalarType::Float) * (writer.prop(radius_max) - writer.prop(radius_min)))
        * writer.lit(2.0);
    let size = (diameter.clone() * writer.prop(stretch))
        .vec3(diameter, writer.lit(1.0))
        .expr();
    let color = writer.prop(color).pack4x8unorm().expr();
    let acceleration = writer.prop(acceleration).expr();
    let position = writer.lit(Vec3::ZERO).expr();
    let velocity = velocity.expr();
    let age = writer.lit(0.0).expr();
    let roundness = writer.lit(1.0).expr();
    let mut fade = bevy_hanabi::Gradient::new();
    fade.add_key(0.0, Vec4::ONE);
    fade.add_key(0.65, Vec4::ONE);
    fade.add_key(1.0, Vec4::new(0.65, 0.65, 0.65, 0.0));
    EffectAsset::new(
        BURST_LIMIT,
        SpawnerSettings::once(1.0.into()),
        writer.finish(),
    )
    .with_name("mod directional droplets")
    .with_simulation_space(SimulationSpace::Global)
    .with_simulation_condition(SimulationCondition::Always)
    .init(SetAttributeModifier::new(Attribute::POSITION, position))
    .init(SetAttributeModifier::new(Attribute::VELOCITY, velocity))
    .init(SetAttributeModifier::new(Attribute::AGE, age))
    .init(SetAttributeModifier::new(Attribute::LIFETIME, lifetime))
    .init(SetAttributeModifier::new(Attribute::SIZE3, size))
    .init(SetAttributeModifier::new(Attribute::COLOR, color))
    .update(AccelModifier::new(acceleration))
    .render(OrientModifier::new(OrientMode::AlongVelocity))
    .render(RoundModifier { roundness })
    .render(ColorOverLifetimeModifier {
        gradient: fade,
        blend: ColorBlendMode::Modulate,
        mask: ColorBlendMask::RGBA,
    })
}

fn properties(emission: &Emission) -> EffectProperties {
    let color = Color::srgba(
        emission.color[0],
        emission.color[1],
        emission.color[2],
        emission.color[3],
    )
    .to_linear()
    .to_vec4();
    EffectProperties::default().with_properties([
        ("speed_min".into(), emission.speed[0].into()),
        ("speed_max".into(), emission.speed[1].into()),
        ("radius_min".into(), emission.radius[0].into()),
        ("radius_max".into(), emission.radius[1].into()),
        ("lifetime_min".into(), emission.lifetime[0].into()),
        ("lifetime_max".into(), emission.lifetime[1].into()),
        ("spread".into(), emission.spread.into()),
        ("stretch".into(), emission.stretch.into()),
        ("color".into(), color.into()),
        ("acceleration".into(), emission.acceleration.into()),
    ])
}

/// Capture the submitting mod's loaded revision and world before guest Update runs.
/// File reload requests are asynchronous; only a completed asset load changes this lease.
pub(crate) fn publish(mut host: Query<&mut Particles>, owners: Res<Owners>) {
    let Ok(mut host) = host.single_mut() else {
        return;
    };
    host.world = owners.world;
    host.owners = owners.active.clone();
}

/// CPU ownership and budgets; Hanabi integrates and renders the particles on the GPU.
#[allow(clippy::too_many_arguments)] // Bevy exposes ownership, assets, time and GPU readiness as system parameters.
pub(crate) fn update(
    mut commands: Commands,
    mut host: Query<&mut Particles>,
    mut state: ResMut<ParticleState>,
    catalog: Res<Catalog>,
    actors: Option<Res<ScriptActors>>,
    time: Res<Time>,
    compiled: Query<&CompiledParticleEffect>,
    mut effects: ResMut<Assets<EffectAsset>>,
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
    state.live.retain_mut(|burst| {
        burst.advance(
            time.delta_secs(),
            compiled
                .get(burst.entity)
                .is_ok_and(CompiledParticleEffect::is_ready),
        );
        if !burst.retained(&owners, same_world) {
            commands.entity(burst.entity).despawn();
            return false;
        }
        true
    });
    state.world = world;
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
        if state
            .live
            .iter()
            .map(|p| p.emission.count as usize)
            .sum::<usize>()
            + emission.count as usize
            > LIVE_LIMIT
            || state
                .live
                .iter()
                .filter(|p| p.emission.owner == emission.owner)
                .map(|p| p.emission.count as usize)
                .sum::<usize>()
                + emission.count as usize
                > OWNER_LIMIT
        {
            continue;
        }
        let effect = state
            .effect
            .get_or_insert_with(|| effects.add(effect_asset()))
            .clone();
        state.sequence = state.sequence.wrapping_add(1);
        let mut instance = ParticleEffect::new(effect);
        instance.prng_seed = Some(state.sequence);
        let entity = commands
            .spawn((
                Name::new(format!("mod particle burst: {}", emission.owner)),
                instance,
                EffectSpawner::new(&SpawnerSettings::once((emission.count as f32).into())),
                properties(&emission),
                Transform::from_translation(emission.position)
                    .with_rotation(Quat::from_rotation_arc(Vec3::Y, emission.direction)),
            ))
            .id();
        state.live.push(Live {
            emission,
            entity,
            age: 0.0,
            started: false,
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

    #[test]
    fn burst_initializers_generate_shader_code_with_instance_properties() {
        let asset = effect_asset();
        let mut module = asset.module().clone();
        let properties = asset.property_layout();
        let particles = asset.particle_layout();
        let textures = asset.texture_layout();
        let mut context =
            ShaderWriter::new(ModifierContext::Init, &properties, &particles, &textures);
        for modifier in asset.init_modifiers() {
            modifier.apply(&mut module, &mut context).unwrap();
        }
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
    fn emit(particles: &mut Particles, direction: Vec<f32>, count: u32) -> bool {
        particles.burst(
            "test".into(),
            vec![0.0; 3],
            direction,
            vec![0.0, -9.8, 0.0],
            vec![1.0; 4],
            vec![1.0, 3.0],
            vec![0.02, 0.05],
            vec![0.2, 0.7],
            0.3,
            2.0,
            count,
        )
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
            .init_resource::<Assets<EffectAsset>>()
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
    fn count(app: &mut App) -> usize {
        app.world_mut()
            .query::<&ParticleEffect>()
            .iter(app.world())
            .count()
    }
    fn queue(app: &mut App, host: Entity) {
        assert!(emit(
            &mut app.world_mut().get_mut::<Particles>(host).unwrap(),
            vec![1.0, 0.0, 0.0],
            16
        ));
    }
    #[test]
    fn renderer_keeps_pending_effects_and_clears_on_disable_revision_and_world_change() {
        let (mut app, host) = system_app();
        queue(&mut app, host);
        app.update();
        assert_eq!(count(&mut app), 1);
        for _ in 0..8 {
            app.update();
        }
        assert_eq!(
            count(&mut app),
            1,
            "GPU preparation must not consume lifetime"
        );
        app.world_mut().resource_mut::<Catalog>().entries[0].active = false;
        app.update();
        assert_eq!(count(&mut app), 0);
        app.world_mut().resource_mut::<Catalog>().entries[0].active = true;
        queue(&mut app, host);
        app.update();
        queue(&mut app, host);
        app.world_mut()
            .get_mut::<Particles>(host)
            .unwrap()
            .owners
            .get_mut("test")
            .unwrap()
            .revision += 1;
        app.update();
        assert_eq!(count(&mut app), 0);
        queue(&mut app, host);
        app.update();
        assert_eq!(count(&mut app), 1);
        queue(&mut app, host);
        app.world_mut().resource_mut::<ScriptActors>().current = Some(world(2));
        app.update();
        assert_eq!(count(&mut app), 0);
        app.update();
        assert_eq!(count(&mut app), 0);
    }
    #[test]
    fn burst_contract_rejects_invalid_directions_and_counts_and_budgets_particles() {
        let mut queue = fixture();
        assert!(!emit(&mut queue, vec![0.0; 3], 1));
        assert!(!emit(&mut queue, vec![f32::NAN, 1.0, 0.0], 1));
        assert!(!emit(&mut queue, vec![1.0, 0.0], 1));
        assert!(!emit(&mut queue, vec![1.0, 0.0, 0.0], 0));
        assert!(!emit(&mut queue, vec![1.0, 0.0, 0.0], 65));
        for _ in 0..4 {
            assert!(emit(&mut queue, vec![1.0, 0.0, 0.0], 64));
        }
        assert!(!emit(&mut queue, vec![1.0, 0.0, 0.0], 1));
        assert!(range(vec![0.5, 0.1], 0.0, 1.0).is_none());
    }
    #[test]
    fn bursts_expire_and_do_not_survive_disable_reload_or_world_replacement() {
        let mut queue = fixture();
        assert!(emit(&mut queue, vec![2.0, 0.0, 0.0], 10));
        let emission = queue.pending.remove(0);
        assert_eq!(emission.direction, Vec3::X);
        let mut burst = Live {
            emission,
            entity: Entity::PLACEHOLDER,
            age: 0.0,
            started: false,
        };
        assert!(burst.retained(&queue.owners, true));
        assert!(!burst.retained(&queue.owners, false));
        let rotation = Quat::from_rotation_arc(Vec3::Y, burst.emission.direction);
        assert!((rotation * Vec3::Y - Vec3::X).length() < 1.0e-5);
        queue.owners.get_mut("test").unwrap().revision += 1;
        assert!(!burst.retained(&queue.owners, true));
        queue.owners.get_mut("test").unwrap().revision -= 1;
        burst.advance(2.0, false);
        assert!(burst.retained(&queue.owners, true));
        burst.advance(2.0, true);
        assert!(burst.retained(&queue.owners, true));
        burst.advance(0.7, true);
        assert!(!burst.retained(&queue.owners, true));
        queue.owners.clear();
        assert!(!burst.retained(&queue.owners, true));
    }
}
