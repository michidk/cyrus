//! Texture and terrain continuation published with the shared presentation epoch.
use super::*;
use bevy::prelude::{Commands, Mesh, Mesh3d};
use cyrus_assets::environment::Environment;
use cyrus_render::fog::{TextureAnimations, TexturePlaybackSnapshot, TextureSourcesSnapshot};
use cyrus_render::{TerrainMeshReplacement, TerrainPlaybackSnapshot, WaterPlayback};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct RenderPlaybackSnapshot {
    scene_source: cyrus_render::persistence::SceneSourceIdentity,
    textures: TexturePlaybackSnapshot,
    texture_sources: TextureSourcesSnapshot,
    terrain: TerrainPlaybackSnapshot,
    entities: Vec<u64>,
    terrain_paths: Vec<Vec<usize>>,
    distance: cyrus_render::fog::RenderDistance,
}

/// Independently bound replacement resources. Bindings follow saved terrain ordinals.
pub struct PreparedRenderPlayback {
    pub textures: TextureAnimations,
    pub terrain: WaterPlayback,
    pub terrain_bindings: Vec<Entity>,
}

pub struct RenderPlaybackCandidate {
    scene_source: cyrus_render::persistence::SceneSourceIdentity,
    textures: TextureAnimations,
    texture_sources: TextureSourcesSnapshot,
    terrain: WaterPlayback,
    meshes: Vec<TerrainMeshReplacement>,
    roots: Vec<Entity>,
    distance: cyrus_render::fog::RenderDistance,
}

impl RenderPlaybackSnapshot {
    pub(super) fn capture(world: &World, model_root: Option<Entity>) -> Result<Self> {
        let root = world
            .try_query_filtered::<Entity, With<cyrus_game::script::ScriptScene>>()
            .context("session scene bindings absent")?
            .single(world)
            .context("session texture source requires one active scene root")?;
        let roots: Vec<_> = std::iter::once(root).chain(model_root).collect();
        let request =
            resource::<cyrus_render::environment::EnvironmentRequest>(world, "environment")?;
        let environment = resource::<Assets<Environment>>(world, "environment assets")?
            .get(&request.0)
            .context("session render environment absent")?;
        let terrain = resource::<WaterPlayback>(world, "terrain playback")?;
        let distance =
            resource::<cyrus_render::fog::RenderDistance>(world, "render distance")?.clone();
        validate_distance(&distance)?;
        Ok(Self {
            distance,
            scene_source: cyrus_render::persistence::SceneSourceIdentity::capture(world, root)?,
            texture_sources: resource::<TextureAnimations>(world, "texture playback")?
                .snapshot_sources(world, &roots)?,
            textures: resource::<TextureAnimations>(world, "texture playback")?
                .snapshot_playback()?,
            terrain: terrain.snapshot_playback(environment, world)?,
            terrain_paths: terrain
                .terrain_entities()
                .into_iter()
                .map(|entity| hierarchy_path(world, root, entity))
                .collect::<Result<_>>()?,
            entities: terrain
                .terrain_entities()
                .into_iter()
                .map(Entity::to_bits)
                .collect(),
        })
    }

    pub(super) fn terrain_bindings(&self, world: &World, root: Entity) -> Result<Vec<Entity>> {
        self.scene_source.validate(world, root)?;
        ensure!(
            self.terrain_paths.len() == self.entities.len(),
            "session terrain path census differs"
        );
        self.terrain_paths
            .iter()
            .map(|path| resolve_path(world, root, path))
            .collect()
    }

    pub(super) fn clipping(&self, fidelity: &cyrus_game::fidelity::Fidelity) -> Result<[f32; 2]> {
        validate_distance(&self.distance)?;
        self.distance
            .effective(fidelity)
            .context("session render clipping absent")
    }

    pub(super) fn entities(&self) -> &[u64] {
        &self.entities
    }

    pub(super) fn restore(
        self,
        prepared: PreparedRenderPlayback,
        environment: &Environment,
        world: &World,
        roots: Vec<Entity>,
    ) -> Result<RenderPlaybackCandidate> {
        validate_distance(&self.distance)?;
        cyrus_render::fog::validate_scene_materials(world, &roots)?;
        ensure!(
            self.entities.len() == prepared.terrain_bindings.len(),
            "session terrain ordinal census differs"
        );
        ensure!(
            self.entities
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.entities.len(),
            "session saved terrain entities duplicated"
        );
        ensure!(
            self.terrain_bindings(world, roots[0])? == prepared.terrain_bindings,
            "session terrain bindings differ from saved hierarchy paths"
        );
        self.texture_sources
            .validate(&prepared.textures, world, &roots)?;
        let textures = TextureAnimations::from_snapshot(prepared.textures, self.textures)?;
        let (terrain, meshes) = WaterPlayback::from_snapshot(
            prepared.terrain,
            environment,
            self.terrain,
            &prepared.terrain_bindings,
            resource::<Assets<Mesh>>(world, "mesh assets")?,
        )?
        .into_parts();
        Ok(RenderPlaybackCandidate {
            distance: self.distance,
            scene_source: self.scene_source,
            textures,
            texture_sources: self.texture_sources,
            terrain,
            meshes,
            roots,
        })
    }
}

impl RenderPlaybackCandidate {
    pub(super) fn validate(
        &self,
        world: &World,
        scene: &cyrus_render::persistence::SceneCandidate,
    ) -> Result<()> {
        self.scene_source.validate(world, scene.root())?;
        cyrus_render::fog::validate_scene_materials(world, &self.roots)?;
        self.texture_sources
            .validate(&self.textures, world, &self.roots)?;
        let assets = resource::<Assets<Mesh>>(world, "mesh assets")?;
        let old = resource::<WaterPlayback>(world, "live terrain playback")?.terrain_entities();
        let mut handles = std::collections::HashSet::new();
        for replacement in &self.meshes {
            scene.validate_map_entity(world, replacement.entity)?;
            entities::validate_retirement_ancestry(world, replacement.entity)?;
            ensure!(
                world
                    .get::<Mesh3d>(replacement.entity)
                    .is_some_and(|mesh| mesh.0 == replacement.handle)
                    && assets.get(&replacement.handle).is_some(),
                "session terrain replacement {:?}: prepared private mesh disappeared",
                replacement.entity
            );
            ensure!(
                handles.insert(replacement.handle.id()),
                "session terrain private mesh handles alias"
            );
            ensure!(
                !old.iter().any(|entity| world
                    .get::<Mesh3d>(*entity)
                    .is_some_and(|mesh| mesh.0 == replacement.handle)),
                "session terrain replacement shares a live mesh"
            );
        }
        Ok(())
    }

    pub(super) fn queue(self, commands: &mut Commands) {
        commands.queue(move |world: &mut World| {
            for replacement in self.meshes {
                *world
                    .resource_mut::<Assets<Mesh>>()
                    .get_mut(&replacement.handle)
                    .expect("validated private terrain mesh") = replacement.mesh;
                world.entity_mut(replacement.entity).insert((
                    Mesh3d(replacement.handle),
                    replacement.visibility,
                    cyrus_render::TerrainTextureAdmission(replacement.texture_admitted),
                ));
            }
            world.insert_resource(self.distance);
            world.insert_resource(self.terrain);
            world.insert_resource(self.textures);
        });
    }
}

fn hierarchy_path(world: &World, root: Entity, mut entity: Entity) -> Result<Vec<usize>> {
    let mut path = Vec::new();
    let mut seen = std::collections::HashSet::new();
    while entity != root {
        ensure!(
            seen.insert(entity),
            "session terrain hierarchy contains a cycle"
        );
        let parent = world
            .get::<bevy::prelude::ChildOf>(entity)
            .context("session terrain entity lies outside scene root")?
            .parent();
        let index = world
            .get::<bevy::prelude::Children>(parent)
            .and_then(|children| children.iter().position(|child| *child == entity))
            .context("session terrain parent lacks child")?;
        path.push(index);
        entity = parent;
    }
    path.reverse();
    Ok(path)
}

fn resolve_path(world: &World, mut root: Entity, path: &[usize]) -> Result<Entity> {
    for &index in path {
        root = *world
            .get::<bevy::prelude::Children>(root)
            .and_then(|children| children.get(index))
            .context("session terrain hierarchy path missing in replacement scene")?;
    }
    Ok(root)
}

fn validate_distance(distance: &cyrus_render::fog::RenderDistance) -> Result<()> {
    let [near, far] = distance
        .original
        .context("session render clipping absent")?;
    ensure!(
        near.is_finite() && far.is_finite() && near > 0.0 && far > near,
        "session render clipping invalid"
    );
    if let Some(value) = distance.override_far {
        ensure!(
            value.is_finite() && value > near,
            "session render clipping override invalid"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::ChildOf;

    #[test]
    fn saved_clipping_preserves_mode_specific_override_and_rejects_invalid_ranges() {
        use cyrus_game::fidelity::{Area, Fidelity, Mode};
        let distance = cyrus_render::fog::RenderDistance {
            original: Some([7.0, 3800.0]),
            override_far: Some(8000.0),
        };
        let restored: cyrus_render::fog::RenderDistance =
            serde_json::from_slice(&serde_json::to_vec(&distance).unwrap()).unwrap();
        validate_distance(&restored).unwrap();
        let mut fidelity = Fidelity::default();
        assert_eq!(restored.effective(&fidelity), Some([7.0, 3800.0]));
        fidelity.set(Area::Fog, Mode::Modern);
        assert_eq!(restored.effective(&fidelity), Some([7.0, 8000.0]));
        assert!(
            validate_distance(&cyrus_render::fog::RenderDistance {
                original: Some([7.0, 3800.0]),
                override_far: Some(6.0),
            })
            .is_err()
        );
    }

    #[test]
    fn terrain_paths_rebind_across_entity_generations_and_reject_missing_nodes() {
        let mut world = World::new();
        let old_root = world.spawn_empty().id();
        let old_branch = world.spawn(ChildOf(old_root)).id();
        let old_mesh = world.spawn(ChildOf(old_branch)).id();
        let path = hierarchy_path(&world, old_root, old_mesh).unwrap();
        assert_eq!(path, vec![0, 0]);
        let new_root = world.spawn_empty().id();
        let new_branch = world.spawn(ChildOf(new_root)).id();
        let new_mesh = world.spawn(ChildOf(new_branch)).id();
        assert_eq!(resolve_path(&world, new_root, &path).unwrap(), new_mesh);
        assert!(hierarchy_path(&world, new_root, old_mesh).is_err());
        world.despawn(new_mesh);
        assert!(resolve_path(&world, new_root, &path).is_err());
    }
}
