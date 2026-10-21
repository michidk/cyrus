//! Installed GXA images that scripts draw over the world frame (`ViewGXA`).
//!
//! A script names the file relative to the game directory (`system\stmap01.gxa`) and a frame.
//! Each frame is exported on demand as `overlay/<stem>-<frame>.png`, decoded with the palette
//! the GXA file carries.

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

/// Splits a cache path made by [`cache_path`] back into the stem and the frame.
#[cfg(feature = "parsers")]
fn parse(path: &str) -> Option<(&str, usize)> {
    let name = path.strip_prefix("overlay/")?.strip_suffix(".png")?;
    let (stem, frame) = name.rsplit_once('-')?;
    (!stem.is_empty() && stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then_some(())?;
    Some((stem, frame.parse().ok()?))
}

#[cfg(feature = "parsers")]
pub(crate) fn prepare(
    source: &dyn crate::source::ReadAssets,
    path: &str,
    output: &mut dyn crate::preparation::Output,
) -> anyhow::Result<()> {
    use anyhow::Context;
    let (stem, index) = parse(path).with_context(|| format!("overlay: invalid path {path}"))?;
    let file = format!("SYSTEM/{}.GXA", stem.to_ascii_uppercase());
    let gxa = rgpre::import::gxa::parse_gxa_file(
        &crate::preparation::read(source, &file).with_context(|| format!("overlay: {file}"))?,
    )?;
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
    use super::cache_path;

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

    #[cfg(feature = "parsers")]
    #[test]
    fn cache_paths_parse_back_to_their_stem_and_frame() {
        assert_eq!(super::parse("overlay/stmap01-03.png"), Some(("stmap01", 3)));
        assert_eq!(super::parse("overlay/../x-00.png"), None);
        assert_eq!(super::parse("hud/stmap01-00.png"), None);
    }
}
