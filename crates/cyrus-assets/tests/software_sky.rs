#![cfg(feature = "parsers")]
//! The portable viewer admits and renders configured software skies, not raw GXA previews.
use cyrus_assets::{Content, runtime::Library, source::test_support::MemoryAssets};
use std::collections::BTreeMap;

fn fixture_files() -> BTreeMap<String, Vec<u8>> {
    let mut header = [0; 34];
    header[32..34].copy_from_slice(&1_i16.to_le_bytes());
    let mut palette = vec![0_u8; 768];
    for i in 0..256 {
        palette[i * 3] = (i & 63) as u8;
    }
    let mut bitmap = vec![0; 18];
    bitmap[2..4].copy_from_slice(&512_i16.to_le_bytes());
    bitmap[4..6].copy_from_slice(&512_i16.to_le_bytes());
    bitmap.extend((0..512 * 512).map(|i| ((i / 512 + i % 512) & 255) as u8));
    let mut gxa = Vec::new();
    for (tag, data) in [
        (b"BMHD", header.as_slice()),
        (b"BPAL", palette.as_slice()),
        (b"BBMP", bitmap.as_slice()),
    ] {
        gxa.extend(tag);
        gxa.extend((data.len() as u32).to_be_bytes());
        gxa.extend(data);
    }
    gxa.extend(b"END ");
    let mut col = vec![0; 8];
    col.extend(palette.iter().map(|c| c * 4));
    BTreeMap::from([
        ("WORLD.INI".into(), b"[world]\nworld_map[0]=MAPS\\START.RGM\nworld_palette[0]=3DART\\SUNSET.COL\nworld_sky[0]=SYSTEM\\SUNSET.GXA\nworld_haze[0]=28\nworld_background[0]=2\n".to_vec()),
        ("SYSTEM.INI".into(), b"[system]\n[xngine]\nsky_move=1\nsky_xrotate=3\nsky_yrotate=63\n".to_vec()),
        ("system/sunset.gxa".into(),gxa), ("3dart/sunset.col".into(), col),
        ("3dart/HAZE.028".into(), (0..16384).map(|i| ((i/256+i%256)&255) as u8).collect()),
    ])
}
fn fixture() -> MemoryAssets {
    MemoryAssets::new(fixture_files()).unwrap()
}
#[test]
fn configured_software_sky_reaches_portable_viewer_as_projected_opaque_pixels() -> anyhow::Result<()>
{
    let mut library = Library::open(fixture(), |_, _, _| {})?;
    let id = library
        .catalog
        .entries
        .iter()
        .find(|e| e.format_label() == "Software skies")
        .expect("WORLD.INI software sky must have a rendered catalog entry")
        .id
        .clone();
    let preview = library.prepare(&id, None)?.preview;
    let Content::Images { frames } = preview.content else {
        panic!("expected rendered sky")
    };
    assert_eq!((frames[0].width, frames[0].height), (640, 480));
    assert_eq!(&frames[0].rgba[0..4], &[125, 0, 0, 255]);
    assert_eq!(
        &frames[0].rgba[640 * 240 * 4..640 * 240 * 4 + 4],
        &[8, 0, 0, 255]
    );
    assert!(
        frames[0]
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[3] == 255)
    );
    Ok(())
}
#[test]
fn draw_boundary_and_movement_switch_preserve_the_camera_and_background() -> anyhow::Result<()> {
    use cyrus_assets::software_sky::{SoftwareSky, View};
    let mut files = fixture_files();
    let source = MemoryAssets::new(files.clone())?;
    let view = View {
        draw: 1,
        angles: [0, 0],
        screen_scale: 256,
    };
    let sky = SoftwareSky::prepare(&source, 0)?;
    let first = sky.indices(view)?;
    let second = sky.indices(View { draw: 2, ..view })?;
    assert_ne!(first[..640 * 240], second[..640 * 240]);
    assert_eq!(first[640 * 240..], second[640 * 240..]);
    assert_eq!(first, sky.indices(view)?);
    files.insert(
        "SYSTEM.INI".into(),
        files["SYSTEM.INI"]
            .iter()
            .map(|&c| if c == b'1' { b'0' } else { c })
            .collect(),
    );
    let source = MemoryAssets::new(files)?;
    let sky = SoftwareSky::prepare(&source, 0)?;
    assert_eq!(sky.indices(view)?, sky.indices(View { draw: 2, ..view })?);
    Ok(())
}

#[test]
fn software_preparation_retains_six_bit_palette_identity_and_contextual_errors()
-> anyhow::Result<()> {
    use cyrus_assets::software_sky::{SoftwareSky, View};
    let view = View {
        draw: 1,
        angles: [0, 0],
        screen_scale: 256,
    };
    let mut files = fixture_files();
    let source = MemoryAssets::new(files.clone())?;
    let expected = SoftwareSky::prepare(&source, 0)?.rgba(view)?;
    for channel in &mut files.get_mut("3dart/sunset.col").unwrap()[8..] {
        *channel |= 3;
    }
    let source = MemoryAssets::new(files.clone())?;
    assert_eq!(SoftwareSky::prepare(&source, 0)?.rgba(view)?, expected);
    files.get_mut("3dart/HAZE.028").unwrap().pop();
    let source = MemoryAssets::new(files)?;
    let error = SoftwareSky::prepare(&source, 0)
        .err()
        .expect("truncated haze must fail");
    assert!(format!("{error:#}").contains("world 0: 3dart/HAZE.028"));
    Ok(())
}
