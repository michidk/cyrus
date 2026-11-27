//! PNG decoding through the public asset library, using synthetic RGBA pixels.
#![cfg(feature = "test-support")]
use cyrus_assets::{
    Content,
    runtime::Library,
    source::{ReadAssets, test_support::MemoryAssets},
};

// Independently encoded PNGs: two unfiltered RGBA rows, and a valid zlib stream
// containing no scanlines. Neither fixture depends on the decoder under test.
const RGBA_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 3, 0, 0, 0, 2, 8, 6, 0,
    0, 0, 157, 116, 102, 26, 0, 0, 0, 31, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 8,
    27, 64, 20, 3, 143, 146, 133, 95, 84, 245, 156, 189, 151, 94, 253, 251, 15, 0, 120, 67, 11, 24,
    206, 185, 175, 225, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];
const EMPTY_SCANLINES_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 3, 0, 0, 0, 2, 8, 6, 0,
    0, 0, 157, 116, 102, 26, 0, 0, 0, 8, 73, 68, 65, 84, 120, 156, 3, 0, 0, 0, 0, 1, 72, 6, 137,
    210, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

#[test]
fn png_encoder_output_prepares_through_the_asset_library() -> anyhow::Result<()> {
    use image::{
        ImageEncoder,
        codecs::png::{CompressionType, FilterType, PngEncoder},
    };
    let pixels = [12, 34, 56, 255, 78, 90, 123, 0];
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Best, FilterType::NoFilter)
        .write_image(&pixels, 2, 1, image::ExtendedColorType::Rgba8)?;
    let source = MemoryAssets::new([("encoded.png", png.clone())])?;
    let mut library = Library::open(source, |_, _, _| {})?;
    let id = library.catalog.entries[0].id.clone();
    let prepared = library.prepare(&id, None)?;
    let Content::Images { frames } = prepared.preview.content else {
        panic!("expected encoded PNG image");
    };
    assert_eq!(frames.len(), 1);
    assert_eq!((frames[0].width, frames[0].height), (2, 1));
    assert_eq!(frames[0].rgba, pixels);
    assert_eq!(library.source.read("encoded.png")?, png);
    Ok(())
}

#[test]
fn compressed_png_preserves_rgba_dimensions_and_source_bytes() -> anyhow::Result<()> {
    let source = MemoryAssets::new([("sample.png", RGBA_PNG)])?;
    let mut library = Library::open(source, |_, _, _| {})?;
    let id = library.catalog.entries[0].id.clone();
    let prepared = library.prepare(&id, None)?;
    prepared.preview.validate()?;
    let Content::Images { frames } = prepared.preview.content else {
        panic!("expected PNG image");
    };
    assert_eq!(frames.len(), 1);
    assert_eq!((frames[0].width, frames[0].height), (3, 2));
    assert_eq!(
        frames[0].rgba,
        [
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 12, 34, 56, 78, 90, 123, 156, 189, 210,
            234, 254, 255
        ]
    );
    assert_eq!(library.source.read("sample.png")?, RGBA_PNG);
    Ok(())
}

#[test]
fn compressed_png_without_scanlines_reports_its_source() -> anyhow::Result<()> {
    let source = MemoryAssets::new([("broken.png", EMPTY_SCANLINES_PNG)])?;
    let mut library = Library::open(source, |_, _, _| {})?;
    let id = library.catalog.entries[0].id.clone();
    let prepared = library.prepare(&id, None)?;
    let Content::Unavailable { reason } = prepared.preview.content else {
        panic!("missing PNG scanlines must not prepare an image");
    };
    assert!(reason.contains("broken.png"), "{reason}");
    assert!(reason.to_ascii_lowercase().contains("png"), "{reason}");
    assert_eq!(library.source.read("broken.png")?, EMPTY_SCANLINES_PNG);
    Ok(())
}
