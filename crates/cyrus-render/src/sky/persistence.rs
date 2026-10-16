//! Sky continuation with caller-prepared images and no random draw or scroll advance.
use super::{Sky, SkyMaterial, SkyScrollSync};
use crate::environment::{Environment, EnvironmentRequest};
use anyhow::{Context, Result, ensure};
use bevy::prelude::{Assets, Commands, Entity, Handle, Image, MeshMaterial3d, Vec2, Vec4, World};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct SkySnapshot {
    version: u32,
    environment: String,
    velocity: [f32; 2],
    speed: f32,
    tick: u64,
    plane: [f32; 4],
    fog: [f32; 4],
    scroll: [f32; 4],
    sun_position: [f32; 4],
    sun_color: [f32; 4],
    pending_sync: Option<(u16, [f32; 2])>,
}

/// Image handles prepared from the validated environment by the rendering host.
pub struct SkyImages {
    pub sky: Handle<Image>,
    pub sun: Handle<Image>,
}

pub struct SkyCandidate {
    sky: Sky,
    material: SkyMaterial,
    sync: SkyScrollSync,
}

fn identity(environment: &Environment) -> Result<String> {
    Ok(blake3::hash(&serde_json::to_vec(environment)?)
        .to_hex()
        .to_string())
}

impl SkySnapshot {
    /// Capture exactly one prepared sky, including the background-only sky indoors.
    pub fn capture(world: &World) -> Result<Self> {
        let request = world
            .get_resource::<EnvironmentRequest>()
            .context("sky environment request absent")?;
        let environment = world
            .get_resource::<Assets<Environment>>()
            .context("sky environments absent")?
            .get(&request.0)
            .context("sky environment is not prepared")?;
        let environment = identity(environment)?;
        ensure!(
            world
                .try_query::<&Sky>()
                .map_or(0, |mut query| query.iter(world).count())
                == 1,
            "sky snapshot requires exactly one sky"
        );
        let mut query = world
            .try_query::<(&Sky, &MeshMaterial3d<SkyMaterial>)>()
            .context("sky material components absent")?;
        let (sky, handle) = query.single(world).context("sky material binding absent")?;
        let material = world
            .get_resource::<Assets<SkyMaterial>>()
            .context("sky materials absent")?
            .get(&handle.0)
            .context("sky material not prepared")?;
        let sync = world
            .get_resource::<SkyScrollSync>()
            .context("sky synchronization owner absent")?;
        let saved = Self {
            version: 1,
            environment,
            velocity: sky.velocity.to_array(),
            speed: sky.speed,
            tick: sky.tick,
            plane: material.plane.to_array(),
            fog: material.fog_rgb.to_array(),
            scroll: material.scroll.to_array(),
            sun_position: material.sun_position.to_array(),
            sun_color: material.sun_color.to_array(),
            pending_sync: sync
                .pending
                .map(|(direction, offset)| (direction, offset.to_array())),
        };
        saved.validate()?;
        Ok(saved)
    }

    fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "sky snapshot version unsupported");
        ensure!(
            self.velocity
                .iter()
                .chain(&self.plane)
                .chain(&self.fog)
                .chain(&self.scroll)
                .chain(&self.sun_position)
                .chain(&self.sun_color)
                .all(|v| v.is_finite())
                && self.speed.is_finite(),
            "sky snapshot contains nonfinite state"
        );
        if let Some((direction, offset)) = self.pending_sync {
            SkyScrollSync::default().set(direction, offset)?;
        }
        Ok(())
    }

    /// Inputs must already include the restored world's retained environment edits.
    pub fn restore(self, environment: &Environment, images: SkyImages) -> Result<SkyCandidate> {
        self.validate()?;
        ensure!(
            self.environment == identity(environment)?,
            "sky replacement environment differs"
        );
        Ok(SkyCandidate {
            sky: Sky {
                velocity: Vec2::from_array(self.velocity),
                speed: self.speed,
                tick: self.tick,
            },
            material: SkyMaterial {
                image: images.sky,
                sun_image: images.sun,
                plane: Vec4::from_array(self.plane),
                fog_rgb: Vec4::from_array(self.fog),
                scroll: Vec4::from_array(self.scroll),
                sun_position: Vec4::from_array(self.sun_position),
                sun_color: Vec4::from_array(self.sun_color),
            },
            sync: SkyScrollSync {
                pending: self
                    .pending_sync
                    .map(|(direction, offset)| (direction, Vec2::from_array(offset))),
            },
        })
    }
}

impl SkyCandidate {
    /// Install onto an already prepared sky entity at the complete session commit.
    /// The queued operation creates a private material without altering shared assets.
    pub fn queue(self, commands: &mut Commands, entity: Entity) {
        commands.queue(move |world: &mut World| {
            let material = world
                .resource_mut::<Assets<SkyMaterial>>()
                .add(self.material);
            world
                .entity_mut(entity)
                .insert((self.sky, MeshMaterial3d(material)));
            world.insert_resource(self.sync);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::PresentationClock;
    use bevy::prelude::{App, Real, Time, Update};
    fn environment() -> Environment {
        Environment {
            texture_scale: [1, 1],
            sky: None,
            background: [0; 3],
            clipping: [1.0, 10000.0],
            fog_table: [[0; 4]; 16],
            water: None,
            terrain_lighting: None,
            model_lighting: Default::default(),
            prelighting: Default::default(),
            object_lights: vec![],
        }
    }
    fn app() -> App {
        let mut app = App::new();
        let mut environments = Assets::<Environment>::default();
        let source = environments.add(environment());
        app.insert_resource(environments)
            .insert_resource(EnvironmentRequest(source))
            .init_resource::<Assets<SkyMaterial>>()
            .init_resource::<SkyScrollSync>()
            .init_resource::<Time<Real>>()
            .init_resource::<cyrus_game::fidelity::Fidelity>()
            .add_systems(Update, super::super::animate_sky);
        app
    }
    #[test]
    fn sky_replacement_preserves_next_scroll_with_rebased_host_clock() {
        let mut original = app();
        let material = original
            .world_mut()
            .resource_mut::<Assets<SkyMaterial>>()
            .add(SkyMaterial {
                image: Handle::default(),
                sun_image: Handle::default(),
                plane: Vec4::ZERO,
                fog_rgb: Vec4::ZERO,
                scroll: Vec4::new(255.9, -3.5, 0.0, 0.0),
                sun_position: Vec4::ZERO,
                sun_color: Vec4::ZERO,
            });
        original.world_mut().spawn((
            Sky {
                velocity: Vec2::new(0.7, -0.3),
                speed: -3.0,
                tick: 179,
            },
            MeshMaterial3d(material),
        ));
        original
            .world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs(10));
        let saved = SkySnapshot::capture(original.world_mut()).unwrap();
        let bytes = serde_json::to_vec(&saved).unwrap();
        let mut wrong = environment();
        wrong.background[0] = 1;
        assert!(
            saved
                .clone()
                .restore(
                    &wrong,
                    SkyImages {
                        sky: Handle::default(),
                        sun: Handle::default()
                    }
                )
                .is_err()
        );
        let clock = PresentationClock::snapshot(None, original.world().resource::<Time<Real>>());
        let candidate = saved
            .restore(
                &environment(),
                SkyImages {
                    sky: Handle::default(),
                    sun: Handle::default(),
                },
            )
            .unwrap();
        let mut resumed = app();
        let clock =
            PresentationClock::from_snapshot(clock, resumed.world().resource::<Time<Real>>())
                .unwrap();
        resumed.insert_resource(clock);
        let sky = resumed.world_mut().spawn_empty().id();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        candidate.queue(&mut Commands::new(&mut queue, resumed.world()), sky);
        queue.apply(resumed.world_mut());
        assert_eq!(
            serde_json::to_vec(&SkySnapshot::capture(resumed.world_mut()).unwrap()).unwrap(),
            bytes
        );
        original.update();
        resumed.update();
        assert_eq!(
            serde_json::to_vec(&SkySnapshot::capture(resumed.world_mut()).unwrap()).unwrap(),
            serde_json::to_vec(&SkySnapshot::capture(original.world_mut()).unwrap()).unwrap()
        );
        for app in [&mut original, &mut resumed] {
            app.world_mut()
                .resource_mut::<Time<Real>>()
                .advance_by(std::time::Duration::from_millis(120));
            app.update();
        }
        assert_eq!(
            serde_json::to_vec(&SkySnapshot::capture(resumed.world_mut()).unwrap()).unwrap(),
            serde_json::to_vec(&SkySnapshot::capture(original.world_mut()).unwrap()).unwrap()
        );
    }
}
