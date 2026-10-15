//! Per-world environment preparation from original bytes on either target.
use crate::environment::{Environment, SkyImage, SunImage, Water, water_vertices};
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
use crate::install::GameInstall;
use crate::source::ReadAssets;
use anyhow::Context;
use rgpre::import::{bsi, fog_ini, palette::Palette, wld, world_ini::WorldEntry};
use std::path::Path;

fn value<'a>(ini: &'a str, key: &str, id: u32) -> Option<&'a str> {
    let key = format!("{key}[{id}]");
    ini.lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(k, _)| k.trim().eq_ignore_ascii_case(&key))
        .map(|(_, v)| v.trim())
        .next_back()
}

fn model_lights(
    source: &dyn ReadAssets,
    entry: &WorldEntry,
    system: &str,
) -> anyhow::Result<(
    crate::environment::Prelighting,
    Vec<crate::environment::LocalLight>,
)> {
    use crate::environment::{LocalLight, Prelighting, StaticLightingInstance};
    use rgpre::import::{rgm, system_ini::SystemIni};
    let settings = SystemIni::parse(system)?.system;
    let mut result = Prelighting::default();
    let bake = settings.lights_load.unwrap_or(true) && settings.statics_load.unwrap_or(true);
    let objects = settings.objects_load.unwrap_or(true);
    if !bake && !objects {
        return Ok((result, Vec::new()));
    }
    let filename = entry
        .map
        .rsplit(['/', '\\'])
        .next()
        .context("empty map filename")?;
    let path = crate::preparation::path(source, &format!("maps/{filename}"))?;
    let map = rgm::parse_rgm_file(&source.read(&path)?)
        .with_context(|| format!("world {}: lighting map {path}", entry.index))?;
    let mut object_lights = Vec::new();
    if objects {
        let scripts = rgm::disassemble_rgm_scripts(&map, None);
        for light in map.object_lights(entry.index) {
            let name = light.script_name();
            let script = scripts
                .iter()
                .find(|(actor, _)| actor == &name)
                .map(|(_, script)| script)
                .with_context(|| {
                    format!(
                        "world {}: light {} missing script {name}",
                        entry.index, light.id
                    )
                })?;
            anyhow::ensure!(
                script.script_pc == 0,
                "world {}: light {} script {name} has an unsupported active lighting program",
                entry.index,
                light.id
            );
            object_lights.push(LocalLight {
                position: [light.pos_x, light.pos_y, light.pos_z as i32],
                radius: light.radius,
                rgb: [light.red, light.green, light.blue],
            });
        }
    }
    if bake {
        result.lights = map
            .prelight_lights(entry.index)
            .into_iter()
            .map(|light| LocalLight {
                position: light.position,
                radius: light.radius,
                rgb: light.rgb,
            })
            .collect();
    }
    if !settings.statics_load.unwrap_or(true)
        || (result.lights.is_empty() && object_lights.is_empty())
    {
        return Ok((result, object_lights));
    }
    for section in &map.sections {
        if let rgm::RgmSection::Mps(_, records) = section {
            for (index, record) in records.iter().enumerate() {
                let model = record.model_name();
                if model.is_empty() {
                    continue;
                }
                let name = format!("S{index:03}_{model}");
                let instance = StaticLightingInstance {
                    position: [
                        record.pos_x,
                        record.pos_y,
                        i32::try_from(record.pos_z)
                            .with_context(|| format!("world {}: {name} position Z", entry.index))?,
                    ],
                    matrix: record.rotation_matrix,
                };
                anyhow::ensure!(
                    result.instances.insert(name.clone(), instance).is_none(),
                    "world {}: duplicate static lighting node {name}",
                    entry.index
                );
            }
        }
    }
    Ok((result, object_lights))
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

fn fog_table(source: &dyn ReadAssets, id: u32) -> anyhow::Result<[[u8; 4]; 16]> {
    for filename in [format!("fxart/FOG{id}.INI"), "fxart/FOG.INI".into()] {
        if let Some(file) = source.manifest().find(&filename) {
            let table = fog_ini::parse_fog_ini(&source.text(&file.path)?)
                .with_context(|| format!("world {id}: {}", file.path))?;
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

fn sun_image(source: &dyn ReadAssets, ini: &str, id: u32) -> anyhow::Result<Option<SunImage>> {
    let Some(filename) = value(ini, "world_sunimg", id).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let path = crate::preparation::path(source, &format!("fxart/{filename}"))?;
    let bytes = source
        .read(&path)
        .with_context(|| format!("world {id}: sun {}", path))?;
    let file = bsi::parse_bsi_file(&bytes).with_context(|| format!("world {id}: sun {}", path))?;
    let frame = file
        .images
        .first()
        .with_context(|| format!("world {id}: empty sun BSI {}", path))?;
    anyhow::ensure!(
        file.images.len() == 1 && frame.frame_count == 1,
        "world {id}: unsupported multi-frame sun BSI"
    );
    let angle = |key| -> anyhow::Result<f64> {
        let value = value(ini, key, id)
            .unwrap_or("0")
            .parse::<u16>()
            .with_context(|| format!("world {id}: {key}"))?;
        anyhow::ensure!(value < 2048, "world {id}: {key} must be below 2048");
        // Preserve the original angle table's rounded turn constant.
        #[allow(clippy::approx_constant)]
        Ok(f64::from(value) * 6.2831852 / 2048.0)
    };
    let direction = angle("world_sunangle")?;
    let skew = angle("world_sunskew")?;
    let scale = sky_parameter(ini, "world_sunscale", id, 12800)?;
    anyhow::ensure!(
        (1..=i16::MAX as i32).contains(&scale),
        "world {id}: invalid world_sunscale"
    );
    let texture_scale = if frame.tex_scale == 0 {
        256
    } else {
        frame.tex_scale
    };
    let factor = scale as f32 / 100.0 * f32::from(texture_scale) / 256.0 * 0.5;
    let color = value(ini, "world_sunimgrgb", id)
        .unwrap_or("255,255,200")
        .split(',')
        .map(|v| v.trim().parse::<u8>())
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("world {id}: world_sunimgrgb requires three RGB bytes"))?;
    Ok(Some(SunImage {
        size: [frame.width.into(), frame.height.into()],
        alpha: frame.pixel_data.clone(),
        center: [direction.sin(), direction.cos(), skew.sin()]
            .map(|v| -(v as f32 * 16000.0).trunc()),
        half_size: [frame.width as f32 * factor, frame.height as f32 * factor],
        color: color
            .try_into()
            .map_err(|_| anyhow::anyhow!("world {id}: world_sunimgrgb requires three RGB bytes"))?,
    }))
}

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub fn export(
    install: &GameInstall,
    entry: &WorldEntry,
    terrain: Option<&wld::WldFile>,
    out: &Path,
) -> anyhow::Result<()> {
    let source = crate::native::Files::scan(install, false)?;
    prepare(
        &source,
        entry,
        terrain,
        Path::new(out.file_name().context("environment output filename")?),
        &mut crate::preparation::DiskOutput(out.parent().unwrap_or(Path::new("."))),
    )
}

pub(crate) fn prepare(
    source: &dyn ReadAssets,
    entry: &WorldEntry,
    terrain: Option<&wld::WldFile>,
    out: &Path,
    output: &mut dyn crate::preparation::Output,
) -> anyhow::Result<()> {
    let ini = crate::preparation::text(source, "WORLD.INI")?;
    let system = crate::preparation::text(source, "SYSTEM.INI")?;
    let sky = if entry.world.is_some() && !sky_disabled(&system)? {
        value(&ini, "world_skyfx", entry.index)
            .map(|filename| -> anyhow::Result<SkyImage> {
                let relative = format!("fxart/{filename}");
                let path = crate::preparation::path(source, &relative)?;
                let file = bsi::parse_bsi_file(&source.read(&path)?)
                    .with_context(|| format!("sky {}", path))?;
                let frame = file.images.first().context("empty sky BSI")?;
                anyhow::ensure!(
                    file.images.len() == 1 && frame.frame_count == 1,
                    "unsupported multi-frame sky BSI"
                );
                let col = Path::new(&relative).with_extension("COL");
                let col = crate::preparation::path(source, &col.to_string_lossy())?;
                let sky_palette = Palette::parse(&source.read(&col)?)?;
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
                    sun: sun_image(source, &ini, entry.index)?,
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
    let fog_table = fog_table(source, entry.index)?;
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
    let inputs = rgpre::import::world_lighting::WorldLighting::parse(&ini, entry.index)?;
    let [x, y, z] = inputs.direction;
    let model_lighting = crate::environment::ModelLighting {
        ambient: inputs.ambient,
        directional: inputs.directional,
        direction: [-x, -y, z],
    };
    let terrain_lighting = terrain
        .map(|terrain| -> anyhow::Result<_> {
            Ok(crate::environment::TerrainLighting {
                side: terrain.combined_map(0)?.len().isqrt(),
                ambient: inputs.ambient,
                directional: inputs.directional,
                direction: [-x, -y, z],
            })
        })
        .transpose()?;
    let (prelighting, object_lights) = model_lights(source, entry, &system)?;
    let texture_scale = rgpre::import::system_ini::SystemIni::parse(&system)?
        .glide
        .text_scale
        .unwrap_or([4, 2]);
    rgpre::import::bsi::GlideTextureLayout::new([1, 1], Some(texture_scale))?;
    let environment = Environment {
        texture_scale,
        prelighting,
        object_lights,
        model_lighting,
        terrain_lighting,
        sky,
        background,
        clipping,
        fog_table,
        water,
    };
    environment.validate()?;
    output.publish(
        &out.with_extension("environment"),
        &serde_json::to_vec(&environment)?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(feature = "native", not(target_arch = "wasm32")))]
    #[test]
    fn installed_object_lights_follow_object_loading_independently_of_map_baking() {
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
        let install = match GameInstall::open(&root) {
            Ok(install) => install,
            Err(error) => {
                eprintln!("skipped: no install ({error})");
                return;
            }
        };
        let source = crate::native::Files::scan(&install, false).unwrap();
        let worlds = rgpre::import::world_ini::WorldIni::parse(
            &crate::preparation::text(&source, "WORLD.INI").unwrap(),
        );
        let entry = worlds
            .entries
            .iter()
            .find(|entry| entry.index == 20)
            .unwrap();
        for (settings, baked, runtime, placements) in [
            ("objects_load=no", true, false, true),
            ("lights_load=no", false, true, true),
            ("statics_load=no", false, true, false),
            ("objects_load=no\nlights_load=no", false, false, false),
        ] {
            let (prelight, objects) =
                model_lights(&source, entry, &format!("[system]\n{settings}")).unwrap();
            assert_eq!(!prelight.lights.is_empty(), baked, "{settings}");
            assert_eq!(!objects.is_empty(), runtime, "{settings}");
            assert_eq!(!prelight.instances.is_empty(), placements, "{settings}");
        }
    }

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
