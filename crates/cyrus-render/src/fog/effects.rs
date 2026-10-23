//! Environmental pool presentation and retained draw cadence.
mod geometry;
mod timing;
pub(super) use timing::EffectDrawClock;

use super::{EffectDraw, EffectDrawView, SceneMaterial, TextureAnimations};
use crate::{camera::ViewCamera, environment::EnvironmentRequest};
use bevy::{
    camera::visibility::{NoFrustumCulling, VisibilitySystems},
    prelude::*,
};
use cyrus_assets::environment::Environment;
use cyrus_game::{AppState, fidelity::Fidelity, script::ScriptActors};
use cyrus_world::lifecycle::PreparedWorld;

#[derive(Component)]
pub(super) struct EffectBillboard {
    pub slot: usize,
    pub alpha: u16,
    pub admitted: bool,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(in crate::fog) struct RetainedEffectDraw {
    pub slot: usize,
    pub descriptor: u32,
    pub position: [i32; 3],
    pub frame: usize,
}

struct Slot {
    entity: Entity,
    mesh: Handle<Mesh>,
    material: Handle<SceneMaterial>,
    shape: Option<(Vec2, u8, u16)>,
    position: Vec3,
}

#[derive(Resource, Default)]
struct LiveEffects {
    owner: Option<(AssetId<Environment>, u32, u64)>,
    repaint: bool,
    pending: Option<Vec<(usize, cyrus_game::script::EnvironmentalEffect)>>,
    slots: Vec<Slot>,
    /// Presentation slots for actor flat records, grown on demand and ordered after the pool.
    flat_slots: Vec<Slot>,
    pending_flats: Option<Vec<cyrus_game::script::ActorFlatDraw>>,
    flat_packets: Vec<(Vec3, super::EffectDraw)>,
    /// Flat textures already reported as missing from the prepared images.
    reported_flats: std::collections::BTreeSet<u16>,
    lights: crate::lighting::runtime::RuntimeLights,
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<LiveEffects>()
        .add_systems(
            Update,
            bind.in_set(cyrus_game::script::SceneSubmission)
                .after(super::texture::TextureSystems::Bind),
        )
        .add_systems(
            Update,
            capture
                .in_set(cyrus_game::script::SceneSubmission)
                .after(bind),
        )
        .add_systems(
            PostUpdate,
            draw.after(super::texture::TextureSystems::Request)
                .after(VisibilitySystems::CheckVisibility),
        );
}

// Bevy supplies independent source stores, logical ownership and derived GPU assets.
#[allow(clippy::too_many_arguments)]
fn bind(
    mut commands: Commands,
    request: Res<EnvironmentRequest>,
    environments: Res<Assets<Environment>>,
    prepared: Option<Res<PreparedWorld>>,
    ready: Option<Res<crate::ReadyScene>>,
    mut actors: Option<ResMut<ScriptActors>>,
    mut textures: ResMut<TextureAnimations>,
    fidelity: Res<Fidelity>,
    distance: Res<super::RenderDistance>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SceneMaterial>>,
    mut live: ResMut<LiveEffects>,
) {
    let world = actors.as_mut().and_then(|actors| actors.current.as_mut());
    let owner = world
        .as_ref()
        .zip(prepared.as_ref())
        .and_then(|(world, prepared)| {
            (prepared.world == Some(world.world)
                && request.0.path() == Some(&prepared.environment_path)
                && ready
                    .as_ref()
                    .is_some_and(|ready| ready.0 == prepared.animations))
            .then_some((request.0.id(), world.world, world.generation()))
        });
    if live.owner != owner {
        // A session replacement supplies its own packets and clock. Fresh logical
        // generations must not inherit the previous world's retained draw.
        if owner.is_some() && !textures.effects_repaint {
            textures.effect_draws.clear();
            textures.effect_clock = Default::default();
        }
        let live = &mut *live;
        let released = live.slots.drain(..).chain(live.flat_slots.drain(..));
        for slot in released {
            commands.entity(slot.entity).despawn();
            meshes.remove(slot.mesh.id());
            materials.remove(slot.material.id());
        }
        live.owner = None;
        live.pending = None;
        live.pending_flats = None;
        live.flat_packets.clear();
        live.reported_flats.clear();
    }
    let (Some(owner), Some(world), Some(environment), Some(clipping)) = (
        owner,
        world,
        environments.get(&request.0),
        distance.effective(&fidelity),
    ) else {
        return;
    };
    if live.owner == Some(owner)
        || !textures.is_bound_to(request.0.id())
        || textures.effects_error.is_some()
    {
        return;
    }
    let Some(source) = &environment.effects else {
        return;
    };
    let result = (|| -> anyhow::Result<()> {
        source.validate()?;
        for (id, image) in &source.images {
            anyhow::ensure!(
                image.flags & 1 != 0 && image.flags & !0x363 == 0,
                "effect texture {id}: drawing flags {:#x} unsupported",
                image.flags
            );
        }
        // Images and geometry inputs are ready before selected death can allocate a slot.
        world.prepare_environment_effects(source)?;
        let fog = super::fog_extension(environment, &fidelity, clipping);
        if !source.disabled {
            for index in 0..source.capacity as usize {
                let placeholder = EffectDraw {
                    texture: Handle::default(),
                    frame: 0,
                    half_extents: Vec2::ZERO,
                    radius: 0.0,
                    brightness: 255,
                    alpha: 255,
                    flags: 1,
                };
                let mesh = meshes.add(geometry::mesh(&placeholder)?);
                let material =
                    materials.add(geometry::material(&placeholder, &fog, &Handle::default()));
                let entity = commands
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::default(),
                        GlobalTransform::default(),
                        Visibility::Visible,
                        NoFrustumCulling,
                        EffectBillboard {
                            slot: index,
                            alpha: 255,
                            admitted: false,
                        },
                    ))
                    .id();
                live.slots.push(Slot {
                    entity,
                    mesh,
                    material,
                    shape: None,
                    position: Vec3::ZERO,
                });
            }
        }
        live.owner = Some(owner);
        live.repaint = true;
        Ok(())
    })();
    if let Err(error) = result {
        let message = format!("world {} effect presentation: {error:#}", owner.1);
        error!("{message}");
        textures.effects_error = Some(message);
    }
}

// The original builds effect packets before object updates and lifetime expiry.
// Independent ECS inputs keep scheduler admission and presentation time separate.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn capture(
    timing: (
        Res<Time<Real>>,
        Option<Res<cyrus_game::script::WorldClockSample>>,
        Option<Res<crate::presentation_clock::PresentationClock>>,
    ),
    state: Option<Res<State<AppState>>>,
    logbook: Option<Res<cyrus_game::logbook::LogbookScreen>>,
    request: Res<EnvironmentRequest>,
    console: Option<Res<cyrus_console::Console>>,
    cameras: Query<(), With<ViewCamera>>,
    mut actors: Option<ResMut<ScriptActors>>,
    mut textures: ResMut<TextureAnimations>,
    mut live: ResMut<LiveEffects>,
) {
    live.pending = None;
    let stopped = actors
        .as_ref()
        .is_some_and(|actors| actors.world_loop_outcome.is_some());
    let world = actors.as_mut().and_then(|actors| actors.current.as_mut());
    let Some(world) =
        world.filter(|world| live.owner == Some((request.0.id(), world.world, world.generation())))
    else {
        textures.effect_clock.pause();
        return;
    };
    if stopped
        || cameras.single().is_err()
        || console.is_some_and(|console| console.open)
        || !state.is_some_and(|state| *state.get() == AppState::InGame)
        || logbook.is_some_and(|screen| screen.suspended())
        || textures.effects_error.is_some()
    {
        textures.effect_clock.pause();
        return;
    }
    let (time, clock, presentation) = timing;
    let elapsed =
        crate::presentation_clock::PresentationClock::elapsed(presentation.as_deref(), &time);
    let Some(step) = clock
        .as_ref()
        .and_then(|sample| sample.presentation_step(world.generation()))
    else {
        textures.effect_clock.pause();
        return;
    };
    let ticks = step.nominal_ticks;
    match textures.effect_clock.advance(elapsed) {
        Ok(draws) => {
            let effects = world.prepare_environment_effect_draws(ticks, draws);
            if draws != 0 {
                live.pending = Some(
                    effects
                        .map(|(slot, effect)| (slot, effect.clone()))
                        .collect(),
                );
                match world.actor_flat_draws() {
                    Ok(flats) => live.pending_flats = Some(flats),
                    Err(error) => {
                        textures.effects_error = Some(format!(
                            "world {} actor flat submission: {error:#}",
                            world.world
                        ))
                    }
                }
            }
        }
        Err(error) => {
            textures.effects_error = Some(format!(
                "world {} effect submission: {error:#}",
                world.world
            ))
        }
    }
}

// Admission runs after ordinary image requests and retains each packet's selected handle.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    mut commands: Commands,
    timing: (
        Res<Time<Real>>,
        Option<Res<crate::presentation_clock::PresentationClock>>,
    ),
    state: Option<Res<State<AppState>>>,
    logbook: Option<Res<cyrus_game::logbook::LogbookScreen>>,
    request: Res<EnvironmentRequest>,
    environments: Res<Assets<Environment>>,
    fidelity: Res<Fidelity>,
    distance: Res<super::RenderDistance>,
    mut actors: Option<ResMut<ScriptActors>>,
    mut textures: ResMut<TextureAnimations>,
    mut live: ResMut<LiveEffects>,
    assets: (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<SceneMaterial>>,
        ResMut<Assets<Image>>,
    ),
    mut camera: Query<
        (
            &GlobalTransform,
            &Projection,
            Option<&mut bevy::pbr::ScreenSpaceTransmission>,
        ),
        With<ViewCamera>,
    >,
    flats: Query<(), With<crate::flat::Flat>>,
) {
    let (time, presentation) = timing;
    let (mut meshes, mut materials, mut images) = assets;
    let world = actors.as_mut().and_then(|actors| actors.current.as_mut());
    let (
        Some(world),
        Some(owner),
        Ok((camera, projection, transmission)),
        Some(environment),
        Some(clipping),
    ) = (
        world,
        live.owner,
        camera.single_mut(),
        environments.get(&request.0),
        distance.effective(&fidelity),
    )
    else {
        textures.effect_clock.pause();
        return;
    };
    if owner != (request.0.id(), world.world, world.generation())
        || !state.is_some_and(|state| *state.get() == AppState::InGame)
        || logbook.is_some_and(|screen| screen.suspended())
        || textures.effects_error.is_some()
    {
        textures.effect_clock.pause();
        return;
    }
    let elapsed =
        crate::presentation_clock::PresentationClock::elapsed(presentation.as_deref(), &time);
    let result = (|| -> anyhow::Result<()> {
        let projection = projection.get_clip_from_view();
        let scale = Vec2::new(projection.x_axis.x, projection.y_axis.y);
        if let Some(effects) = live.pending.take() {
            let now = cyrus_game::clock::bios_tick(elapsed);
            let mut retained = Vec::new();
            for (index, effect) in effects {
                let position = exported_position(effect.position);
                let center = camera.affine().inverse().transform_point3(position) * 20.0;
                let view = EffectDrawView {
                    center: [center.x, center.y, -center.z],
                    projection_scale: scale,
                    clipping,
                };
                if let Some(packet) = textures.request_effect_draw(
                    request.0.id(),
                    effect.descriptor,
                    effect.frame,
                    &view,
                    now,
                )? {
                    retained.push(RetainedEffectDraw {
                        slot: index,
                        descriptor: effect.descriptor,
                        position: effect.position,
                        frame: packet.frame,
                    });
                }
            }
            textures.effect_draws = retained;
            live.repaint = true;
        }
        live.repaint |= std::mem::take(&mut textures.effects_repaint);
        if let Some(flats) = live.pending_flats.take() {
            let now = cyrus_game::clock::bios_tick(elapsed);
            let mut packets = Vec::new();
            for flat in flats {
                let texture = flat.descriptor as u16;
                if !textures.effect_image_prepared(texture) {
                    if live.reported_flats.insert(texture) {
                        error!(
                            "world {}: actor flat texture {texture:#06x} has no prepared image; its flats are not drawn (re-export the world)",
                            owner.1
                        );
                    }
                    continue;
                }
                let position = exported_position(flat.position);
                let center = camera.affine().inverse().transform_point3(position) * 20.0;
                let view = EffectDrawView {
                    center: [center.x, center.y, -center.z],
                    projection_scale: scale,
                    clipping,
                };
                let explicit = if flat.descriptor & 0x20000 != 0 {
                    Some(usize::try_from(flat.frame).map_err(|_| {
                        anyhow::anyhow!(
                            "actor flat texture {texture}: negative frame {}",
                            flat.frame
                        )
                    })?)
                } else {
                    None
                };
                if let Some(packet) = textures.request_actor_flat_draw(
                    request.0.id(),
                    texture,
                    explicit,
                    &view,
                    now,
                )? {
                    packets.push((position, packet));
                }
            }
            live.flat_packets = packets;
            live.repaint = true;
        }
        if !live.repaint {
            for slot in live.slots.iter().chain(&live.flat_slots) {
                let transform =
                    Transform::from_translation(slot.position).with_rotation(camera.rotation());
                commands
                    .entity(slot.entity)
                    .insert((transform, GlobalTransform::from(transform)));
            }
            if let Some(mut transmission) = transmission {
                transmission.steps =
                    (flats.iter().len() + textures.effect_draws.len() + live.flat_packets.len())
                        .max(1);
            }
            return Ok(());
        }
        for (index, slot) in live.slots.iter().chain(&live.flat_slots).enumerate() {
            commands.entity(slot.entity).insert(EffectBillboard {
                slot: index,
                alpha: 255,
                admitted: false,
            });
        }
        let fog = super::fog_extension(environment, &fidelity, clipping);
        let lights = live.lights.update(
            request.0.id(),
            &world.object_lights(),
            camera.translation(),
            &mut images,
        );
        let mut admitted = 0;
        for retained in &textures.effect_draws {
            let index = retained.slot;
            let position = exported_position(retained.position);
            let packet = textures.retained_effect_draw(retained.descriptor, retained.frame)?;
            let slot = live
                .slots
                .get_mut(index)
                .ok_or_else(|| anyhow::anyhow!("effect slot {index}: presentation slot absent"))?;
            let shape = (packet.half_extents, packet.brightness, packet.flags);
            if slot.shape != Some(shape) {
                meshes.insert(slot.mesh.id(), geometry::mesh(&packet)?)?;
                slot.shape = Some(shape);
            }
            materials.insert(
                slot.material.id(),
                geometry::material(&packet, &fog, &lights),
            )?;
            slot.position = position;
            let transform = Transform::from_translation(position).with_rotation(camera.rotation());
            commands.entity(slot.entity).insert((
                transform,
                GlobalTransform::from(transform),
                EffectBillboard {
                    slot: index,
                    alpha: packet.alpha,
                    admitted: true,
                },
            ));
            admitted += 1;
        }
        let pool = live.slots.len();
        let packets = std::mem::take(&mut live.flat_packets);
        while live.flat_slots.len() < packets.len() {
            let placeholder = EffectDraw {
                texture: Handle::default(),
                frame: 0,
                half_extents: Vec2::ZERO,
                radius: 0.0,
                brightness: 255,
                alpha: 255,
                flags: 1,
            };
            let mesh = meshes.add(geometry::mesh(&placeholder)?);
            let material =
                materials.add(geometry::material(&placeholder, &fog, &Handle::default()));
            let entity = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::default(),
                    GlobalTransform::default(),
                    Visibility::Visible,
                    NoFrustumCulling,
                    EffectBillboard {
                        slot: pool + live.flat_slots.len(),
                        alpha: 255,
                        admitted: false,
                    },
                ))
                .id();
            live.flat_slots.push(Slot {
                entity,
                mesh,
                material,
                shape: None,
                position: Vec3::ZERO,
            });
        }
        for (index, (position, packet)) in packets.iter().enumerate() {
            let slot = &mut live.flat_slots[index];
            let shape = (packet.half_extents, packet.brightness, packet.flags);
            if slot.shape != Some(shape) {
                meshes.insert(slot.mesh.id(), geometry::mesh(packet)?)?;
                slot.shape = Some(shape);
            }
            materials.insert(
                slot.material.id(),
                geometry::material(packet, &fog, &lights),
            )?;
            slot.position = *position;
            let transform = Transform::from_translation(*position).with_rotation(camera.rotation());
            commands.entity(slot.entity).insert((
                transform,
                GlobalTransform::from(transform),
                EffectBillboard {
                    slot: pool + index,
                    alpha: packet.alpha,
                    admitted: true,
                },
            ));
        }
        live.flat_packets = packets;
        if let Some(mut transmission) = transmission {
            transmission.steps = (flats.iter().len() + admitted + live.flat_packets.len()).max(1);
        }
        live.repaint = false;
        Ok(())
    })();
    if let Err(error) = result {
        let message = format!("world {} effect draw: {error:#}", owner.1);
        error!("{message}");
        textures.effects_error = Some(message);
    }
}

fn exported_position([x, y, z]: [i32; 3]) -> Vec3 {
    Vec3::new(
        -(x as f32) / 20.0,
        -(y as f32) / 20.0,
        -((16_777_215.0 - f64::from(z) * 256.0) as f32) * (1.0 / 5120.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn installed_world_binding_waits_for_readiness_and_replaces_generation() -> anyhow::Result<()> {
        use cyrus_assets::{install::GameInstall, native::Files, script::Scripts};
        use cyrus_game::script::ScriptWorld;
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        let Ok(install) = GameInstall::open(&root) else {
            eprintln!("skipped: no install (live effect world binding)");
            return Ok(());
        };
        let files = Files::open(&install)?;
        let environment: Environment = serde_json::from_slice(
            &cyrus_assets::game::prepare(&files, "START.environment")?["START.environment"],
        )?;
        let scripts: Scripts = serde_json::from_slice(
            &cyrus_assets::game::prepare(&files, "START.scripts")?["START.scripts"],
        )?;
        let capacity = environment
            .effects
            .as_ref()
            .expect("installed effects")
            .capacity as usize;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Environment>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<SceneMaterial>>()
            .init_resource::<EnvironmentRequest>()
            .init_resource::<TextureAnimations>()
            .init_resource::<Fidelity>()
            .insert_resource(super::super::RenderDistance {
                original: Some(environment.clipping),
                override_far: None,
            })
            .insert_resource(State::new(AppState::InGame));
        let handle: Handle<Environment> = app
            .world()
            .resource::<AssetServer>()
            .load("START.environment");
        let mut textures = TextureAnimations::prepare(app.world_mut(), handle.id(), &[])?;
        textures.prepare_effects(
            environment.effects.as_ref().unwrap(),
            &mut app.world_mut().resource_mut::<Assets<Image>>(),
        )?;
        app.world_mut()
            .resource_mut::<Assets<Environment>>()
            .insert(handle.id(), environment)?;
        app.insert_resource(textures)
            .insert_resource(EnvironmentRequest(handle));
        let prepared = PreparedWorld {
            requested_name: "START".into(),
            world: Some(scripts.world),
            animations: Handle::default(),
            scripts: Handle::default(),
            player_path: "START.player".into(),
            scene_path: "START.glb".into(),
            environment_path: "START.environment".into(),
        };
        let mut actors = ScriptActors::default();
        actors.current = Some(ScriptWorld::prepare(&scripts, 10)?);
        app.insert_resource(actors).insert_resource(prepared);
        app.world_mut().spawn((
            ViewCamera,
            GlobalTransform::default(),
            Projection::Perspective(PerspectiveProjection::default()),
        ));
        register(&mut app);
        app.update();
        assert!(app.world().resource::<LiveEffects>().slots.is_empty());
        assert!(
            app.world()
                .resource::<ScriptActors>()
                .current
                .as_ref()
                .unwrap()
                .environmental_effects()
                .is_none()
        );
        app.insert_resource(crate::ReadyScene(Handle::default()));
        app.update();
        // A prepared world without an admitted clock sample cannot submit effects.
        assert!(app.world().resource::<LiveEffects>().pending.is_none());
        let untouched = serde_json::to_value(EffectDrawClock::default())?;
        assert_eq!(
            serde_json::to_value(&app.world().resource::<TextureAnimations>().effect_clock)?,
            untouched
        );
        // Repaint a retained packet without another pool traversal or image request.
        let (descriptor, image_name) = {
            let environments = app.world().resource::<Assets<Environment>>();
            let environment = environments
                .get(&app.world().resource::<EnvironmentRequest>().0)
                .unwrap();
            let (&id, image) = environment
                .effects
                .as_ref()
                .unwrap()
                .images
                .iter()
                .next()
                .unwrap();
            (u32::from(id), image.name.clone())
        };
        let environment_id = app.world().resource::<EnvironmentRequest>().0.id();
        {
            let mut textures = app.world_mut().resource_mut::<TextureAnimations>();
            textures.hold(&image_name, Some(0))?;
            textures.effect_draws = vec![RetainedEffectDraw {
                slot: 0,
                descriptor,
                position: [100, -200, 65500],
                frame: 0,
            }];
        }
        let saved = app
            .world()
            .resource::<TextureAnimations>()
            .snapshot_playback()?;
        let before = serde_json::to_value(&saved)?;
        let environment_source = {
            let environments = app.world().resource::<Assets<Environment>>();
            environments
                .get(&app.world().resource::<EnvironmentRequest>().0)
                .unwrap()
                .effects
                .clone()
                .unwrap()
        };
        let mut replacement = TextureAnimations::prepare(app.world_mut(), environment_id, &[])?;
        replacement.prepare_effects(
            &environment_source,
            &mut app.world_mut().resource_mut::<Assets<Image>>(),
        )?;
        app.insert_resource(TextureAnimations::from_snapshot(replacement, saved)?);
        use bevy::ecs::system::RunSystemOnce;
        app.world_mut().run_system_once(draw).unwrap();
        let slot = &app.world().resource::<LiveEffects>().slots[0];
        assert!(
            app.world()
                .get::<EffectBillboard>(slot.entity)
                .unwrap()
                .admitted
        );
        assert_eq!(
            app.world()
                .get::<GlobalTransform>(slot.entity)
                .unwrap()
                .translation(),
            exported_position([100, -200, 65500])
        );
        assert_eq!(
            serde_json::to_value(
                app.world()
                    .resource::<TextureAnimations>()
                    .snapshot_playback()?
            )?,
            before
        );
        let first: Vec<_> = app
            .world()
            .resource::<LiveEffects>()
            .slots
            .iter()
            .map(|slot| slot.entity)
            .collect();
        assert_eq!(first.len(), capacity);
        assert!(
            app.world()
                .resource::<ScriptActors>()
                .current
                .as_ref()
                .unwrap()
                .environmental_effects()
                .is_some()
        );
        assert!(
            app.world()
                .resource::<TextureAnimations>()
                .effects_error
                .is_none()
        );
        app.world_mut().resource_mut::<ScriptActors>().current =
            Some(ScriptWorld::prepare(&scripts, 11)?);
        app.world_mut().run_system_once(bind).unwrap();
        app.world_mut().run_system_once(draw).unwrap();
        assert!(
            first
                .iter()
                .all(|entity| app.world().get_entity(*entity).is_err())
        );
        assert_eq!(app.world().resource::<LiveEffects>().slots.len(), capacity);
        assert!(
            app.world()
                .resource::<TextureAnimations>()
                .effect_draws
                .is_empty()
        );
        assert!(
            app.world()
                .resource::<LiveEffects>()
                .slots
                .iter()
                .all(|slot| !app
                    .world()
                    .get::<EffectBillboard>(slot.entity)
                    .unwrap()
                    .admitted)
        );
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), capacity);
        assert_eq!(
            app.world().resource::<Assets<SceneMaterial>>().len(),
            capacity
        );
        assert!(
            app.world()
                .resource::<TextureAnimations>()
                .effects_error
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn submission_phase_runs_with_the_real_player_and_script_plugins() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .init_state::<AppState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<EnvironmentRequest>()
            .init_resource::<Assets<Environment>>()
            .init_resource::<Assets<cyrus_assets::animation::Animations>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<SceneMaterial>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<TextureAnimations>()
            .init_resource::<Fidelity>()
            .init_resource::<super::super::RenderDistance>()
            .add_plugins((
                cyrus_game::player::PlayerPlugin,
                cyrus_game::script::ScriptPlugin,
            ));
        register(&mut app);
        app.update();
        app.update();
        assert!(app.world().resource::<LiveEffects>().owner.is_none());
        assert!(
            app.world()
                .resource::<TextureAnimations>()
                .effects_error
                .is_none()
        );
    }

    #[test]
    fn schedule_runs_without_a_world_and_releases_previous_gpu_slots() -> anyhow::Result<()> {
        let mut app = App::new();
        app.init_resource::<Time<Real>>()
            .init_resource::<EnvironmentRequest>()
            .init_resource::<Assets<Environment>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<SceneMaterial>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<TextureAnimations>()
            .init_resource::<Fidelity>()
            .init_resource::<super::super::RenderDistance>();
        register(&mut app);
        let packet = EffectDraw {
            texture: Handle::default(),
            frame: 0,
            half_extents: Vec2::ONE,
            radius: 1.0,
            brightness: 255,
            alpha: 255,
            flags: 1,
        };
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(geometry::mesh(&packet)?);
        let entity = app.world_mut().spawn_empty().id();
        app.world_mut()
            .resource_mut::<LiveEffects>()
            .slots
            .push(Slot {
                entity,
                mesh: mesh.clone(),
                material: Handle::default(),
                shape: None,
                position: Vec3::ZERO,
            });
        app.world_mut().resource_mut::<LiveEffects>().owner =
            Some((Handle::<Environment>::default().id(), 1, 1));
        app.update();
        assert!(app.world().get_entity(entity).is_err());
        assert!(app.world().resource::<Assets<Mesh>>().get(&mesh).is_none());
        assert!(app.world().resource::<LiveEffects>().slots.is_empty());
        assert!(
            app.world()
                .resource::<TextureAnimations>()
                .effects_error
                .is_none()
        );
        app.update();
        Ok(())
    }
}
