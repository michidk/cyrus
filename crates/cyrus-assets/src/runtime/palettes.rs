//! Palette and texture relationships derived from installed world and model references.
use super::{Library, geometry, prepare};
use crate::{Category, Entry, Preview, Source, source::ReadAssets};
use anyhow::{Context, Result};
use rgpre::{
    gltf::TextureCache,
    import::{self, model3d::TextureData},
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) type TextureReferences = BTreeMap<(u16, u16), BTreeSet<String>>;

pub(super) fn link<R: ReadAssets>(
    library: &Library<R>,
    preview: &mut Preview,
    palettes: &[String],
) {
    preview.related.extend(
        library
            .catalog
            .entries
            .iter()
            .filter(|entry| palettes.contains(&entry.path))
            .map(|entry| entry.id.clone()),
    );
}

pub(super) fn image<R: ReadAssets>(
    library: &Library<R>,
    entry: &Entry,
    image: u16,
    preview: &mut Preview,
) -> Result<()> {
    let filename = entry
        .path
        .rsplit('/')
        .next()
        .unwrap_or(&entry.path)
        .to_ascii_uppercase();
    let Some(bank) = filename
        .strip_prefix("TEXBSI.")
        .and_then(|id| id.parse::<u16>().ok())
    else {
        // Standalone sky images use their companion COL, independently of world palettes.
        let companion = std::path::Path::new(&entry.path).with_extension("COL");
        if let Some(file) = library.source.manifest().find(&companion.to_string_lossy()) {
            link(library, preview, std::slice::from_ref(&file.path));
        }
        return Ok(());
    };
    if library.texture_references.borrow().is_none() {
        *library.texture_references.borrow_mut() = Some(index(library)?);
    }
    if let Some(ids) = library
        .texture_references
        .borrow()
        .as_ref()
        .and_then(|index| index.get(&(bank, image)))
    {
        preview.related.extend(ids.iter().cloned());
    }
    Ok(())
}

fn index<R: ReadAssets>(library: &Library<R>) -> Result<TextureReferences> {
    let mut index = TextureReferences::new();
    let banks: BTreeSet<u16> = library
        .source
        .manifest()
        .files
        .iter()
        .filter_map(|file| {
            file.path
                .rsplit('/')
                .next()?
                .to_ascii_uppercase()
                .strip_prefix("TEXBSI.")?
                .parse()
                .ok()
        })
        .collect();
    for (i, entry) in library.catalog.entries.iter().enumerate() {
        if !matches!(entry.source, Source::File | Source::Archive) {
            continue;
        }
        if entry.category != Category::Models {
            continue;
        }
        library.source.progress(
            i,
            library.catalog.entries.len(),
            "Finding texture palette references",
        );
        let mut model_entry = entry.clone();
        if matches!(entry.source, Source::Archive) {
            model_entry.source = Source::Model(0);
        }
        let palettes = prepare::model_palettes(library, &model_entry)?;
        if palettes.is_empty() {
            continue;
        }
        let mut ids: BTreeSet<_> = library
            .catalog
            .entries
            .iter()
            .filter(|candidate| palettes.contains(&candidate.path))
            .map(|candidate| candidate.id.clone())
            .collect();
        ids.insert(entry.id.clone());
        let bytes = library.source.read(&entry.path)?;
        let models = if matches!(entry.source, Source::Archive) {
            let archive = import::rob::parse_rob_file(&bytes)
                .with_context(|| format!("{}: texture references", entry.path))?;
            let mut models = Vec::new();
            for segment in archive.segments {
                if segment.points_to_external_file() {
                    let name = segment.name();
                    let model = library
                        .catalog
                        .entries
                        .iter()
                        .find(|candidate| {
                            matches!(candidate.source, Source::File)
                                && candidate.category == Category::Models
                                && std::path::Path::new(&candidate.path)
                                    .file_stem()
                                    .is_some_and(|stem| stem.eq_ignore_ascii_case(&name))
                        })
                        .with_context(|| {
                            format!("{}: missing external model {name}", entry.path)
                        })?;
                    models.push(import::model3d::parse_3d_file(
                        &library.source.read(&model.path)?,
                    )?);
                } else if segment.has_embedded_3d_data() || segment.has_special_embedded_data() {
                    models.push(import::model3d::parse_3d_file(&segment.data).with_context(
                        || {
                            format!(
                                "{}: model {} texture references",
                                entry.path,
                                segment.name()
                            )
                        },
                    )?);
                }
            }
            models
        } else {
            vec![
                import::model3d::parse_3d_file(&bytes)
                    .with_context(|| format!("{}: texture references", entry.path))?,
            ]
        };
        for model in models {
            for face in model.face_data {
                if let TextureData::Texture {
                    texture_id,
                    image_id,
                } = face.texture_data
                {
                    let bank =
                        TextureCache::resolve_texture_id(texture_id, |id| banks.contains(&id));
                    index
                        .entry((bank, image_id.into()))
                        .or_default()
                        .extend(ids.iter().cloned());
                }
            }
        }
    }
    if geometry::find_name(&library.source, "WORLD.INI").is_some() {
        for world in prepare::world_entries(library)?.entries {
            let Some(path) = world
                .world
                .as_ref()
                .and_then(|path| geometry::find_name(&library.source, path))
            else {
                continue;
            };
            let terrain = import::wld::parse_wld_file(&library.source.read(&path)?)?;
            let metadata = import::wld::export_wld_metadata(&terrain)?;
            let palette = geometry::palette_path(&library.source, &world.palette)?;
            let ids: BTreeSet<_> = library
                .catalog
                .entries
                .iter()
                .filter(|entry| {
                    entry.path == path
                        || entry.path == palette
                        || matches!(entry.source, Source::World(id) if id == world.index)
                })
                .map(|entry| entry.id.clone())
                .collect();
            // Preservation's WLD rules fix terrain textures to bank 302.
            for image in metadata["combined"]["texture_indices"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|value| value.as_u64())
            {
                index
                    .entry((302, image as u16))
                    .or_default()
                    .extend(ids.iter().cloned());
            }
        }
    }
    Ok(index)
}
