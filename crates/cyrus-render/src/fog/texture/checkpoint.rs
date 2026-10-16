//! Replacement of shared texture timelines against an independently prepared image census.
use super::{Playback, TextureAnimations};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Exact decoded frame identities for the bound scene, independent of runtime handles.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextureSourcesSnapshot {
    images: std::collections::BTreeMap<String, Vec<[u8; 32]>>,
}

impl TextureAnimations {
    /// Fingerprint every bound animation frame under the prepared scene root.
    /// Missing CPU image data or incomplete binding rejects instead of weakening identity.
    pub fn snapshot_sources(
        &self,
        world: &bevy::prelude::World,
        roots: &[bevy::prelude::Entity],
    ) -> Result<TextureSourcesSnapshot> {
        use anyhow::Context;
        use bevy::prelude::{Assets, Children, Image};
        let assets = world
            .get_resource::<Assets<Image>>()
            .context("texture image assets absent")?;
        let mut images = std::collections::BTreeMap::new();
        let mut pending = roots.to_vec();
        let mut visited = std::collections::BTreeSet::new();
        while let Some(entity) = pending.pop() {
            ensure!(
                visited.insert(entity),
                "texture scene hierarchy repeats an entity"
            );
            ensure!(
                world.get_entity(entity).is_ok(),
                "texture scene entity {entity:?} absent"
            );
            if let Some(children) = world.get::<Children>(entity) {
                pending.extend(children.iter());
            }
            let binding = world.get::<super::Binding>(entity);
            if world.get::<bevy::prelude::Mesh3d>(entity).is_some()
                && let Some(extras) = world.get::<bevy::gltf::GltfMaterialExtras>(entity)
            {
                let metadata: super::MaterialExtras = serde_json::from_str(&extras.value)
                    .with_context(|| {
                        format!("texture source entity {entity:?}: invalid metadata")
                    })?;
                ensure!(
                    metadata.rgpre_texture_animation.is_none() || binding.is_some(),
                    "texture source entity {entity:?}: authored animation is not bound"
                );
            }
            let Some(binding) = binding else {
                continue;
            };
            let mut frames = Vec::with_capacity(binding.frames.len());
            for (index, handle) in binding.frames.iter().enumerate() {
                let image = assets.get(handle).with_context(|| {
                    format!("texture {} frame {index}: image absent", binding.key)
                })?;
                let data = image.data.as_ref().with_context(|| {
                    format!(
                        "texture {} frame {index}: CPU image bytes absent",
                        binding.key
                    )
                })?;
                let descriptor = &image.texture_descriptor;
                let mut hash = blake3::Hasher::new();
                hash.update(&serde_json::to_vec(&(
                    descriptor.size.width,
                    descriptor.size.height,
                    descriptor.size.depth_or_array_layers,
                    descriptor.mip_level_count,
                    descriptor.sample_count,
                    format!("{:?}", descriptor.dimension),
                    format!("{:?}", descriptor.format),
                    format!("{:?}", image.data_order),
                ))?);
                hash.update(data);
                frames.push(*hash.finalize().as_bytes());
            }
            if let Some(previous) = images.insert(binding.key.clone(), frames.clone()) {
                ensure!(
                    previous == frames,
                    "texture {}: shared frame contents differ",
                    binding.key
                );
            }
        }
        ensure!(
            images.len() == self.states.len()
                && self.states.iter().all(|(key, state)| images
                    .get(key)
                    .is_some_and(|frames| frames.len() == state.count)),
            "texture source census differs from bound playback"
        );
        Ok(TextureSourcesSnapshot { images })
    }
}

impl TextureSourcesSnapshot {
    pub fn validate(
        &self,
        prepared: &TextureAnimations,
        world: &bevy::prelude::World,
        roots: &[bevy::prelude::Entity],
    ) -> Result<()> {
        ensure!(
            *self == prepared.snapshot_sources(world, roots)?,
            "prepared texture frame content differs"
        );
        Ok(())
    }
}

/// Opaque shared-image timeline state, without image bytes or runtime asset IDs.
/// The session separately validates installed sources and retains the presentation
/// clock epoch used by absolute BIOS-tick deadlines.
#[derive(Clone, Serialize, Deserialize)]
pub struct TexturePlaybackSnapshot {
    version: u32,
    images: Vec<ImagePlayback>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ImagePlayback {
    image: String,
    count: usize,
    delay: u32,
    frame: Option<usize>,
    deadline: u32,
    held: bool,
}

impl TextureAnimations {
    /// Capture after scene texture binding has completed. The owning session
    /// captures its presentation clock at the same completed render boundary.
    pub fn snapshot_playback(&self) -> Result<TexturePlaybackSnapshot> {
        ensure!(
            self.environment.is_some(),
            "texture playback environment is not bound"
        );
        let mut images: Vec<_> = self
            .states
            .iter()
            .map(|(image, state)| ImagePlayback {
                image: image.clone(),
                count: state.count,
                delay: state.delay,
                frame: state.playback.frame,
                deadline: state.playback.deadline,
                held: state.held,
            })
            .collect();
        images.sort_by(|a, b| a.image.cmp(&b.image));
        Ok(TexturePlaybackSnapshot { version: 1, images })
    }

    /// Consume an off-live, fully bound replacement. Validate its exact prepared
    /// image census before restoring frames, deadlines and inspection holds.
    /// Construction neither advances a timeline nor changes a live resource.
    /// Prepared image bytes and the shared clock are validated by the session.
    pub fn from_snapshot(mut prepared: Self, snapshot: TexturePlaybackSnapshot) -> Result<Self> {
        ensure!(
            snapshot.version == 1,
            "texture playback snapshot version {} unsupported",
            snapshot.version
        );
        ensure!(
            prepared.environment.is_some(),
            "texture replacement environment is not bound"
        );
        ensure!(
            snapshot.images.len() == prepared.states.len(),
            "texture replacement image census differs"
        );
        let mut previous: Option<&str> = None;
        for saved in &snapshot.images {
            ensure!(
                previous.is_none_or(|previous| previous < saved.image.as_str()),
                "texture {}: duplicate or unordered image",
                saved.image
            );
            previous = Some(&saved.image);
            let state = prepared.states.get(&saved.image).ok_or_else(|| {
                anyhow::anyhow!("texture {}: missing prepared image", saved.image)
            })?;
            ensure!(
                saved.count > 0
                    && saved.delay > 0
                    && saved.count == state.count
                    && saved.delay == state.delay,
                "texture {}: prepared frame count or delay differs",
                saved.image
            );
            ensure!(
                saved.frame.is_none_or(|frame| frame < saved.count),
                "texture {}: frame outside prepared range",
                saved.image
            );
            ensure!(
                !saved.held || saved.frame.is_some(),
                "texture {}: held image has no frame",
                saved.image
            );
            ensure!(
                saved.frame.is_some() || saved.deadline == 0,
                "texture {}: unrequested image has a deadline",
                saved.image
            );
        }
        for saved in snapshot.images {
            let state = prepared
                .states
                .get_mut(&saved.image)
                .expect("validated texture census");
            state.playback = Playback {
                frame: saved.frame,
                deadline: saved.deadline,
            };
            state.held = saved.held;
        }
        prepared.restored_binding = true;
        Ok(prepared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::{Environment, EnvironmentRequest};
    use crate::fog::texture::{ImageState, bind};
    use bevy::prelude::*;
    use serde_json::json;

    fn prepared() -> TextureAnimations {
        TextureAnimations {
            environment: Some(Handle::<Environment>::default().id()),
            states: [
                (
                    "a00000".into(),
                    ImageState {
                        count: 4,
                        delay: 2,
                        playback: Playback::default(),
                        held: false,
                    },
                ),
                (
                    "b00000".into(),
                    ImageState {
                        count: 1,
                        delay: 1,
                        playback: Playback::default(),
                        held: false,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            restored_binding: false,
        }
    }

    #[test]
    fn source_identity_uses_frame_bytes_and_rejects_missing_census() -> anyhow::Result<()> {
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        let image = world.resource_mut::<Assets<Image>>().add(Image::default());
        let root = world.spawn_empty().id();
        let first = world
            .spawn((
                ChildOf(root),
                super::super::Binding {
                    key: "a00000".into(),
                    frames: vec![image.clone(); 4],
                },
            ))
            .id();
        world.spawn((
            ChildOf(root),
            super::super::Binding {
                key: "b00000".into(),
                frames: vec![image.clone()],
            },
        ));
        let owner = prepared();
        let saved = owner.snapshot_sources(&world, &[root])?;
        saved.validate(&owner, &world, &[root])?;
        let duplicate = world.resource_mut::<Assets<Image>>().add(Image::default());
        world
            .get_mut::<super::super::Binding>(first)
            .unwrap()
            .frames = vec![duplicate.clone(); 4];
        saved.validate(&owner, &world, &[root])?;
        world
            .resource_mut::<Assets<Image>>()
            .get_mut(&duplicate)
            .unwrap()
            .data
            .as_mut()
            .unwrap()[0] ^= 1;
        assert!(saved.validate(&owner, &world, &[root]).is_err());
        world
            .resource_mut::<Assets<Image>>()
            .get_mut(&duplicate)
            .unwrap()
            .data
            .as_mut()
            .unwrap()[0] ^= 1;
        saved.validate(&owner, &world, &[root])?;
        world.entity_mut(first).remove::<super::super::Binding>();
        assert!(saved.validate(&owner, &world, &[root]).is_err());
        Ok(())
    }

    #[test]
    fn replacement_keeps_held_unrequested_and_deadline_continuation() -> anyhow::Result<()> {
        let mut live = prepared();
        live.states.get_mut("a00000").unwrap().request(100);
        live.states.get_mut("a00000").unwrap().request(102);
        live.hold("a00000", Some(3))?;
        let saved = serde_json::to_value(live.snapshot_playback()?)?;
        let mut restored =
            TextureAnimations::from_snapshot(prepared(), serde_json::from_value(saved.clone())?)?;
        assert_eq!(serde_json::to_value(restored.snapshot_playback()?)?, saved);
        for owner in [&mut live, &mut restored] {
            owner.states.get_mut("a00000").unwrap().request(1000);
            assert_eq!(owner.snapshot()[0].frame, Some(3));
            assert!(owner.snapshot()[0].held);
            assert_eq!(owner.snapshot()[1].frame, None);
            owner.hold("a00000", None)?;
            owner.states.get_mut("a00000").unwrap().request(103);
            assert_eq!(owner.snapshot()[0].frame, Some(3));
            owner.states.get_mut("a00000").unwrap().request(104);
            assert_eq!(owner.snapshot()[0].frame, Some(0));
            owner.states.get_mut("b00000").unwrap().request(104);
            assert_eq!(owner.snapshot()[1].frame, Some(0));
        }
        assert_eq!(
            serde_json::to_value(restored.snapshot_playback()?)?,
            serde_json::to_value(live.snapshot_playback()?)?
        );
        Ok(())
    }

    #[test]
    fn invalid_candidates_preserve_the_live_timelines() -> anyhow::Result<()> {
        let mut live = prepared();
        live.hold("a00000", Some(2))?;
        let saved = serde_json::to_value(live.snapshot_playback()?)?;
        let mut cases = Vec::new();
        for (field, value) in [
            ("count", json!(5)),
            ("delay", json!(3)),
            ("frame", json!(4)),
            ("image", json!("missing")),
        ] {
            let mut invalid = saved.clone();
            invalid["images"][0][field] = value;
            cases.push(invalid);
        }
        let mut invalid = saved.clone();
        invalid["version"] = json!(2);
        cases.push(invalid);
        let mut invalid = saved.clone();
        invalid["images"][1] = saved["images"][0].clone();
        cases.push(invalid);
        let mut invalid = saved.clone();
        invalid["images"][1]["held"] = json!(true);
        cases.push(invalid);
        let mut invalid = saved.clone();
        invalid["images"][1]["deadline"] = json!(1);
        cases.push(invalid);
        let mut invalid = saved.clone();
        invalid["images"].as_array_mut().unwrap().pop();
        cases.push(invalid);
        for invalid in cases {
            assert!(
                TextureAnimations::from_snapshot(prepared(), serde_json::from_value(invalid)?)
                    .is_err()
            );
            assert_eq!(serde_json::to_value(live.snapshot_playback()?)?, saved);
        }
        assert!(TextureAnimations::default().snapshot_playback().is_err());
        assert!(
            TextureAnimations::from_snapshot(
                TextureAnimations::default(),
                serde_json::from_value(saved)?
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn restored_binding_survives_once_and_later_scene_requests_clear_it() -> anyhow::Result<()> {
        let mut live = prepared();
        live.hold("a00000", Some(3))?;
        let saved = live.snapshot_playback()?;
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .insert_resource(EnvironmentRequest(Handle::default()))
            .insert_resource(TextureAnimations::from_snapshot(prepared(), saved.clone())?)
            .add_systems(Update, bind);
        app.update();
        assert_eq!(
            app.world().resource::<TextureAnimations>().snapshot()[0].frame,
            Some(3)
        );
        app.update();
        assert!(app.world().resource::<TextureAnimations>().snapshot()[0].held);
        app.insert_resource(EnvironmentRequest(Handle::default()));
        app.update();
        assert!(
            app.world()
                .resource::<TextureAnimations>()
                .snapshot()
                .is_empty()
        );
        let other = Assets::<Environment>::default().reserve_handle();
        app.insert_resource(TextureAnimations::from_snapshot(prepared(), saved)?);
        app.insert_resource(EnvironmentRequest(other));
        app.update();
        assert!(
            app.world()
                .resource::<TextureAnimations>()
                .snapshot()
                .is_empty()
        );
        Ok(())
    }
}
