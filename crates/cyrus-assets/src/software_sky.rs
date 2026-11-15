//! Software sky preparation and indexed scanline presentation, shared by both viewer hosts.
use crate::source::ReadAssets;
use anyhow::{Context, Result, ensure};
use rgpre::import::{gxa, palette::Palette, software_texture::PaletteLookupTable};

/// Camera and draw boundary for the original 640 by 480 full viewport.
#[derive(Clone, Copy)]
pub struct View {
    /// Shared active-sky draw count, including this draw.
    pub draw: u32,
    /// Camera heading and pitch in 2048-step angles.
    pub angles: [i32; 2],
    /// Installed screen scale, clamped by world entry to at least 128.
    pub screen_scale: i32,
}

/// Prepared original palette indices. No Glide plane, sun or BIOS clock is involved.
pub struct SoftwareSky {
    size: usize,
    indices: Vec<u8>,
    palette: [[u8; 3]; 256],
    haze: PaletteLookupTable,
    background: u8,
    movement: bool,
    rotation: [i32; 2],
}

impl SoftwareSky {
    /// Read one world's GXA, palette and haze table through the portable asset source.
    pub fn prepare(source: &dyn ReadAssets, world: u32) -> Result<Self> {
        let ini = crate::preparation::text(source, "WORLD.INI")?;
        let worlds = rgpre::import::world_ini::WorldIni::parse(&ini);
        let entry = worlds
            .entries
            .iter()
            .find(|entry| entry.index == world)
            .with_context(|| format!("software sky: world {world} absent"))?;
        let value = |key: &str| crate::environment_export::value(&ini, key, world);
        let system = rgpre::import::system_ini::SystemIni::parse(&crate::preparation::text(
            source,
            "SYSTEM.INI",
        )?)?;
        ensure!(
            !system.xngine.sky_disable.unwrap_or(false),
            "world {world}: software sky disabled"
        );
        let filename = value("world_sky")
            .filter(|s| !s.is_empty())
            .with_context(|| format!("world {world}: no software sky"))?;
        let path = crate::preparation::path(source, filename)?;
        let gxa = gxa::parse_gxa_file(&source.read(&path)?)
            .with_context(|| format!("world {world}: software sky {path}"))?;
        ensure!(
            gxa.frames.len() == 1,
            "world {world}: unsupported multi-frame software sky {path}"
        );
        let frame = &gxa.frames[0];
        ensure!(
            frame.width == frame.height && matches!(frame.width, 256 | 512),
            "world {world}: unsupported software sky dimensions {}x{}",
            frame.width,
            frame.height
        );
        let palette_path = crate::preparation::path(source, &entry.palette)?;
        let palette = Palette::parse(&source.read(&palette_path)?)?;
        let vga = Palette {
            colors: palette.colors.map(|rgb| rgb.map(|c| c >> 2)),
        };
        let indices = if frame.palette.map(|rgb| rgb.map(|c| c >> 2)) == vga.colors {
            frame.indices.clone()
        } else {
            let mapping = frame.palette.map(|rgb| vga.match_vga(rgb.map(|c| c >> 2)));
            frame
                .indices
                .iter()
                .map(|&i| mapping[usize::from(i)])
                .collect()
        };
        let number = |key: &str, fallback: i32| -> Result<i32> {
            let number = value(key)
                .unwrap_or("0")
                .parse::<i32>()
                .with_context(|| format!("world {world}: {key}"))?;
            Ok(if number == 0 { fallback } else { number })
        };
        let haze_id = value("world_haze")
            .context("software sky missing world_haze")?
            .parse::<u32>()
            .with_context(|| format!("world {world}: world_haze"))?;
        let haze_path = crate::preparation::path(source, &format!("3dart/HAZE.{haze_id:03}"))?;
        let haze = PaletteLookupTable::parse(&source.read(&haze_path)?)
            .with_context(|| format!("world {world}: {haze_path}"))?;
        let default_rotation =
            |setting: Option<i32>, fallback| setting.filter(|&n| n != 0).unwrap_or(fallback);
        Ok(Self {
            size: usize::from(frame.width),
            indices,
            palette: vga.colors.map(|rgb| rgb.map(|c| (c << 2) | (c >> 4))),
            haze,
            background: value("world_background")
                .unwrap_or("0")
                .parse()
                .with_context(|| format!("world {world}: world_background"))?,
            movement: system.xngine.sky_move.unwrap_or(false),
            rotation: [
                number(
                    "world_sky_xrotate",
                    default_rotation(system.xngine.sky_xrotate, 3),
                )?,
                number(
                    "world_sky_yrotate",
                    default_rotation(system.xngine.sky_yrotate, 63),
                )?,
            ],
        })
    }

    /// Draw palette indices for a full 640 by 480 viewport. Other viewports remain unsupported.
    /// The caller owns the shared draw counter; drawing does not reset or advance it.
    pub fn indices(&self, view: View) -> Result<Vec<u8>> {
        ensure!(
            view.screen_scale >= 128,
            "software sky screen scale must be at least 128"
        );
        let scale = i64::from(view.screen_scale);
        let mask = (self.size - 1) as i64;
        let heading = i64::from(view.angles[0]);
        let mut row = ((((-heading & 2047) / 2) & mask)
            + if self.movement {
                i64::from(view.draw) & mask
            } else {
                0
            })
            * 256;
        let mut row_step = (65536 / scale) >> 1;
        let row_acceleration = (i64::from(self.rotation[1]) * 256 / scale) >> 1;
        let mut column_step = 128_i64;
        let column_acceleration = (i64::from(self.rotation[0]) * 256 / scale) >> 1;
        let center = i64::from(view.angles[1] & 2047) * 128;
        let mut haze = 63 * 256;
        let mut output = vec![self.background; 640 * 480];
        for y in 0..240 {
            row += row_step;
            row_step += row_acceleration;
            column_step += column_acceleration;
            let mut column = center - 320 * column_step;
            let source_row = ((row >> 8) & mask) as usize * self.size;
            let haze_row = (haze >> 8) as usize;
            for x in 0..640 {
                column += column_step;
                let index = self.indices[source_row + ((column >> 8) & mask) as usize];
                output[y * 640 + x] = self.haze.rows[haze_row][usize::from(index)];
            }
            haze = (haze - 16384 / 240).max(0);
        }
        Ok(output)
    }

    /// Expand a draw through the selected world palette, keeping index zero opaque.
    pub fn rgba(&self, view: View) -> Result<Vec<u8>> {
        Ok(self
            .indices(view)?
            .into_iter()
            .flat_map(|i| {
                let [r, g, b] = self.palette[usize::from(i)];
                [r, g, b, 255]
            })
            .collect())
    }
}
