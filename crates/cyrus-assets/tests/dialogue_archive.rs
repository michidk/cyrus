//! Dialogue loading reads selected original ranges without preloading other payloads.
#![cfg(feature = "parsers")]
use cyrus_assets::{
    sound::dialogue::DialogueArchive,
    source::{FileInfo, Manifest, ReadAssets},
};
use std::cell::RefCell;
struct Source {
    manifest: Manifest,
    bytes: Vec<u8>,
    reads: RefCell<Vec<(usize, usize)>>,
}
impl ReadAssets for Source {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    fn read_range(&self, _: &str, start: u64, length: usize) -> anyhow::Result<Vec<u8>> {
        let start = start as usize;
        self.reads.borrow_mut().push((start, length));
        Ok(self.bytes[start..start + length].to_vec())
    }
}
#[test]
fn dialogue_reads_text_without_loading_unrequested_invalid_audio() {
    let mut bytes = Vec::new();
    let mut entries = Vec::new();
    for (tag, payload) in [
        (b"text", &[0, 0, 2, 0, 0, 0, b'H', b'i'][..]),
        (b"bad!", &[0, 1][..]),
    ] {
        bytes.extend(tag);
        bytes.extend((payload.len() as u32).to_be_bytes());
        entries.push((*tag, bytes.len() as u32, payload.len() as u32));
        bytes.extend(payload);
    }
    let offset = bytes.len() as u32;
    for (tag, start, size) in entries {
        bytes.extend(tag);
        bytes.extend(start.to_le_bytes());
        bytes.extend(size.to_le_bytes());
    }
    bytes.extend(b"RNAV");
    bytes.extend(offset.to_le_bytes());
    bytes.extend(2u32.to_le_bytes());
    let source = Source {
        manifest: Manifest {
            files: vec![FileInfo {
                path: "ENGLISH.RTX".into(),
                bytes: bytes.len() as u64,
                digest: "a".repeat(64),
            }],
        },
        bytes,
        reads: RefCell::default(),
    };
    let mut archive = DialogueArchive::open(&source, "english.rtx").unwrap();
    assert!(
        source
            .reads
            .borrow()
            .iter()
            .all(|(start, _)| *start >= offset as usize)
    );
    source.reads.borrow_mut().clear();
    match archive.read(*b"TEXT").unwrap() {
        rgpre::import::rtx::RtxEntry::Text { text, .. } => assert_eq!(text, "Hi"),
        _ => panic!("expected text"),
    }
    assert_eq!(*source.reads.borrow(), [(8, 8)]);
    let error = archive.read(*b"bad!").unwrap_err();
    assert!(format!("{error:#}").contains("bad!"));
    assert!(
        archive
            .read(*b"none")
            .unwrap_err()
            .to_string()
            .contains("none")
    );
}

#[test]
fn dialogue_lifetime_uses_sample_presence_and_integer_pcm_duration() {
    use cyrus_assets::sound::dialogue::line_lifetime;
    use rgpre::import::{
        rtx::{RtxAudioHeader, RtxEntry},
        sfx::AudioType,
    };
    let text = RtxEntry::Text {
        tag: *b"text",
        text: "Hello".into(),
    };
    assert_eq!(line_lifetime(&text, None).unwrap(), 75);
    assert!(line_lifetime(&text, Some(0)).is_err());
    let voice = RtxEntry::Audio {
        tag: *b"voic",
        label: "Hello".into(),
        header: RtxAudioHeader {
            audio_type: AudioType::Mono16,
            sample_rate: 11025,
            base_volume: 100,
            loop_flag: 0,
            loop_offset: 0,
            loop_end: u32::MAX,
            audio_length: 11026,
        },
        pcm_data: vec![0; 11026],
    };
    assert_eq!(line_lifetime(&voice, None).unwrap(), 75);
    assert_eq!(line_lifetime(&voice, Some(0)).unwrap(), 128);
    assert_eq!(line_lifetime(&voice, Some(15)).unwrap(), 128);
}

fn cache_source() -> Source {
    let mut bytes = Vec::new();
    let mut entries = Vec::new();
    for index in 0..21u8 {
        let tag = [b't', b'x', b'a' + index, b'!'];
        let payload = [0, 0, 2, 0, 0, 0, b'H', b'i'];
        bytes.extend(tag);
        bytes.extend((payload.len() as u32).to_be_bytes());
        entries.push((tag, bytes.len() as u32, payload.len() as u32));
        bytes.extend(payload);
    }
    let offset = bytes.len() as u32;
    for (tag, start, size) in entries {
        bytes.extend(tag);
        bytes.extend(start.to_le_bytes());
        bytes.extend(size.to_le_bytes());
    }
    bytes.extend(b"RNAV");
    bytes.extend(offset.to_le_bytes());
    bytes.extend(21u32.to_le_bytes());
    Source {
        manifest: Manifest {
            files: vec![FileInfo {
                path: "ENGLISH.RTX".into(),
                bytes: bytes.len() as u64,
                digest: "a".repeat(64),
            }],
        },
        bytes,
        reads: RefCell::default(),
    }
}

struct NoSamples;
impl cyrus_assets::sound::DialogueSamples for NoSamples {
    fn playing(&mut self, _: u8) -> anyhow::Result<bool> {
        panic!("text must not query sample status")
    }
    fn stop(&mut self, _: u8) -> anyhow::Result<()> {
        panic!("text must not stop samples")
    }
}

#[test]
fn dialogue_cache_reuses_labels_retains_at_age_equality_and_never_evicts_on_exhaustion() {
    use cyrus_assets::sound::DialogueCache;
    let source = cache_source();
    let mut archive = DialogueArchive::open(&source, "english.rtx").unwrap();
    let mut cache = DialogueCache::default();
    assert_eq!(
        cache.load(*b"txa!", |label| archive.read(label)).unwrap(),
        0
    );
    source.reads.borrow_mut().clear();
    assert_eq!(
        cache.load(*b"txa!", |label| archive.read(label)).unwrap(),
        0
    );
    assert!(source.reads.borrow().is_empty());
    for index in 1..20u8 {
        assert_eq!(
            cache
                .load([b't', b'x', b'a' + index, b'!'], |label| archive
                    .read(label))
                .unwrap(),
            usize::from(index)
        );
    }
    source.reads.borrow_mut().clear();
    assert!(
        cache
            .load(*b"txu!", |label| archive.read(label))
            .unwrap_err()
            .to_string()
            .contains("exhausted")
    );
    assert!(source.reads.borrow().is_empty());
    cache.advance(100, &mut NoSamples).unwrap();
    assert!(cache.payload(0).is_some());
    cache.advance(8, &mut NoSamples).unwrap();
    assert!(
        cache.payload(0).is_some(),
        "equality retains the entry at age zero"
    );
    cache.advance(1, &mut NoSamples).unwrap();
    assert!(cache.payload(0).is_none());
    assert_eq!(
        cache.load(*b"txu!", |label| archive.read(label)).unwrap(),
        0
    );
    cache.reset(&mut NoSamples).unwrap();
    assert!(cache.payload(0).is_none());
}

#[test]
fn dialogue_cache_replaces_retained_payload_when_use_flag_is_clear() {
    use cyrus_assets::sound::DialogueCache;
    let source = cache_source();
    let mut archive = DialogueArchive::open(&source, "english.rtx").unwrap();
    let mut cache = DialogueCache::default();
    assert_eq!(
        cache.load(*b"txa!", |label| archive.read(label)).unwrap(),
        0
    );
    cache.advance(0, &mut NoSamples).unwrap();
    assert!(cache.payload(0).is_some());
    assert_eq!(
        cache.load(*b"txb!", |label| archive.read(label)).unwrap(),
        0
    );
    assert!(
        matches!(cache.payload(0), Some(rgpre::import::rtx::RtxEntry::Text { tag, .. }) if tag == b"txb!")
    );
    assert_eq!(
        cache.load(*b"txa!", |label| archive.read(label)).unwrap(),
        1
    );
    cache.advance(8, &mut NoSamples).unwrap();
    cache.advance(8, &mut NoSamples).unwrap();
    assert!(cache.payload(0).is_some());
    assert_eq!(
        cache.load(*b"txb!", |label| archive.read(label)).unwrap(),
        0
    );
    cache.advance(9, &mut NoSamples).unwrap();
    assert!(
        cache.payload(0).is_some(),
        "a fresh lookup marks the entry used again"
    );
}

struct PreparationSource {
    manifest: Manifest,
    rtx: Vec<u8>,
}
impl ReadAssets for PreparationSource {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    fn read_range(&self, path: &str, start: u64, length: usize) -> anyhow::Result<Vec<u8>> {
        let data = if path.eq_ignore_ascii_case("SYSTEM.INI") {
            b"[system]\nrtx_filename=ENGLISH.RTX\n".as_slice()
        } else {
            self.rtx.as_slice()
        };
        Ok(data[start as usize..start as usize + length].to_vec())
    }
}
fn preparation_source(tag: [u8; 4], payload: &[u8]) -> PreparationSource {
    let mut rtx = Vec::new();
    rtx.extend(tag);
    rtx.extend((payload.len() as u32).to_be_bytes());
    rtx.extend(payload);
    let offset = rtx.len() as u32;
    rtx.extend(tag);
    rtx.extend(8u32.to_le_bytes());
    rtx.extend((payload.len() as u32).to_le_bytes());
    rtx.extend(b"RNAV");
    rtx.extend(offset.to_le_bytes());
    rtx.extend(1u32.to_le_bytes());
    PreparationSource {
        manifest: Manifest {
            files: vec![
                FileInfo {
                    path: "SYSTEM.INI".into(),
                    bytes: b"[system]\nrtx_filename=ENGLISH.RTX\n".len() as u64,
                    digest: "a".repeat(64),
                },
                FileInfo {
                    path: "ENGLISH.RTX".into(),
                    bytes: rtx.len() as u64,
                    digest: "b".repeat(64),
                },
            ],
        },
        rtx,
    }
}

#[test]
fn dialogue_preparation_returns_only_selected_text_and_rejects_missing_voice_path() {
    let source = preparation_source(*b"text", &[0, 0, 2, 0, 0, 0, b'H', b'i']);
    // The uppercase authored key remains metadata identity; lookup folds it.
    let files = cyrus_assets::game::prepare(&source, "dialogue/54455854.dialogue").unwrap();
    assert_eq!(files.len(), 1);
    let entry: cyrus_assets::sound::DialogueEntry =
        serde_json::from_slice(&files["dialogue/54455854.dialogue"]).unwrap();
    assert_eq!(entry.label, *b"TEXT");
    assert_eq!(entry.text, "Hi");
    assert_eq!(entry.text_lifetime, 30);
    assert!(entry.voice.is_none());
    assert!(
        cyrus_assets::game::prepare(&source, "dialogue/54455854.wav")
            .unwrap_err()
            .to_string()
            .contains("did not produce")
    );
    for path in [
        "dialogue/../x.dialogue",
        "dialogue/ABCDEF00.dialogue",
        "dialogue/74657874.unsupported",
        "dialogue/zzzzzzzz.dialogue",
    ] {
        assert!(
            cyrus_assets::game::prepare(&source, path).is_err(),
            "{path}"
        );
    }
}

#[test]
fn dialogue_preparation_preserves_pcm_header_and_both_original_lifetimes() {
    let mut payload = vec![0, 1, 2, 0, 0, 0, b'H', b'i'];
    payload.extend(1u32.to_le_bytes());
    payload.extend(1u32.to_le_bytes());
    payload.extend(11025u32.to_le_bytes());
    payload.extend([113, 0]);
    payload.extend(0u32.to_le_bytes());
    payload.extend(u32::MAX.to_le_bytes());
    payload.extend(11026u32.to_le_bytes());
    payload.push(0);
    payload.extend(vec![0x40; 11026]);
    let source = preparation_source(*b"voic", &payload);
    let files = cyrus_assets::game::prepare(&source, "dialogue/766f6963.wav").unwrap();
    assert_eq!(files.len(), 2);
    let entry: cyrus_assets::sound::DialogueEntry =
        serde_json::from_slice(&files["dialogue/766f6963.dialogue"]).unwrap();
    assert_eq!(entry.text_lifetime, 30);
    let voice = entry.voice.unwrap();
    assert_eq!(voice.lifetime, 128);
    assert_eq!(voice.base_volume, 113);
    assert_eq!(
        (voice.channels, voice.bits_per_sample, voice.sample_rate),
        (1, 16, 11025)
    );
    assert_eq!(voice.audio, "dialogue/766f6963.wav");
    assert_eq!(&files[&voice.audio][44..], vec![0x40; 11026].as_slice());
}
