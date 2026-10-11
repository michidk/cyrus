//! Format adapters use preservation parsers and exporters, never binary layout knowledge.
use super::{Library, geometry};
use crate::source::{AssetFile, ReadAssets};
use crate::{Content, Entry, Picture, Preview, Source, media};
use anyhow::{Context, Result, ensure};
use rgpre::{
    gltf,
    import::{
        self, FileType,
        model3d::{FrameType, TextureData},
    },
};
use serde_json::json;

pub(super) fn prepare<R: ReadAssets>(library: &Library<R>, entry: &Entry) -> Result<Preview> {
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
                }],
            };
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
            preview.content = Content::Model { frames: vec![url] };
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
            preview.content = audio(library, entry, &pcm, 44100, 2, 16)?;
            return Ok(preview);
        }
        Source::CdMovie(name) => {
            let bytes = super::movie_bytes(&library.source, name)?;
            preview.content = movie(library, entry, &bytes)?;
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
                preview.details.extend([
                    ("Width".into(), image.width.to_string()),
                    ("Height".into(), image.height.to_string()),
                    (
                        "Palette".into(),
                        if image.palette.is_some() {
                            "Embedded"
                        } else {
                            "Choose a palette; default shows indices"
                        }
                        .into(),
                    ),
                ]);
                let frames = (0..=image.extra_frames.len())
                    .map(|i| Picture {
                        label: format!("Frame {i}"),
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
                    model(library, entry, &bytes, &mut preview)?
                } else {
                    ensure!(
                        segment.has_embedded_3d_data() || segment.has_special_embedded_data(),
                        "unsupported ROB segment type {}",
                        segment.segment_type
                    );
                    model(library, entry, &segment.data, &mut preview)?
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
            Some(FileType::Gxa) => {
                let image = import::gxa::parse_gxa_file(bytes)?;
                Content::Images {
                    frames: image
                        .frames
                        .into_iter()
                        .enumerate()
                        .map(|(i, f)| Picture {
                            label: format!("Frame {i}"),
                            width: f.width.into(),
                            height: f.height.into(),
                            rgba: f.rgba,
                            indices: None,
                        })
                        .collect(),
                }
            }
            Some(FileType::Col) => Content::Palette {
                colors: import::palette::Palette::parse(bytes)?.colors.to_vec(),
            },
            Some(FileType::Fnt) => {
                let font = import::fnt::parse_fnt(bytes)?;
                let content = crate::decode::font(&font)?;
                preview
                    .details
                    .push(("Description".into(), font.header.description_text));
                content
            }

            Some(FileType::Model3d | FileType::Model3dc) => {
                model(library, entry, bytes, &mut preview)?
            }

            Some(FileType::Rgm) => {
                let map = import::rgm::parse_rgm_file(bytes)?;
                let worlds = world_entries(library)?;
                let matching: Vec<_> = worlds
                    .entries
                    .iter()
                    .filter(|w| stem(&w.map).eq_ignore_ascii_case(stem(&entry.path)))
                    .collect();
                for world in matching {
                    related_world(library, &mut preview, world.index);
                }
                Content::Data {
                    value: import::rgm::export_rgm_metadata_json(&map, None),
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
                Content::Data {
                    value: json!({"terrain":format!("{:#?}",terrain.header),"world_previews":"Choose a related world for the authored palette and placements."}),
                }
            }
            Some(FileType::Pvo) => {
                let tree = import::pvo::parse_pvo_file(bytes)?;
                Content::Data {
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
                let image = image::load_from_memory(bytes)?.into_rgba8();
                Content::Images {
                    frames: vec![Picture {
                        label: entry.name.clone(),
                        width: image.width(),
                        height: image.height(),
                        rgba: image.into_raw(),
                        indices: None,
                    }],
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
                    &pcm,
                    spec.sample_rate,
                    spec.channels,
                    spec.bits_per_sample,
                )?
            }
            None if entry.category == crate::Category::Movies => movie(library, entry, bytes)?,
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

fn model<R: ReadAssets>(
    library: &Library<R>,
    entry: &Entry,
    source: &[u8],
    preview: &mut Preview,
) -> Result<Content> {
    let mut model = import::model3d::parse_3d_file(source)?;
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
    let worlds = if geometry::find_name(&library.source, "WORLD.INI").is_some() {
        world_entries(library)?
    } else {
        import::world_ini::WorldIni::parse("")
    };
    let mut palette_names = worlds
        .find_by_map_stem(stem(&entry.path))
        .iter()
        .map(|world| world.palette.to_ascii_lowercase())
        .collect::<Vec<_>>();
    palette_names.sort();
    palette_names.dedup();
    let palette = if palette_names.len() == 1 {
        let path = geometry::palette_path(&library.source, &palette_names[0])?;
        preview.details.push(("Palette".into(), path.clone()));
        Some(import::palette::Palette::parse(
            &library.source.read(&path)?,
        )?)
    } else {
        preview.details.push((
            "Palette".into(),
            "No unique world palette; embedded colors or grayscale indices".into(),
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
    let count = if model
        .frame_data
        .first()
        .is_some_and(|f| f.frame_type == FrameType::AnimatedI32)
    {
        model.frame_vertex_data.len()
    } else {
        1
    };
    if model.header.num_frames > 1 && count == 1 {
        preview.details.push((
            "Frame limit".into(),
            "Only base geometry is supported for this encoding.".into(),
        ));
    }
    let mut frames = Vec::new();
    for i in 0..count {
        let url = library.output(&media_id, &format!("{i}.glb"));
        if library.cached(&url) {
            frames.push(url);
            continue;
        }
        if i > 0 {
            model.vertex_coords = model.frame_vertex_data[i].coords.clone();
            model.face_normals = model
                .frame_normal_data
                .get(i)
                .context("missing frame normals")?
                .face_normals
                .clone();
        }
        let (json, bytes) = gltf::convert_models_to_gltf(
            std::slice::from_ref(&model),
            palette.as_ref(),
            Some(&mut textures),
            false,
        )?;
        library.publish(&url, &gltf::to_glb(&json, &bytes)?)?;
        frames.push(url);
    }
    preview.details.push((
        "Appearance".into(),
        "Generic textured preview; world palette and lighting may differ.".into(),
    ));
    Ok(Content::Model { frames })
}
fn audio<R: ReadAssets>(
    library: &Library<R>,
    entry: &Entry,
    pcm: &[u8],
    rate: u32,
    channels: u16,
    bits: u16,
) -> Result<Content> {
    let wav = media::pcm_wav(pcm, rate, channels, bits)?;
    let url = library.output(&entry.id, "wav");
    library.publish(&url, &wav)?;
    Ok(Content::Audio {
        wav: url,
        peaks: media::peaks(pcm, bits, 1024),
        seconds: pcm.len() as f64 / (f64::from(rate) * f64::from(channels) * f64::from(bits / 8)),
    })
}
fn movie<R: ReadAssets>(library: &Library<R>, entry: &Entry, bytes: &[u8]) -> Result<Content> {
    let header = import::smacker::SmackerHeader::parse(bytes).map_err(anyhow::Error::msg)?;
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
