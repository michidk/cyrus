//! Player-owned scene inputs shared by native and browser runtimes.
use super::{
    collision_data::CollisionScene, collision_world::StaticCollision, definition::PlayerDefinition,
    ground::TerrainGround,
};
use anyhow::Context;
use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::prelude::{Asset, Handle, Resource, TypePath};
use serde::{Deserialize, Serialize};

/// Installed ordinary-grounded movement settings, in original engine units.
#[derive(Debug, Serialize, Deserialize)]
pub struct MovementSettings {
    pub standing_height: i32,
    pub post_collide_height: u32,
    pub slide_range: i32,
    pub turn_speed: u32,
    pub turn_max_speed: u32,
    pub poly_push_units: u32,
}

/// Validated player inputs. Loading this asset does not enable unsupported movement.
#[derive(Asset, TypePath)]
pub struct PlayerScene {
    pub world: u32,
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
    /// Decode the exported player cache, validating terrain before runtime queries.
    pub fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        serde_json::from_slice::<SceneData>(bytes)?.decode()
    }
}

/// Current world's player inputs, independent of render meshes.
#[derive(Resource)]
pub struct PlayerSceneRequest(pub Handle<PlayerScene>);

#[derive(Serialize, Deserialize)]
pub(crate) struct SceneData {
    pub version: u32,
    pub world: u32,
    pub keyboard_codes: Option<[u8; 8]>,
    pub settings: MovementSettings,
    pub definition: serde_json::Value,
    pub heights: Option<Vec<i16>>,
    pub water: Option<Vec<bool>>,
    pub collision: Option<CollisionScene>,
}

impl SceneData {
    pub(crate) fn decode(self) -> anyhow::Result<PlayerScene> {
        anyhow::ensure!(
            self.version == 2,
            "world {}: unsupported player schema {}; re-export",
            self.world,
            self.version
        );
        let collision = self.collision.with_context(|| {
            format!(
                "world {}: missing player collision data; re-export",
                self.world
            )
        })?;
        collision
            .validate()
            .with_context(|| format!("world {} collision", self.world))?;
        let static_collision = StaticCollision::new(&collision)
            .with_context(|| format!("world {} static collision", self.world))?;
        let terrain = match (self.heights, self.water) {
            (Some(heights), Some(water)) => Some(
                TerrainGround::new(heights, water)
                    .with_context(|| format!("world {} player terrain", self.world))?,
            ),
            (None, None) => None,
            _ => anyhow::bail!("world {}: incomplete player terrain", self.world),
        };
        let (definition, unavailable) = match PlayerDefinition::from_metadata(&self.definition) {
            Ok(definition) => (Some(definition), None),
            Err(error) => (
                None,
                Some(format!("world {} player: {error:#}", self.world)),
            ),
        };
        Ok(PlayerScene {
            world: self.world,
            keyboard_codes: self.keyboard_codes,
            settings: self.settings,
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
