//! Installed GXA images that scripts draw over the world frame (`ViewGXA`).
//!
//! A script names the file relative to the game directory (`system\stmap01.gxa`) and a frame.
//! Each frame is exported on demand as `overlay/<stem>-<frame>.png`, decoded with the palette
//! the GXA file carries. `AnimateGXA`'s second image slot is exported the same way, as
//! `overlay/<stem>-<first>+<offset>.png`: frame `first + offset` when the file has that frame and
//! frame `first + 1` otherwise.

/// Cache path of the exported frame of the GXA file a script named, or `None` when the name is not
/// a plain file name in the install's `system` directory.
pub fn cache_path(name: &str, frame: i32) -> Option<String> {
    let (directory, file) = name.split_once(['\\', '/'])?;
    let stem = directory
        .eq_ignore_ascii_case("system")
        .then_some(file)?
        .rsplit_once('.')
        .filter(|(_, extension)| extension.eq_ignore_ascii_case("gxa"))?
        .0
        .to_ascii_lowercase();
    let frame = u16::try_from(frame).ok()?;
    (!stem.is_empty() && stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then(|| format!("overlay/{stem}-{frame:02}.png"))
}

/// Cache path of `AnimateGXA`'s second image slot: frame `first + offset` of the file when it
/// has that frame, else frame `first + 1`. `None` when the name or either number is invalid.
pub fn second_slot_cache_path(name: &str, first: i32, offset: i32) -> Option<String> {
    let first_path = cache_path(name, first)?;
    let offset = u16::try_from(offset).ok()?;
    let stem = first_path.strip_suffix(".png")?;
    Some(format!("{stem}+{offset}.png"))
}

/// The frame a second slot shows: `first + offset` when it is below the frame count, else
/// `first + 1`.
#[cfg(any(feature = "parsers", test))]
fn second_slot_frame(first: usize, offset: usize, count: usize) -> usize {
    if first + offset < count {
        first + offset
    } else {
        first + 1
    }
}

/// Splits a cache path made by [`cache_path`] or [`second_slot_cache_path`] back into the stem,
/// the frame and the second slot's offset.
#[cfg(feature = "parsers")]
fn parse(path: &str) -> Option<(&str, usize, Option<usize>)> {
    let name = path.strip_prefix("overlay/")?.strip_suffix(".png")?;
    let (stem, frame) = name.rsplit_once('-')?;
    (!stem.is_empty() && stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then_some(())?;
    match frame.split_once('+') {
        Some((frame, offset)) => Some((stem, frame.parse().ok()?, Some(offset.parse().ok()?))),
        None => Some((stem, frame.parse().ok()?, None)),
    }
}

#[cfg(feature = "parsers")]
pub(crate) fn prepare(
    source: &dyn crate::source::ReadAssets,
    path: &str,
    output: &mut dyn crate::preparation::Output,
) -> anyhow::Result<()> {
    use anyhow::Context;
    let (stem, first, offset) =
        parse(path).with_context(|| format!("overlay: invalid path {path}"))?;
    let file = format!("SYSTEM/{}.GXA", stem.to_ascii_uppercase());
    let gxa = rgpre::import::gxa::parse_gxa_file(
        &crate::preparation::read(source, &file).with_context(|| format!("overlay: {file}"))?,
    )?;
    let index = offset.map_or(first, |offset| {
        second_slot_frame(first, offset, gxa.frames.len())
    });
    let frame = gxa
        .frames
        .into_iter()
        .nth(index)
        .with_context(|| format!("overlay: {file} has no frame {index}"))?;
    let image = image::RgbaImage::from_raw(frame.width.into(), frame.height.into(), frame.rgba)
        .with_context(|| format!("overlay: {file} frame {index} pixel count is invalid"))?;
    output.image(std::path::Path::new(path), &image)
}

#[cfg(test)]
mod tests {
    use super::{cache_path, second_slot_cache_path, second_slot_frame};

    #[test]
    fn script_names_map_to_a_cache_path_by_stem_and_frame() {
        assert_eq!(
            cache_path("system\\stmap01.gxa", 0).as_deref(),
            Some("overlay/stmap01-00.png")
        );
        assert_eq!(
            cache_path("SYSTEM/Mirror3.GXA", 12).as_deref(),
            Some("overlay/mirror3-12.png")
        );
        for invalid in [
            "system\\stmap01.bmp",
            "..\\x.gxa",
            "system\\a b.gxa",
            "system\\.gxa",
            "stmap01.gxa",
            "system\\x\\y.gxa",
        ] {
            assert_eq!(cache_path(invalid, 0), None, "{invalid}");
        }
        assert_eq!(cache_path("system\\stmap01.gxa", -1), None);
    }

    #[test]
    fn second_slot_paths_carry_the_first_frame_and_offset_and_pick_a_frame_by_count() {
        assert_eq!(
            second_slot_cache_path("system\\stmap01.gxa", 4, 2).as_deref(),
            Some("overlay/stmap01-04+2.png")
        );
        assert_eq!(second_slot_cache_path("system\\stmap01.gxa", 4, -1), None);
        assert_eq!(second_slot_cache_path("system\\stmap01.bmp", 4, 1), None);
        // Frame first + offset exists below the count; otherwise first + 1.
        assert_eq!(second_slot_frame(4, 2, 7), 6);
        assert_eq!(second_slot_frame(4, 3, 7), 5);
        assert_eq!(second_slot_frame(0, 9, 2), 1);
    }

    #[cfg(feature = "parsers")]
    #[test]
    fn cache_paths_parse_back_to_their_stem_and_frame() {
        assert_eq!(
            super::parse("overlay/stmap01-03.png"),
            Some(("stmap01", 3, None))
        );
        assert_eq!(
            super::parse("overlay/stmap01-03+12.png"),
            Some(("stmap01", 3, Some(12)))
        );
        assert_eq!(super::parse("overlay/../x-00.png"), None);
        assert_eq!(super::parse("hud/stmap01-00.png"), None);
    }
}
