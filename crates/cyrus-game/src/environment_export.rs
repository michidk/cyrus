//! Native extraction of per-world environment assets into the browser cache.
use crate::environment::{Environment, SkyImage, Water, water_vertices};
use crate::install::GameInstall;
use rgpre::import::{gxa, palette::Palette, wld, world_ini::WorldEntry};
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

pub fn export(
    install: &GameInstall,
    entry: &WorldEntry,
    palette: &Palette,
    terrain: Option<&wld::WldFile>,
    out: &Path,
) -> anyhow::Result<()> {
    let ini = std::fs::read_to_string(asset_path(&install.root, "WORLD.INI")?)?;
    let sky = if entry.world.is_some() {
        value(&ini, "world_sky", entry.index)
            .map(|path| -> anyhow::Result<SkyImage> {
                let file = gxa::parse_gxa_file(&std::fs::read(asset_path(&install.root, path)?)?)?;
                let frame = file
                    .frames
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("empty sky GXA"))?;
                Ok(SkyImage {
                    width: frame.width.into(),
                    height: frame.height.into(),
                    rgba: frame.rgba,
                })
            })
            .transpose()?
    } else {
        None
    };
    let background_index = value(&ini, "world_background", entry.index)
        .unwrap_or("0")
        .parse::<u8>()?;
    // Sky-color index 2 has an undocumented derivation; use black until established.
    let background = if matches!(background_index, 0 | 2) {
        [0; 3]
    } else {
        palette.colors[background_index as usize]
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
    fn indexed_values_do_not_mix_worlds_and_last_assignment_wins() {
        let ini = "world_wave[1]=1,2,3\nworld_wave[11]=4,5,6\nWORLD_WAVE[1]=7,8,9";
        assert_eq!(value(ini, "world_wave", 1), Some("7,8,9"));
        assert_eq!(value(ini, "world_wave", 11), Some("4,5,6"));
        assert_eq!(value(ini, "world_wave", 2), None);
    }
}
