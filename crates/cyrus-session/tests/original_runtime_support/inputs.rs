use super::*;

pub struct InstalledSources {
    scripts: BTreeMap<u32, Scripts>,
    animations: BTreeMap<u32, Animations>,
    players: BTreeMap<u32, PlayerScene>,
    pub player_bytes: BTreeMap<u32, Vec<u8>>,
    models: BTreeMap<u32, Vec<rgpre::import::savegame::SaveModelAllocation>>,
    walks: BTreeMap<u32, Vec<u32>>,
    ropes: BTreeMap<u32, Vec<u32>>,
    channels: BTreeMap<u8, u8>,
}
impl InstalledSources {
    pub fn load(
        slot: &SaveSlot,
        files: &cyrus_assets::native::Files,
        install: &GameInstall,
    ) -> Result<Self> {
        let mut scripts = BTreeMap::new();
        let mut animations = BTreeMap::new();
        let mut players = BTreeMap::new();
        let mut player_bytes = BTreeMap::new();
        let mut models = BTreeMap::new();
        let mut walks = BTreeMap::new();
        let mut ropes = BTreeMap::new();
        for game in std::iter::once(&slot.game).chain(slot.maps.values()) {
            let id = game.metadata.world_id;
            let key = format!("{id}.scripts");
            let data = cyrus_assets::game::prepare(files, &key)?;
            let script: Scripts = serde_json::from_slice(&data[&key])?;
            let animation: Animations = serde_json::from_slice(&data[&format!("{id}.animations")])?;
            let player = PlayerScene::decode(&data[&format!("{id}.player")])?;
            player_bytes.insert(id, data[&format!("{id}.player")].clone());
            models.insert(id, installed_model_allocations(game, &animation, install)?);
            walks.insert(
                id,
                (0..animation.metadata["walk_node_maps"]
                    .as_array()
                    .context("walk maps absent")?
                    .len())
                    .map(|i| i as u32)
                    .collect::<Vec<_>>(),
            );
            ropes.insert(
                id,
                (1..=animation.metadata["ropes"].as_array().map_or(0, Vec::len))
                    .map(|i| i as u32)
                    .collect::<Vec<_>>(),
            );
            scripts.insert(id, script);
            animations.insert(id, animation);
            players.insert(id, player);
        }
        Ok(Self {
            scripts,
            animations,
            players,
            player_bytes,
            models,
            walks,
            ropes,
            channels: (0u8..16).map(|i| (i, i)).collect(),
        })
    }
    pub fn sources(&self) -> BTreeMap<u32, WorldSnapshotSources<'_>> {
        self.scripts
            .iter()
            .map(|(&id, script)| {
                (
                    id,
                    WorldSnapshotSources {
                        scripts: script,
                        animations: &self.animations[&id],
                        collision: &self.players[&id].collision,
                        player: Some(&self.players[&id]),
                        effects: None,
                        identity_memo: None,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>()
    }
    pub fn allocations(&self) -> BTreeMap<u32, OriginalWorldAllocations<'_>> {
        self.scripts
            .keys()
            .map(|&id| {
                (
                    id,
                    OriginalWorldAllocations {
                        models: &self.models[&id],
                        walk_maps: &self.walks[&id],
                        ropes: &self.ropes[&id],
                        ambient_channels: &self.channels,
                    },
                )
            })
            .collect()
    }
    pub fn prior(&self, id: u32) -> Result<ScriptWorld> {
        let mut prior = ScriptWorld::prepare(&self.scripts[&id], 1)?;
        prior.prepare_spatial()?;
        let player = prior.selected_player().context("prior player absent")?;
        prior.initialize_player_spatial(player, [0; 3], [0; 3])?;
        prior.prepare_model_resources(
            &self.animations[&id],
            self.players[&id]
                .actor_collision
                .as_ref()
                .context("models absent")?,
        )?;
        prior.initialize_player_animation(player, &self.animations[&id], 0, &mut |_| Ok(()))?;
        Ok(prior)
    }
}

fn installed_model_allocations(
    game: &SaveGame,
    animations: &cyrus_assets::animation::Animations,
    install: &GameInstall,
) -> Result<Vec<rgpre::import::savegame::SaveModelAllocation>> {
    let worlds = cyrus_assets::scene::read_worlds(install)?;
    let entry = worlds
        .entries
        .iter()
        .find(|entry| entry.index == game.metadata.world_id)
        .context("saved world not installed")?;
    let filename = entry
        .map
        .rsplit(['/', '\\'])
        .next()
        .context("empty map name")?;
    let path = cyrus_assets::scene::find_file(&install.root.join("maps"), filename)
        .context("installed map absent")?;
    let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(path)?)?;
    let allocations = rgpre::import::savegame::SaveModelAllocations::from_world(
        &map,
        &["CYRUS", "GREMLIN"],
        |source| {
            let file = source.rsplit(['/', '\\']).next().unwrap_or(source);
            let key = file
                .rsplit_once('.')
                .map_or(file, |(stem, _)| stem)
                .to_ascii_uppercase();
            let geometry = animations.models.get(&key).ok_or_else(|| {
                rgpre::error::Error::Parse(format!("installed geometry {key} missing"))
            })?;
            Ok((key, geometry.positions.len()))
        },
    )?;
    Ok(allocations.entries().to_vec())
}
