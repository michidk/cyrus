//! Player-owned scene inputs shared by native and browser runtimes.
use super::definition::PlayerDefinition;
use anyhow::Context;
use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::prelude::{Asset, Handle, Resource, TypePath};
use cyrus_assets::collision::CollisionScene;
pub use cyrus_assets::player::{MovementSettings, SceneData};
use cyrus_collision::{collision_world::StaticCollision, ground::TerrainGround};

/// Validated player inputs. Loading this asset does not enable unsupported movement.
#[derive(Asset, TypePath)]
pub struct PlayerScene {
    pub world: u32,
    /// Authored model radii in map units; absent entries require re-export.
    pub model_radii: std::collections::BTreeMap<String, f32>,
    /// Installed keyboard defaults, independent of per-user menu preferences.
    pub keyboard_codes: Option<[u8; 8]>,
    pub settings: MovementSettings,
    pub definition: Option<PlayerDefinition>,
    pub terrain: Option<TerrainGround>,
    pub collision: CollisionScene,
    /// Prepared original polygons for runtime instance queries.
    pub static_collision: StaticCollision,
    /// Why the installed player definition cannot be used in this world.
    pub unavailable: Option<String>,
}

impl PlayerScene {
    /// Select static candidates using the current player model's authored radius.
    /// `None` means no active model; a named model missing cache data is an error.
    pub fn candidates(
        &self,
        view: [i32; 2],
        position: [i64; 3],
        model: Option<&str>,
    ) -> anyhow::Result<cyrus_collision::collision_world::StaticCandidates> {
        let radius = model
            .map(|name| {
                self.model_radii.get(name).copied().with_context(|| {
                    format!(
                        "world {} player model {name}: radius absent; re-export world",
                        self.world
                    )
                })
            })
            .transpose()?;
        self.static_collision.candidates(view, position, radius)
    }

    /// Resolve selected support without changing actor surface state or the shared veto.
    pub fn support_surface(
        &self,
        support: &cyrus_collision::ground::SelectedSupport,
        position: [i64; 3],
    ) -> anyhow::Result<u8> {
        match support.source() {
            cyrus_collision::ground::SupportSource::Model { model, face } => {
                self.static_collision.face_surface(model, face)
            }
            cyrus_collision::ground::SupportSource::Terrain => {
                let context = || format!("world {} terrain surface", self.world);
                let terrain = self
                    .terrain
                    .as_ref()
                    .context("missing terrain")
                    .with_context(context)?;
                let assignments = self
                    .collision
                    .texture_surfaces
                    .as_ref()
                    .context("missing SURFACE.INI assignments; re-export world")
                    .with_context(context)?;
                Ok(terrain
                    .material(position, assignments)
                    .with_context(context)?
                    .context("support outside terrain material grid")
                    .with_context(context)?
                    .surface)
            }
        }
    }

    /// Decode the exported player cache, validating terrain before runtime queries.
    pub fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        Self::from_data(serde_json::from_slice::<SceneData>(bytes)?)
    }
}

/// Current world's player inputs, independent of render meshes.
#[derive(Resource)]
pub struct PlayerSceneRequest(pub Handle<PlayerScene>);

impl PlayerScene {
    fn from_data(data: SceneData) -> anyhow::Result<PlayerScene> {
        data.validate()?;
        anyhow::ensure!(
            data.version == 2,
            "world {}: unsupported player schema {}; re-export",
            data.world,
            data.version
        );
        let collision = data.collision.with_context(|| {
            format!(
                "world {}: missing player collision data; re-export",
                data.world
            )
        })?;
        collision
            .validate()
            .with_context(|| format!("world {} collision", data.world))?;
        let static_collision = StaticCollision::new(&collision)
            .with_context(|| format!("world {} static collision", data.world))?;
        let terrain = match (data.heights, data.water, data.terrain_textures) {
            (Some(heights), Some(water), textures) => Some(
                TerrainGround::new(heights, water, textures)
                    .with_context(|| format!("world {} player terrain", data.world))?,
            ),
            (None, None, None) => None,
            _ => anyhow::bail!("world {}: incomplete player terrain", data.world),
        };
        let (definition, unavailable) = match PlayerDefinition::from_metadata(&data.definition) {
            Ok(definition) => (Some(definition), None),
            Err(error) => (
                None,
                Some(format!("world {} player: {error:#}", data.world)),
            ),
        };
        Ok(PlayerScene {
            model_radii: data.model_radii,
            world: data.world,
            keyboard_codes: data.keyboard_codes,
            settings: data.settings,
            definition,
            terrain,
            collision,
            static_collision,
            unavailable,
        })
    }
}

#[derive(Default, TypePath)]
pub(super) struct PlayerSceneLoader;
impl AssetLoader for PlayerSceneLoader {
    type Asset = PlayerScene;
    type Settings = ();
    type Error = anyhow::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> anyhow::Result<PlayerScene> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        PlayerScene::decode(&bytes)
    }

    fn extensions(&self) -> &[&str] {
        &["player"]
    }
}

/// Host binding adapters run after this set to observe the new sidecar request.
#[derive(bevy::prelude::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerSceneLoading;

/// Starts game-owned runtime preparation from the committed world's sidecar path.
pub(super) fn request(
    mut commands: bevy::prelude::Commands,
    assets: bevy::prelude::Res<bevy::prelude::AssetServer>,
    prepared: Option<bevy::prelude::Res<cyrus_world::lifecycle::PreparedWorld>>,
) {
    use bevy::prelude::DetectChanges;
    let Some(prepared) = prepared else {
        return;
    };
    if prepared.is_changed() {
        commands.insert_resource(PlayerSceneRequest(
            assets.load(prepared.player_path.clone()),
        ));
    }
}
