//! Installed environmental effect textures and map-owned drawing attributes.
#[cfg(feature = "parsers")]
use anyhow::Context;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// World-local effect pool settings and decoded source images.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Effects {
    /// Signed WORLD.INI world_ambientfx face brightness for explicit-frame drawing.
    /// Older caches lack this input and cannot prepare effect presentation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ambient_brightness: Option<i16>,
    /// Installed max_effects after the signed zero clamp.
    pub capacity: u32,
    /// Installed disable_effects, independent of capacity.
    pub disabled: bool,
    /// Lower-sixteen-bit packed texture identity to decoded image.
    pub images: BTreeMap<u16, EffectImage>,
}

/// One installed shared image timeline, independent of an effect's lifetime frame.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EffectImage {
    pub name: String,
    /// Original image dimensions in pixels.
    pub size: [u32; 2],
    /// Original BSI texture scale in Q8 units; zero has already resolved to 256.
    pub texture_scale: u16,
    /// Original signed frame delay in milliseconds.
    pub delay_ms: i16,
    /// Base drawing flag one OR map FLAT flags.
    pub flags: u16,
    /// Original constant alpha input, including the valid value 256.
    pub alpha: u16,
    /// Complete RGBA source frames using the selected world's palette.
    pub frames: Vec<Vec<u8>>,
}

impl Effects {
    /// Reject malformed frame payloads before renderer allocation or indexing.
    pub fn validate(&self) -> Result<()> {
        for (identity, image) in &self.images {
            let context = || format!("environment effect texture {identity}");
            let bytes = image.size[0]
                .checked_mul(image.size[1])
                .and_then(|n| n.checked_mul(4));
            ensure!(
                image.size.into_iter().all(|n| n > 0)
                    && bytes.is_some()
                    && image.texture_scale > 0
                    && (1..=256).contains(&image.alpha)
                    && !image.name.is_empty()
                    && !image.frames.is_empty(),
                "{}: invalid image metadata",
                context()
            );
            ensure!(
                image
                    .frames
                    .iter()
                    .all(|frame| Some(frame.len() as u64) == bytes.map(u64::from)),
                "{}: frame dimensions differ from pixel bytes",
                context()
            );
        }
        Ok(())
    }
}

#[cfg(feature = "parsers")]
pub(crate) fn prepare(
    source: &dyn crate::source::ReadAssets,
    settings: &rgpre::import::system_ini::SystemSettings,
    map: &rgpre::import::rgm::RgmFile,
    palette: &rgpre::import::palette::Palette,
    ambient_brightness: i16,
    script_flat_textures: &std::collections::BTreeSet<u16>,
) -> Result<Effects> {
    use rgpre::import::bsi::BsiIndex;
    use std::io::Cursor;
    let mut effects = Effects {
        ambient_brightness: Some(ambient_brightness),
        capacity: settings
            .max_effects
            .context("SYSTEM.INI [system] max_effects missing")?
            .max(0) as u32,
        disabled: settings
            .disable_effects
            .context("SYSTEM.INI [system] disable_effects missing")?,
        images: BTreeMap::new(),
    };
    if !effects.disabled && effects.capacity > 0 {
        // Fixed engine identities from preservation's selected-death effect contract, then the
        // identities installed scripts pass to `AddFlatEffect` (0x2380, the Catacombs braziers).
        // Type-three placements and the textures the world's scripts give the `Flat*` natives are
        // drawn through actor flat records.
        let mut identities = vec![0xb081u16, 0x2480, 0x2580, 0x2500, 0x2380];
        for section in &map.sections {
            if let rgpre::import::rgm::RgmSection::MpobParsed(_, records) = section {
                identities.extend(
                    records
                        .iter()
                        .filter(|record| record.object_type == 3)
                        .map(|record| record.texture_data_raw as u16),
                );
            }
        }
        identities.extend(script_flat_textures);
        identities.sort_unstable();
        identities.dedup();
        for identity in identities {
            let archive = identity >> 7;
            let image_id = identity & 127;
            let path = format!("fxart/TEXBSI.{archive:03}");
            let mut bytes = Cursor::new(crate::preparation::read(source, &path)?);
            let index = BsiIndex::read(&mut bytes).with_context(|| path.clone())?;
            let position = index
                .images
                .iter()
                .position(|image| image.image_index == image_id)
                .with_context(|| format!("{path}: effect image {image_id} missing"))?;
            let image = index
                .read_image(&mut bytes, position)
                .with_context(|| format!("{path}: image {image_id}"))?;
            let attributes = map.flat_texture_attributes(identity);
            let frames = (0..=image.extra_frames.len())
                .map(|frame| {
                    image
                        .decode_frame_rgba(frame, Some(palette))
                        .with_context(|| format!("{path}: image {image_id} frame {frame} missing"))
                })
                .collect::<Result<_>>()?;
            effects.images.insert(
                identity,
                EffectImage {
                    name: image.name,
                    size: [u32::from(image.width), u32::from(image.height)],
                    texture_scale: if image.tex_scale == 0 {
                        256
                    } else {
                        image.tex_scale
                    },
                    delay_ms: image.anim_delay,
                    flags: 1 | attributes.map_or(0, |attributes| u16::from(attributes.flags)),
                    alpha: attributes.map_or(255, |attributes| attributes.alpha()),
                    frames,
                },
            );
        }
    }
    effects.validate()?;
    Ok(effects)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_effect_images_reject_truncated_rgba_with_texture_context() -> Result<()> {
        let effects = Effects {
            ambient_brightness: Some(-32768),
            capacity: 1,
            disabled: false,
            images: BTreeMap::from([(
                7,
                EffectImage {
                    name: "synthetic".into(),
                    size: [1, 1],
                    texture_scale: 256,
                    delay_ms: 71,
                    flags: 1,
                    alpha: 256,
                    frames: vec![vec![10, 20, 30, 0]],
                },
            )]),
        };
        let mut decoded: Effects = serde_json::from_slice(&serde_json::to_vec(&effects)?)?;
        assert_eq!(decoded.ambient_brightness, Some(-32768));
        let mut old = serde_json::to_value(&effects)?;
        old.as_object_mut().unwrap().remove("ambient_brightness");
        let old: Effects = serde_json::from_value(old)?;
        assert_eq!(old.ambient_brightness, None);
        assert!(
            !serde_json::to_value(&old)?
                .as_object()
                .unwrap()
                .contains_key("ambient_brightness")
        );
        decoded.validate()?;
        decoded.images.get_mut(&7).unwrap().frames[0].pop();
        let error = decoded.validate().unwrap_err().to_string();
        assert!(error.contains("texture 7"), "{error}");
        assert!(error.contains("frame dimensions"), "{error}");
        Ok(())
    }
}
