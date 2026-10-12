//! Format adapters use preservation parsers and exporters, never binary layout knowledge.
use super::{Library, geometry};
use crate::source::{AssetFile, ReadAssets};
use crate::{Content, Entry, Picture, Preview, Source, media};
use anyhow::{Context, Result, ensure};
use rgpre::{
    gltf,
    import::{self, FileType, model3d::TextureData},
};
use serde_json::json;

pub(super) fn prepare<R: ReadAssets>(
    library: &Library<R>,
    entry: &Entry,
    palette: Option<&str>,
) -> Result<Preview> {
    if let Some(error) = &entry.error {
        anyhow::bail!("{error}");
    }
    let mut preview = Preview {
        content: Content::Unavailable {
            reason: "No decoder for this file type".into(),
        },
        details: vec![
            ("Source".into(), entry.path.clone()),
            ("Format".into(), entry.format.clone()),
            ("Bytes".into(), entry.bytes.to_string()),
        ],
        frame_interval_ms: None,
        metadata: None,
        related: library
            .catalog
            .entries
            .iter()
            .filter(|e| {
                e.path == entry.path
                    && e.id != entry.id
                    && (matches!(entry.source, Source::File | Source::Archive)
                        || matches!(e.source, Source::File | Source::Archive))
            })
            .map(|e| e.id.clone())
            .collect(),
    };
    match &entry.source {
        Source::Splash(index) => {
            let splash = import::glide_splash::parse_glide_splash(
                &library.source.read("splash/glide2x_emu.ovl")?,
            )?;
            let texture = splash
                .textures
                .get(*index)
                .context("missing splash texture")?;
            preview.content = Content::Images {
                frames: vec![Picture {
                    label: entry.name.clone(),
                    width: texture.width,
                    height: texture.height,
                    rgba: texture.rgba.clone(),
                    indices: None,
                    embedded_palette: false,
                }],
            };
            preview.metadata = visual_metadata(&preview.content);
            return Ok(preview);
        }
        Source::World(id) => {
            let world = geometry::world(&library.source, &id.to_string())?;
            let url = library.output(&entry.id, "glb");
            library.publish(&url, &world.glb)?;
            preview
                .details
                .push(("Palette".into(), world.entry.palette));
            preview
                .details
                .push(("Models".into(), world.models.len().to_string()));
            preview.content = Content::Model {
                frames: vec![url],
                palettes: Vec::new(),
                data: Some(import::rgm::export_rgm_metadata_json(&world.rgm, None)),
            };
            let names: std::collections::BTreeSet<_> =
                world.models.iter().map(|model| &model.model_name).collect();
            for name in names {
                related_name(library, &mut preview, name);
            }
            preview.related.sort();
            preview.related.dedup();
            return Ok(preview);
        }
        Source::CdTrack(track) => {
            let pcm = super::track_pcm(&library.source, *track)?;
            preview.content = audio(library, entry, &mut preview, &pcm, 44100, 2, 16)?;
            return Ok(preview);
        }
        Source::CdMovie(name) => {
            let bytes = super::movie_bytes(&library.source, name)?;
            preview.content = movie(library, entry, &mut preview, &bytes)?;
            return Ok(preview);
        }
        _ => {}
    }
    let path = &entry.path;
    let input = library.source_file(path)?;
    preview.content = match input.as_ref() {
        Input::Bsi(archive) => {
            if let Source::Image(index) = entry.source {
                let mut file = AssetFile::open(&library.source, path)?.without_read_ahead();
                let image = archive.read_image(&mut file, index)?;
                preview.metadata = Some(
                    json!({"name":image.name,"image_index":image.image_index,"width":image.width,"height":image.height,"x_offset":image.x_offset,"y_offset":image.y_offset,"frame_count":image.frame_count,"frame_interval_ms":image.anim_delay,"texture_scale":image.tex_scale,"data_encoding":image.data_encoding,"is_animated":image.is_animated,"palette":image.palette}),
                );
                preview.details.extend([
                    ("Width".into(), image.width.to_string()),
                    ("Height".into(), image.height.to_string()),
                    (
                        "Palette".into(),
                        if image.palette.is_some() {
                            "Embedded"
                        } else {
                            "Indexed; choose a display palette"
                        }
                        .into(),
                    ),
                ]);
                let frames = (0..=image.extra_frames.len())
                    .map(|i| Picture {
                        label: format!("Frame {i}"),
                        embedded_palette: image.palette.is_some(),
                        width: image.width.into(),
                        height: image.height.into(),
                        rgba: image.decode_frame_rgba(i, None).unwrap_or_default(),
                        indices: Some(if i == 0 {
                            image.pixel_data.clone()
                        } else {
                            image.extra_frames[i - 1].clone()
                        }),
                    })
                    .collect();
                Content::Images { frames }
            } else {
                Content::Data {
                    value: json!({"images":archive.images.iter().map(|i|json!({"name":i.name,"index":i.image_index,"loading":"Select an image to decode its dimensions and frames"})).collect::<Vec<_>>()}),
                }
            }
        }
        Input::Rob(archive) => {
            if let Source::Model(index) = entry.source {
                let mut file = AssetFile::open(&library.source, path)?.without_read_ahead();
                let segment = archive.read_segment(&mut file, index)?;
                if segment.points_to_external_file() {
                    let name = segment.name();
                    related_name(library, &mut preview, &name);
                    let candidates: Vec<_> = library
                        .catalog
                        .entries
                        .iter()
                        .filter(|e| {
                            matches!(e.source, Source::File)
                                && e.category == crate::Category::Models
                                && stem(&e.path).eq_ignore_ascii_case(&name)
                        })
                        .collect();
                    ensure!(
                        candidates.len() == 1,
                        "external model {name}: expected one standalone source, found {}",
                        candidates.len()
                    );
                    let bytes = library.source.read(&candidates[0].path)?;
                    model(library, entry, &bytes, &mut preview, palette)?
                } else {
                    ensure!(
                        segment.has_embedded_3d_data() || segment.has_special_embedded_data(),
                        "unsupported ROB segment type {}",
                        segment.segment_type
                    );
                    model(library, entry, &segment.data, &mut preview, palette)?
                }
            } else {
                Content::Data {
                    value: json!({"members":archive.segments.iter().map(|s|json!({"name":s.name(),"type":s.segment_type,"flags":s.segment_flags,"bytes":s.data_size})).collect::<Vec<_>>()}),
                }
            }
        }
        Input::Sfx(bank) => {
            if let Source::Effect(index) = entry.source {
                let mut file = AssetFile::open(&library.source, path)?.without_read_ahead();
                let effect = bank.read_effect(&mut file, index)?;
                preview.details.extend([
                    ("Sample rate".into(), effect.sample_rate.to_string()),
                    ("Channels".into(), effect.audio_type.channels().to_string()),
                ]);
                audio(
                    library,
                    entry,
                    &mut preview,
                    &effect.pcm_data,
                    effect.sample_rate,
                    effect.audio_type.channels(),
                    effect.audio_type.bits_per_sample(),
                )?
            } else {
                Content::Data {
                    value: json!({"effects":bank.effects.len()}),
                }
            }
        }
        Input::Rtx(bank) => {
            if let Source::Speech(index) = entry.source {
                let mut file = AssetFile::open(&library.source, path)?.without_read_ahead();
                match bank.read_entry(&mut file, index)? {
                    import::rtx::RtxEntry::Text { text, tag } => Content::Data {
                        value: json!({"tag":String::from_utf8_lossy(&tag),"text":text}),
                    },
                    import::rtx::RtxEntry::Audio {
                        header,
                        pcm_data,
                        label,
                        ..
                    } => {
                        if let Some(related) = library.catalog.entries.iter().find(|other| {
                            other.path == entry.path && other.name == label && other.id != entry.id
                        }) {
                            preview.related.push(related.id.clone());
                        }
                        preview.details.push(("Label".into(), label));
                        audio(
                            library,
                            entry,
                            &mut preview,
                            &pcm_data,
                            header.sample_rate,
                            header.audio_type.channels(),
                            header.audio_type.bits_per_sample(),
                        )?
                    }
                }
            } else {
                Content::Data {
                    value: json!({"entries":bank.entries.len(),"loading":"Select a member to read its text or audio"}),
                }
            }
        }
        Input::File(bytes) => match FileType::from_path(std::path::Path::new(path)) {
            Some(FileType::Pcx) => {
                let image = import::pcx::parse_pcx_file(bytes)?;
                preview.metadata = Some(
                    json!({"version":image.version,"encoding":image.encoding,"x_min":image.x_min,"y_min":image.y_min,"bytes_per_line":image.bytes_per_line,"width":image.image.width,"height":image.image.height,"palette":image.image.palette.as_slice()}),
                );
                Content::Images {
                    frames: vec![indexed_picture(image.image, 0)],
                }
            }
            Some(FileType::Lbm) => {
                let image = import::lbm::parse_lbm_file(bytes)?;
                preview.metadata = Some(
                    json!({"compression":image.compression,"masking":image.masking,"width":image.image.width,"height":image.image.height,"transparent_index":image.image.transparent_index,"palette":image.image.palette.as_slice(),"chunks":image.chunks.iter().map(|chunk|json!({"tag":String::from_utf8_lossy(&chunk.tag),"bytes":chunk.data.len()})).collect::<Vec<_>>()}),
                );
                Content::Images {
                    frames: vec![indexed_picture(image.image, 0)],
                }
            }
            Some(FileType::Anm) => {
                let animation = import::anm::parse_anm_file(bytes)?;
                preview.metadata = Some(
                    json!({"declared_frame_count":animation.declared_frame_count,"display_frame_count":animation.frames.len(),"frames_per_second":animation.frames_per_second,"frame_interval_ms":1000. / f64::from(animation.frames_per_second),"last_delta_valid":animation.last_delta_valid,"pages":animation.pages,"record_sizes":animation.records.iter().map(|record|record.len()).collect::<Vec<_>>(),"loop_delta_bytes":animation.loop_delta.map(|record|record.len()),"frames":animation.frames.iter().map(|frame|json!({"width":frame.width,"height":frame.height,"transparent_index":frame.transparent_index})).collect::<Vec<_>>(),"palette":animation.frames.first().map(|frame|frame.palette.as_slice())}),
                );
                preview.frame_interval_ms = Some(1000. / f64::from(animation.frames_per_second));
                preview.details.push((
                    "Frames per second".into(),
                    animation.frames_per_second.to_string(),
                ));
                Content::Images {
                    frames: animation
                        .frames
                        .into_iter()
                        .enumerate()
                        .map(|(i, image)| indexed_picture(image, i))
                        .collect(),
                }
            }
            Some(FileType::Gxa) => gxa_images(bytes, &mut preview)?,
            Some(FileType::Col) => Content::Palette {
                colors: import::palette::Palette::parse(bytes)?.colors.to_vec(),
            },
            Some(FileType::Fnt) => {
                let font = import::fnt::parse_fnt(bytes)?;
                let content = crate::decode::font(&font)?;
                preview.metadata = Some(
                    json!({"description":font.header.description_text,"max_width":font.header.max_width,"line_height":font.header.line_height,"character_start":font.header.character_start,"character_count":font.header.character_count,"chunk_order":font.chunk_order,"palette":font.palette.colors,"glyphs":font.glyphs.iter().map(|glyph|json!({"enabled":glyph.enabled,"offset_left":glyph.offset_left,"offset_top":glyph.offset_top,"width":glyph.width,"height":glyph.height})).collect::<Vec<_>>()}),
                );
                preview
                    .details
                    .push(("Description".into(), font.header.description_text));
                content
            }

            Some(FileType::Model3d | FileType::Model3dc) => {
                model(library, entry, bytes, &mut preview, palette)?
            }

            Some(FileType::Rgm) => {
                let map = import::rgm::parse_rgm_file(bytes)?;
                let worlds = world_entries(library)?;
                let matching: Vec<_> = worlds
                    .entries
                    .iter()
                    .filter(|w| stem(&w.map).eq_ignore_ascii_case(stem(&entry.path)))
                    .collect();
                for world in &matching {
                    related_world(library, &mut preview, world.index);
                }
                let value = import::rgm::export_rgm_metadata_json(&map, None);
                if let Some(world) = matching.first() {
                    let world = geometry::world(&library.source, &world.index.to_string())?;
                    let url = library.output(&entry.id, "glb");
                    library.publish(&url, &world.glb)?;
                    Content::Model {
                        frames: vec![url],
                        palettes: Vec::new(),
                        data: Some(value),
                    }
                } else {
                    Content::Data { value }
                }
            }
            Some(FileType::Wld) => {
                let terrain = import::wld::parse_wld_file(bytes)?;
                let worlds = world_entries(library)?;
                for world in worlds.entries.iter().filter(|w| {
                    w.world
                        .as_ref()
                        .is_some_and(|p| stem(p).eq_ignore_ascii_case(stem(&entry.path)))
                }) {
                    related_world(library, &mut preview, world.index);
                }
                let (json, binary) =
                    gltf::convert_wld_scene_to_gltf(&terrain, 0, &[], None, None, false)?;
                let url = library.output(&entry.id, "glb");
                library.publish(&url, &gltf::to_glb(&json, &binary)?)?;
                Content::Model {
                    frames: vec![url],
                    palettes: Vec::new(),
                    data: Some(
                        json!({"header_fields":terrain.header.fields.as_slice(),"sections":terrain.sections.len(),"world_previews":"Choose a related world for the authored palette and placements."}),
                    ),
                }
            }
            Some(FileType::Pvo) => {
                let tree = import::pvo::parse_pvo_file(bytes)?;
                Content::Spatial {
                    cells: geometry::visibility_cells(&tree)?,
                    value: json!({
                "depth":tree.header.depth,"center":[tree.header.center_x,tree.header.center_y,tree.header.center_z],"cell_size":tree.header.cell_size,
                "nodes":tree.octr_nodes.iter().map(|n|json!({"id":n.byte_offset,"leaf":n.leaf_ref,"children":n.child_refs})).collect::<Vec<_>>(),
                "leaves":tree.plst_leaves.iter().map(|l|json!({"id":l.byte_offset,"entries":l.entries.iter().map(|e|json!({"count":e.count,"start":e.mlst_start})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
                "polygon_indices":tree.mlst_indices }),
                }
            }
            Some(FileType::Cht) => Content::Data {
                value: json!(
                    import::cht::parse_cht_file(bytes)?
                        .entries
                        .iter()
                        .map(|e| json!({"index":e.index,"name":e.name,"value":e.value}))
                        .collect::<Vec<_>>()
                ),
            },
            None if entry.category == crate::Category::Images => {
                match image::load_from_memory(bytes) {
                    Ok(image) => {
                        let image = image.into_rgba8();
                        Content::Images { frames: vec![Picture {label:entry.name.clone(),width:image.width(),height:image.height(),rgba:image.into_raw(),indices:None,embedded_palette:false}] }
                    }
                    Err(error) if entry.format.eq_ignore_ascii_case("BMP") => {
                        gxa_images(bytes, &mut preview).with_context(|| format!("BMP: decoding failed for both Windows bitmap and GXA encodings: {error}"))?
                    }
                    Err(error) => return Err(error).context("Original image is malformed or uses an unsupported encoding"),
                }
            }
            None if entry.category == crate::Category::Audio => {
                let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes))?;
                let spec = reader.spec();
                ensure!(
                    spec.sample_format == hound::SampleFormat::Int
                        && matches!(spec.bits_per_sample, 8 | 16),
                    "unsupported WAV sample encoding"
                );
                let pcm = if spec.bits_per_sample == 8 {
                    reader
                        .samples::<i8>()
                        .map(|s| s.map(|s| (i16::from(s) + 128) as u8))
                        .collect::<std::result::Result<Vec<_>, _>>()?
                } else {
                    reader
                        .samples::<i16>()
                        .map(|s| s.map(i16::to_le_bytes))
                        .collect::<std::result::Result<Vec<_>, _>>()?
                        .into_iter()
                        .flatten()
                        .collect()
                };
                audio(
                    library,
                    entry,
                    &mut preview,
                    &pcm,
                    spec.sample_rate,
                    spec.channels,
                    spec.bits_per_sample,
                )?
            }
            None if entry.category == crate::Category::Movies => {
                movie(library, entry, &mut preview, bytes)?
            }
            None if entry.category == crate::Category::Data => text_data(bytes, &entry.path)?,
            None => Content::Unavailable {
                reason: format!(
                    "{} has no preservation decoder. File retained in the catalog.",
                    entry.format
                ),
            },
            Some(kind) => anyhow::bail!("{}: expected an archive source", kind.display_name()),
        },
    };
    if preview.metadata.is_none() {
        preview.metadata = visual_metadata(&preview.content);
    }
    preview.related.sort();
    preview.related.dedup();
    Ok(preview)
}
fn stem(path: &str) -> &str {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    file.rsplit_once('.').map_or(file, |(s, _)| s)
}
fn related_name<R: ReadAssets>(library: &Library<R>, preview: &mut Preview, name: &str) {
    for entry in &library.catalog.entries {
        if stem(&entry.path).eq_ignore_ascii_case(name) || entry.name.eq_ignore_ascii_case(name) {
            preview.related.push(entry.id.clone());
        }
    }
}
fn related_world<R: ReadAssets>(library: &Library<R>, preview: &mut Preview, id: u32) {
    preview.related.extend(
        library
            .catalog
            .entries
            .iter()
            .filter(|e| matches!(e.source,Source::World(i) if i==id))
            .map(|e| e.id.clone()),
    );
}
fn world_entries<R: ReadAssets>(library: &Library<R>) -> Result<import::world_ini::WorldIni> {
    let path = geometry::find_name(&library.source, "WORLD.INI").context("missing WORLD.INI")?;
    Ok(import::world_ini::WorldIni::parse(
        &library.source.text(&path)?,
    ))
}

// World membership comes from preservation's parsed placements and actor animation references.
fn model_palettes<R: ReadAssets>(library: &Library<R>, entry: &Entry) -> Result<Vec<String>> {
    if geometry::find_name(&library.source, "WORLD.INI").is_none() {
        return Ok(Vec::new());
    }
    let worlds = world_entries(library)?;
    if matches!(entry.source, Source::Model(_)) {
        let mut palettes = worlds
            .find_by_map_stem(stem(&entry.path))
            .iter()
            .map(|world| geometry::palette_path(&library.source, &world.palette))
            .collect::<Result<Vec<_>>>()?;
        palettes.sort();
        palettes.dedup();
        return Ok(palettes);
    }
    if library.model_palettes.borrow().is_none() {
        let mut by_map: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for world in &worlds.entries {
            let Some(path) = geometry::find_name(&library.source, &world.map) else {
                continue;
            };
            by_map
                .entry(path)
                .or_default()
                .push(geometry::palette_path(&library.source, &world.palette)?);
        }
        let mut by_model: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for (path, palettes) in by_map {
            let bytes = library.source.read(&path)?;
            let scene = import::rgm::parse_rgm_file(&bytes)
                .with_context(|| format!("model palette references in {path}"))?;
            let metadata = import::rgm::export_rgm_metadata_json(&scene, None);
            let mut names = Vec::new();
            for section in &scene.sections {
                match section {
                    import::rgm::RgmSection::Mps(_, records) => {
                        names.extend(records.iter().map(|r| r.model_name()))
                    }
                    import::rgm::RgmSection::MpobParsed(_, records) => {
                        names.extend(records.iter().map(|r| r.model_name()))
                    }
                    import::rgm::RgmSection::MprpParsed(_, records) => {
                        for record in records {
                            names.extend([record.rope_model.clone(), record.static_model.clone()]);
                        }
                    }
                    _ => {}
                }
            }
            for actor in metadata["actors"].as_array().into_iter().flatten() {
                for reference in actor["raan_entries"].as_array().into_iter().flatten() {
                    if let Some(path) = reference["file_path"].as_str() {
                        names.push(path.into());
                    }
                }
            }
            for name in names {
                let name = name.rsplit(['/', '\\']).next().unwrap_or(&name);
                let name = stem(name).to_ascii_uppercase();
                if !name.is_empty() {
                    by_model
                        .entry(name)
                        .or_default()
                        .extend(palettes.iter().cloned());
                }
            }
        }
        for palettes in by_model.values_mut() {
            palettes.sort();
            palettes.dedup();
        }
        *library.model_palettes.borrow_mut() = Some(by_model);
    }
    Ok(library
        .model_palettes
        .borrow()
        .as_ref()
        .and_then(|models| models.get(&stem(&entry.path).to_ascii_uppercase()))
        .cloned()
        .unwrap_or_default())
}

fn model<R: ReadAssets>(
    library: &Library<R>,
    entry: &Entry,
    source: &[u8],
    preview: &mut Preview,
    selected_palette: Option<&str>,
) -> Result<Content> {
    let model = import::model3d::parse_3d_file(source)?;
    preview.metadata = Some(
        json!({"version":String::from_utf8_lossy(&model.header.version),"vertex_count":model.header.num_vertices,"face_count":model.header.num_faces,"frame_count":model.header.num_frames,"radius":model.header.radius,"total_face_vertices":model.header.total_face_vertices}),
    );
    preview.details.extend([
        ("Vertices".into(), model.header.num_vertices.to_string()),
        ("Faces".into(), model.header.num_faces.to_string()),
        (
            "Authored frames".into(),
            model.header.num_frames.to_string(),
        ),
    ]);
    let references: std::collections::HashSet<_> = model
        .face_data
        .iter()
        .filter_map(|face| match face.texture_data {
            TextureData::Texture {
                texture_id,
                image_id,
            } => Some((texture_id, image_id)),
            _ => None,
        })
        .collect();
    for (texture_id, image_id) in &references {
        let bank = format!("TEXBSI.{texture_id:03}");
        let member = format!("{image_id} ·");
        for e in &library.catalog.entries {
            if e.path
                .rsplit('/')
                .next()
                .is_some_and(|p| p.eq_ignore_ascii_case(&bank))
                && e.name.starts_with(&member)
            {
                preview.related.push(e.id.clone());
            }
        }
    }
    let palettes = model_palettes(library, entry)?;
    let selected_palette = selected_palette
        .or_else(|| palettes.first().map(String::as_str))
        .filter(|name| !name.is_empty());
    let palette = if let Some(name) = selected_palette {
        ensure!(
            palettes.iter().any(|p| p == name),
            "palette {name} is not used by this model's worlds"
        );
        preview.details.push(("Palette".into(), name.into()));
        Some(import::palette::Palette::parse(
            &library.source.read(name)?,
        )?)
    } else {
        preview.details.push((
            "Palette".into(),
            "Without palette · embedded colors or grayscale indices".into(),
        ));
        None
    };
    let mut hash = blake3::Hasher::new();
    hash.update(source);
    if let Some(palette) = &palette {
        for color in &palette.colors {
            hash.update(color);
        }
    }
    let media_id = format!("model-{}", hash.finalize().to_hex());
    let banks = references.iter().map(|(bank, _)| *bank).collect();
    let mut textures = geometry::textures(
        &library.source,
        palette.as_ref().map(|palette| import::palette::Palette {
            colors: palette.colors,
        }),
        Some(&banks),
    )?;
    let count = (model.header.num_frames as usize).max(1);
    let mut frames = Vec::new();
    for i in 0..count {
        if i % 8 == 0 {
            library.source.progress(i, count, "Preparing model frames");
        }
        let url = library.output(&media_id, &format!("{i}.glb"));
        if library.cached(&url) {
            frames.push(url);
            continue;
        }
        let posed = model.at_frame(i)?;
        let (json, bytes) = gltf::convert_models_to_gltf(
            std::slice::from_ref(&posed),
            palette.as_ref(),
            Some(&mut textures),
            false,
        )?;
        library.publish(&url, &gltf::to_glb(&json, &bytes)?)?;
        frames.push(url);
    }
    library.source.progress(count, count, "Model frames ready");
    preview.details.push((
        "Appearance".into(),
        "Generic textured preview; world palette and lighting may differ.".into(),
    ));
    Ok(Content::Model {
        frames,
        palettes,
        data: None,
    })
}
fn audio<R: ReadAssets>(
    library: &Library<R>,
    entry: &Entry,
    preview: &mut Preview,
    pcm: &[u8],
    rate: u32,
    channels: u16,
    bits: u16,
) -> Result<Content> {
    let wav = media::pcm_wav(pcm, rate, channels, bits)?;
    preview.metadata = Some(
        json!({"sample_rate":rate,"channels":channels,"bits_per_sample":bits,"sample_frames":pcm.len() / (usize::from(channels) * usize::from(bits / 8))}),
    );
    let url = library.output(&entry.id, "wav");
    library.publish(&url, &wav)?;
    Ok(Content::Audio {
        wav: url,
        peaks: media::peaks(pcm, bits, 1024),
        seconds: pcm.len() as f64 / (f64::from(rate) * f64::from(channels) * f64::from(bits / 8)),
    })
}
fn movie<R: ReadAssets>(
    library: &Library<R>,
    entry: &Entry,
    preview: &mut Preview,
    bytes: &[u8],
) -> Result<Content> {
    let header = import::smacker::SmackerHeader::parse(bytes).map_err(anyhow::Error::msg)?;
    preview.metadata = Some(
        json!({"width":header.width,"height":header.height,"frame_count":header.frames,"frame_interval_microseconds":header.frame_micros,"double_height":header.double_height,"has_audio":header.has_audio}),
    );
    library.source.progress(0, 1, "Opening movie decoder");
    let mut decoder = smk::Smk::open_memory(bytes)?;
    let info = decoder.info();
    let audio = decoder.info_audio();
    let url = library.output(&entry.id, "smk");
    library.publish(&url, bytes)?;
    let wav = if audio.track_mask & 1 != 0 {
        decoder.enable_audio(0, true);
        decoder.enable_video(false);
        let mut pcm = Vec::new();
        decoder.first_frame()?;
        for i in 0..info.frame_count {
            if i % 120 == 0 {
                library.source.progress(
                    i as usize,
                    info.frame_count as usize,
                    "Decoding movie audio",
                );
            }
            if i > 0 {
                decoder.next_frame()?;
            }
            if let Some(chunk) = decoder.audio_data(0) {
                pcm.extend_from_slice(chunk);
            }
        }
        let url = library.output(&entry.id, "wav");
        library.publish(
            &url,
            &media::pcm_wav(
                &pcm,
                audio.rate[0],
                audio.channels[0].into(),
                audio.bitdepth[0].into(),
            )?,
        )?;
        Some(url)
    } else {
        None
    };
    ensure!(header.frames > 0, "empty movie");
    Ok(Content::Movie {
        smk: url,
        wav,
        seconds: info.frame_count as f64 * info.microseconds_per_frame / 1_000_000.,
    })
}
fn gxa_images(bytes: &[u8], preview: &mut Preview) -> Result<Content> {
    let image = import::gxa::parse_gxa_file(bytes)?;
    preview.metadata = Some(
        json!({"encoding":"GXA","title":image.title,"frame_count":image.frame_count,"frames":image.frames.iter().map(|frame|json!({"width":frame.width,"height":frame.height})).collect::<Vec<_>>()}),
    );
    Ok(Content::Images {
        frames: image
            .frames
            .into_iter()
            .enumerate()
            .map(|(i, frame)| Picture {
                label: format!("Frame {i}"),
                width: frame.width.into(),
                height: frame.height.into(),
                rgba: frame.rgba,
                indices: None,
                embedded_palette: false,
            })
            .collect(),
    })
}
fn visual_metadata(content: &Content) -> Option<serde_json::Value> {
    match content {
        Content::Images { frames } => Some(
            json!({"frame_count":frames.len(),"frames":frames.iter().map(|frame|json!({"label":frame.label,"width":frame.width,"height":frame.height,"indexed":frame.indices.is_some(),"embedded_palette":frame.embedded_palette})).collect::<Vec<_>>()}),
        ),
        Content::Palette { colors } => Some(json!({"colors":colors})),
        _ => None,
    }
}
fn indexed_picture(image: import::indexed_image::IndexedImage, index: usize) -> Picture {
    Picture {
        label: format!("Frame {index}"),
        width: image.width.into(),
        height: image.height.into(),
        rgba: image.rgba(),
        indices: Some(image.pixels),
        embedded_palette: true,
    }
}
fn text_data(bytes: &[u8], path: &str) -> Result<Content> {
    let text = String::from_utf8_lossy(bytes);
    Ok(Content::Data {
        value: super::definitions::decode(&text, path)?,
    })
}

/// Only the most recently used file is retained, including its archive index.
pub(super) enum Input {
    Bsi(import::bsi::BsiIndex),
    Rob(import::rob::RobIndex),
    Sfx(import::sfx::SfxIndex),
    Rtx(import::rtx::RtxIndex),
    File(Vec<u8>),
}
impl Input {
    pub(super) fn read(path: &str, source: &dyn ReadAssets) -> Result<Self> {
        let mut file = AssetFile::open(source, path)?.without_read_ahead();
        Ok(match FileType::from_path(std::path::Path::new(path)) {
            Some(FileType::Bsi) => Self::Bsi(import::bsi::BsiIndex::read(&mut file)?),
            Some(FileType::Rob) => Self::Rob(import::rob::RobIndex::read(&mut file)?),
            Some(FileType::Sfx) => Self::Sfx(import::sfx::SfxIndex::read(&mut file)?),
            Some(FileType::Rtx) => Self::Rtx(import::rtx::RtxIndex::read(&mut file)?),
            _ => Self::File(source.read(path)?),
        })
    }
}
