//! Original effects bank, loaded without inventing script or ambient triggers.
use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Asset, TypePath, Deserialize, Serialize)]
pub struct SoundBank {
    pub effects: Vec<SoundEffect>,
    #[serde(skip)]
    #[dependency]
    pub audio: Vec<Handle<AudioSource>>,
}

#[derive(Deserialize, Serialize)]
pub struct SoundEffect {
    pub looping: bool,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
}

#[derive(Default, TypePath)]
struct SoundBankLoader;
impl AssetLoader for SoundBankLoader {
    type Asset = SoundBank;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        context: &mut LoadContext<'_>,
    ) -> Result<SoundBank, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let mut bank: SoundBank = serde_json::from_slice(&bytes)?;
        bank.audio = (0..bank.effects.len())
            .map(|i| context.load(format!("cache://sounds/{i:03}.wav")))
            .collect();
        Ok(bank)
    }
    fn extensions(&self) -> &[&str] {
        &["sounds"]
    }
}

#[derive(Resource)]
pub struct SoundBankHandle(pub Handle<SoundBank>);

#[derive(Message)]
pub enum SoundCommand {
    Play(usize),
    StopAll,
}

#[derive(Component)]
pub struct PlayingSound;

pub struct SoundPlugin;
impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<SoundBank>()
            .init_asset_loader::<SoundBankLoader>()
            .add_message::<SoundCommand>()
            .add_systems(
                Startup,
                |mut commands: Commands, assets: Res<AssetServer>| {
                    commands.insert_resource(SoundBankHandle(
                        assets.load("cache://sounds/bank.sounds"),
                    ));
                },
            )
            .add_systems(Update, play_sounds);
    }
}

fn play_sounds(
    mut commands: Commands,
    mut requests: MessageReader<SoundCommand>,
    handle: Res<SoundBankHandle>,
    banks: Res<Assets<SoundBank>>,
    playing: Query<Entity, With<PlayingSound>>,
) {
    for request in requests.read() {
        match request {
            SoundCommand::StopAll => {
                for entity in &playing {
                    commands.entity(entity).despawn();
                }
            }
            SoundCommand::Play(index) => {
                let Some(bank) = banks.get(&handle.0) else {
                    warn!("sound bank is not loaded");
                    continue;
                };
                let Some(effect) = bank.effects.get(*index) else {
                    warn!("unknown sound effect {index}");
                    continue;
                };
                commands.spawn((
                    PlayingSound,
                    AudioPlayer::new(bank.audio[*index].clone()),
                    if effect.looping {
                        PlaybackSettings::LOOP
                    } else {
                        PlaybackSettings::DESPAWN
                    },
                ));
            }
        }
    }
}
