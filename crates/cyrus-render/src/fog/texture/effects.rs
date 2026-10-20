//! Prepared environmental images share the scene's canonical image timelines.
use super::{Binding, ImageState, Playback, TextureAnimations};
use anyhow::{Result, ensure};
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use cyrus_assets::effects::Effects;
use std::collections::BTreeMap;

pub(super) struct PreparedEffects {
    source: String,
    pub(super) bindings: BTreeMap<u16, Binding>,
    metadata: BTreeMap<u16, DrawMetadata>,
    ambient: Option<i16>,
}

struct DrawMetadata {
    size: [u32; 2],
    scale: f32,
    flags: u16,
    alpha: u16,
}

/// Camera-space admission inputs in original map units, with positive Z forward.
/// X and Y have not yet been multiplied by the projection scales.
pub struct EffectDrawView {
    pub center: [f32; 3],
    pub projection_scale: Vec2,
    pub clipping: [f32; 2],
}

/// One bound-admitted billboard request, before polygon processing or raster coverage.
/// Dimensions and radius are original map units; alpha is the original constant input.
pub struct EffectDraw {
    pub texture: Handle<Image>,
    pub frame: usize,
    pub half_extents: Vec2,
    pub radius: f32,
    pub brightness: u8,
    pub alpha: u16,
    pub flags: u16,
}

impl TextureAnimations {
    /// Prepare installed effect frames without requesting or advancing any image.
    /// Call with this owner's exact environment, before binding its logical pool.
    /// All metadata validates before allocation. Rebinding the same source is a no-op;
    /// changed sources reject without modifying frames or shared deadlines.
    /// Effect frames join the same source census as scene-bound animated textures.
    pub fn prepare_effects(&mut self, source: &Effects, assets: &mut Assets<Image>) -> Result<()> {
        ensure!(
            self.environment.is_some(),
            "effect texture environment is not bound"
        );
        source.validate()?;
        let identity = blake3::hash(&serde_json::to_vec(source)?)
            .to_hex()
            .to_string();
        if let Some(prepared) = &self.effects {
            ensure!(
                prepared.source == identity,
                "environment effect image source changed"
            );
            return Ok(());
        }
        let mut metadata = BTreeMap::new();
        let mut shared = BTreeMap::new();
        for (&id, image) in &source.images {
            let delay = self
                .validate_image(&image.name, image.frames.len(), image.delay_ms)
                .map_err(|error| anyhow::anyhow!("environment effect texture {id}: {error:#}"))?;
            let key = image.name.to_ascii_lowercase();
            if let Some(previous) = shared.insert(key.clone(), image) {
                ensure!(
                    previous.size == image.size
                        && previous.frames == image.frames
                        && previous.delay_ms == image.delay_ms,
                    "effect texture {key}: shared source contents differ"
                );
            }
            metadata.insert(id, (key, delay));
        }
        let mut frames = BTreeMap::new();
        for (key, image) in shared {
            let [width, height] = image.size;
            let handles = image
                .frames
                .iter()
                .map(|rgba| {
                    // GLTF scene images reverse rows; effect UVs use that same orientation.
                    let stride = width as usize * 4;
                    let pixels = rgba.chunks_exact(stride).rev().flatten().copied().collect();
                    let mut frame = Image::new(
                        Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                        TextureDimension::D2,
                        pixels,
                        TextureFormat::Rgba8UnormSrgb,
                        RenderAssetUsages::default(),
                    );
                    frame.sampler = ImageSampler::nearest();
                    assets.add(frame)
                })
                .collect::<Vec<_>>();
            frames.insert(key, handles);
        }
        let mut bindings = BTreeMap::new();
        for (id, (key, delay)) in metadata {
            let frames = frames[&key].clone();
            self.states
                .entry(key.clone())
                .or_insert_with(|| ImageState {
                    count: frames.len(),
                    delay,
                    playback: Playback::default(),
                    held: false,
                });
            bindings.insert(id, Binding { key, frames });
        }
        self.effects = Some(PreparedEffects {
            source: identity,
            bindings,
            metadata: source
                .images
                .iter()
                .map(|(&id, image)| {
                    (
                        id,
                        DrawMetadata {
                            size: image.size,
                            scale: f32::from(image.texture_scale) / 256.0,
                            flags: image.flags,
                            alpha: image.alpha,
                        },
                    )
                })
                .collect(),
            ambient: source.ambient_brightness,
        });
        Ok(())
    }

    /// Request a prepared effect image only after its source bound is admitted.
    /// `descriptor` and `lifetime_frame` are retained pool values. This does not
    /// advance lifetime or apply the pool's draw-time movement.
    /// Explicit selection replaces the shared current frame, preserving its deadline;
    /// later automatic requests continue from it. Inspection holds suppress both paths.
    /// Wrong environments, missing sources and invalid explicit frames reject without
    /// changing playback. Bound rejection returns `None` without requesting an image.
    /// Verified: preservation SOUP frame submission and texture-animation contracts.
    pub fn request_effect_draw(
        &mut self,
        environment: AssetId<cyrus_assets::environment::Environment>,
        descriptor: u32,
        lifetime_frame: i32,
        view: &EffectDrawView,
        now: u32,
    ) -> Result<Option<EffectDraw>> {
        ensure!(
            self.is_bound_to(environment),
            "effect draw environment differs from prepared images"
        );
        ensure!(
            self.effects_error.is_none(),
            "effect image preparation failed: {:?}",
            self.effects_error
        );
        ensure!(
            view.center.into_iter().all(f32::is_finite)
                && view.projection_scale.is_finite()
                && view.projection_scale.min_element() > 0.0
                && view.clipping.into_iter().all(f32::is_finite)
                && view.clipping[0] >= 0.0
                && view.clipping[1] > view.clipping[0],
            "effect draw has invalid camera-space admission inputs"
        );
        let id = descriptor as u16;
        if id == 0 || id == u16::MAX {
            return Ok(None);
        }
        let prepared = self
            .effects
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("effect images are not prepared"))?;
        let binding = prepared
            .bindings
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("effect texture {id}: prepared image absent"))?;
        let metadata = &prepared.metadata[&id];
        let alternate = descriptor & 0x20000 != 0;
        let geometry_scale = if alternate {
            0.5
        } else if descriptor & 0x10000 != 0 {
            90.0 / 256.0
        } else {
            1.0
        };
        let radius = crate::flat::bound_radius(metadata.size[0], metadata.size[1], metadata.scale)
            * geometry_scale;
        let [x, y, z] = view.center;
        if super::admission::classify(
            [x * view.projection_scale.x, y * view.projection_scale.y, z],
            radius,
            view.projection_scale,
            view.clipping,
        )
        .is_none()
        {
            return Ok(None);
        }
        let brightness = if alternate {
            prepared.ambient.ok_or_else(|| {
                anyhow::anyhow!("effect texture {id}: world ambient brightness absent")
            })? as u8
        } else {
            255
        };
        let explicit = if alternate {
            let frame = usize::try_from(lifetime_frame).map_err(|_| {
                anyhow::anyhow!("effect texture {id}: negative explicit frame {lifetime_frame}")
            })?;
            ensure!(
                frame < binding.frames.len(),
                "effect texture {id}: explicit frame {frame} outside 0..{}",
                binding.frames.len()
            );
            Some(frame)
        } else {
            None
        };
        let state = self
            .states
            .get_mut(&binding.key)
            .ok_or_else(|| anyhow::anyhow!("effect texture {id}: shared timeline absent"))?;
        if !state.held {
            if let Some(frame) = explicit {
                state.playback.frame = Some(frame);
            } else {
                state.request(now);
            }
        }
        let frame = state.playback.frame.unwrap_or(0);
        Ok(Some(EffectDraw {
            texture: binding.frames[frame].clone(),
            frame,
            half_extents: Vec2::new(metadata.size[0] as f32, metadata.size[1] as f32)
                * 0.5
                * metadata.scale
                * geometry_scale,
            radius,
            brightness,
            alpha: if alternate { 140 } else { metadata.alpha },
            flags: if alternate { 0x201 } else { metadata.flags },
        }))
    }

    /// Preparation failure for the current environment, before any pool is enabled.
    pub fn effect_preparation_error(&self) -> Option<&str> {
        self.effects_error.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_effect_preparation_rejects_capture_and_replacement() -> Result<()> {
        let environment = Some(Handle::<cyrus_assets::environment::Environment>::default().id());
        let healthy = TextureAnimations {
            environment,
            ..default()
        };
        let saved = healthy.snapshot_playback()?;
        let failed = TextureAnimations {
            environment,
            effects_error: Some("invalid installed image".into()),
            ..default()
        };
        assert_eq!(
            failed.effect_preparation_error(),
            Some("invalid installed image")
        );
        assert!(failed.snapshot_playback().is_err());
        assert!(failed.snapshot_sources(&World::new(), &[]).is_err());
        assert!(TextureAnimations::from_snapshot(failed, saved).is_err());
        Ok(())
    }

    #[test]
    fn restored_effect_sources_survive_adoption_and_clear_on_world_replacement() -> Result<()> {
        let environment = Handle::<cyrus_assets::environment::Environment>::default();
        let mut prepared = TextureAnimations {
            environment: Some(environment.id()),
            ..default()
        };
        let source = Effects {
            ambient_brightness: Some(78),
            capacity: 0,
            disabled: false,
            images: BTreeMap::new(),
        };
        let mut assets = Assets::default();
        prepared.prepare_effects(&source, &mut assets)?;
        let saved = prepared.snapshot_playback()?;
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<cyrus_assets::environment::Environment>>()
            .insert_resource(assets)
            .insert_resource(crate::environment::EnvironmentRequest(environment.clone()))
            .insert_resource(TextureAnimations::from_snapshot(prepared, saved)?)
            .add_systems(Update, super::super::bind);
        app.update();
        assert!(
            app.world()
                .resource::<TextureAnimations>()
                .effects
                .is_some()
        );
        app.update();
        assert!(
            app.world()
                .resource::<TextureAnimations>()
                .effects
                .is_some()
        );
        app.insert_resource(crate::environment::EnvironmentRequest(environment));
        app.update();
        assert!(
            app.world()
                .resource::<TextureAnimations>()
                .effects
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn effect_preparation_joins_existing_scene_deadlines_and_validates_shared_bytes() -> Result<()>
    {
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        let root = world.spawn_empty().id();
        let mut owner = TextureAnimations {
            environment: Some(Handle::<cyrus_assets::environment::Environment>::default().id()),
            ..default()
        };
        owner.states.insert(
            "a00000".into(),
            ImageState {
                count: 2,
                delay: 2,
                playback: Playback {
                    frame: Some(1),
                    deadline: 102,
                },
                held: false,
            },
        );
        let source = Effects {
            ambient_brightness: Some(78),
            capacity: 1,
            disabled: false,
            images: BTreeMap::from([(
                0x2500,
                cyrus_assets::effects::EffectImage {
                    name: "A00000".into(),
                    size: [1, 1],
                    texture_scale: 256,
                    delay_ms: 100,
                    flags: 1,
                    alpha: 151,
                    frames: vec![vec![10, 20, 30, 255], vec![40, 50, 60, 255]],
                },
            )]),
        };
        owner.prepare_effects(&source, &mut world.resource_mut::<Assets<Image>>())?;
        assert_eq!(owner.states.len(), 1);
        assert_eq!(owner.states["a00000"].playback.deadline, 102);
        assert_eq!(owner.snapshot()[0].frame, Some(1));
        let binding = &owner.effects.as_ref().unwrap().bindings[&0x2500];
        world.spawn((
            ChildOf(root),
            Binding {
                key: binding.key.clone(),
                frames: binding.frames.clone(),
            },
        ));
        owner.snapshot_sources(&world, &[root])?;
        owner.states.get_mut("a00000").unwrap().request(101);
        assert_eq!(owner.snapshot()[0].frame, Some(1));
        owner.states.get_mut("a00000").unwrap().request(102);
        assert_eq!(owner.snapshot()[0].frame, Some(0));
        assert_eq!(owner.states["a00000"].playback.deadline, 104);
        let mut conflict = source.clone();
        conflict.images.get_mut(&0x2500).unwrap().delay_ms = 200;
        let mut fresh = TextureAnimations {
            environment: owner.environment,
            ..default()
        };
        fresh.states.insert(
            "a00000".into(),
            ImageState {
                count: 2,
                delay: 2,
                playback: Playback::default(),
                held: false,
            },
        );
        let count = world.resource::<Assets<Image>>().len();
        assert!(
            fresh
                .prepare_effects(&conflict, &mut world.resource_mut::<Assets<Image>>())
                .is_err()
        );
        assert_eq!(world.resource::<Assets<Image>>().len(), count);
        Ok(())
    }
}
