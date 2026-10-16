//! Portable, per-label dialogue assets and their audio dependencies.
use anyhow::{Result, ensure};
use bevy::{
    asset::{AssetLoader, LoadContext, io::Reader},
    prelude::*,
};

/// One prepared RTX entry, with playback-owned optional audio dependency.
#[derive(Asset, TypePath, Clone)]
pub struct DialogueClip {
    pub entry: cyrus_assets::sound::DialogueEntry,
    #[dependency]
    pub audio: Option<Handle<AudioSource>>,
}

#[derive(Default, TypePath)]
pub(super) struct DialogueClipLoader;
impl AssetLoader for DialogueClipLoader {
    type Asset = DialogueClip;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        context: &mut LoadContext<'_>,
    ) -> Result<DialogueClip> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let entry: cyrus_assets::sound::DialogueEntry =
            anyhow::Context::context(serde_json::from_slice(&bytes), "dialogue entry metadata")?;
        ensure!(
            entry.version == 1,
            "dialogue entry: unsupported schema {}",
            entry.version
        );
        let key = entry
            .label
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        ensure!(
            context.path().path() == std::path::Path::new(&format!("dialogue/{key}.dialogue")),
            "dialogue entry: authored label does not match cache path"
        );
        let audio = entry
            .voice
            .as_ref()
            .map(|voice| {
                ensure!(
                    voice.audio == format!("dialogue/{key}.wav"),
                    "dialogue entry {key}: audio path mismatch"
                );
                ensure!(
                    voice.sample_rate > 0
                        && matches!(voice.channels, 1 | 2)
                        && matches!(voice.bits_per_sample, 8 | 16),
                    "dialogue entry {key}: unsupported PCM layout"
                );
                Ok(context.load(format!("cache://{}", voice.audio)))
            })
            .transpose()?;
        Ok(DialogueClip { entry, audio })
    }
    fn extensions(&self) -> &[&str] {
        &["dialogue"]
    }
}

/// Shared RTX allocation result. A sample is an original reusable slot in 0..16;
/// lifetime uses the original signed counter before display formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogueStart {
    pub sample: Option<u8>,
    pub lifetime: i32,
}

impl cyrus_assets::sound::DialoguePayload for DialogueClip {
    fn has_voice(&self) -> bool {
        self.entry.voice.is_some()
    }
    fn release_text(&mut self) {
        self.entry.text = String::new();
    }
}

/// In-flight per-label preparation. Completed payloads move into DialogueCache,
/// so this owner retains only requests that have not yet been consumed.
#[derive(Default)]
pub(super) struct DialoguePreparation {
    requests: std::collections::HashMap<[u8; 4], DialogueRequest>,
}

enum DialogueRequest {
    #[cfg(not(target_arch = "wasm32"))]
    Exporting(bevy::tasks::Task<Result<()>>),
    Loading(Handle<DialogueClip>),
    Failed(String),
}

impl DialoguePreparation {
    pub(super) fn clear(&mut self) {
        self.requests.clear();
    }

    pub(super) fn read(
        &mut self,
        label: [u8; 4],
        server: &AssetServer,
        clips: &Assets<DialogueClip>,
        #[cfg(not(target_arch = "wasm32"))] install: Option<&cyrus_assets::install::GameInstall>,
    ) -> Result<std::task::Poll<DialogueClip>> {
        use std::task::Poll;
        let key = label
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let path = format!("cache://dialogue/{key}.dialogue");
        let pending = self.requests.len();
        if let std::collections::hash_map::Entry::Vacant(entry) = self.requests.entry(label) {
            ensure!(
                pending < 20,
                "dialogue {key}: twenty pending preparations already retained"
            );
            #[cfg(not(target_arch = "wasm32"))]
            let request = {
                let install =
                    anyhow::Context::context(install, "dialogue preparation: install unavailable")?
                        .clone();
                let cache = install.cache_path(std::path::Path::new(cyrus_assets::CACHE_DIR))?;
                DialogueRequest::Exporting(bevy::tasks::IoTaskPool::get().spawn(async move {
                    cyrus_assets::sound::dialogue::export::export(&install, &cache, label)
                }))
            };
            #[cfg(target_arch = "wasm32")]
            let request = DialogueRequest::Loading(server.load(path.clone()));
            entry.insert(request);
        }
        let request = self
            .requests
            .get_mut(&label)
            .expect("inserted dialogue request");
        #[cfg(not(target_arch = "wasm32"))]
        if let DialogueRequest::Exporting(task) = request {
            match bevy::tasks::futures::check_ready(task) {
                Some(Ok(())) => *request = DialogueRequest::Loading(server.load(path.clone())),
                Some(Err(error)) => {
                    *request =
                        DialogueRequest::Failed(format!("dialogue {key} preparation: {error:#}"))
                }
                None => return Ok(Poll::Pending),
            }
        }
        let handle = match request {
            DialogueRequest::Loading(handle) => handle,
            DialogueRequest::Failed(error) => anyhow::bail!("{error}"),
            #[cfg(not(target_arch = "wasm32"))]
            DialogueRequest::Exporting(_) => unreachable!("exporting dialogue was polled above"),
        };
        if let Some(bevy::asset::LoadState::Failed(error)) = server.get_load_state(handle.id()) {
            anyhow::bail!("dialogue {key} metadata: {error}");
        }
        if let Some(bevy::asset::RecursiveDependencyLoadState::Failed(error)) =
            server.get_recursive_dependency_load_state(handle.id())
        {
            anyhow::bail!("dialogue {key} dependency: {error}");
        }
        if !server.is_loaded_with_dependencies(handle.id()) {
            return Ok(Poll::Pending);
        }
        let clip = anyhow::Context::context(clips.get(&*handle), "loaded dialogue payload absent")?
            .clone();
        ensure!(
            clip.entry.label == label,
            "dialogue {key}: prepared label mismatch"
        );
        self.requests.remove(&label);
        Ok(Poll::Ready(clip))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use bevy::asset::io::{
        AssetSourceBuilder,
        memory::{Dir, MemoryAssetReader},
    };
    use std::{
        path::Path,
        task::Poll,
        time::{Duration, Instant},
    };

    #[test]
    fn dialogue_preparation_waits_for_voice_and_reports_dependency_failure() {
        for valid_audio in [true, false] {
            let directory = Dir::default();
            directory.insert_asset_text(Path::new("dialogue/54455354.dialogue"), &serde_json::json!({
                "version": 1, "label": [84,69,83,84], "text": "Prepared voice", "text_lifetime": 210,
                "voice": { "audio": "dialogue/54455354.wav", "base_volume": 113,
                    "sample_rate": 11025, "channels": 1, "bits_per_sample": 8,
                    "loop_flag": 0, "loop_offset": 0, "loop_end": 0, "lifetime": 128 }
            }).to_string());
            if valid_audio {
                directory.insert_asset(
                    Path::new("dialogue/54455354.wav"),
                    cyrus_assets::audio::pcm_wav(
                        rgpre::import::sfx::AudioType::Mono8,
                        11025,
                        &[128; 64],
                    )
                    .unwrap(),
                );
            }
            let mut app = App::new();
            app.register_asset_source(
                "cache",
                AssetSourceBuilder::new(move || {
                    Box::new(MemoryAssetReader {
                        root: directory.clone(),
                    })
                }),
            )
            .add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<AudioSource>()
            .init_asset_loader::<bevy::audio::AudioLoader>()
            .init_asset::<DialogueClip>()
            .init_asset_loader::<DialogueClipLoader>();
            app.finish();
            app.cleanup();
            let server = app.world().resource::<AssetServer>().clone();
            let handle = server.load("cache://dialogue/54455354.dialogue");
            let mut preparation = DialoguePreparation::default();
            preparation
                .requests
                .insert(*b"TEST", DialogueRequest::Loading(handle));
            assert!(matches!(
                preparation
                    .read(
                        *b"TEST",
                        &server,
                        app.world().resource::<Assets<DialogueClip>>(),
                        None
                    )
                    .unwrap(),
                Poll::Pending
            ));
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                app.update();
                let result = preparation.read(
                    *b"TEST",
                    &server,
                    app.world().resource::<Assets<DialogueClip>>(),
                    None,
                );
                match result {
                    Ok(Poll::Pending) => {
                        assert!(
                            Instant::now() < deadline,
                            "dialogue dependency never resolved"
                        );
                        std::thread::yield_now();
                    }
                    Ok(Poll::Ready(clip)) => {
                        assert!(valid_audio);
                        let voice = clip.audio.unwrap();
                        assert!(
                            app.world()
                                .resource::<Assets<AudioSource>>()
                                .contains(voice.id())
                        );
                        assert_eq!(clip.entry.text, "Prepared voice");
                        assert!(
                            preparation.requests.is_empty(),
                            "consumed preparation must release its root handle"
                        );
                        break;
                    }
                    Err(error) => {
                        assert!(!valid_audio, "valid voice failed: {error:#}");
                        assert!(
                            error.to_string().contains("54455354 dependency"),
                            "{error:#}"
                        );
                        break;
                    }
                }
            }
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct DialoguePreparationSnapshot {
    requests: Vec<([u8; 4], Option<String>)>,
}
impl DialoguePreparationSnapshot {
    pub(super) fn required_labels(&self) -> impl Iterator<Item = [u8; 4]> + '_ {
        self.requests
            .iter()
            .filter_map(|(label, failure)| failure.is_none().then_some(*label))
    }
}
impl DialoguePreparation {
    /// Export tasks must reach an asset request before a coherent save boundary.
    /// Capture does not poll them or consume their completion.
    pub(super) fn snapshot(&self, server: &AssetServer) -> Result<DialoguePreparationSnapshot> {
        let mut requests = Vec::with_capacity(self.requests.len());
        for (&label, request) in &self.requests {
            let key = label
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let failure = match request {
                #[cfg(not(target_arch = "wasm32"))]
                DialogueRequest::Exporting(_) => {
                    anyhow::bail!("dialogue {label:?}: export still in flight at save boundary")
                }
                DialogueRequest::Failed(error) => Some(error.clone()),
                DialogueRequest::Loading(handle) => {
                    if let Some(bevy::asset::LoadState::Failed(error)) =
                        server.get_load_state(handle.id())
                    {
                        Some(format!("dialogue {key} metadata: {error}"))
                    } else if let Some(bevy::asset::RecursiveDependencyLoadState::Failed(error)) =
                        server.get_recursive_dependency_load_state(handle.id())
                    {
                        Some(format!("dialogue {key} dependency: {error}"))
                    } else {
                        None
                    }
                }
            };
            requests.push((label, failure));
        }
        requests.sort_by_key(|(label, _)| *label);
        Ok(DialoguePreparationSnapshot { requests })
    }

    /// The host prepares asset requests before construction; this never starts I/O.
    pub(super) fn from_snapshot(
        snapshot: DialoguePreparationSnapshot,
        prepared: &std::collections::BTreeMap<[u8; 4], Handle<DialogueClip>>,
        server: &AssetServer,
        clips: &Assets<DialogueClip>,
    ) -> Result<Self> {
        ensure!(
            snapshot.requests.len() <= 20,
            "saved dialogue preparation exceeds capacity"
        );
        let mut requests = std::collections::HashMap::new();
        for (label, failure) in snapshot.requests {
            let request = if let Some(failure) = failure {
                DialogueRequest::Failed(failure)
            } else {
                let handle = anyhow::Context::with_context(prepared.get(&label), || {
                    format!("saved dialogue {label:?}: prepared request absent")
                })?;
                ensure!(
                    server.is_loaded_with_dependencies(handle.id()),
                    "saved dialogue {label:?}: replacement dependencies not ready"
                );
                let clip =
                    anyhow::Context::context(clips.get(handle), "restored dialogue asset absent")?;
                ensure!(
                    clip.entry.label == label,
                    "saved dialogue {label:?}: replacement label differs"
                );
                DialogueRequest::Loading(handle.clone())
            };
            ensure!(
                requests.insert(label, request).is_none(),
                "saved dialogue {label:?}: duplicate preparation"
            );
        }
        Ok(Self { requests })
    }
}

impl DialogueClip {
    /// Preserve retained text separately from immutable installed voice metadata.
    pub(super) fn from_saved_entry(
        saved: cyrus_assets::sound::DialogueEntry,
        installed: &Self,
    ) -> Result<Self> {
        ensure!(
            saved.text.is_empty() || saved.text == installed.entry.text,
            "dialogue {:?}: saved text differs from installed text",
            saved.label
        );
        let mut expected = installed.entry.clone();
        expected.text = saved.text.clone();
        ensure!(
            serde_json::to_value(&saved)? == serde_json::to_value(&expected)?,
            "dialogue {:?}: saved payload metadata differs",
            saved.label
        );
        ensure!(
            saved.voice.is_some() == installed.audio.is_some(),
            "dialogue {:?}: prepared voice dependency differs",
            saved.label
        );
        Ok(Self {
            entry: saved,
            audio: installed.audio.clone(),
        })
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct DialogueClipSnapshot {
    pub entry: cyrus_assets::sound::DialogueEntry,
    audio_hash: Option<String>,
}
impl DialogueClip {
    pub(super) fn snapshot(&self, sources: &Assets<AudioSource>) -> Result<DialogueClipSnapshot> {
        let audio_hash = self
            .audio
            .as_ref()
            .map(|handle| -> Result<_> {
                let source = anyhow::Context::context(
                    sources.get(handle),
                    "dialogue snapshot voice source absent",
                )?;
                Ok(blake3::hash(&source.bytes).to_hex().to_string())
            })
            .transpose()?;
        ensure!(
            self.entry.voice.is_some() == audio_hash.is_some(),
            "dialogue snapshot voice ownership differs"
        );
        Ok(DialogueClipSnapshot {
            entry: self.entry.clone(),
            audio_hash,
        })
    }

    pub(super) fn from_snapshot(
        saved: DialogueClipSnapshot,
        installed: &Self,
        sources: &Assets<AudioSource>,
    ) -> Result<Self> {
        let identity = installed.snapshot(sources)?;
        ensure!(
            saved.audio_hash == identity.audio_hash,
            "dialogue {:?}: saved voice content differs",
            saved.entry.label
        );
        Self::from_saved_entry(saved.entry, installed)
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    #[test]
    fn voice_replacement_rejects_changed_bytes_with_identical_metadata() -> Result<()> {
        let mut sources = Assets::<AudioSource>::default();
        let handle = sources.add(AudioSource {
            bytes: vec![1, 2, 3, 4].into(),
        });
        let mut clip = DialogueClip {
            entry: cyrus_assets::sound::DialogueEntry {
                version: 1,
                label: *b"TEST",
                text: "Retained".into(),
                text_lifetime: 120,
                voice: Some(cyrus_assets::sound::DialogueVoice {
                    audio: "dialogue/54455354.wav".into(),
                    base_volume: 127,
                    sample_rate: 22050,
                    channels: 1,
                    bits_per_sample: 8,
                    loop_flag: 0,
                    loop_offset: 0,
                    loop_end: 0,
                    lifetime: 1,
                }),
            },
            audio: Some(handle.clone()),
        };
        let saved = clip.snapshot(&sources)?;
        let restored = DialogueClip::from_snapshot(saved.clone(), &clip, &sources)?;
        assert_eq!(restored.entry.text, "Retained");
        let mut released = saved.clone();
        released.entry.text.clear();
        assert!(
            DialogueClip::from_snapshot(released, &clip, &sources)?
                .entry
                .text
                .is_empty()
        );
        clip.audio = Some(sources.add(AudioSource {
            bytes: vec![1, 2, 3, 5].into(),
        }));
        assert!(DialogueClip::from_snapshot(saved, &clip, &sources).is_err());
        assert_eq!(&*sources.get(&handle).unwrap().bytes, &[1, 2, 3, 4]);
        Ok(())
    }
}
