#![cfg(feature = "parsers")]
//! Configured cursor archives reach shared preparation and software placement.
use cyrus_assets::{
    cursor::{ASSET_PATH, CursorArchive},
    source::{FileInfo, Manifest, ReadAssets},
};
use std::{cell::RefCell, collections::BTreeMap};

struct Source {
    manifest: Manifest,
    files: BTreeMap<String, Vec<u8>>,
    reads: RefCell<Vec<String>>,
}
impl Source {
    fn new(setting: &str, path: &str, bytes: Vec<u8>) -> Self {
        let files: BTreeMap<String, Vec<u8>> = BTreeMap::from([
            (
                "System.ini".into(),
                format!("[system]\n{setting}\n").into_bytes(),
            ),
            (path.into(), bytes),
        ]);
        let manifest = Manifest {
            files: files
                .iter()
                .map(|(path, bytes)| FileInfo {
                    path: path.clone(),
                    bytes: bytes.len() as u64,
                    digest: "a".repeat(64),
                })
                .collect(),
        };
        Self {
            manifest,
            files,
            reads: RefCell::default(),
        }
    }
}
impl ReadAssets for Source {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    fn read_range(&self, path: &str, start: u64, length: usize) -> anyhow::Result<Vec<u8>> {
        self.reads.borrow_mut().push(path.into());
        let bytes = self
            .files
            .get(path)
            .ok_or_else(|| anyhow::anyhow!("unexpected read {path}"))?;
        Ok(bytes
            .get(start as usize..start as usize + length)
            .ok_or_else(|| anyhow::anyhow!("truncated read {path}"))?
            .to_vec())
    }
}

fn gxa() -> Vec<u8> {
    let mut header = [0_u8; 34];
    header[32..34].copy_from_slice(&1_i16.to_le_bytes());
    let palette = [0_u8; 768];
    let mut frame = vec![0_u8; 18];
    frame[2..4].copy_from_slice(&1_i16.to_le_bytes());
    frame[4..6].copy_from_slice(&1_i16.to_le_bytes());
    frame.push(17);
    let mut bytes = Vec::new();
    for (tag, data) in [
        (b"BMHD", header.as_slice()),
        (b"BPAL", palette.as_slice()),
        (b"BBMP", frame.as_slice()),
    ] {
        bytes.extend_from_slice(tag);
        bytes.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
        bytes.extend_from_slice(data);
    }
    bytes.extend_from_slice(b"END ");
    bytes
}

#[test]
fn configured_dos_path_round_trips_exact_archive_through_game_preparation() -> anyhow::Result<()> {
    let original = gxa();
    let source = Source::new(
        "pointers=ART\\CUSTOM.BMP",
        "art/custom.bmp",
        original.clone(),
    );
    let files = cyrus_assets::game::prepare(&source, ASSET_PATH)?;
    assert_eq!(
        files.keys().map(String::as_str).collect::<Vec<_>>(),
        [ASSET_PATH]
    );
    let archive: CursorArchive = serde_json::from_slice(&files[ASSET_PATH])?;
    archive.validate()?;
    assert_eq!(archive.version, 1);
    assert_eq!(archive.source, "art/custom.bmp");
    assert_eq!(archive.bytes, original);
    assert_eq!(*source.reads.borrow(), ["System.ini", "art/custom.bmp"]);
    let record: serde_json::Value = serde_json::from_slice(&files[ASSET_PATH])?;
    assert!(record.get("hotspot").is_none());
    assert!(record.get("selected").is_none());
    Ok(())
}

#[test]
fn missing_unsafe_and_invalid_sources_fail_with_setting_or_archive_context() {
    for (setting, expected) in [
        ("", "SYSTEM.INI [system] pointers missing"),
        (
            "pointers=missing.bmp",
            "SYSTEM.INI [system] pointers=missing.bmp",
        ),
        (
            "pointers=..\\outside.bmp",
            "SYSTEM.INI [system] pointers=..\\outside.bmp",
        ),
        (
            "pointers=art/custom.bmp",
            "art/custom.bmp: cursor archive from SYSTEM.INI [system] pointers",
        ),
    ] {
        let source = Source::new(setting, "art/custom.bmp", b"invalid".to_vec());
        let error = cyrus_assets::game::prepare(&source, ASSET_PATH).unwrap_err();
        assert!(format!("{error:#}").contains(expected), "{error:#}");
    }
}

#[test]
fn cache_record_rejects_changed_schema_and_unsafe_origin() {
    let mut archive = CursorArchive {
        version: 2,
        source: "art/custom.bmp".into(),
        bytes: vec![1],
    };
    assert!(
        archive
            .validate()
            .unwrap_err()
            .to_string()
            .contains("schema changed")
    );
    archive.version = 1;
    archive.source = "../outside.bmp".into();
    assert!(archive.validate().is_err());
    archive.source = "art/custom.bmp".into();
    archive.bytes.clear();
    assert!(
        archive
            .validate()
            .unwrap_err()
            .to_string()
            .contains("empty cursor archive")
    );
}

#[test]
fn source_read_failure_identifies_configured_archive_and_setting() {
    let mut source = Source::new("pointers=art/custom.bmp", "art/custom.bmp", gxa());
    source.files.remove("art/custom.bmp");
    let error = cyrus_assets::game::prepare(&source, ASSET_PATH).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("art/custom.bmp: reading SYSTEM.INI [system] pointers archive"));
    assert!(message.contains("unexpected read art/custom.bmp"));
}

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
#[test]
fn native_export_matches_portable_record_and_rejects_install_output() -> anyhow::Result<()> {
    let temp = std::env::temp_dir().join(format!(
        "cyrus-cursor-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos(),
    ));
    let root = temp.join("install");
    let cache = temp.join("cache");
    std::fs::create_dir_all(root.join("art"))?;
    let bytes = gxa();
    std::fs::write(
        root.join("SYSTEM.INI"),
        b"[system]\npointers=ART\\CUSTOM.BMP\n",
    )?;
    std::fs::write(root.join("art/custom.bmp"), &bytes)?;
    let result = (|| -> anyhow::Result<()> {
        let install = cyrus_assets::install::GameInstall {
            root: root.canonicalize()?,
        };
        let error = cyrus_assets::cursor::export(&install, &root).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cache must be outside the original install")
        );
        cyrus_assets::cursor::export(&install, &cache)?;
        let source = Source::new("pointers=ART\\CUSTOM.BMP", "art/custom.bmp", bytes.clone());
        let portable = cyrus_assets::game::prepare(&source, ASSET_PATH)?;
        assert_eq!(std::fs::read(cache.join(ASSET_PATH))?, portable[ASSET_PATH]);
        assert_eq!(std::fs::read(root.join("art/custom.bmp"))?, bytes);
        assert_eq!(
            std::fs::read(root.join("SYSTEM.INI"))?,
            b"[system]\npointers=ART\\CUSTOM.BMP\n"
        );
        assert!(!root.join(ASSET_PATH).exists());
        let mut replacement = bytes.clone();
        // Change the synthetic image's pixel while retaining a valid GXA container.
        let pixel = replacement.len() - 5;
        replacement[pixel] = 18;
        std::fs::write(root.join("art/other.bmp"), &replacement)?;
        std::fs::write(
            root.join("SYSTEM.INI"),
            b"[system]\npointers=ART\\OTHER.BMP\n",
        )?;
        cyrus_assets::cursor::export(&install, &cache)?;
        let published = std::fs::read(cache.join(ASSET_PATH))?;
        let refreshed: CursorArchive = serde_json::from_slice(&published)?;
        assert_eq!(refreshed.source, "art/other.bmp");
        assert_eq!(refreshed.bytes, replacement);
        std::fs::write(root.join("art/other.bmp"), b"malformed")?;
        assert!(cyrus_assets::cursor::export(&install, &cache).is_err());
        assert_eq!(std::fs::read(cache.join(ASSET_PATH))?, published);
        assert_eq!(std::fs::read(root.join("art/custom.bmp"))?, bytes);
        Ok(())
    })();
    std::fs::remove_dir_all(&temp)?;
    result
}

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
#[test]
fn installed_cursor_preparation_preserves_archive_and_decodes_software_descriptors()
-> anyhow::Result<()> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
        });
    let Ok(install) = cyrus_assets::install::GameInstall::open(&root) else {
        eprintln!("skipped: no install (cursor configured archive)");
        return Ok(());
    };
    let source = cyrus_assets::native::Files::open(&install)?;
    let system = rgpre::import::system_ini::SystemIni::parse(&source.text("SYSTEM.INI")?)?;
    let configured = system
        .system
        .pointers
        .ok_or_else(|| anyhow::anyhow!("SYSTEM.INI [system] pointers missing"))?;
    let path = configured.replace('\\', "/");
    let expected = source
        .manifest()
        .find(&path)
        .ok_or_else(|| anyhow::anyhow!("missing configured cursor {path}"))?;
    let original = source.read(&expected.path)?;
    let prepared = cyrus_assets::game::prepare(&source, ASSET_PATH)?;
    let record: CursorArchive = serde_json::from_slice(&prepared[ASSET_PATH])?;
    record.validate()?;
    assert_eq!(record.source, expected.path);
    assert_eq!(record.bytes, original);
    let software = record.software_frames()?;
    assert_eq!(software.len(), 14);
    assert!(
        software.iter().all(|cursor| cursor.rgba.len()
            == usize::from(cursor.size[0]) * usize::from(cursor.size[1]) * 4)
    );
    eprintln!("installed software cursor descriptors: {}", software.len());
    assert_eq!(source.read(&expected.path)?, original);
    Ok(())
}

#[test]
fn software_cursor_pairs_decode_through_prepared_public_record() -> anyhow::Result<()> {
    let bytes = cursor_gxa(&[&[17, 0, 17, 17], &[16, 0, 0, 16], &[0, 0, 0, 0]]);
    let source = Source::new("pointers=art/custom.bmp", "art/custom.bmp", bytes);
    let files = cyrus_assets::game::prepare(&source, ASSET_PATH)?;
    let archive: CursorArchive = serde_json::from_slice(&files[ASSET_PATH])?;
    let cursors = archive.software_frames()?;
    assert_eq!(cursors.len(), 1);
    assert_eq!(cursors[0].size, [2, 2]);
    assert_eq!(cursors[0].hotspot, Some([1, 1]));
    assert_eq!(cursors[0].top_left([20, 30]), Some([19, 29]));
    assert_eq!(cursors[0].top_left([i16::MIN, 0]), Some([i16::MAX, -1]));
    assert_eq!(cursors[0].rgba.len(), 16);
    assert_eq!(&cursors[0].rgba[..8], &[0, 0, 0, 255, 0, 0, 0, 0]);
    Ok(())
}

#[test]
fn software_cursor_missing_marker_remains_unknown_and_corruption_is_contextual()
-> anyhow::Result<()> {
    let bytes = cursor_gxa(&[&[17; 4], &[0; 4], &[0; 4]]);
    let mut archive = CursorArchive {
        version: 1,
        source: "art/custom.bmp".into(),
        bytes,
    };
    assert_eq!(archive.software_frames()?[0].hotspot, None);
    assert_eq!(archive.software_frames()?[0].top_left([20, 30]), None);
    archive.bytes = b"corrupt".to_vec();
    assert!(
        format!("{:#}", archive.software_frames().unwrap_err())
            .contains("art/custom.bmp: software cursor descriptors")
    );
    Ok(())
}

fn cursor_gxa(frames: &[&[u8; 4]]) -> Vec<u8> {
    let mut header = [0_u8; 34];
    header[32..34].copy_from_slice(&(frames.len() as i16).to_le_bytes());
    let mut bytes = Vec::new();
    for (tag, data) in [(b"BMHD", header.as_slice()), (b"BPAL", &[0_u8; 768])] {
        bytes.extend_from_slice(tag);
        bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
        bytes.extend_from_slice(data);
    }
    let mut all_frames = Vec::new();
    for pixels in frames {
        let mut frame = vec![0_u8; 18];
        frame[2..4].copy_from_slice(&2_i16.to_le_bytes());
        frame[4..6].copy_from_slice(&2_i16.to_le_bytes());
        frame.extend_from_slice(*pixels);
        all_frames.extend_from_slice(&frame);
    }
    bytes.extend_from_slice(b"BBMP");
    bytes.extend_from_slice(&(all_frames.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&all_frames);
    bytes.extend_from_slice(b"END ");
    bytes
}
