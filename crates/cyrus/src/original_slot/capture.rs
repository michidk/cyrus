//! World-bound native encoded-colour readback for original saved-image assembly.
use super::assembly::SavedImages;
use anyhow::{Result, ensure};
use rgpre::import::savegame::{SaveGame, SaveImageKind, SaveSection};

/// One opaque top-left, row-major encoded RGBA8 framebuffer readback.
/// This is a rendered image, not linear lighting values or a native-save PNG.
pub struct CapturedFrame<'a> {
    /// Resolved world at render request time, never inferred from destination metadata.
    pub world: u32,
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

/// Packed screen and preview captured together, retaining their original world.
/// This group alone cannot establish complete original-slot export support.
#[derive(Debug, Clone)]
pub struct CapturedImages {
    world: u32,
    screen: SaveSection,
    thumbnail: SaveSection,
}

impl CapturedImages {
    /// Resolved capture-world identity.
    pub fn world(&self) -> u32 {
        self.world
    }

    /// Immutable SCR3 and THMB records, in that order.
    pub fn records(&self) -> (&SaveSection, &SaveSection) {
        (&self.screen, &self.thumbnail)
    }

    /// Feed the typed assembler only for the matching current save document.
    /// Metadata supplies decoder context; no non-image state is produced or changed.
    pub fn admit(&self, current: &SaveGame) -> Result<SavedImages> {
        ensure!(
            self.world == current.metadata.world_id,
            "SAVEGAME.SAV world {} SCR3/THMB: native capture world {} differs",
            current.metadata.world_id,
            self.world
        );
        SavedImages::decode(&SaveGame {
            metadata: current.metadata.clone(),
            sections: vec![self.screen.clone(), self.thumbnail.clone()],
        })
    }
}

/// Pack native readback into the original screen/preview group without I/O.
///
/// Preservation's saved-image contract specifies RGB565 words and independent
/// top-left sampling into 640x480 and 256x192. Capture contains already rendered
/// encoded channels: packing takes their high 5/6/5 bits without another dither
/// or lighting operation. Each output samples the source independently, rather
/// than deriving THMB from the reduced SCR3. The supported view is opaque 4:3;
/// widescreen cropping, transparent composites and unavailable readback reject.
/// A complete-slot validator is still mandatory after `CapturedImages::admit`.
pub fn capture_images(
    active_world: u32,
    frame: Option<CapturedFrame<'_>>,
) -> Result<CapturedImages> {
    (|| {
        let frame =
            frame.ok_or_else(|| anyhow::anyhow!("native framebuffer capture unavailable"))?;
        ensure!(
            frame.world == active_world,
            "capture world {} differs from active world {active_world}",
            frame.world
        );
        ensure!(
            frame.width > 0 && frame.height > 0,
            "unsupported zero framebuffer dimensions"
        );
        ensure!(
            u64::from(frame.width) * 3 == u64::from(frame.height) * 4,
            "unsupported framebuffer {}x{}: 4:3 view required",
            frame.width,
            frame.height
        );
        let width = usize::try_from(frame.width)?;
        let height = usize::try_from(frame.height)?;
        let bytes = width.checked_mul(height).and_then(|n| n.checked_mul(4));
        ensure!(
            bytes == Some(frame.rgba.len()),
            "framebuffer bytes {} differ from dimensions {}x{} RGBA8",
            frame.rgba.len(),
            width,
            height
        );
        ensure!(
            frame.rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255),
            "unsupported non-opaque framebuffer capture"
        );
        let record = |kind: SaveImageKind, name| {
            let mut data = Vec::with_capacity(kind.width() * kind.height() * 2);
            let sx = f64::from(frame.width as f32 / kind.width() as f32);
            let sy = f64::from(frame.height as f32 / kind.height() as f32);
            for y in 0..kind.height() {
                let row = (y as f64 * sy) as usize;
                for x in 0..kind.width() {
                    let column = (x as f64 * sx) as usize;
                    let offset = (row * width + column) * 4;
                    let p = &frame.rgba[offset..offset + 4];
                    let word = (u16::from(p[0] >> 3) << 11)
                        | (u16::from(p[1] >> 2) << 5)
                        | u16::from(p[2] >> 3);
                    data.extend_from_slice(&word.to_le_bytes());
                }
            }
            SaveSection { name, data }
        };
        Ok(CapturedImages {
            world: active_world,
            screen: record(SaveImageKind::Screen, *b"SCR3"),
            thumbnail: record(SaveImageKind::Thumbnail, *b"THMB"),
        })
    })()
    .map_err(|error: anyhow::Error| {
        error.context(format!(
            "SAVEGAME.SAV world {active_world} SCR3/THMB: native capture"
        ))
    })
}
