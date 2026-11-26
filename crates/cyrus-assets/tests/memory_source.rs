//! Shared fixture contracts through the public original-file access interfaces.
#![cfg(feature = "test-support")]
use cyrus_assets::source::{AssetFile, ReadAssets, test_support::MemoryAssets};
use std::io::{Read, Seek, SeekFrom};

#[test]
fn memory_source_preserves_manifest_order_and_reads_case_insensitive_ranges() -> anyhow::Result<()>
{
    let source = MemoryAssets::new([
        ("maps/FIRST.bin", b"abcdef".as_slice()),
        ("SYSTEM.INI", b"[system]\nvalue=7\n".as_slice()),
    ])?;
    source.manifest().validate()?;
    let paths: Vec<_> = source
        .manifest()
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(paths, ["maps/FIRST.bin", "SYSTEM.INI"]);
    let first = &source.manifest().files[0];
    assert_eq!(first.bytes, 6);
    assert_eq!(first.digest, blake3::hash(b"abcdef").to_hex().to_string());
    assert_eq!(source.read("MAPS/first.BIN")?, b"abcdef");
    assert_eq!(source.text("system.ini")?, "[system]\nvalue=7\n");
    assert_eq!(source.read_range("maps/first.bin", 2, 3)?, b"cde");
    let mut file = AssetFile::open(&source, "MAPS/FIRST.BIN")?;
    file.seek(SeekFrom::End(-2))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail)?;
    assert_eq!(tail, b"ef");
    Ok(())
}

#[test]
fn memory_source_rejects_missing_truncated_and_overflowing_ranges() -> anyhow::Result<()> {
    let source = MemoryAssets::new([("file.bin", b"abc".as_slice())])?;
    assert!(source.read_range("file.bin", 3, 0)?.is_empty());
    for (path, start, length) in [
        ("missing.bin", 0, 1),
        ("file.bin", 2, 2),
        ("file.bin", 4, 0),
        ("file.bin", u64::MAX, 2),
    ] {
        let error = source
            .read_range(path, start, length)
            .unwrap_err()
            .to_string();
        assert!(error.contains(path), "{error}");
    }
    let empty = MemoryAssets::default();
    assert!(empty.manifest().files.is_empty());
    assert!(
        empty
            .read("missing.bin")
            .unwrap_err()
            .to_string()
            .contains("missing.bin")
    );
    Ok(())
}

#[test]
fn memory_source_rejects_invalid_and_case_conflicting_manifest_paths() {
    assert!(MemoryAssets::new([("../escape", Vec::new())]).is_err());
    let error = MemoryAssets::new([("FILE.bin", vec![1]), ("file.BIN", vec![2])])
        .unwrap_err()
        .to_string();
    assert!(error.contains("duplicate source path"), "{error}");
}
