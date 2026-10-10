//! Native extraction of per-world environment assets into the browser cache.
use crate::environment::{Environment, SkyImage, Water, water_vertices};
use crate::install::GameInstall;
use anyhow::Context;
use rgpre::import::{bsi, fog_ini, palette::Palette, wld, world_ini::WorldEntry};
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

fn clipping(system: &str, world: &str, id: u32) -> anyhow::Result<[f32; 2]> {
    let mut section = "";
    let mut near = None;
    let mut far = None;
    for line in system.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = name;
        } else if section.eq_ignore_ascii_case("xngine")
            && let Some((key, value)) = line.split_once('=')
        {
            let target = match key.trim().to_ascii_lowercase().as_str() {
                "front_plane" => &mut near,
                "back_plane" => &mut far,
                _ => continue,
            };
            *target = Some(
                value
                    .trim()
                    .parse::<i32>()
                    .with_context(|| format!("SYSTEM.INI [xngine] {}", key.trim()))?
                    as f32,
            );
        }
    }
    let near = near.context("SYSTEM.INI [xngine] missing front_plane")?;
    let far = far.context("SYSTEM.INI [xngine] missing back_plane")?;
    let far = sky_parameter(world, "world_back_plane", id, far as i32)? as f32;
    anyhow::ensure!(
        near > 0.0 && far > near,
        "world {id}: invalid front_plane/back_plane range"
    );
    Ok([near, far])
}

fn fog_table(root: &Path, id: u32) -> anyhow::Result<[[u8; 4]; 16]> {
    let directory = asset_path(root, "fxart")?;
    for filename in [format!("FOG{id}.INI"), "FOG.INI".into()] {
        let path = std::fs::read_dir(&directory)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&filename)
            });
        if let Some(path) = path {
            let table = fog_ini::parse_fog_ini(&std::fs::read_to_string(path.path())?)
                .with_context(|| format!("world {id}: {}", path.path().display()))?;
            return Ok(std::array::from_fn(|i| {
                std::array::from_fn(|j| table[i * 4 + j])
            }));
        }
    }
    Ok([[0; 4]; 16])
}

fn background_color(ini: &str, id: u32) -> anyhow::Result<[u8; 3]> {
    let context = || format!("world {id}: world_fogrgb requires three RGB bytes");
    let channels = value(ini, "world_fogrgb", id)
        .unwrap_or("0,0,0")
        .split(',')
        .map(|v| v.trim().parse::<u8>())
        .collect::<Result<Vec<_>, _>>()
        .with_context(context)?;
    channels.try_into().map_err(|_| anyhow::anyhow!(context()))
}

pub fn export(
    install: &GameInstall,
    entry: &WorldEntry,
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
                    speed: value(&ini, "world_skyspeed", entry.index)
                        .unwrap_or("0")
                        .parse::<u8>()
                        .with_context(|| format!("world {}: world_skyspeed", entry.index))?,
                })
            })
            .transpose()
            .with_context(|| format!("world {}: world_skyfx", entry.index))?
    } else {
        None
    };
    // Verified Glide background fill in preservation's docs/engine/sky.md.
    let background = background_color(&ini, entry.index)?;
    let clipping = clipping(&system, &ini, entry.index)?;
    let fog_table = fog_table(&install.root, entry.index)?;
    let water = if let (Some(terrain), Some(parameters)) =
        (terrain, value(&ini, "world_wave", entry.index))
    {
        let parameters: Vec<f32> = parameters
            .split(',')
            .map(|v| v.trim().parse::<i32>().map(|v| v as f32))
            .collect::<Result<_, _>>()
            .with_context(|| format!("world {}: world_wave", entry.index))?;
        let parameters: [f32; 3] = parameters.try_into().map_err(|_| {
            anyhow::anyhow!(
                "world {}: world_wave requires three parameters",
                entry.index
            )
        })?;
        let textures = terrain.combined_map(2)?;
        let side = textures.len().isqrt();
        anyhow::ensure!(
            side >= 2 && side * side == textures.len(),
            "invalid terrain grid"
        );
        Some(Water {
            parameters,
            side,
            animated_vertices: water_vertices(&textures, side),
        })
    } else {
        None
    };
    let environment = Environment {
        sky,
        background,
        clipping,
        fog_table,
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
    fn scene_clipping_uses_system_values_and_nonzero_world_override() {
        let system = "[other]\nback_plane=9000\n[xngine]\nfront_plane=7\nback_plane=3800";
        assert_eq!(clipping(system, "", 1).unwrap(), [7.0, 3800.0]);
        assert_eq!(
            clipping(system, "world_back_plane[1]=0", 1).unwrap(),
            [7.0, 3800.0]
        );
        assert_eq!(
            clipping(system, "world_back_plane[1]=5000", 1).unwrap(),
            [7.0, 5000.0]
        );
        for value in ["-1", "7", "bad"] {
            assert!(clipping(system, &format!("world_back_plane[1]={value}"), 1).is_err());
        }
        assert!(clipping("[other]\nfront_plane=7\nback_plane=3800", "", 1).is_err());
    }

    #[test]
    fn glide_background_uses_fog_rgb_instead_of_legacy_palette_modes() {
        let ini = "world_background[1]=2\nworld_fogrgb[1]=200, 190, 250\nworld_fogrgb[27]=0,0,10";
        assert_eq!(background_color(ini, 1).unwrap(), [200, 190, 250]);
        assert_eq!(background_color(ini, 27).unwrap(), [0, 0, 10]);
        assert_eq!(
            background_color("world_background[6]=171", 6).unwrap(),
            [0; 3]
        );
        for rgb in ["200,190", "200,190,250,0", "256,0,0", "-1,0,0", "bad,0,0"] {
            let error = background_color(&format!("world_fogrgb[1]={rgb}"), 1).unwrap_err();
            assert!(error.to_string().contains("world 1: world_fogrgb"));
        }
    }
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
