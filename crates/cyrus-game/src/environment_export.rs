//! Native extraction of per-world environment assets into the browser cache.
use crate::environment::{Environment, SkyImage, Water, water_vertices};
use crate::install::GameInstall;
use anyhow::Context;
use rgpre::import::{bsi, palette::Palette, wld, world_ini::WorldEntry};
use std::path::{Path, PathBuf};

fn value<'a>(ini: &'a str, key: &str, id: u32) -> Option<&'a str> {
    let key = format!("{key}[{id}]");
    ini.lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(k, _)| k.trim().eq_ignore_ascii_case(&key))
        .map(|(_, v)| v.trim())
        .next_back()
}

fn asset_path(root: &Path, relative: &str) -> anyhow::Result<PathBuf> {
    let mut path = root.to_path_buf();
    for component in relative.split(['\\', '/']) {
        anyhow::ensure!(
            !component.is_empty() && component != "." && component != "..",
            "invalid asset path"
        );
        path = std::fs::read_dir(&path)?
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(component)
            })
            .map(|entry| entry.path())
            .ok_or_else(|| anyhow::anyhow!("missing {relative}"))?;
    }
    Ok(path)
}

fn sky_parameter(ini: &str, key: &str, id: u32, default: i32) -> anyhow::Result<i32> {
    let parsed = value(ini, key, id)
        .unwrap_or("0")
        .parse::<i32>()
        .with_context(|| format!("world {id}: {key}"))?;
    Ok(if parsed == 0 { default } else { parsed })
}

fn sky_disabled(ini: &str) -> anyhow::Result<bool> {
    let mut section = "";
    let mut disabled = false;
    for line in ini.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = name;
        } else if section.eq_ignore_ascii_case("xngine")
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("sky_disable")
        {
            disabled = value
                .trim()
                .parse::<i32>()
                .context("SYSTEM.INI [xngine] sky_disable")?
                == 1;
        }
    }
    Ok(disabled)
}

pub fn export(
    install: &GameInstall,
    entry: &WorldEntry,
    palette: &Palette,
    terrain: Option<&wld::WldFile>,
    out: &Path,
) -> anyhow::Result<()> {
    let ini = std::fs::read_to_string(asset_path(&install.root, "WORLD.INI")?)?;
    let system = std::fs::read_to_string(asset_path(&install.root, "SYSTEM.INI")?)?;
    let sky = if entry.world.is_some() && !sky_disabled(&system)? {
        value(&ini, "world_skyfx", entry.index)
            .map(|filename| -> anyhow::Result<SkyImage> {
                let relative = format!("fxart/{filename}");
                let path = asset_path(&install.root, &relative)?;
                let file = bsi::parse_bsi_file(&std::fs::read(&path)?)
                    .with_context(|| format!("sky {}", path.display()))?;
                let frame = file.images.first().context("empty sky BSI")?;
                anyhow::ensure!(
                    file.images.len() == 1 && frame.frame_count == 1,
                    "unsupported multi-frame sky BSI"
                );
                let col = Path::new(&relative).with_extension("COL");
                let col = asset_path(&install.root, &col.to_string_lossy())?;
                let sky_palette = Palette::parse(&std::fs::read(col)?)?;
                // Glide sky sampling is opaque, including palette index 0.
                let rgba = frame
                    .pixel_data
                    .iter()
                    .flat_map(|&index| {
                        let [r, g, b] = sky_palette.colors[index as usize];
                        [r, g, b, 255]
                    })
                    .collect();
                Ok(SkyImage {
                    width: frame.width.into(),
                    height: frame.height.into(),
                    rgba,
                    level: sky_parameter(&ini, "world_skylevel", entry.index, -3500)? as f32,
                    scale: sky_parameter(&ini, "world_skyscale", entry.index, 2000)? as f32,
                })
            })
            .transpose()
            .with_context(|| format!("world {}: world_skyfx", entry.index))?
    } else {
        None
    };
    let background_index = value(&ini, "world_background", entry.index)
        .unwrap_or("0")
        .parse::<u8>()?;
    // Preservation's sky docs distinguish black, unresolved sky color, and palette indices.
    let background = match background_index {
        0 => Some([0; 3]),
        2 => None,
        index => Some(palette.colors[index as usize]),
    };
    let water = if let (Some(terrain), Some(parameters)) =
        (terrain, value(&ini, "world_wave", entry.index))
    {
        let parameters: Vec<f32> = parameters
            .split(',')
            .map(|v| v.trim().parse())
            .collect::<Result<_, _>>()?;
        let parameters: [f32; 3] = parameters
            .try_into()
            .map_err(|_| anyhow::anyhow!("world_wave requires three parameters"))?;
        let textures = terrain.combined_map(2)?;
        let side = textures.len().isqrt();
        anyhow::ensure!(
            side >= 2 && side * side == textures.len(),
            "invalid terrain grid"
        );
        Some(Water {
            parameters,
            side,
            vertices: water_vertices(&textures, side),
        })
    } else {
        None
    };
    let environment = Environment {
        sky,
        background,
        water,
    };
    environment.validate()?;
    let path = out.with_extension("environment");
    let temporary = out.with_extension("environment.tmp");
    std::fs::write(&temporary, serde_json::to_vec(&environment)?)?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sky_configuration_uses_engine_defaults_and_the_xngine_section() {
        assert_eq!(sky_parameter("", "world_skyscale", 1, 2000).unwrap(), 2000);
        assert_eq!(
            sky_parameter("world_skylevel[1]=0", "world_skylevel", 1, -3500).unwrap(),
            -3500
        );
        assert_eq!(
            sky_parameter("world_skylevel[1]=-4500", "world_skylevel", 1, -3500).unwrap(),
            -4500
        );
        assert!(
            sky_parameter("world_skylevel[1]=bad", "world_skylevel", 1, -3500)
                .unwrap_err()
                .to_string()
                .contains("world 1: world_skylevel")
        );
        assert!(!sky_disabled("[other]\nsky_disable=1").unwrap());
        assert!(sky_disabled("[XNGINE]\nSKY_DISABLE=1").unwrap());
        assert!(!sky_disabled("[xngine]\nsky_disable=1\nsky_disable=0").unwrap());
        assert!(sky_disabled("[xngine]\nsky_disable=bad").is_err());
    }
    #[test]
    fn indexed_values_do_not_mix_worlds_and_last_assignment_wins() {
        let ini = "world_wave[1]=1,2,3\nworld_wave[11]=4,5,6\nWORLD_WAVE[1]=7,8,9";
        assert_eq!(value(ini, "world_wave", 1), Some("7,8,9"));
        assert_eq!(value(ini, "world_wave", 11), Some("4,5,6"));
        assert_eq!(value(ini, "world_wave", 2), None);
    }
}
